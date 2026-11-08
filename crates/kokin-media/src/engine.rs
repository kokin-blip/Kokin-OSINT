//! Native parsers are reachable only in the separately sandboxed worker binary.
use crate::{Error, Observation, Request, Result};
use image::{DynamicImage, ImageReader};
use pdfium_render::prelude::*;
use serde_json::json;
use std::{io::Cursor, path::Path};
const MAX_PIXELS: u64 = 16_000_000;
const MAX_TEXT: usize = 4 * 1024 * 1024;
fn processing(e: impl std::fmt::Display) -> Error {
    Error::Processing(e.to_string())
}
fn png(image: &DynamicImage) -> std::result::Result<Vec<u8>, Error> {
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(processing)?;
    Ok(out.into_inner())
}
/// DCT pHash is a duplicate-candidate aid, never an identity/authenticity judgment.
fn phash(image: &DynamicImage) -> String {
    let gray = image
        .resize_exact(32, 32, image::imageops::FilterType::Triangle)
        .to_luma8();
    let mut coefficients = Vec::with_capacity(64);
    for v in 0..8 {
        for u in 0..8 {
            let mut sum = 0.;
            for y in 0..32 {
                for x in 0..32 {
                    sum += f64::from(gray.get_pixel(x, y)[0])
                        * ((f64::from(2 * x + 1) * u as f64 * std::f64::consts::PI) / 64.).cos()
                        * ((f64::from(2 * y + 1) * v as f64 * std::f64::consts::PI) / 64.).cos();
                }
            }
            coefficients.push(sum);
        }
    }
    let mut sorted = coefficients[1..].to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[31];
    let bits = coefficients
        .iter()
        .enumerate()
        .skip(1)
        .fold(0u64, |bits, (i, c)| bits | ((u64::from(*c > median)) << i));
    format!("{bits:016x}")
}
fn located_bounds(bounds: [f32; 4]) -> Option<[f32; 4]> {
    (bounds
        .iter()
        .all(|v| v.is_finite() && v.abs() <= 1_000_000.)
        && bounds[0] <= bounds[2]
        && bounds[1] <= bounds[3])
        .then_some(bounds)
}
fn raster(
    bytes: &[u8],
    media: &str,
) -> std::result::Result<(DynamicImage, Vec<Observation>, Vec<String>), Error> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(processing)?;
    let format = reader.format().ok_or(Error::Unsupported)?;
    let expected = match media {
        "image/jpeg" => image::ImageFormat::Jpeg,
        "image/png" => image::ImageFormat::Png,
        "image/webp" => image::ImageFormat::WebP,
        "image/tiff" => image::ImageFormat::Tiff,
        _ => return Err(Error::Unsupported),
    };
    if format != expected {
        return Err(Error::Processing(
            "declared media type does not match source bytes".into(),
        ));
    }
    let (w, h) = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(processing)?
        .into_dimensions()
        .map_err(processing)?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(Error::Limit);
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16000);
    limits.max_image_height = Some(16000);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut image = reader.decode().map_err(processing)?;
    let mut observations = Vec::new();
    let mut gaps=vec!["Visual content has not been interpreted; metadata or OCR does not establish image authenticity.".into()];
    match exif::Reader::new().read_from_container(&mut Cursor::new(bytes)) {
        Ok(exif) => {
            let orientation = exif
                .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0))
                .unwrap_or(1);
            image = match orientation {
                2 => image.fliph(),
                3 => image.rotate180(),
                4 => image.flipv(),
                5 => image.rotate90().fliph(),
                6 => image.rotate90(),
                7 => image.rotate270().fliph(),
                8 => image.rotate270(),
                _ => image,
            };
            for field in exif.fields().take(512) {
                let value = field.display_value().with_unit(&exif).to_string();
                if value.len() <= 8192 {
                    observations.push(Observation{kind:"image.exif".into(),value,locator:json!({"scheme":"exif_field","tag":field.tag.number(),"ifd":field.ifd_num.index(),"engine":"kamadak-exif-0.6.1"})});
                } else {
                    gaps.push("An oversized EXIF field was omitted.".into());
                }
            }
        }
        Err(exif::Error::NotFound(_)) => {}
        Err(_) => gaps.push("EXIF metadata is absent or could not be parsed completely.".into()),
    }
    if format == image::ImageFormat::Tiff {
        gaps.push("Only the first TIFF frame is processed in this engine version; additional frames may exist.".into());
    }
    observations.push(Observation {
        kind: "image.dimensions".into(),
        value: format!(
            "{} × {} pixels after orientation",
            image.width(),
            image.height()
        ),
        locator: json!({"scheme":"image_dimensions","engine":"image-0.25.6"}),
    });
    Ok((image, observations, gaps))
}
fn ocr(
    image: &DynamicImage,
    page: u16,
    data: &Path,
) -> std::result::Result<Vec<Observation>, Error> {
    let data = data
        .to_str()
        .ok_or_else(|| processing("invalid OCR resource path"))?;
    let mut engine = leptess::LepTess::new(Some(data), "eng").map_err(processing)?;
    engine
        .set_image_from_mem(&png(image)?)
        .map_err(processing)?;
    let tsv = engine.get_tsv_text(0).map_err(processing)?;
    if tsv.len() > MAX_TEXT {
        return Err(Error::Limit);
    }
    let mut out = Vec::new();
    for row in tsv.lines().skip(1) {
        let cells: Vec<_> = row.splitn(12, '\t').collect();
        if cells.len() != 12 || cells[0] != "5" || cells[11].trim().is_empty() {
            continue;
        }
        let coordinate = |i: usize| cells[i].parse::<u32>().map_err(processing);
        let x = coordinate(6)?;
        let y = coordinate(7)?;
        let w = coordinate(8)?;
        let h = coordinate(9)?;
        if x.saturating_add(w) > image.width()
            || y.saturating_add(h) > image.height()
            || cells[11].len() > 8192
        {
            return Err(Error::Limit);
        }
        out.push(Observation{kind:"ocr.word".into(),value:cells[11].into(),locator:json!({"scheme":"ocr_region","page":page,"region":[x,y,w,h],"image_size":[image.width(),image.height()],"engine":format!("tesseract-{}",tesseract_plumbing::version().to_string_lossy()),"resource":"eng-tessdata_fast-87416418657359cb625c412a48b6e1d6d41c29bd","verification":"recorded_ocr_only"})});
        if out.len() > 10000 {
            return Err(Error::Limit);
        }
    }
    Ok(out)
}
pub fn process(
    bytes: &[u8],
    request: &Request,
    pdfium_library: &Path,
    ocr_data: Option<&Path>,
) -> std::result::Result<Result, Error> {
    if request.version != 1 || bytes.len() > crate::MAX_INPUT {
        return Err(Error::Limit);
    }
    let mut out = Result {
        version: 1,
        engine: "media-1;image-0.25.6;pdfium-7881".into(),
        observations: Vec::new(),
        thumbnail_png: Vec::new(),
        perceptual_hash: None,
        pages: 1,
        gaps: Vec::new(),
    };
    if let Some(recipe) = &request.image_recipe {
        recipe.validate()?;
        if request.ocr {
            return Err(Error::Unsupported);
        }
        out.engine.push_str(";query-prepare-1");
    }
    if request.ocr {
        out.engine.push_str(&format!(
            ";tesseract-{};eng-tessdata_fast-87416418657359cb625c412a48b6e1d6d41c29bd",
            tesseract_plumbing::version().to_string_lossy()
        ));
    }
    if request.media_type == "application/pdf" {
        if !bytes.starts_with(b"%PDF-") {
            return Err(processing("PDF signature is missing"));
        }
        // The vetted PDFium binary has V8 and XFA disabled; the wrapper features remain disabled too.
        let pdfium = Pdfium::new(Pdfium::bind_to_library(pdfium_library).map_err(processing)?);
        let document = pdfium
            .load_pdf_from_byte_slice(bytes, None)
            .map_err(processing)?;
        out.pages = u16::try_from(document.pages().len()).map_err(|_| Error::Limit)?;
        if out.pages == 0 || request.page >= out.pages {
            return Err(processing("page is outside this PDF"));
        }
        let mut total = 0usize;
        let text_pages = if request.image_recipe.is_some() {
            0
        } else {
            128
        };
        for (page_index, page) in document.pages().iter().take(text_pages).enumerate() {
            let text = page.text().map_err(processing)?;
            let chars = text.chars();
            let mut value = String::new();
            let mut start = 0;
            let mut end = 0;
            let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for ch in chars.iter() {
                let c = ch.unicode_string().unwrap_or_default();
                if total + c.len() > MAX_TEXT || out.observations.len() >= 10000 {
                    out.gaps
                        .push("PDF text budget reached; remaining text is not indexed.".into());
                    break;
                }
                total += c.len();
                if let Ok(b) = ch.tight_bounds() {
                    bounds[0] = bounds[0].min(b.left().value);
                    bounds[1] = bounds[1].min(b.bottom().value);
                    bounds[2] = bounds[2].max(b.right().value);
                    bounds[3] = bounds[3].max(b.top().value);
                }
                end = ch.index() + 1;
                value.push_str(&c);
                if value.len() >= 4000 {
                    out.observations.push(Observation{kind:"pdf.text".into(),value:std::mem::take(&mut value),locator:json!({"scheme":"pdf_char_range","page":page_index,"start":start,"end":end,"bounds":located_bounds(bounds),"coordinates":"pdf_points_bottom_left","engine":"pdfium-7881"})});
                    start = end;
                    bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                }
            }
            if !value.trim().is_empty() {
                out.observations.push(Observation{kind:"pdf.text".into(),value,locator:json!({"scheme":"pdf_char_range","page":page_index,"start":start,"end":end,"bounds":located_bounds(bounds),"coordinates":"pdf_points_bottom_left","engine":"pdfium-7881"})});
            }
            if chars.is_empty() {
                out.gaps.push(format!(
                    "Page {} has no native text; visual content requires review or optional OCR.",
                    page_index + 1
                ));
            }
        }
        if out.pages > 128 && request.image_recipe.is_none() {
            out.gaps
                .push("Only the first 128 PDF pages were indexed.".into());
        }
        let page = document
            .pages()
            .get(i32::from(request.page))
            .map_err(processing)?;
        let bitmap = page
            .render_with_config(
                &PdfRenderConfig::new()
                    .set_target_width(1600)
                    .set_maximum_height(1600),
            )
            .map_err(processing)?;
        let rgba = image::RgbaImage::from_raw(
            bitmap.width() as u32,
            bitmap.height() as u32,
            bitmap.as_rgba_bytes(),
        )
        .ok_or(Error::Limit)?;
        let image = DynamicImage::ImageRgba8(rgba);
        if request.ocr {
            out.observations.extend(ocr(
                &image,
                request.page,
                ocr_data.ok_or_else(|| processing("English OCR resource is not installed"))?,
            )?);
            out.gaps.push("OCR covers only the selected page and verifies against recorded OCR, not independently against source pixels.".into());
        }
        if let Some(recipe) = &request.image_recipe {
            let prepared = crate::image_query::prepare(image, recipe)?;
            out.perceptual_hash = Some(phash(&prepared));
            out.thumbnail_png = png(&prepared)?;
            out.observations = vec![Observation {
                kind: "image.query_recipe".into(),
                value: serde_json::to_string(recipe).map_err(processing)?,
                locator: json!({"scheme":"query_recipe","version":1,"page":request.page,"coordinates":"orientation_corrected_source_thousandths"}),
            }];
            out.gaps.push("Prepared query image is a derived reading, not an original capture. Redactions cover only the selected regions; inspect the output before external use.".into());
        } else {
            out.perceptual_hash = Some(phash(&image));
            out.thumbnail_png = png(&image.thumbnail(800, 800))?;
        }
        out.gaps.push("Native PDF text does not cover drawings or establish reading order; inspect the page rendering.".into());
    } else {
        if request.page != 0 {
            return Err(processing("image preview has only page zero"));
        }
        let (image, observations, gaps) = raster(bytes, &request.media_type)?;
        out.observations = observations;
        out.gaps = gaps;
        out.perceptual_hash = Some(phash(&image));
        if request.ocr {
            out.observations.extend(ocr(
                &image,
                0,
                ocr_data.ok_or_else(|| processing("English OCR resource is not installed"))?,
            )?);
        }
        if let Some(recipe) = &request.image_recipe {
            let prepared = crate::image_query::prepare(image, recipe)?;
            out.perceptual_hash = Some(phash(&prepared));
            out.thumbnail_png = png(&prepared)?;
            out.observations = vec![Observation {
                kind: "image.query_recipe".into(),
                value: serde_json::to_string(recipe).map_err(processing)?,
                locator: json!({"scheme":"query_recipe","version":1,"page":0,"coordinates":"orientation_corrected_source_thousandths"}),
            }];
            out.gaps.push("Prepared query image is derived. Redaction covers only chosen regions; inspect before sharing.".into());
        } else {
            out.thumbnail_png = png(&image.thumbnail(800, 800))?;
        }
    }
    if out.observations.len() > 10000
        || serde_json::to_vec(&out).map_err(processing)?.len() > crate::MAX_OUTPUT
    {
        return Err(Error::Limit);
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    fn request(media: &str) -> Request {
        Request {
            version: 1,
            media_type: media.into(),
            page: 0,
            ocr: false,
            image_recipe: None,
        }
    }
    #[test]
    fn raster_formats_hashes_and_limits_are_explicit() {
        let image = DynamicImage::ImageRgb8(image::RgbImage::from_fn(8, 4, |x, y| {
            image::Rgb([(x * 20) as u8, (y * 40) as u8, 90])
        }));
        for (format, media) in [
            (image::ImageFormat::Png, "image/png"),
            (image::ImageFormat::Jpeg, "image/jpeg"),
            (image::ImageFormat::WebP, "image/webp"),
            (image::ImageFormat::Tiff, "image/tiff"),
        ] {
            let mut bytes = Cursor::new(Vec::new());
            image.write_to(&mut bytes, format).unwrap();
            let result =
                process(bytes.get_ref(), &request(media), Path::new("missing"), None).unwrap();
            assert!(result.thumbnail_png.starts_with(b"\x89PNG"));
            assert_eq!(result.perceptual_hash.as_ref().unwrap().len(), 16);
            assert!(result.gaps.iter().any(|g| g.contains("authenticity")));
            if media == "image/tiff" {
                assert!(result.gaps.iter().any(|g| g.contains("first TIFF frame")));
            }
        }
        assert!(process(
            b"malformed",
            &request("image/png"),
            Path::new("missing"),
            None
        )
        .is_err());
        assert!(process(
            b"not HEIC",
            &request("image/heic"),
            Path::new("missing"),
            None
        )
        .is_err());
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        assert!(process(
            bytes.get_ref(),
            &request("image/jpeg"),
            Path::new("missing"),
            None
        )
        .is_err());
        let mut ocr_request = request("image/png");
        ocr_request.ocr = true;
        assert!(process(bytes.get_ref(), &ocr_request, Path::new("missing"), None).is_err());
    }
    #[test]
    fn oversized_dimensions_are_refused_before_pixel_allocation() {
        let mut bytes = include_bytes!("../tests/fixtures/synthetic.png").to_vec();
        bytes[16..20].copy_from_slice(&5000u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&5000u32.to_be_bytes());
        let crc = crc32fast::hash(&bytes[12..29]);
        bytes[29..33].copy_from_slice(&crc.to_be_bytes());
        assert!(matches!(
            process(&bytes, &request("image/png"), Path::new("missing"), None),
            Err(Error::Limit)
        ));
    }
    #[test]
    fn jpeg_exif_orientation_is_located_and_applied() {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(8, 4)
            .write_to(&mut bytes, image::ImageFormat::Jpeg)
            .unwrap();
        let mut exif =
            b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0".to_vec();
        let length = (exif.len() + 2) as u16;
        let mut jpeg = bytes.into_inner();
        let mut segment = vec![0xff, 0xe1];
        segment.extend(length.to_be_bytes());
        segment.append(&mut exif);
        jpeg.splice(2..2, segment);
        let result = process(&jpeg, &request("image/jpeg"), Path::new("missing"), None).unwrap();
        assert!(result
            .observations
            .iter()
            .any(|o| o.kind == "image.exif" && o.locator["tag"] == 274));
        assert!(result
            .observations
            .iter()
            .any(|o| o.value == "4 × 8 pixels after orientation"));
    }
}

/// Full oriented raster for face analysis, never the reduced media thumbnail.
pub fn face_raster(
    bytes: &[u8],
    media: &str,
    page: u16,
    pdfium_library: &Path,
) -> std::result::Result<image::RgbImage, Error> {
    if bytes.len() > crate::MAX_INPUT {
        return Err(Error::Limit);
    }
    if media != "application/pdf" {
        return Ok(raster(bytes, media)?.0.to_rgb8());
    }
    let pdfium = Pdfium::new(Pdfium::bind_to_library(pdfium_library).map_err(processing)?);
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(processing)?;
    let page = document.pages().get(i32::from(page)).map_err(processing)?;
    let bitmap = page
        .render_with_config(
            &PdfRenderConfig::new()
                .set_target_width(1600)
                .set_maximum_height(2400),
        )
        .map_err(processing)?;
    let rgba = image::RgbaImage::from_raw(
        bitmap.width() as u32,
        bitmap.height() as u32,
        bitmap.as_rgba_bytes(),
    )
    .ok_or(Error::Limit)?;
    Ok(DynamicImage::ImageRgba8(rgba).to_rgb8())
}
