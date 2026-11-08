use super::*;
fn fixture() -> Vec<u8> {
    let meta = br#"{"name":"synthetic test archive","vector_layers":[]}"#;
    let mut data = vec![0; 127];
    data[..7].copy_from_slice(b"PMTiles");
    data[7] = 3;
    for (offset, value) in [
        (8, 127u64),
        (16, 1),
        (24, 128),
        (32, meta.len() as u64),
        (40, 128 + meta.len() as u64),
        (48, 0),
        (56, 128 + meta.len() as u64),
        (64, 0),
    ] {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    data[97] = 1;
    data[98] = 1;
    data[99] = 1;
    data[100] = 0;
    data[101] = 6;
    for (offset, value) in [
        (102, -1800000000i32),
        (106, -850000000),
        (110, 1800000000),
        (114, 850000000),
    ] {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    data.push(0);
    data.extend_from_slice(meta);
    data
}
#[test]
fn header_and_directory_reject_truncation_overflow_and_unsupported_compression() {
    let data = fixture();
    let h = header(&data[..127], data.len() as u64).unwrap();
    assert_eq!(h.max_zoom, 6);
    assert!(directory(&[0], &h).is_ok());
    assert!(header(&data[..126], data.len() as u64).is_err());
    let mut bad = data.clone();
    bad[97] = 3;
    assert!(header(&bad[..127], bad.len() as u64).is_err());
    bad[97] = 1;
    bad[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(header(&bad[..127], bad.len() as u64).is_err());
    assert!(directory(&[255; 20], &h).is_err());
    assert!(directory(&[1, 0, 1, 10, 1], &h).is_err());
}
#[test]
fn local_ranges_never_escape_declared_archive() {
    let dir = std::env::temp_dir().join(format!("kokin-pmtiles-{}", std::process::id()));
    std::fs::write(&dir, fixture()).unwrap();
    let h = validate(&dir).unwrap();
    let mut file = File::open(&dir).unwrap();
    assert_eq!(range(&mut file, h.bytes, 0, 127).unwrap().len(), 127);
    assert!(range(&mut file, h.bytes, u64::MAX, 1).is_err());
    assert!(range(&mut file, h.bytes, h.bytes, 1).is_err());
    assert!(range(&mut file, h.bytes, 0, 4 * 1024 * 1024 + 1).is_err());
    std::fs::remove_file(dir).unwrap();
}
#[test]
fn coarse_world_extraction_preserves_tile_bytes_and_refuses_partial_transfers() {
    let mut data = fixture();
    let meta = data.split_off(128);
    data.truncate(127);
    let directory = vec![1, 0, 0xd5, 0x2a, 4, 1]; // one deduplicated tile across 5,461 IDs
    for (offset, value) in [
        (16, directory.len() as u64),
        (24, 127 + directory.len() as u64),
        (56, 127 + directory.len() as u64 + meta.len() as u64),
        (64, 4),
    ] {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    data.extend(directory);
    data.extend(meta);
    data.extend(b"tile");
    let plan = world::plan(&data[..127], data.len() as u64, |o, l| {
        Ok(data[o as usize..(o + l) as usize].to_vec())
    })
    .unwrap();
    assert_eq!(plan.download_bytes, 4);
    assert_eq!(plan.ranges.len(), 1);
    assert!(plan.assemble(|_, _| Ok(vec![0])).is_err());
    let output = plan
        .assemble(|o, l| Ok(data[o as usize..(o + l) as usize].to_vec()))
        .unwrap();
    assert!(output.ends_with(b"tile"));
    let h = header(&output[..127], output.len() as u64).unwrap();
    assert_eq!(h.max_zoom, 6);
    directory_validate(&output, &h);
}
fn directory_validate(output: &[u8], h: &Header) {
    let root = &output[h.root.0 as usize..(h.root.0 + h.root.1) as usize];
    directory(&decompress(root, h.internal_compression).unwrap(), h).unwrap();
}
