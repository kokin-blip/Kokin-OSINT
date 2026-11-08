//! Readable, explicitly plaintext reports. Content is escaped, never executed.
use crate::views::ReportPayload;
use crate::{CommandError, ErrorCode};
use kokin_store::OpenCase;
use serde_json::Value;
fn invalid(message: &str) -> CommandError {
    CommandError::new(ErrorCode::InvalidValue, message)
}
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn render(
    open: &OpenCase,
    id: &str,
    format: &str,
    expected_revision: i64,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<String, CommandError> {
    let workspace = kokin_store::workspace::get(&open.conn, id)
        .map_err(|_| invalid("report could not be read"))?
        .ok_or_else(|| invalid("report not found"))?;
    if workspace.revision != expected_revision {
        return Err(invalid(
            "the report changed; reload and save its latest revision before export",
        ));
    }
    if workspace.kind != "report" || workspace.archived {
        return Err(invalid("select an active report"));
    }
    let report: ReportPayload = serde_json::from_value(workspace.payload)
        .map_err(|_| invalid("invalid report structure"))?;
    if report.version != 1
        || report.findings.is_empty()
        || report.findings.len() > 100
        || report
            .findings
            .iter()
            .any(|f| f.citations.is_empty() || f.citations.len() > 100)
    {
        return Err(invalid("each finding needs at least one case citation"));
    }
    if report.boards.len() > 20 {
        return Err(invalid(
            "include at most 20 graph illustrations in one report",
        ));
    }
    let mut sections=vec![(workspace.name,format!("As of {} · revision {}\nPlaintext analyst report. Citation verification is not a digital signature.\n{}",kokin_store::now_utc_rfc3339(),workspace.revision,report.summary))];
    let mut assembled_bytes = sections[0].0.len() + sections[0].1.len();
    for finding in report.findings {
        let mut text = finding.text;
        for citation in finding.citations {
            if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(invalid("report assembly cancelled"));
            }
            if text.len() > 8 * 1024 * 1024 {
                return Err(invalid(
                    "report exceeds 8 MiB; split it into smaller reports",
                ));
            }
            if !matches!(
                citation.role.as_str(),
                "supports" | "contradicts" | "context"
            ) {
                return Err(invalid("invalid evidence role"));
            }
            let details: Result<Value, CommandError> = match citation.kind.as_str() {
                "observation" => crate::read::observation(open, &citation.id).and_then(|v| {
                    serde_json::to_value(v).map_err(|_| invalid("citation encoding failed"))
                }),
                "entity" => crate::read::entity(open, &citation.id).and_then(|v| {
                    serde_json::to_value(v).map_err(|_| invalid("citation encoding failed"))
                }),
                "identifier" | "relationship" => {
                    crate::read::subject(open, &citation.kind, &citation.id).and_then(|v| {
                        serde_json::to_value(v).map_err(|_| invalid("citation encoding failed"))
                    })
                }
                _ => {
                    return Err(invalid(
                        "report citations must identify observations or analytical records",
                    ))
                }
            };
            text.push_str(&format!(
                "\n\n{}: {} {}\n",
                citation.role, citation.kind, citation.id
            ));
            match details{Ok(value)=>text.push_str(&citation_description(open,&citation.kind,&value,cancel)?),Err(_)=>text.push_str("Unavailable evidence or record. This citation could not be verified; no quote is asserted.")}
        }
        assembled_bytes += finding.heading.len() + text.len();
        if assembled_bytes > 8 * 1024 * 1024 {
            return Err(invalid(
                "report exceeds 8 MiB; split it into smaller reports",
            ));
        }
        sections.push((finding.heading, text));
    }
    sections.push(("Limitations and uncertainty".into(), report.limitations));
    let mut illustrations = Vec::new();
    for board in &report.boards {
        let mut value = kokin_store::workspace::get(&open.conn, board)
            .map_err(|_| invalid("board could not be read"))?;
        if let Some(ref mut board) = value {
            if board.kind == "board" {
                crate::workspace::hydrate_board(&open.conn, board)?;
                illustrations.push(graph_svg(board));
            }
        }
        sections.push((
            "Graph board reference".into(),
            value
                .map(|v| {
                    format!(
                        "{} · {} · revision {}. Layout and distance are not confidence.",
                        v.name, v.id, v.revision
                    )
                })
                .unwrap_or_else(|| "Referenced board is unavailable.".into()),
        ));
    }
    if report.map_illustrations.len() > 20 {
        return Err(invalid("include at most 20 map illustrations"));
    }
    for id in &report.map_illustrations {
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(invalid("report assembly cancelled"));
        }
        match crate::maps::illustration(open, id) {
            Ok((bytes, credit, locations)) => {
                let mut description=format!("Analyst-rendered map illustration {id}. {credit}. Basemap archive excluded from case packages; image remains recorded context, not source evidence.");
                for location in locations {
                    let oid: Result<(String,String,String,f64,f64), _> = open.conn.query_row(
                        "SELECT observation_id,uncertainty,rationale,longitude,latitude FROM grounded_location WHERE id=?1",
                        [&location],
                        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
                    );
                    if let Ok((oid, uncertainty, rationale, lon, lat)) = oid {
                        description.push_str(&format!("\nAnalyst-entered coordinates {lat}, {lon}; uncertainty: {uncertainty}; rationale: {rationale}"));
                        match crate::read::observation(open, &oid) {
                            Ok(obs) => {
                                let value = serde_json::to_value(obs)
                                    .map_err(|_| invalid("map citation encoding failed"))?;
                                description.push_str(&format!(
                                    "\nLocation {location}: {}",
                                    observation_description(&value)
                                ));
                            }
                            Err(_) => description.push_str("\nLocation source unavailable."),
                        }
                    } else {
                        description.push_str("\nReferenced grounded location unavailable.");
                    }
                }
                sections.push((
                    "Map illustration evidence and limitations".into(),
                    description,
                ));
                use base64::Engine;
                let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                illustrations.push(if format=="html"{format!("<figure><img alt=\"Analyst-rendered map illustration\" src=\"data:image/png;base64,{encoded}\"><figcaption>{}</figcaption></figure>",escape(&credit))}else{format!("\n![Analyst-rendered map illustration](data:image/png;base64,{encoded})\n{}\n",markdown_text(&credit))});
            }
            Err(_) => sections.push((
                "Map illustration unavailable".into(),
                format!(
                    "Recorded illustration {id} is unavailable or damaged. No image is asserted."
                ),
            )),
        }
    }
    let output = match format{"markdown"=>Ok(sections.into_iter().map(|(heading,text)|format!("## {}\n\n{}\n",markdown_text(&heading),markdown_text(&text))).collect::<Vec<_>>().join("\n")+&illustrations.join("\n")),"html"=>Ok(format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:\"><title>Investigation report</title><style>body{{font:16px system-ui;max-width:75ch;margin:40px auto;padding:24px}}pre{{font:inherit;white-space:pre-wrap;overflow-wrap:anywhere}}</style></head><body>{}</body></html>",sections.into_iter().map(|(heading,text)|format!("<section><h2>{}</h2><pre>{}</pre></section>",escape(&heading),escape(&text))).collect::<Vec<_>>().join("")+&illustrations.join(""))),_=>Err(invalid("choose Markdown or HTML"))}?;
    if output.len() > 8 * 1024 * 1024 {
        return Err(invalid(
            "report exceeds 8 MiB; split it into smaller reports",
        ));
    }
    Ok(output)
}
fn observation_description(value: &Value) -> String {
    let source = value["lineage"]["collection"]["source"]["raw_locator"]
        .as_str()
        .unwrap_or("source unavailable");
    let agreement = value["quote"]["agreement"]
        .as_str()
        .unwrap_or("unavailable");
    let agreement = if matches!(agreement, "derived_exact" | "structured_exact") {
        format!(
            "{agreement}: {}",
            value["quote"]["basis"].as_str().unwrap_or(
                "Transformation verification; not independently verified against source pixels."
            )
        )
    } else {
        agreement.to_owned()
    };
    let quoted = value["quote"]["excerpt"]["quoted"]
        .as_str()
        .unwrap_or("No verified excerpt available.");
    format!("Source: {source}\nObservation: {}\nRecorded value: {}\nCollected: {}\nCapture completeness: {}\n{}\nLocation: {}\nCitation verdict: {agreement}\nSource excerpt: {quoted}\n{}",value["observation_id"].as_str().unwrap_or(""),value["value"].as_str().unwrap_or(""),value["lineage"]["collection"]["capture"]["requested_utc"].as_str().unwrap_or("unrecorded"),value["lineage"]["collection"]["capture"]["capture_completeness"].as_str().unwrap_or("unrecorded"),if value["lineage"]["collection"]["capture"]["from_fixture"].as_bool()==Some(true){"Replayed fixture capture; not a live collection."}else{""},value["locator"]["raw_json"].as_str().unwrap_or("unrecorded"),if !value["superseded"].is_null(){"This observation has been superseded; review its replacement."}else{""})
}
fn citation_description(
    open: &OpenCase,
    kind: &str,
    value: &Value,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<String, CommandError> {
    if kind == "observation" {
        return Ok(observation_description(value));
    }
    let mut description = format!(
        "Record: {}\n",
        value["display_name"]
            .as_str()
            .or(value["title"].as_str())
            .unwrap_or("")
    );
    if value["redirected"].as_bool() == Some(true) {
        description.push_str(&format!("Merged alias {} now resolves to {}. Original evidence and assessments retain their record attribution.\n",value["requested_id"].as_str().unwrap_or(""),value["entity_id"].as_str().unwrap_or("")));
    }
    for field in value["fields"].as_array().into_iter().flatten() {
        if let Some(pair) = field.as_array() {
            if pair.len() == 2 {
                description.push_str(&format!(
                    "{}: {}\n",
                    pair[0].as_str().unwrap_or(""),
                    pair[1].as_str().unwrap_or("")
                ));
            }
        }
    }
    for dimension in value["confidence"].as_array().into_iter().flatten() {
        description.push_str(&format!(
            "\n{}: {}\n",
            dimension["title"].as_str().unwrap_or(""),
            dimension["agreement"].as_str().unwrap_or("unassessed")
        ));
        for assessment in dimension["values"].as_array().into_iter().flatten() {
            description.push_str(&format!(
                "{} · {} · {} · record {}\nFactors: {}\n",
                assessment["label"].as_str().unwrap_or(""),
                assessment["actor"].as_str().unwrap_or(""),
                assessment["assessed_utc"].as_str().unwrap_or(""),
                assessment["entity_id"].as_str().unwrap_or(""),
                assessment["contributing_factors_json"]
                    .as_str()
                    .unwrap_or("")
            ));
        }
    }
    for link in value["evidence"].as_array().into_iter().flatten() {
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(invalid("report assembly cancelled"));
        }
        description.push_str(&format!(
            "\n{}: {} {}\n",
            link["role"].as_str().unwrap_or(""),
            link["evidence_kind"].as_str().unwrap_or(""),
            link["evidence_id"].as_str().unwrap_or("")
        ));
        if link["evidence_kind"].as_str() == Some("observation") {
            match crate::read::observation(open, link["evidence_id"].as_str().unwrap_or("")) {
                Ok(observation) => description.push_str(&observation_description(
                    &serde_json::to_value(observation)
                        .map_err(|_| invalid("citation encoding failed"))?,
                )),
                Err(_) => {
                    description.push_str("Unavailable evidence; this quote could not be verified.")
                }
            }
        }
        if description.len() > 8 * 1024 * 1024 {
            return Err(invalid(
                "report exceeds 8 MiB; split it into smaller reports",
            ));
        }
    }
    Ok(description)
}

fn markdown_text(text: &str) -> String {
    let mut escaped = String::new();
    for character in text.chars() {
        if "\\`*_{}[]()#+-.!|".contains(character) {
            escaped.push('\\');
        }
        match character {
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '&' => escaped.push_str("&amp;"),
            _ => escaped.push(character),
        }
    }
    escaped
}
fn graph_svg(board: &kokin_store::workspace::Workspace) -> String {
    let nodes = board.payload["view"]["nodes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let edges = board.payload["view"]["edges"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut locations = std::collections::HashMap::new();
    for (i, node) in nodes.iter().take(300).enumerate() {
        let id = node["id"].as_str().unwrap_or("");
        let x = board.payload["positions"][id]["x"]
            .as_f64()
            .unwrap_or(100. + node["depth"].as_u64().unwrap_or(0) as f64 * 260.)
            .clamp(-1e6, 1e6);
        let y = board.payload["positions"][id]["y"]
            .as_f64()
            .unwrap_or(50. + i as f64 * 70.)
            .clamp(-1e6, 1e6);
        locations.insert(id, (x, y));
    }
    let left = locations.values().map(|(x, _)| x - 100.).fold(0., f64::min);
    let top = locations.values().map(|(_, y)| y - 40.).fold(0., f64::min);
    let right = locations
        .values()
        .map(|(x, _)| x + 130.)
        .fold(300., f64::max);
    let bottom = locations
        .values()
        .map(|(_, y)| y + 60.)
        .fold(200., f64::max);
    let mut graphic=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" role=\"img\" aria-label=\"{}\" viewBox=\"{} {} {} {}\" width=\"100%\"><title>{}: layout does not indicate confidence</title>",escape(&board.name),left,top,right-left,bottom-top,escape(&board.name));
    for edge in edges.iter().take(1000) {
        if let (Some((x, y)), Some((to_x, to_y))) = (
            locations.get(edge["from"].as_str().unwrap_or("")),
            locations.get(edge["to"].as_str().unwrap_or("")),
        ) {
            graphic.push_str(&format!("<line x1=\"{x}\" y1=\"{y}\" x2=\"{to_x}\" y2=\"{to_y}\" stroke=\"#526070\"/><text x=\"{}\" y=\"{}\" font-size=\"12\">{}</text>",(x+to_x)/2.,(y+to_y)/2.-15.,escape(edge["kind"].as_str().unwrap_or(""))));
        }
    }
    for node in nodes.iter().take(300) {
        if let Some((x, y)) = locations.get(node["id"].as_str().unwrap_or("")) {
            graphic.push_str(&format!("<g><rect x=\"{}\" y=\"{}\" width=\"190\" height=\"40\" rx=\"4\" fill=\"#f4f6f8\" stroke=\"#526070\"/><text x=\"{}\" y=\"{}\" font-size=\"12\">{}</text></g>",x-80.,y-20.,x-70.,y+5.,escape(node["name"].as_str().unwrap_or(""))));
        }
    }
    graphic.push_str("</svg>");
    graphic
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn citation_description_keeps_source_identity_and_fixture_status() {
        let value = serde_json::json!({"observation_id":"o","value":"claim","locator":{"raw_json":"location"},"quote":{"agreement":"exact","excerpt":{"quoted":"claim"}},"lineage":{"collection":{"source":{"raw_locator":"https://example.test/source"},"capture":{"requested_utc":"2026-10-12T00:00:00Z","capture_completeness":"static_html_only","from_fixture":true}}},"superseded":null});
        let text = observation_description(&value);
        assert!(text.contains("https://example.test/source"));
        assert!(text.contains("Replayed fixture"));
        assert!(text.contains("exact"));
    }

    #[test]
    fn hostile_markup_is_text() {
        assert_eq!(escape("<script>&\"'"), "&lt;script&gt;&amp;&quot;&#39;");
    }
}
