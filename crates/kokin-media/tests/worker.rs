#![allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires the locally packaged native engine; deliberately separate from default offline fixtures"]
fn isolated_worker_processes_a_synthetic_image_and_rejects_cancellation() {
    use std::sync::{atomic::AtomicBool, Arc};
    let binary = std::path::PathBuf::from(
        std::env::var_os("KOKIN_TEST_MEDIA_BIN").expect("packaged worker"),
    );
    let native = std::path::PathBuf::from(
        std::env::var_os("KOKIN_TEST_MEDIA_NATIVE").expect("packaged engines"),
    );
    let request = kokin_media::Request {
        version: 1,
        media_type: "image/png".into(),
        page: 0,
        ocr: false,
        image_recipe: None,
    };
    let bytes = include_bytes!("fixtures/synthetic.png").to_vec();
    let result = kokin_media::worker::run(
        &binary,
        &native,
        None,
        bytes.clone(),
        &request,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(result.pages, 1);
    assert!(result.thumbnail_png.starts_with(b"\x89PNG"));
    assert_eq!(result.perceptual_hash.unwrap().len(), 16);
    assert_eq!(result.observations[0].kind, "image.dimensions");
    assert!(kokin_media::worker::run(
        &binary,
        &native,
        None,
        bytes,
        &request,
        Arc::new(AtomicBool::new(true))
    )
    .is_err());
}

fn pdf(text: &str) -> Vec<u8> {
    let stream = format!("BT /F1 24 Tf 30 100 Td ({text}) Tj ET");
    let objects=["<< /Type /Catalog /Pages 2 0 R >>".to_string(),"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".into(),"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),format!("<< /Length {} >>\nstream\n{stream}\nendstream",stream.len())];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).bytes());
    }
    let start = out.len();
    out.extend(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{start}\n%%EOF").bytes());
    out
}
#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires the reviewed packaged PDF/OCR engines and installed English data"]
fn real_pdf_text_ocr_and_malformed_inputs_are_bounded() {
    use std::sync::{atomic::AtomicBool, Arc};
    let binary = std::path::PathBuf::from(std::env::var_os("KOKIN_TEST_MEDIA_BIN").unwrap());
    let native = std::path::PathBuf::from(std::env::var_os("KOKIN_TEST_MEDIA_NATIVE").unwrap());
    let data = std::path::PathBuf::from(std::env::var_os("KOKIN_TEST_OCR_DATA").unwrap());
    let request = kokin_media::Request {
        version: 1,
        media_type: "application/pdf".into(),
        page: 0,
        ocr: false,
        image_recipe: None,
    };
    let result = kokin_media::worker::run(
        &binary,
        &native,
        None,
        pdf("KOKIN TEST EVIDENCE"),
        &request,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(result.observations.iter().any(|o| o.kind == "pdf.text"
        && o.value.contains("KOKIN TEST EVIDENCE")
        && o.locator["page"] == 0));
    assert!(result.gaps.iter().any(|g| g.contains("drawings")));
    let mut ocr = request;
    ocr.ocr = true;
    let result = kokin_media::worker::run(
        &binary,
        &native,
        Some(&data),
        pdf("KOKIN TEST EVIDENCE"),
        &ocr,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(result.observations.iter().any(|o| o.kind == "ocr.word"
        && o.value == "EVIDENCE"
        && o.locator["verification"] == "recorded_ocr_only"));
    ocr.ocr = false;
    assert!(kokin_media::worker::run(
        &binary,
        &native,
        None,
        b"%PDF-malformed".to_vec(),
        &ocr,
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
    ocr.page = 1;
    assert!(kokin_media::worker::run(
        &binary,
        &native,
        None,
        pdf("text"),
        &ocr,
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
}

#[cfg(all(target_os = "macos", feature = "engine"))]
#[test]
#[ignore = "real scanned PDF/OCR acceptance with the reviewed packaged engines"]
fn scanned_pdf_has_no_native_text_and_optional_ocr_recovers_located_words() {
    use std::{
        io::Cursor,
        sync::{atomic::AtomicBool, Arc},
    };
    let binary = std::path::PathBuf::from(std::env::var_os("KOKIN_TEST_MEDIA_BIN").unwrap());
    let native = std::path::PathBuf::from(std::env::var_os("KOKIN_TEST_MEDIA_NATIVE").unwrap());
    let data = std::path::PathBuf::from(std::env::var_os("KOKIN_TEST_OCR_DATA").unwrap());
    let mut request = kokin_media::Request {
        version: 1,
        media_type: "application/pdf".into(),
        page: 0,
        ocr: false,
        image_recipe: None,
    };
    let source = kokin_media::worker::run(
        &binary,
        &native,
        None,
        pdf("KOKIN TEST EVIDENCE"),
        &request,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    let image = image::load_from_memory(&source.thumbnail_png)
        .unwrap()
        .to_rgb8();
    let mut jpeg = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image.clone())
        .write_to(&mut jpeg, image::ImageFormat::Jpeg)
        .unwrap();
    let jpeg = jpeg.into_inner();
    let draw = b"q 400 0 0 200 0 0 cm /Im0 Do Q";
    let mut image_object=format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",image.width(),image.height(),jpeg.len()).into_bytes();
    image_object.extend(jpeg);
    image_object.extend(b"\nendstream");
    let objects=vec![b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>".to_vec(),format!("<< /Length {} >>\nstream\n{}\nendstream",draw.len(),String::from_utf8_lossy(draw)).into_bytes(),image_object];
    let mut scanned = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(scanned.len());
        scanned.extend(format!("{} 0 obj\n", i + 1).bytes());
        scanned.extend(object);
        scanned.extend(b"\nendobj\n");
    }
    let xref = scanned.len();
    scanned.extend(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        scanned.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    scanned.extend(format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF").bytes());
    let raw = kokin_media::worker::run(
        &binary,
        &native,
        None,
        scanned.clone(),
        &request,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(raw.observations.is_empty());
    assert!(raw.gaps.iter().any(|g| g.contains("no native text")));
    request.ocr = true;
    let ocr = kokin_media::worker::run(
        &binary,
        &native,
        Some(&data),
        scanned,
        &request,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(ocr
        .observations
        .iter()
        .any(|o| o.kind == "ocr.word" && o.value == "EVIDENCE" && o.locator["region"].is_array()));
}

#[cfg(all(target_os = "macos", feature = "engine"))]
#[test]
#[ignore = "explicit native query-preparation acceptance with packaged worker"]
fn prepared_image_recipe_runs_inside_the_contained_worker() {
    use image::GenericImageView;
    use std::sync::{atomic::AtomicBool, Arc};
    let binary = std::path::PathBuf::from(std::env::var_os("KOKIN_TEST_MEDIA_BIN").unwrap());
    let native = std::path::PathBuf::from(std::env::var_os("KOKIN_TEST_MEDIA_NATIVE").unwrap());
    let recipe = kokin_media::image_query::Recipe {
        version: 1,
        crop: Some(kokin_media::image_query::Rect {
            x: 0,
            y: 0,
            width: 500,
            height: 1000,
        }),
        quarter_turns: 1,
        mirror_horizontal: true,
        obscure: vec![kokin_media::image_query::Rect {
            x: 0,
            y: 0,
            width: 1000,
            height: 1000,
        }],
        ..Default::default()
    };
    let source = include_bytes!("fixtures/synthetic.png");
    let original = image::load_from_memory(source).unwrap();
    let request = kokin_media::Request {
        version: 1,
        media_type: "image/png".into(),
        page: 0,
        ocr: false,
        image_recipe: Some(recipe.clone()),
    };
    let result = kokin_media::worker::run(
        &binary,
        &native,
        None,
        source.to_vec(),
        &request,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    let prepared = image::load_from_memory(&result.thumbnail_png).unwrap();
    assert_eq!(
        prepared.dimensions(),
        (original.height(), original.width().div_ceil(2))
    );
    assert!(prepared.to_rgba8().pixels().all(|p| p.0 == [0, 0, 0, 255]));
    assert!(result.engine.contains("query-prepare-1"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&result.observations[0].value).unwrap(),
        serde_json::to_value(recipe).unwrap()
    );
    assert_eq!(result.observations.len(), 1);
}
