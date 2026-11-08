#![cfg(feature = "inference")]
use kokin_faces::{cosine, inference::Engine};
use std::path::PathBuf;
#[test]
#[ignore = "requires explicitly staged pinned ONNX resources and runtime"]
fn real_portraits_match_variations_and_reject_different_people() {
    let models =
        PathBuf::from(std::env::var_os("KOKIN_FACE_TEST_MODELS").expect("test model directory"));
    let runtime = PathBuf::from(std::env::var_os("KOKIN_FACE_TEST_RUNTIME").expect("test runtime"));
    let mut engine = Engine::open(
        &runtime,
        &models.join("face_detection_yunet_2023mar.onnx"),
        &models.join("face_recognition_sface_2021dec.onnx"),
    )
    .unwrap();
    let a = image::load_from_memory(include_bytes!("fixtures/astronaut.png"))
        .unwrap()
        .to_rgb8();
    let b = image::load_from_memory(include_bytes!("fixtures/grace_hopper.jpg"))
        .unwrap()
        .to_rgb8();
    let first = engine.detect(&a, 0, None).unwrap();
    let second = engine.detect(&b, 0, None).unwrap();
    assert!(
        !first.is_empty() && !second.is_empty(),
        "both portraits need a detected face"
    );
    let small = image::imageops::resize(&a, 256, 256, image::imageops::FilterType::Triangle);
    let variation = engine.detect(&small, 0, None).unwrap();
    assert!(!variation.is_empty());
    let same = cosine(&first[0].vector, &variation[0].vector).unwrap();
    let different = cosine(&first[0].vector, &second[0].vector).unwrap();
    println!("same variation cosine={same}; different portraits cosine={different}");
    assert!(same >= 0.5);
    assert!(different < 0.363);
    let mut pair = image::RgbImage::new(a.width() + b.width(), a.height().max(b.height()));
    image::imageops::replace(&mut pair, &a, 0, 0);
    image::imageops::replace(&mut pair, &b, a.width() as i64, 0);
    assert!(engine.detect(&pair, 0, None).unwrap().len() >= 2);
    assert!(engine
        .detect(&image::RgbImage::new(640, 480), 0, None)
        .unwrap()
        .is_empty());
    let tiny = image::imageops::resize(&a, 16, 16, image::imageops::FilterType::Triangle);
    assert!(engine.detect(&tiny, 0, None).unwrap().is_empty());
}
