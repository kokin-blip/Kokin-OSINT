//! Authenticated local PDF/OCR-to-batch previews. Decryption and parsing run
//! outside the case mutex; publication rechecks generation and source keys.
use crate::{CommandError, ErrorCode};
use kokin_connectors::candidates::{CandidatePreview, Segment};
use kokin_store::OpenCase;
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};
fn invalid(s: impl ToString) -> CommandError {
    CommandError::new(ErrorCode::InvalidValue, s.to_string())
}
struct Observation {
    id: String,
    kind: String,
    text: String,
    locator: Value,
}
pub struct Input {
    artifact: String,
    root: PathBuf,
    key: kokin_keys::CaseMasterKey,
    source: kokin_blob::BlobRef,
    snapshots: BTreeMap<String, kokin_blob::BlobRef>,
    observations: Vec<Observation>,
    warnings: Vec<String>,
}
fn readable(open: &OpenCase, hash: &str) -> Result<kokin_blob::BlobRef, CommandError> {
    match kokin_store::blob_access(&open.conn, hash)? {
        kokin_store::BlobAccess::Readable(r) => Ok(r),
        _ => Err(invalid(
            "source or extraction snapshot is shredded or unavailable",
        )),
    }
}
pub fn prepare(open: &OpenCase, artifact: &str) -> Result<Input, CommandError> {
    let (media, hash): (String, String) = open.conn.query_row(
        "SELECT media_type,content_hash FROM artifact WHERE id=?1",
        [artifact],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if !crate::media::supported(&media) {
        return Err(invalid(
            "choose imported PDF or OCR image evidence; text files can use the file preview",
        ));
    }
    let source = readable(open, &hash)?;
    if source.size > kokin_media::MAX_INPUT as u64 {
        return Err(invalid("source exceeds the native 64 MiB preview limit"));
    }
    let mut query=open.conn.prepare("SELECT o.id,o.kind,o.value,o.locator_json FROM observation o WHERE o.artifact_id=?1 AND o.kind IN('pdf.text','ocr.word') AND NOT EXISTS(SELECT 1 FROM observation_supersession s WHERE s.superseded_id=o.id) ORDER BY CAST(json_extract(o.locator_json,'$.page') AS INTEGER),o.rowid LIMIT 10001")?;
    let mut rows = query.query([artifact])?;
    let mut observations = vec![];
    let mut snapshots = BTreeMap::new();
    let mut total = 0;
    let mut snapshot_bytes = 0;
    let mut warnings = vec![];
    while let Some(row) = rows.next()? {
        let text: String = row.get(2)?;
        if observations.len() == 10_000 || total + text.len() > 2_000_000 {
            warnings.push("Evidence text preview reached its 2 MB/10,000-observation bound; later text is omitted.".into());
            break;
        }
        let locator_json: String = row.get(3)?;
        if locator_json.len() > 8192 {
            return Err(invalid(
                "native observation locator exceeds its 8 KiB bound",
            ));
        }
        let locator: Value = serde_json::from_str(&locator_json).map_err(invalid)?;
        let hash = locator["snapshot_hash"]
            .as_str()
            .ok_or_else(|| invalid("native extraction snapshot reference missing"))?;
        if !snapshots.contains_key(hash) {
            let reference = readable(open, hash)?;
            snapshot_bytes += reference.size;
            if reference.size > kokin_media::MAX_OUTPUT as u64 || snapshot_bytes > 32 * 1024 * 1024
            {
                return Err(invalid("extraction snapshots exceed the 32 MiB preview budget; use a smaller source or page selection"));
            }
            let belongs:bool=open.conn.query_row("SELECT EXISTS(SELECT 1 FROM media_derivative WHERE artifact_id=?1 AND snapshot_hash=?2)",rusqlite::params![artifact,hash],|r|r.get(0))?;
            if !belongs {
                return Err(invalid(
                    "extraction snapshot does not belong to this source",
                ));
            }
            snapshots.insert(hash.into(), reference);
        }
        total += text.len();
        observations.push(Observation {
            id: row.get(0)?,
            kind: row.get(1)?,
            text,
            locator,
        });
    }
    if observations.is_empty() {
        warnings.push("No current PDF/OCR text is available. Import and extract the source in Evidence first; OCR and missing text require review.".into());
    }
    warnings.push("Only current extracted observations are previewed. Fragmented OCR words or identifiers split across text chunks may need manual joining; no match does not establish absence.".into());
    Ok(Input {
        artifact: artifact.into(),
        root: open.paths.blobs(),
        key: open.cmk.clone(),
        source,
        snapshots,
        observations,
        warnings,
    })
}
pub struct Output {
    pub preview: CandidatePreview,
    artifact: String,
    hashes: Vec<String>,
    observation_ids: Vec<String>,
}
pub fn process(input: Input) -> Result<Output, CommandError> {
    let store = kokin_blob::BlobStore::new(&input.root);
    store
        .read_to(&input.source, &input.key, &mut std::io::sink())
        .map_err(invalid)?;
    let mut decoded = BTreeMap::<String, kokin_media::Result>::new();
    let mut warnings = input.warnings;
    for (hash, reference) in &input.snapshots {
        let raw = store.get(reference, &input.key).map_err(invalid)?;
        let snapshot: kokin_media::Result = serde_json::from_slice(&raw).map_err(invalid)?;
        warnings.extend(snapshot.gaps.iter().take(20).cloned());
        if snapshot.gaps.len() > 20 {
            warnings.push(format!("Only the first 20 of {} extraction gaps are shown; inspect source evidence for full coverage.",snapshot.gaps.len()));
        }
        decoded.insert(hash.clone(), snapshot);
    }
    let mut segments = vec![];
    let mut observation_ids = vec![];
    for observation in input.observations {
        let hash = observation.locator["snapshot_hash"]
            .as_str()
            .ok_or_else(|| invalid("snapshot hash missing"))?;
        let index = observation.locator["snapshot_index"]
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .ok_or_else(|| invalid("snapshot index missing"))?;
        let recorded = decoded
            .get(hash)
            .and_then(|s| s.observations.get(index))
            .ok_or_else(|| invalid("snapshot observation missing"))?;
        let mut locator = observation.locator.clone();
        if let Some(v) = locator.as_object_mut() {
            v.remove("kind");
            v.remove("snapshot_hash");
            v.remove("snapshot_index");
        }
        if recorded.kind != observation.kind
            || recorded.value != observation.text
            || recorded.locator != locator
        {
            return Err(invalid(
                "extracted text does not match its authenticated native snapshot",
            ));
        }
        observation_ids.push(observation.id.clone());
        segments.push(Segment {
            origin_id: observation.id,
            text: observation.text,
        });
    }
    let mut preview = kokin_connectors::candidates::extract(&segments).map_err(invalid)?;
    preview.warnings.extend(warnings);
    let mut hashes = input.snapshots.keys().cloned().collect::<Vec<_>>();
    hashes.push(input.source.content_hash);
    Ok(Output {
        preview,
        artifact: input.artifact,
        hashes,
        observation_ids,
    })
}
pub fn validate_publication(open: &OpenCase, output: &Output) -> Result<(), CommandError> {
    for hash in &output.hashes {
        readable(open, hash)?;
    }
    let ids = serde_json::to_string(&output.observation_ids).map_err(invalid)?;
    let stale:bool=open.conn.query_row("SELECT EXISTS(SELECT 1 FROM observation_supersession s JOIN observation o ON o.id=s.superseded_id WHERE o.artifact_id=?1 AND o.id IN(SELECT value FROM json_each(?2)))",rusqlite::params![output.artifact,ids],|r|r.get(0))?;
    if stale {
        return Err(invalid(
            "source extraction changed while preparing this preview; load it again",
        ));
    }
    Ok(())
}

/// Origins are navigation/provenance, never an analyst identity conclusion.
pub fn validate_origins(
    open: &OpenCase,
    selectors: &[kokin_connectors::research::Selector],
    origins: &[kokin_connectors::research::InputOrigin],
) -> Result<(), CommandError> {
    if origins.len() > 15 {
        return Err(invalid("at most 15 input-origin groups are allowed"));
    }
    for origin in origins {
        if !selectors.iter().any(|s| {
            s.kind == origin.selector.kind
                && s.normalized == origin.selector.normalized
                && s.original == origin.selector.original
        }) || origin.observation_ids.is_empty()
            || origin.observation_ids.len() > 100
            || origin
                .observation_ids
                .iter()
                .any(|id| id.is_empty() || id.len() > 128)
        {
            return Err(invalid(
                "input origins must cite 1–100 observations for a selected query",
            ));
        }
        for id in &origin.observation_ids {
            let hash:String=open.conn.query_row("SELECT a.content_hash FROM observation o JOIN artifact a ON a.id=o.artifact_id WHERE o.id=?1 AND NOT EXISTS(SELECT 1 FROM observation_supersession s WHERE s.superseded_id=o.id)",[id],|r|r.get(0)).map_err(|_|invalid("input observation is unavailable, superseded or from another case"))?;
            readable(open, &hash)?;
        }
    }
    Ok(())
}
