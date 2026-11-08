//! One request per contained process. No case key, database path, or sockets.
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn preview(image: &image::RgbImage) -> Result<Vec<u8>> {
    let source = image::DynamicImage::ImageRgb8(image.clone());
    let raster = if image.width() > 1400 || image.height() > 1400 {
        source.thumbnail(1400, 1400)
    } else {
        source
    };
    let mut png = std::io::Cursor::new(Vec::new());
    raster.write_to(&mut png, image::ImageFormat::Png)?;
    Ok(png.into_inner())
}

struct BoundedOutput<W> {
    inner: W,
    written: usize,
}
impl<W: Write> Write for BoundedOutput<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > kokin_faces::MAX_OUTPUT.saturating_sub(self.written) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "face output bound",
            ));
        }
        let n = self.inner.write(bytes)?;
        self.written += n;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn work() -> Result<()> {
    let mut input = std::io::stdin().lock();
    let mut header = [0; 4];
    input.read_exact(&mut header)?;
    let n = u32::from_le_bytes(header) as usize;
    if n > 4096 {
        return Err("face request length".into());
    }
    let mut json = vec![0; n];
    input.read_exact(&mut json)?;
    let request: kokin_faces::Request = serde_json::from_slice(&json)?;
    request.validate()?;
    let video = request.media_type.starts_with("video/");
    let max = if video {
        kokin_faces::MAX_VIDEO
    } else {
        kokin_media::MAX_INPUT
    };
    let mut bytes = Vec::new();
    if !video {
        (&mut input).take(max as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > max {
            return Err("source input bound".into());
        }
    }
    drop(input);
    let native = PathBuf::from(std::env::var_os("KOKIN_MEDIA_NATIVE").ok_or("native path")?);
    let models = PathBuf::from(std::env::var_os("KOKIN_FACE_MODELS").ok_or("face resources")?);
    let mut engine = if request.preview {
        None
    } else {
        Some(kokin_faces::inference::Engine::open(
            &native.join("face-onnx/libonnxruntime.1.31.0.dylib"),
            &models.join("face_detection_yunet_2023mar.onnx"),
            &models.join("face_recognition_sface_2021dec.onnx"),
        )?)
    };
    let mut out=kokin_faces::Output{version:1,detections:Vec::new(),preview_png:Vec::new(),sampled_frames:0,last_sample_ms:None,sampled_timestamps_ms:Vec::new(),complete:true,limitations:vec!["Face similarity is a candidate comparison, not proof of identity. Small, obscured or missed faces are outside coverage.".into()]};
    if video {
        let mut child = Command::new(native.join("video/kokin-video-frames"))
            .args([
                request.start_ms.to_string(),
                request.end_ms.unwrap_or(86_400_000).to_string(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut stdin = child.stdin.take().ok_or("video stdin")?;
        let writer = std::thread::spawn(move || {
            let mut source = std::io::stdin().take(kokin_faces::MAX_VIDEO as u64 + 1);
            std::io::copy(&mut source, &mut stdin)
        });
        let mut stream = child.stdout.take().ok_or("video stdout")?;
        let result = (|| -> Result<()> {
            loop {
                let mut h = [0u8; 20];
                stream.read_exact(&mut h)?;
                let u32at = |i| u32::from_le_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
                let t = u64::from_le_bytes(h[..8].try_into()?);
                let w = u32at(8);
                let height = u32at(12);
                let rotation = u32at(16);
                if w == 0 && height == 0 {
                    out.complete = t <= 1800;
                    break;
                }
                if w == 0
                    || height == 0
                    || u64::from(w) * u64::from(height) > 16_000_000
                    || out.sampled_frames >= 1800
                {
                    return Err("video frame bound".into());
                }
                let mut rgb = vec![0; w as usize * height as usize * 3];
                stream.read_exact(&mut rgb)?;
                let image = image::RgbImage::from_raw(w, height, rgb).ok_or("frame pixels")?;
                let image = match rotation {
                    0 => image,
                    90 => image::imageops::rotate90(&image),
                    180 => image::imageops::rotate180(&image),
                    270 => image::imageops::rotate270(&image),
                    _ => return Err("video rotation".into()),
                };
                if request.preview {
                    out.preview_png = preview(&image)?;
                } else {
                    out.detections.extend(
                        engine.as_mut().ok_or("recognizer unavailable")?.detect(
                            &image,
                            0,
                            Some(t),
                        )?,
                    );
                }
                out.sampled_frames += 1;
                out.last_sample_ms = Some(t);
                out.sampled_timestamps_ms.push(t);
                if out.detections.len() > kokin_faces::MAX_FACES {
                    return Err("face count bound; use a shorter range".into());
                }
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = child.kill();
        }
        let status = child.wait()?;
        writer.join().map_err(|_| "video writer failed")??;
        result?;
        if !status.success() {
            return Err("video decode failed".into());
        }
        out.limitations.push(format!("Video sampled every two seconds, up to 1,800 frames. Last sampled timestamp: {} ms. Unsampled frames were not searched.",out.last_sample_ms.map(|v|v.to_string()).unwrap_or_else(||"none".into())));
    } else {
        let raster = kokin_media::engine::face_raster(
            &bytes,
            &request.media_type,
            request.page,
            &native.join("pdfium/libpdfium.dylib"),
        )?;
        if request.preview {
            out.preview_png = preview(&raster)?;
        } else {
            out.detections = engine.as_mut().ok_or("recognizer unavailable")?.detect(
                &raster,
                request.page,
                None,
            )?;
        }
        out.sampled_frames = 1;
        if request.media_type == "application/pdf" {
            out.limitations.push("Only the selected rendered PDF page was searched; coordinates refer to its oriented raster.".into());
        }
    }
    // Stream bounded JSON so long videos never allocate a second full copy of
    // every face crop in the contained process.
    let stdout = std::io::stdout();
    let mut output = BoundedOutput {
        inner: std::io::BufWriter::with_capacity(65536, stdout.lock()),
        written: 0,
    };
    serde_json::to_writer(&mut output, &out)?;
    output.flush()?;
    Ok(())
}
fn main() {
    if let Err(error) = work() {
        if cfg!(debug_assertions) {
            eprintln!("Face worker diagnostic: {error}");
        }
        eprintln!("Face worker failed: verify resources, media support and processing limits.");
        std::process::exit(1);
    }
}
