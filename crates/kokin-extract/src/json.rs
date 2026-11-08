//! JSON scalar observations with RFC 6901 locations into verified original bytes.
use kokin_blob::BlobStore;
use kokin_keys::CaseMasterKey;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("JSON extraction limit or malformed document")]
    Invalid,
    #[error("source unavailable")]
    Unavailable,
    #[error("storage: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("store: {0}")]
    Store(#[from] kokin_store::StoreError),
    #[error("blob: {0}")]
    Blob(#[from] kokin_blob::BlobError),
    #[error("random id unavailable")]
    Random,
}
type Result<T> = std::result::Result<T, Error>;
#[derive(Debug)]
pub struct Located {
    pub value: String,
    pub pointer: String,
}
#[derive(Debug)]
pub struct Parsed {
    pub observations: Vec<Located>,
    pub gaps: Vec<String>,
}
pub fn parse(bytes: &[u8]) -> Result<Parsed> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(Error::Invalid);
    }
    let root: Value = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
    let mut out = Parsed {
        observations: Vec::new(),
        gaps: Vec::new(),
    };
    let mut nodes = 0;
    let mut text = 0;
    walk(&root, "", 0, &mut nodes, &mut text, &mut out);
    out.gaps.sort();
    out.gaps.dedup();
    Ok(out)
}
fn walk(
    value: &Value,
    pointer: &str,
    depth: usize,
    nodes: &mut usize,
    text: &mut usize,
    out: &mut Parsed,
) {
    *nodes += 1;
    if *nodes > 20000 || depth > 32 || out.observations.len() >= 10000 {
        out.gaps
            .push("JSON traversal budget reached; additional fields are not indexed.".into());
        return;
    }
    match value {
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                if *nodes > 20000 {
                    break;
                }
                walk(
                    value,
                    &format!("{pointer}/{index}"),
                    depth + 1,
                    nodes,
                    text,
                    out,
                )
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if *nodes > 20000 {
                    break;
                }
                let key = key.replace('~', "~0").replace('/', "~1");
                walk(
                    value,
                    &format!("{pointer}/{key}"),
                    depth + 1,
                    nodes,
                    text,
                    out,
                )
            }
        }
        _ => {
            let value = scalar(value);
            if value.len() > 8192 || *text + value.len() > 4 * 1024 * 1024 || pointer.len() > 4096 {
                out.gaps.push("JSON value, location or text budget reached; omitted fields remain in raw evidence.".into());
                return;
            }
            *text += value.len();
            out.observations.push(Located {
                value,
                pointer: pointer.into(),
            });
        }
    }
}
pub fn scalar(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
/// Resolve a scalar only after the caller verifies the entire original blob.
pub fn citation(bytes: &[u8], pointer: &str, expected: &str) -> Result<Option<String>> {
    if bytes.len() > 8 * 1024 * 1024 || pointer.len() > 4096 {
        return Err(Error::Invalid);
    }
    let root: Value = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
    let Some(value) = root.pointer(pointer) else {
        return Ok(None);
    };
    if value.is_array() || value.is_object() {
        return Ok(None);
    }
    let actual = scalar(value);
    Ok((actual == expected && actual.len() <= 8192).then_some(actual))
}
fn id() -> Result<String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|_| Error::Random)?;
    Ok(kokin_store::blake3_hex(&bytes))
}
pub fn extract(
    conn: &mut Connection,
    blobs: &BlobStore,
    cmk: &CaseMasterKey,
    artifact: &str,
) -> Result<(String, usize, bool)> {
    let (run, now) = begin(conn, artifact)?;
    let parsed = (|| {
        let hash: String = conn.query_row(
            "SELECT content_hash FROM artifact WHERE id=?1",
            [artifact],
            |r| r.get(0),
        )?;
        let kokin_store::BlobAccess::Readable(reference) = kokin_store::blob_access(conn, &hash)?
        else {
            return Err(Error::Unavailable);
        };
        if reference.size > 8 * 1024 * 1024 {
            return Err(Error::Invalid);
        }
        let bytes = blobs.get(&reference, cmk)?;
        let parsed = parse(&bytes)?;
        Ok::<Parsed, Error>(parsed)
    })();
    match parsed {
        Ok(parsed) => commit_run(conn, artifact, &parsed, run, now),
        Err(error) => {
            let _=conn.execute("UPDATE transform_run SET status='failed',finished_utc=?2,error_code='json.failed',error_detail=?3 WHERE id=?1",params![run,now,error.to_string()]);
            Err(error)
        }
    }
}
/// Commit observations parsed from the exact broker bytes. The caller must first
/// capture those bytes as this artifact; parsing can run outside its case lock.
pub fn commit(
    conn: &mut Connection,
    artifact: &str,
    parsed: &Parsed,
) -> Result<(String, usize, bool)> {
    let (run, now) = begin(conn, artifact)?;
    commit_run(conn, artifact, parsed, run, now)
}
fn begin(conn: &Connection, artifact: &str) -> Result<(String, String)> {
    let run = id()?;
    let now = kokin_store::now_utc_rfc3339();
    conn.execute("INSERT INTO transform_run VALUES(?1,'extract.json','1',?2,'json-1',?3,NULL,'running',NULL,NULL)",params![run,env!("CARGO_PKG_VERSION"),now])?;
    conn.execute(
        "INSERT INTO run_subject VALUES(?1,'artifact',?2)",
        params![run, artifact],
    )?;
    Ok((run, now))
}
fn commit_run(
    conn: &mut Connection,
    artifact: &str,
    parsed: &Parsed,
    run: String,
    now: String,
) -> Result<(String, usize, bool)> {
    let outcome: Result<(usize, bool)> = (|| {
        let tx = conn.transaction()?;
        let previous: Vec<(String, String)> = {
            let mut stmt=tx.prepare("SELECT o.id,o.locator_json FROM observation o JOIN transform_run r ON r.id=o.run_id WHERE o.artifact_id=?1 AND r.transform_name='extract.json' AND o.id NOT IN(SELECT superseded_id FROM observation_supersession)")?;
            let rows = stmt
                .query_map([artifact], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            drop(stmt);
            rows
        };
        let mut replacements = std::collections::HashMap::new();
        for observation in &parsed.observations {
            let oid = id()?;
            let locator=json!({"kind":"json_pointer","pointer":observation.pointer,"representation":"decoded_scalar"}).to_string();
            tx.execute(
                "INSERT INTO observation VALUES(?1,?2,?3,'json.value',?4,?5,?6)",
                params![oid, artifact, run, observation.value, locator, now],
            )?;
            tx.execute(
                "INSERT INTO derivation VALUES(?1,?2,'artifact',?3,'observation',?4)",
                params![id()?, run, artifact, oid],
            )?;
            replacements.insert(locator, oid);
        }
        for (old, locator) in previous {
            tx.execute(
                "INSERT INTO observation_supersession VALUES(?1,?2,?3,?4)",
                params![old, replacements.get(&locator), run, now],
            )?;
        }
        for gap in &parsed.gaps {
            tx.execute("INSERT INTO coverage_gap VALUES(?1,?2,'artifact',?3,'json_partial',?4,1,'limitations',?5)",params![id()?,run,artifact,gap,now])?;
        }
        kokin_store::audit::append(&tx,&kokin_store::NewAuditEvent{actor:"user:local",action:"evidence.json_extracted",subject_kind:Some("artifact"),subject_id:Some(artifact),payload_json:&json!({"observations":parsed.observations.len(),"partial":!parsed.gaps.is_empty()}).to_string()})?;
        tx.commit()?;
        Ok((parsed.observations.len(), parsed.gaps.is_empty()))
    })();
    match outcome {
        Ok((count, complete)) => {
            conn.execute(
                "UPDATE transform_run SET status='succeeded',finished_utc=?2 WHERE id=?1",
                params![run, now],
            )?;
            Ok((run, count, complete))
        }
        Err(error) => {
            let _=conn.execute("UPDATE transform_run SET status='failed',finished_utc=?2,error_code='json.failed',error_detail=?3 WHERE id=?1",params![run,now,error.to_string()]);
            Err(error)
        }
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn pointers_and_normalized_scalars_are_reproducible() {
        let bytes = br#"{"a/b":{"~key":["literal <script>",1,true,null]}}"#;
        let result = parse(bytes).unwrap();
        assert_eq!(result.observations[0].pointer, "/a~1b/~0key/0");
        let root: Value = serde_json::from_slice(bytes).unwrap();
        for observation in result.observations {
            assert_eq!(
                scalar(root.pointer(&observation.pointer).unwrap()),
                observation.value
            );
        }
    }
    #[test]
    fn malformed_depth_and_oversized_values_do_not_look_complete() {
        assert!(parse(b"{broken").is_err());
        let bytes = json!({"oversized":"x".repeat(9000)}).to_string();
        let parsed = parse(bytes.as_bytes()).unwrap();
        assert!(parsed.observations.is_empty());
        assert!(!parsed.gaps.is_empty());
    }
}
