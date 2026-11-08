//! YuNet BGR detection and SFace RGB inference, following OpenCV's public pipeline.
use crate::{Detection, Error, Result};
use image::{Rgb, RgbImage};
use ort::{session::Session, value::Tensor};
use std::{io::Cursor, path::Path};
fn err(e: impl std::fmt::Display) -> Error {
    Error(e.to_string())
}
pub struct Engine {
    detector: Session,
    recognizer: Session,
}
impl Engine {
    pub fn open(runtime: &Path, detector: &Path, recognizer: &Path) -> Result<Self> {
        ort::init_from(runtime.to_str().ok_or_else(|| err("runtime path"))?)
            .with_name("kokin-faces")
            .with_telemetry(false)
            .commit()
            .map_err(err)?;
        let session = |path: &Path| {
            Session::builder()
                .and_then(|b| b.with_intra_threads(2))
                .and_then(|b| b.commit_from_file(path))
                .map_err(err)
        };
        Ok(Self {
            detector: session(detector)?,
            recognizer: session(recognizer)?,
        })
    }
    pub fn detect(
        &mut self,
        image: &RgbImage,
        page: u16,
        timestamp_ms: Option<u64>,
    ) -> Result<Vec<Detection>> {
        let (w, h) = image.dimensions();
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 16_000_000 {
            return Err(err("image pixel bound"));
        }
        let scale = 640. / w.max(h) as f32;
        let rw = (w as f32 * scale).round().max(1.) as u32;
        let rh = (h as f32 * scale).round().max(1.) as u32;
        let resized = image::imageops::resize(image, rw, rh, image::imageops::FilterType::Triangle);
        let mut data = vec![0f32; 3 * 640 * 640];
        for (x, y, p) in resized.enumerate_pixels() {
            for c in 0..3 {
                data[c * 640 * 640 + y as usize * 640 + x as usize] = p[2 - c] as f32;
            }
        }
        let input = Tensor::from_array(([1, 3, 640, 640], data)).map_err(err)?;
        let outputs = self.detector.run(ort::inputs![input]).map_err(err)?;
        let mut candidates = Vec::new();
        for stride in [8usize, 16, 32] {
            let get = |prefix: &str| -> Result<Vec<f32>> {
                let key = format!("{prefix}_{stride}");
                let value = outputs
                    .get(&key)
                    .ok_or_else(|| err("YuNet output missing"))?;
                let (_, data) = value.try_extract_tensor::<f32>().map_err(err)?;
                Ok(data.to_vec())
            };
            let cls = get("cls")?;
            let obj = get("obj")?;
            let bbox = get("bbox")?;
            let kps = get("kps")?;
            let cols = 640 / stride;
            let n = cols * cols;
            if cls.len() != n || obj.len() != n || bbox.len() != n * 4 || kps.len() != n * 10 {
                return Err(err("YuNet output shape"));
            }
            for i in 0..n {
                let score = (cls[i].clamp(0., 1.) * obj[i].clamp(0., 1.)).sqrt();
                if !score.is_finite() || score < 0.9 {
                    continue;
                }
                let cx = ((i % cols) as f32 + bbox[i * 4]) * stride as f32;
                let cy = ((i / cols) as f32 + bbox[i * 4 + 1]) * stride as f32;
                let bw = bbox[i * 4 + 2].exp() * stride as f32;
                let bh = bbox[i * 4 + 3].exp() * stride as f32;
                let x = ((cx - bw / 2.) / scale).clamp(0., w as f32);
                let y = ((cy - bh / 2.) / scale).clamp(0., h as f32);
                let right = ((cx + bw / 2.) / scale).clamp(0., w as f32);
                let bottom = ((cy + bh / 2.) / scale).clamp(0., h as f32);
                if right - x < 16. || bottom - y < 16. {
                    continue;
                }
                let mut landmarks = [[0.; 2]; 5];
                for k in 0..5 {
                    landmarks[k] = [
                        ((i % cols) as f32 + kps[i * 10 + k * 2]) * stride as f32 / scale,
                        ((i / cols) as f32 + kps[i * 10 + k * 2 + 1]) * stride as f32 / scale,
                    ];
                }
                candidates.push(([x, y, right - x, bottom - y], landmarks, score));
            }
        }
        drop(outputs);
        candidates.sort_by(|a, b| b.2.total_cmp(&a.2));
        candidates.truncate(5000);
        let mut out: Vec<Detection> = Vec::new();
        for (bbox, landmarks, score) in candidates {
            if out.iter().any(|d| iou(d.bbox, bbox) > 0.3) {
                continue;
            }
            if out.len() >= 100 {
                return Err(err("more than 100 faces in one frame"));
            }
            let crop = align(image, &landmarks)?;
            let mut data = vec![0f32; 3 * 112 * 112];
            for (x, y, p) in crop.enumerate_pixels() {
                for c in 0..3 {
                    data[c * 112 * 112 + y as usize * 112 + x as usize] = p[c] as f32;
                }
            }
            let output = self
                .recognizer
                .run(ort::inputs![
                    Tensor::from_array(([1, 3, 112, 112], data)).map_err(err)?
                ])
                .map_err(err)?;
            let (_, values) = output[0].try_extract_tensor::<f32>().map_err(err)?;
            let mut vector = values.to_vec();
            crate::normalize(&mut vector)?;
            let mut png = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(crop)
                .write_to(&mut png, image::ImageFormat::Png)
                .map_err(err)?;
            let d = Detection {
                bbox,
                landmarks,
                image_size: [w, h],
                detection_score: score,
                vector,
                crop_png: png.into_inner(),
                page,
                timestamp_ms,
            };
            d.validate()?;
            out.push(d);
        }
        Ok(out)
    }
}
fn iou(a: [f32; 4], b: [f32; 4]) -> f32 {
    let intersection = (a[0] + a[2]).min(b[0] + b[2]) - (a[0].max(b[0]));
    let height = (a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1]);
    let area = intersection.max(0.) * height.max(0.);
    area / (a[2] * a[3] + b[2] * b[3] - area)
}
pub const TEMPLATE: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];
/// Least-squares orientation-preserving similarity transform, inverse-mapped bilinearly.
pub fn align(image: &RgbImage, points: &[[f32; 2]; 5]) -> Result<RgbImage> {
    let mean = |p: &[[f32; 2]; 5]| {
        [
            p.iter().map(|p| p[0]).sum::<f32>() / 5.,
            p.iter().map(|p| p[1]).sum::<f32>() / 5.,
        ]
    };
    let src = mean(points);
    let dst = mean(&TEMPLATE);
    let (mut a, mut b, mut den) = (0., 0., 0.);
    for (s, t) in points.iter().zip(TEMPLATE) {
        let x = s[0] - src[0];
        let y = s[1] - src[1];
        let u = t[0] - dst[0];
        let v = t[1] - dst[1];
        a += x * u + y * v;
        b += x * v - y * u;
        den += x * x + y * y;
    }
    if !den.is_finite() || den < 1e-6 {
        return Err(err("degenerate landmarks"));
    }
    a /= den;
    b /= den;
    let determinant = a * a + b * b;
    if !determinant.is_finite() || determinant < 1e-10 {
        return Err(err("degenerate alignment"));
    }
    let tx = dst[0] - a * src[0] + b * src[1];
    let ty = dst[1] - b * src[0] - a * src[1];
    Ok(RgbImage::from_fn(112, 112, |x, y| {
        let u = x as f32 - tx;
        let v = y as f32 - ty;
        let sx = (a * u + b * v) / determinant;
        let sy = (-b * u + a * v) / determinant;
        let x0 = sx.floor() as i32;
        let y0 = sy.floor() as i32;
        let fx = sx - x0 as f32;
        let fy = sy - y0 as f32;
        let mut result = [0.; 3];
        for (dx, dy, weight) in [
            (0, 0, (1. - fx) * (1. - fy)),
            (1, 0, fx * (1. - fy)),
            (0, 1, (1. - fx) * fy),
            (1, 1, fx * fy),
        ] {
            let xx = x0 + dx;
            let yy = y0 + dy;
            if xx >= 0 && yy >= 0 && xx < image.width() as i32 && yy < image.height() as i32 {
                let p = image.get_pixel(xx as u32, yy as u32);
                for c in 0..3 {
                    result[c] += p[c] as f32 * weight;
                }
            }
        }
        Rgb(result.map(|v| v.round().clamp(0., 255.) as u8))
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_alignment() {
        let img = RgbImage::from_fn(112, 112, |x, y| Rgb([x as u8, y as u8, 20]));
        let aligned = align(&img, &TEMPLATE).unwrap();
        for (x, y, p) in aligned.enumerate_pixels() {
            assert!((p[0] as i32 - x as i32).abs() <= 1);
            assert!((p[1] as i32 - y as i32).abs() <= 1);
        }
    }
}
