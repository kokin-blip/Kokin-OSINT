//! Plaintext research exports use the same evidence-aware projection as the UI.
use crate::{ExportError, Result};
use serde_json::Value;
fn string(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").into()
}
fn cell(s: &str) -> String {
    // Quote all cells and neutralize spreadsheet formula entry points, including leading whitespace.
    let dangerous = s
        .trim_start_matches(char::is_whitespace)
        .starts_with(['=', '+', '-', '@']);
    format!(
        "\"{}{}\"",
        if dangerous { "'" } else { "" },
        s.replace('"', "\"\"")
    )
}
pub fn render(conn: &rusqlite::Connection, ids: &[String], format: &str) -> Result<Vec<u8>> {
    let rows = kokin_store::research::selected(conn, ids)?;
    match format{
  "json"=>serde_json::to_vec_pretty(&serde_json::json!({"format":"kokin-research-export-v1","warning":"Plaintext export. Provider assertions and similarities are not verified identities. Evidence IDs refer to this case.","results":rows})).map_err(ExportError::Encode),
  "csv"=>{
   let mut csv=String::from("result_id,run_id,selector_type,original,normalized,source,outcome,url,observed_utc,source_reported_utc,indexed_utc,artifact_id,from_cache,fields_json,limitation\r\n");
   for row in rows{
    let mut fields=vec![string(&row,"id"),string(&row,"run_id"),string(&row["selector"],"kind"),string(&row["selector"],"original"),string(&row["selector"],"normalized")];
    for key in ["source","outcome","url","observed_utc","source_reported_utc","indexed_utc","artifact_id"]{fields.push(string(&row,key));}
    fields.push(row["from_cache"].to_string());fields.push(row["fields"].to_string());fields.push(string(&row,"limitation"));
    csv.push_str(&fields.iter().map(|s|cell(s)).collect::<Vec<_>>().join(","));csv.push_str("\r\n");
   }Ok(csv.into_bytes())
  },_=>Err(ExportError::Malformed("choose CSV or JSON result export".into()))
 }
}
/// Atomically publish a complete plaintext export without replacing an existing file.
pub fn publish(destination: &std::path::Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    pending.write_all(bytes)?;
    pending.as_file().sync_all()?;
    pending
        .persist_noclobber(destination)
        .map_err(|e| ExportError::Io(e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_preserves_quotes_and_neutralizes_spreadsheet_execution() {
        assert_eq!(cell("a,\"b\""), "\"a,\"\"b\"\"\"");
        assert_eq!(
            cell(" \t=HYPERLINK(\"x\")"),
            "\"' \t=HYPERLINK(\"\"x\"\")\""
        );
        assert_eq!(cell("ordinary"), "\"ordinary\"");
    }
}
