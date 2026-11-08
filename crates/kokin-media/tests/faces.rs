#![cfg(all(feature = "engine", target_os = "macos"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};
fn paths() -> (PathBuf, PathBuf, PathBuf) {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let probe = repo.join("target/media-probe");
    (
        probe.join("MacOS/kokin-face-worker"),
        probe.join("Resources/native"),
        std::env::var_os("KOKIN_FACE_TEST_MODELS")
            .expect("model directory")
            .into(),
    )
}
fn request(media: &str) -> kokin_faces::Request {
    kokin_faces::Request {
        version: 1,
        media_type: media.into(),
        page: 0,
        start_ms: 0,
        end_ms: None,
        preview: false,
    }
}
fn process(bytes: Vec<u8>, r: kokin_faces::Request) -> kokin_faces::Output {
    eprintln!(
        "testing {} preview={} start={} end={:?}",
        r.media_type, r.preview, r.start_ms, r.end_ms
    );
    let (b, n, m) = paths();
    kokin_media::face_worker::run(&b, &n, &m, bytes, &r, Arc::new(AtomicBool::new(false))).unwrap()
}
fn jpeg(image: &image::DynamicImage) -> Vec<u8> {
    let mut c = Cursor::new(Vec::new());
    image.write_to(&mut c, image::ImageFormat::Jpeg).unwrap();
    c.into_inner()
}
fn pdf(jpg: &[u8], w: u32, h: u32) -> Vec<u8> {
    let stream = b"q 500 0 0 500 0 0 cm /Image Do Q";
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let objects=[b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 500 500] /Resources << /XObject << /Image 4 0 R >> >> /Contents 5 0 R >>".to_vec(),[format!("<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",jpg.len()).into_bytes(),jpg.to_vec(),b"\nendstream".to_vec()].concat(),[format!("<< /Length {} >>\nstream\n",stream.len()).into_bytes(),stream.to_vec(),b"\nendstream".to_vec()].concat()];
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        bytes.extend(object);
        bytes.extend(b"\nendobj\n");
    }
    let xref = bytes.len();
    bytes.extend(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        bytes.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    bytes
}
fn chunk(id: &[u8; 4], bytes: Vec<u8>) -> Vec<u8> {
    let mut out = id.to_vec();
    out.extend((bytes.len() as u32).to_le_bytes());
    out.extend(&bytes);
    if bytes.len() % 2 == 1 {
        out.push(0);
    }
    out
}
fn list(kind: &[u8; 4], bytes: Vec<u8>) -> Vec<u8> {
    chunk(b"LIST", [kind.to_vec(), bytes].concat())
}
fn avi(jpg: &[u8], w: u32, h: u32, frames: usize) -> Vec<u8> {
    let mut avih = vec![0u8; 56];
    for (offset, n) in [
        (0, 1_000_000),
        (16, frames as u32),
        (24, 1),
        (28, jpg.len() as u32),
        (32, w),
        (36, h),
    ] {
        avih[offset..offset + 4].copy_from_slice(&n.to_le_bytes());
    }
    let mut strh = vec![0u8; 56];
    strh[..4].copy_from_slice(b"vids");
    strh[4..8].copy_from_slice(b"MJPG");
    for (offset, n) in [
        (20, 1u32),
        (24, 1),
        (32, frames as u32),
        (36, jpg.len() as u32),
        (40, u32::MAX),
    ] {
        strh[offset..offset + 4].copy_from_slice(&n.to_le_bytes());
    }
    let mut strf = vec![0u8; 40];
    for (offset, n) in [(0, 40u32), (4, w), (8, h), (20, w * h * 3)] {
        strf[offset..offset + 4].copy_from_slice(&n.to_le_bytes());
    }
    strf[12..14].copy_from_slice(&1u16.to_le_bytes());
    strf[14..16].copy_from_slice(&24u16.to_le_bytes());
    strf[16..20].copy_from_slice(b"MJPG");
    let header = list(
        b"hdrl",
        [
            chunk(b"avih", avih),
            list(
                b"strl",
                [chunk(b"strh", strh), chunk(b"strf", strf)].concat(),
            ),
        ]
        .concat(),
    );
    let movie = list(
        b"movi",
        (0..frames)
            .flat_map(|_| chunk(b"00dc", jpg.to_vec()))
            .collect(),
    );
    let data = [b"AVI ".to_vec(), header, movie].concat();
    chunk(b"RIFF", data)
}
#[test]
#[ignore = "requires prepared, relocated contained face/video engines and explicit model directory"]
fn contained_image_pdf_video_orientation_and_cancellation() {
    let astronaut = include_bytes!("../../kokin-faces/tests/fixtures/astronaut.png");
    let first = process(astronaut.to_vec(), request("image/png"));
    assert_eq!(first.detections.len(), 1);
    let image = image::load_from_memory(astronaut).unwrap();
    let jpg = jpeg(&image);
    let page = process(
        pdf(&jpg, image.width(), image.height()),
        request("application/pdf"),
    );
    assert_eq!(page.detections.len(), 1);
    assert_eq!(page.detections[0].page, 0);
    assert!(page.detections[0].image_size[0] >= 1500);
    // Rotate pixels counterclockwise, then set EXIF 6 so oriented decoding restores them.
    let rotated = jpeg(&image.rotate270());
    let mut exif =
        b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0".to_vec();
    let mut oriented = rotated[..2].to_vec();
    oriented.extend(b"\xff\xe1");
    oriented.extend(((exif.len() + 2) as u16).to_be_bytes());
    oriented.append(&mut exif);
    oriented.extend(&rotated[2..]);
    let oriented = process(oriented, request("image/jpeg"));
    assert_eq!(oriented.detections.len(), 1);
    assert!(
        kokin_faces::cosine(&first.detections[0].vector, &oriented.detections[0].vector).unwrap()
            > 0.8
    );
    let video = avi(&jpg, image.width(), image.height(), 6);
    std::fs::write(std::env::temp_dir().join("kokin-face-fixture.avi"), &video).unwrap();
    let sampled = process(video.clone(), request("video/x-msvideo"));
    assert_eq!(sampled.sampled_frames, 3);
    assert_eq!(
        sampled
            .detections
            .iter()
            .map(|d| d.timestamp_ms.unwrap())
            .collect::<Vec<_>>(),
        vec![0, 2000, 4000]
    );
    let mut r = request("video/x-msvideo");
    r.start_ms = 2000;
    r.end_ms = Some(4000);
    r.preview = true;
    let frame = process(video, r);
    assert_eq!(frame.sampled_frames, 1);
    assert!(!frame.preview_png.is_empty());
    let (b, n, m) = paths();
    let cancel_midway = Arc::new(AtomicBool::new(false));
    let stop = cancel_midway.clone();
    let timer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let long_video = avi(&jpg, image.width(), image.height(), 600);
    let started = std::time::Instant::now();
    assert!(kokin_media::face_worker::run(
        &b,
        &n,
        &m,
        long_video,
        &request("video/x-msvideo"),
        cancel_midway
    )
    .is_err());
    timer.join().unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "cancelled decoder group must stop promptly"
    );
    let (b, n, m) = paths();
    let cancel = Arc::new(AtomicBool::new(true));
    assert!(kokin_media::face_worker::run(
        &b,
        &n,
        &m,
        astronaut.to_vec(),
        &request("image/png"),
        cancel
    )
    .is_err());
    assert!(kokin_media::face_worker::run(
        &b,
        &n,
        &m,
        b"malformed".to_vec(),
        &request("application/pdf"),
        Arc::new(AtomicBool::new(false))
    )
    .is_err());
}

#[test]
#[ignore = "requires prepared native helper and generated H.264 fixtures"]
fn h264_decode_and_movie_rotation_are_preserved() {
    let directory = PathBuf::from(
        std::env::var_os("KOKIN_FACE_TEST_VIDEO_DIR").expect("generated H.264 fixture directory"),
    );
    let upright = process(
        std::fs::read(directory.join("h264-upright-v2.mov")).unwrap(),
        request("video/quicktime"),
    );
    assert_eq!(upright.sampled_frames, 3);
    assert!(!upright.detections.is_empty());
    let mut r = request("video/quicktime");
    r.preview = true;
    r.end_ms = Some(1);
    let rotated = process(
        std::fs::read(directory.join("h264-rotation-v2.mov")).unwrap(),
        r,
    );
    let image = image::load_from_memory(&rotated.preview_png).unwrap();
    assert_eq!((image.width(), image.height()), (384, 512));
}
#[test]
#[ignore = "requires the prepared LGPL helper; exercises its frame cap without inference"]
fn decoder_stops_after_1800_sampled_frames() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let (_, native, _) = paths();
    let pixels = image::DynamicImage::ImageRgb8(image::RgbImage::new(16, 16));
    let jpg = jpeg(&pixels);
    let bytes = avi(&jpg, 16, 16, 3602);
    let mut child = Command::new(native.join("video/kokin-video-frames"))
        .args(["0", "86400000"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || input.write_all(&bytes));
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    assert!(output.status.success());
    let mut offset = 0;
    let mut frames = 0;
    let mut last = 0;
    loop {
        let h = &output.stdout[offset..offset + 20];
        let t = u64::from_le_bytes(h[..8].try_into().unwrap());
        let w = u32::from_le_bytes(h[8..12].try_into().unwrap());
        let height = u32::from_le_bytes(h[12..16].try_into().unwrap());
        offset += 20;
        if w == 0 && height == 0 {
            assert_eq!(t, 1801);
            break;
        }
        frames += 1;
        last = t;
        offset += w as usize * height as usize * 3;
    }
    assert_eq!(frames, 1800);
    assert_eq!(last, 3_598_000);
    assert_eq!(offset, output.stdout.len());
}

#[test]
#[ignore = "requires prepared native media engines but deliberately no face weights"]
fn source_preview_without_model_weights() {
    let (binary, native, _) = paths();
    let directory =
        std::env::temp_dir().join(format!("kokin-empty-face-models-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let mut r = request("image/png");
    r.preview = true;
    let output = kokin_media::face_worker::run(
        &binary,
        &native,
        &directory,
        include_bytes!("../../kokin-faces/tests/fixtures/astronaut.png").to_vec(),
        &r,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    assert!(!output.preview_png.is_empty());
    assert!(output.detections.is_empty());
    std::fs::remove_dir_all(directory).unwrap();
}
