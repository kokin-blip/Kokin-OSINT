//! PMTiles v3 validation/range reads, without any HTTP client or browser path.
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
#[derive(Debug, thiserror::Error)]
#[error("invalid, truncated or unsupported PMTiles archive: {0}")]
pub struct Error(pub String);
#[derive(Debug, Clone)]
pub struct Header {
    pub bytes: u64,
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub tile_type: u8,
    pub compression: u8,
    pub internal_compression: u8,
    pub bounds: [f64; 4],
    pub root: (u64, u64),
    pub metadata: (u64, u64),
    pub leaves: (u64, u64),
    pub tiles: (u64, u64),
}
pub fn validate(path: &Path) -> Result<Header, Error> {
    let mut file = File::open(path).map_err(|_| Error("unreadable".into()))?;
    let size = file.metadata().map_err(|_| Error("metadata".into()))?.len();
    let mut h = [0; 127];
    file.read_exact(&mut h)
        .map_err(|_| Error("header".into()))?;
    header(&h, size).and_then(|header| {
        let root = range(&mut file, size, header.root.0, header.root.1)?;
        let root = decompress(&root, header.internal_compression)?;
        directory(&root, &header)?;
        if header.metadata.1 > 0 {
            let bytes = range(&mut file, size, header.metadata.0, header.metadata.1)?;
            let bytes = decompress(&bytes, header.internal_compression)?;
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| Error("JSON metadata".into()))?;
            if !value.is_object() {
                return Err(Error("metadata must be an object".into()));
            }
        }
        Ok(header)
    })
}
pub fn header(bytes: &[u8], size: u64) -> Result<Header, Error> {
    header_bounded(bytes, size, 32 * 1024 * 1024 * 1024)
}
pub fn source_header(bytes: &[u8], size: u64) -> Result<Header, Error> {
    header_bounded(bytes, size, 1024 * 1024 * 1024 * 1024)
}
fn header_bounded(bytes: &[u8], size: u64, limit: u64) -> Result<Header, Error> {
    if bytes.len() != 127 || &bytes[..7] != b"PMTiles" || bytes[7] != 3 || size > limit {
        return Err(Error("v3 header or size".into()));
    }
    let u = |offset| {
        let mut b = [0; 8];
        b.copy_from_slice(&bytes[offset..offset + 8]);
        u64::from_le_bytes(b)
    };
    let coord = |offset| {
        let mut b = [0; 4];
        b.copy_from_slice(&bytes[offset..offset + 4]);
        f64::from(i32::from_le_bytes(b)) / 1e7
    };
    let root = (u(8), u(16));
    let metadata = (u(24), u(32));
    let leaves = (u(40), u(48));
    let tiles = (u(56), u(64));
    for (offset, length) in [root, metadata, leaves, tiles] {
        if length > 0 && (offset < 127 || offset.checked_add(length).is_none_or(|end| end > size)) {
            return Err(Error("section bounds".into()));
        }
    }
    if root.1 == 0
        || root.1 > 4 * 1024 * 1024
        || metadata.1 > 4 * 1024 * 1024
        || !matches!(bytes[97], 1 | 2)
        || !matches!(bytes[98], 1 | 2)
        || !matches!(bytes[99], 1..=4)
        || bytes[100] > bytes[101]
        || bytes[101] > 22
    {
        return Err(Error("compression, type, directory or zoom limits".into()));
    }
    let bounds = [coord(102), coord(106), coord(110), coord(114)];
    if bounds[0] < -180.0
        || bounds[2] > 180.0
        || bounds[1] < -90.0
        || bounds[3] > 90.0
        || bounds[0] > bounds[2]
        || bounds[1] > bounds[3]
    {
        return Err(Error("geographic bounds".into()));
    }
    let mut sections = vec![root, metadata, leaves, tiles];
    sections.retain(|(_, l)| *l > 0);
    sections.sort();
    for pair in sections.windows(2) {
        if pair[0].0 + pair[0].1 > pair[1].0 {
            return Err(Error("overlapping sections".into()));
        }
    }
    Ok(Header {
        bytes: size,
        min_zoom: bytes[100],
        max_zoom: bytes[101],
        tile_type: bytes[99],
        compression: bytes[98],
        internal_compression: bytes[97],
        bounds,
        root,
        metadata,
        leaves,
        tiles,
    })
}
pub fn range(file: &mut File, size: u64, offset: u64, length: u64) -> Result<Vec<u8>, Error> {
    if length == 0
        || length > 4 * 1024 * 1024
        || offset.checked_add(length).is_none_or(|end| end > size)
    {
        return Err(Error("range bounds".into()));
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| Error("seek".into()))?;
    let mut bytes = vec![0; length as usize];
    file.read_exact(&mut bytes)
        .map_err(|_| Error("short read".into()))?;
    Ok(bytes)
}
pub fn decompress(bytes: &[u8], kind: u8) -> Result<Vec<u8>, Error> {
    match kind {
        1 => Ok(bytes.to_vec()),
        2 => {
            let mut output = Vec::new();
            flate2::read::GzDecoder::new(bytes)
                .take(16 * 1024 * 1024 + 1)
                .read_to_end(&mut output)
                .map_err(|_| Error("gzip".into()))?;
            if output.len() > 16 * 1024 * 1024 {
                return Err(Error("decompression budget".into()));
            }
            Ok(output)
        }
        _ => Err(Error("only none/gzip compression is supported".into())),
    }
}
fn varint(data: &[u8], cursor: &mut usize) -> Result<u64, Error> {
    let mut result = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *data
            .get(*cursor)
            .ok_or_else(|| Error("directory varint".into()))?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err(Error("varint overflow".into()));
        }
        result |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return Ok(result);
        }
    }
    Err(Error("varint overflow".into()))
}
pub fn directory(data: &[u8], h: &Header) -> Result<(), Error> {
    let mut cursor = 0;
    let n = varint(data, &mut cursor)? as usize;
    if n > 500_000 {
        return Err(Error("directory entry budget".into()));
    }
    let mut previous = 0u64;
    for _ in 0..n {
        previous = previous
            .checked_add(varint(data, &mut cursor)?)
            .ok_or_else(|| Error("tile id overflow".into()))?;
        if previous >= (1u64 << 48) {
            return Err(Error("tile id".into()));
        }
    }
    let mut runs = Vec::with_capacity(n);
    for _ in 0..n {
        runs.push(varint(data, &mut cursor)?);
    }
    let mut lengths = Vec::with_capacity(n);
    for _ in 0..n {
        let length = varint(data, &mut cursor)?;
        if length == 0 || length > 4 * 1024 * 1024 {
            return Err(Error("entry length".into()));
        }
        lengths.push(length);
    }
    let mut prior_end = 0u64;
    for i in 0..n {
        let encoded = varint(data, &mut cursor)?;
        let offset = if encoded == 0 && i > 0 {
            prior_end
        } else {
            encoded
                .checked_sub(1)
                .ok_or_else(|| Error("entry offset".into()))?
        };
        prior_end = offset
            .checked_add(lengths[i])
            .ok_or_else(|| Error("entry overflow".into()))?;
        let limit = if runs[i] == 0 { h.leaves.1 } else { h.tiles.1 };
        if prior_end > limit {
            return Err(Error("entry outside tile/leaf section".into()));
        }
    }
    if cursor != data.len() {
        return Err(Error("directory trailing data".into()));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;

pub mod world;
