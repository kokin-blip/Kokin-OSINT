//! Bounded coarse-world extraction; callers own the disclosed network capability.
use crate::{decompress, directory, source_header, varint, Error, Header};
use std::collections::{BTreeMap, HashSet};
const LAST_TILE: u64 = 5460; // All Hilbert tile IDs at z0–z6.
const CAP: u64 = 128 * 1024 * 1024;
#[derive(Clone, Debug)]
struct Entry {
    id: u64,
    run: u64,
    offset: u64,
    length: u64,
}
fn entries(bytes: &[u8], h: &Header) -> Result<Vec<Entry>, Error> {
    directory(bytes, h)?;
    let mut cursor = 0;
    let n = varint(bytes, &mut cursor)? as usize;
    let mut result = Vec::with_capacity(n);
    let mut id = 0;
    for i in 0..n {
        let delta = varint(bytes, &mut cursor)?;
        if i > 0 && delta == 0 {
            return Err(Error("duplicate directory ID".into()));
        }
        id += delta;
        result.push(Entry {
            id,
            run: 0,
            offset: 0,
            length: 0,
        });
    }
    for entry in &mut result {
        entry.run = varint(bytes, &mut cursor)?;
        entry
            .id
            .checked_add(entry.run)
            .ok_or_else(|| Error("tile run overflow".into()))?;
    }
    for entry in &mut result {
        entry.length = varint(bytes, &mut cursor)?;
    }
    let mut end = 0;
    for (i, entry) in result.iter_mut().enumerate() {
        let encoded = varint(bytes, &mut cursor)?;
        entry.offset = if encoded == 0 && i > 0 {
            end
        } else {
            encoded - 1
        };
        end = entry.offset + entry.length;
    }
    Ok(result)
}
#[derive(Clone)]
pub struct Plan {
    header: Vec<u8>,
    metadata: Vec<u8>,
    tiles: Vec<Entry>,
    pub ranges: Vec<(u64, u64)>,
    pub download_bytes: u64,
    pub output_bytes: u64,
}
/// Fetch only public metadata/directories now. Tile data waits for a second consent.
pub fn plan(
    header: &[u8],
    size: u64,
    mut read: impl FnMut(u64, u64) -> Result<Vec<u8>, Error>,
) -> Result<Plan, Error> {
    let h = source_header(header, size)?;
    if h.min_zoom != 0 || h.max_zoom < 6 {
        return Err(Error("source lacks full z0–z6 range".into()));
    }
    let mut requests = vec![(h.root.0, h.root.1, 0)];
    let mut seen = HashSet::new();
    let mut tiles = Vec::new();
    let mut metadata_budget = 0;
    while let Some((offset, length, depth)) = requests.pop() {
        if depth > 4 || seen.len() >= 64 || !seen.insert((offset, length)) {
            return Err(Error("directory recursion budget".into()));
        }
        metadata_budget += length;
        if metadata_budget > 8 * 1024 * 1024 {
            return Err(Error("directory transfer budget".into()));
        }
        let bytes = decompress(&read(offset, length)?, h.internal_compression)?;
        for mut e in entries(&bytes, &h)? {
            if e.id > LAST_TILE {
                continue;
            }
            if e.run == 0 {
                requests.push((h.leaves.0 + e.offset, e.length, depth + 1));
            } else {
                e.run = e.run.min(LAST_TILE - e.id + 1);
                e.offset += h.tiles.0;
                tiles.push(e);
            }
        }
    }
    tiles.sort_by_key(|e| e.id);
    if tiles.len() > 5461
        || tiles.is_empty()
        || tiles
            .windows(2)
            .any(|pair| pair[0].id + pair[0].run > pair[1].id)
    {
        return Err(Error("coarse tile budget or overlap".into()));
    }
    let metadata = if h.metadata.1 > 0 {
        read(h.metadata.0, h.metadata.1)?
    } else {
        Vec::new()
    };
    let unique: BTreeMap<_, _> = tiles.iter().map(|e| ((e.offset, e.length), ())).collect();
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    let mut output_bytes = 0;
    for ((offset, length), _) in unique {
        output_bytes += length;
        if let Some(last) = ranges.last_mut() {
            let end = last.0 + last.1;
            if offset >= end && offset - end <= 4096 && offset + length - last.0 <= 4 * 1024 * 1024
            {
                last.1 = offset + length - last.0;
                continue;
            }
        }
        ranges.push((offset, length));
    }
    let download_bytes = ranges.iter().map(|r| r.1).sum::<u64>();
    if output_bytes > CAP || download_bytes > CAP || ranges.len() > 256 {
        return Err(Error(
            "coarse basemap exceeds 128 MiB or range budget".into(),
        ));
    }
    Ok(Plan {
        header: header.to_vec(),
        metadata,
        tiles,
        ranges,
        download_bytes,
        output_bytes: output_bytes + h.metadata.1 + 16 * 1024 + 127,
    })
}
fn put(value: u64, output: &mut Vec<u8>) {
    let mut value = value;
    while value >= 128 {
        output.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    output.push(value as u8);
}
fn encode(entries: &[Entry]) -> Vec<u8> {
    let mut bytes = Vec::new();
    put(entries.len() as u64, &mut bytes);
    let mut id = 0;
    for e in entries {
        put(e.id - id, &mut bytes);
        id = e.id;
    }
    for e in entries {
        put(e.run, &mut bytes);
    }
    for e in entries {
        put(e.length, &mut bytes);
    }
    let mut end = 0;
    for (i, e) in entries.iter().enumerate() {
        put(
            if i > 0 && e.offset == end {
                0
            } else {
                e.offset + 1
            },
            &mut bytes,
        );
        end = e.offset + e.length;
    }
    bytes
}
impl Plan {
    pub fn assemble(
        &self,
        mut read: impl FnMut(u64, u64) -> Result<Vec<u8>, Error>,
    ) -> Result<Vec<u8>, Error> {
        let mut blocks = Vec::new();
        for &(offset, length) in &self.ranges {
            let data = read(offset, length)?;
            if data.len() as u64 != length {
                return Err(Error("short tile range".into()));
            }
            blocks.push((offset, data));
        }
        let mut data = Vec::new();
        let mut offsets = BTreeMap::new();
        let mut entries = Vec::new();
        for e in &self.tiles {
            let key = (e.offset, e.length);
            let offset = if let Some(offset) = offsets.get(&key) {
                *offset
            } else {
                let offset = data.len() as u64;
                let (base, block) = blocks
                    .iter()
                    .find(|(base, block)| {
                        *base <= e.offset && *base + block.len() as u64 >= e.offset + e.length
                    })
                    .ok_or_else(|| Error("tile unavailable".into()))?;
                let start = (e.offset - base) as usize;
                data.extend_from_slice(&block[start..start + e.length as usize]);
                offsets.insert(key, offset);
                offset
            };
            entries.push(Entry {
                offset,
                ..e.clone()
            });
        }
        let raw = encode(&entries);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, &raw)
            .map_err(|_| Error("directory compression".into()))?;
        let root = encoder
            .finish()
            .map_err(|_| Error("directory compression".into()))?;
        if root.len() > 16 * 1024 - 127 {
            return Err(Error("coarse root exceeds first-page budget".into()));
        }
        let internal = self.header[97];
        let metadata = if self.metadata.is_empty() {
            Vec::new()
        } else {
            decompress(&self.metadata, internal)?
        };
        let metadata = if metadata.is_empty() {
            b"{}".to_vec()
        } else {
            metadata
        };
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut gzip, &metadata)
            .map_err(|_| Error("metadata compression".into()))?;
        let metadata = gzip
            .finish()
            .map_err(|_| Error("metadata compression".into()))?;
        let mut header = self.header.clone();
        let values = [
            (8, 127),
            (16, root.len() as u64),
            (24, 127 + root.len() as u64),
            (32, metadata.len() as u64),
            (40, 0),
            (48, 0),
            (56, 127 + root.len() as u64 + metadata.len() as u64),
            (64, data.len() as u64),
            (72, entries.iter().map(|e| e.run).sum()),
            (80, entries.len() as u64),
            (88, offsets.len() as u64),
        ];
        for (offset, value) in values {
            header[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        header[96] = 0;
        header[97] = 2;
        header[100] = 0;
        header[101] = 6;
        header[118] = header[118].min(6);
        let mut output = header;
        output.extend(root);
        output.extend(metadata);
        output.extend(data);
        crate::header(&output[..127], output.len() as u64)?;
        Ok(output)
    }
}
