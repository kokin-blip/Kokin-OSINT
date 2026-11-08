//! Semantic search is opt-in, local and explicitly coverage-bounded.
use crate::{views::*, CommandError, ErrorCode, Session};
use kokin_ai::{
    embedding::{BgeSmall, EmbeddingProvider},
    vector::{SqliteVectorIndex, VectorIndex},
};
use rusqlite::{params, OptionalExtension};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tauri::{AppHandle, Manager};
fn error(v: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidValue, v)
}
pub fn provider(app: &AppHandle, cancel: &AtomicBool) -> Result<BgeSmall, CommandError> {
    let model = crate::resources::selected(app, "bge-model")?;
    let tokenizer = crate::resources::selected(app, "bge-tokenizer")?;
    for resource in [&model, &tokenizer] {
        crate::resources::verify(
            std::path::Path::new(&resource.destination),
            resource.bytes,
            &resource.sha256,
            cancel,
        )?;
    }
    let runtime = app
        .path()
        .resource_dir()
        .map_err(|_| error("native runtime unavailable"))?
        .join("native/onnx/libonnxruntime.1.22.0.dylib");
    #[cfg(debug_assertions)]
    let runtime = if runtime.is_file() {
        runtime
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("native/onnx/libonnxruntime.1.22.0.dylib")
    };
    BgeSmall::open(
        &runtime,
        std::path::Path::new(&model.destination),
        std::path::Path::new(&tokenizer.destination),
        &format!(
            "bge-small-int8/{}/tokenizer:{}/cls-384-v1",
            model.sha256, tokenizer.sha256
        ),
    )
    .map_err(|e| error(e.to_string()))
}
#[tauri::command]
pub async fn semantic_status(
    app: AppHandle,
    generation: u64,
) -> Result<SemanticStatusView, CommandError> {
    let present = crate::resources::selected(&app, "bge-model")?.present
        && crate::resources::selected(&app, "bge-tokenizer")?.present;
    app.state::<Session>().with_generation(generation,|open|{let indexed=open.conn.query_row("SELECT count(*) FROM observation_embedding",[],|r|r.get(0))?;let observations=open.conn.query_row("SELECT count(*) FROM observation",[],|r|r.get(0))?;let identity=open.conn.query_row("SELECT model_identity FROM embedding_meta",[],|r|r.get(0)).optional()?;let quality:bool=open.conn.query_row("SELECT quality_gate FROM embedding_meta",[],|r|r.get(0)).optional()?.unwrap_or(false);Ok(SemanticStatusView{indexed,observations,model_identity:identity,resources_present:present,quality_gate:quality,limitation:"English bge-small int8, first 512 tokens per observation. Vectors are encrypted and rebuildable; unindexed observations and unread evidence remain outside semantic coverage. Similarity orders results and is not confidence.".into()})})
}
pub fn rebuild(
    app: &AppHandle,
    id: &str,
    generation: u64,
    cancel: &Arc<AtomicBool>,
) -> Result<serde_json::Value, CommandError> {
    app.state::<crate::operations::Operations>().update(
        id,
        "Checking embedding resources and retrieval quality",
        true,
    );
    let mut provider = provider(app, cancel)?;
    let (quality_hits, quality_total) =
        kokin_ai::quality::evaluate(&mut provider).map_err(|e| error(e.to_string()))?;
    let lexical_hits = kokin_ai::quality::lexical_baseline().map_err(|e| error(e.to_string()))?;
    let quality_gate = quality_hits * 4 >= quality_total * 3 && quality_hits >= lexical_hits;
    let session = app.state::<Session>();
    let count: i64 = session.with_generation(generation, |open| {
        Ok(open
            .conn
            .query_row("SELECT count(*) FROM observation", [], |r| r.get(0))?)
    })?;
    if count > 200_000 {
        return Err(error(
            "this rebuild is bounded to 200,000 observations; lexical search remains available",
        ));
    }
    let mut vectors = Vec::with_capacity(count as usize);
    let mut offset = 0;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(error("embedding rebuild cancelled; prior index preserved"));
        }
        let batch: Vec<(String, String)> = session.with_generation(generation, |open| {
            let mut stmt = open
                .conn
                .prepare("SELECT id,value FROM observation ORDER BY id LIMIT 257 OFFSET ?1")?;
            let rows = stmt
                .query_map([offset], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            Ok(rows)
        })?;
        if batch.is_empty() {
            break;
        }
        app.state::<crate::operations::Operations>().update(
            id,
            &format!("Embedding observations: {offset} / {count}"),
            true,
        );
        offset += batch.len() as i64;
        for (oid, text) in batch {
            if cancel.load(Ordering::SeqCst) {
                return Err(error("embedding rebuild cancelled; prior index preserved"));
            }
            if text.trim().is_empty() {
                continue;
            }
            let vector = provider
                .embed(&text.chars().take(16_384).collect::<String>(), false)
                .map_err(|e| error(e.to_string()))?;
            vectors.push((
                oid,
                kokin_store::blake3_hex(text.as_bytes()),
                kokin_ai::vector::encode(&vector).map_err(|e| error(e.to_string()))?,
            ));
        }
    }
    app.state::<crate::operations::Operations>()
        .begin_publish(id)?;
    session.with_generation(generation,|open|{let current:i64=open.conn.query_row("SELECT count(*) FROM observation",[],|r|r.get(0))?;if current!=count{return Err(error("observations changed during rebuild; retry to preserve a consistent index"));}
 let tx=open.conn.transaction()?;tx.execute("DELETE FROM observation_embedding",[])?;for(oid,hash,vector)in &vectors{tx.execute("INSERT INTO observation_embedding VALUES(?1,?2,?3,?4)",params![oid,provider.identity(),hash,vector])?;}
 tx.execute("INSERT OR REPLACE INTO embedding_meta VALUES(1,?1,384,?2,?3)",params![provider.identity(),kokin_store::now_utc_rfc3339(),quality_gate])?;
 kokin_ai::vector::populate_projection(&tx).map_err(|e|error(e.to_string()))?;
 kokin_store::append_audit_event(&tx,&kokin_store::NewAuditEvent{actor:"user:local",action:"search.embeddings_rebuilt",subject_kind:None,subject_id:None,payload_json:&serde_json::json!({"indexed":vectors.len(),"model":provider.identity(),"quality_hits":quality_hits,"quality_total":quality_total}).to_string()})?;tx.commit()?;
 Ok(serde_json::json!({"indexed":vectors.len(),"quality_hits":quality_hits,"quality_total":quality_total,"lexical_hits":lexical_hits,"quality_gate":quality_gate}))})
}
#[tauri::command]
pub async fn semantic_search(
    app: AppHandle,
    generation: u64,
    request: SemanticRequest,
) -> Result<SemanticView, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        if request.version != 1
            || !matches!(request.mode.as_str(), "semantic" | "hybrid")
            || request.search.query.len() > 4096
            || request.search.offset > 1000
        {
            return Err(error("invalid bounded semantic request"));
        }
        let cancel = AtomicBool::new(false);
        let mut provider = provider(&app, &cancel)?;
        let vector = provider
            .embed(&request.search.query, true)
            .map_err(|e| error(e.to_string()))?;
        app.state::<Session>().with_generation(generation, |open| {
            let identity: Option<String> = open
                .conn
                .query_row("SELECT model_identity FROM embedding_meta", [], |r| {
                    r.get(0)
                })
                .optional()?;
            if identity.as_deref() != Some(provider.identity()) {
                return Err(error(
                    "embedding model/pipeline changed; rebuild the index before semantic search",
                ));
            }
            search_vector(open, &request, &vector)
        })
    })
    .await
    .map_err(|_| error("semantic search worker failed"))?
}
pub fn search_vector(
    open: &kokin_store::OpenCase,
    request: &SemanticRequest,
    vector: &[f32],
) -> Result<SemanticView, CommandError> {
    let quality: bool = open
        .conn
        .query_row("SELECT quality_gate FROM embedding_meta", [], |r| r.get(0))
        .optional()?
        .unwrap_or(false);
    if !quality {
        return Err(error("install the embedding resources and rebuild a quality-validated index first; lexical search remains available"));
    }
    let indexed: i64 =
        open.conn
            .query_row("SELECT count(*) FROM observation_embedding", [], |r| {
                r.get(0)
            })?;
    let nearest = SqliteVectorIndex { conn: &open.conn }
        .nearest(vector, 1000)
        .map_err(|_| error("derived vector index unavailable; rebuild it"))?;
    let mut ranked: HashMap<String, (HitView, f64)> = HashMap::new();
    let mut limited_links = false;
    for (rank, (oid, _)) in nearest.iter().enumerate() {
        let (value,kind,withdrawn):(String,String,bool)=open.conn.query_row("SELECT value,kind,EXISTS(SELECT 1 FROM observation_supersession WHERE superseded_id=observation.id) FROM observation WHERE id=?1",[oid],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        if withdrawn && !request.search.include_superseded {
            continue;
        }
        let snippet = value.chars().take(300).collect::<String>();
        let mut hits = vec![HitView {
            subject_kind: "observation".into(),
            subject_id: oid.clone(),
            title: kind.clone(),
            facet: kind,
            snippet: snippet.clone(),
            superseded: withdrawn,
            canonical_id: None,
        }];
        let mut links=open.conn.prepare("SELECT subject_kind,subject_id,role FROM evidence_link WHERE evidence_kind='observation' AND evidence_id=?1 ORDER BY subject_kind,subject_id LIMIT 256")?;
        let records = links
            .query_map([oid], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        limited_links |= records.len() > 256;
        for (kind, sid, role) in records.into_iter().take(256) {
            let row:Option<(String,String,Option<String>)>=match kind.as_str(){
 "entity"=>open.conn.query_row("SELECT e.display_name,e.type_key,m.canonical_id FROM entity e LEFT JOIN er_merge_map m ON m.entity_id=e.id AND m.active=1 WHERE e.id=?1",[&sid],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?,
 "identifier"=>open.conn.query_row("SELECT value,namespace,NULL FROM identifier WHERE id=?1",[&sid],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?,
 "relationship"=>open.conn.query_row("SELECT kind,kind,NULL FROM relationship WHERE id=?1",[&sid],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?,_=>None};
            if let Some((mut title, mut facet, canonical)) = row {
                if kind == "entity" && request.search.resolve_merged.unwrap_or(true) {
                    if let Some(ref id) = canonical {
                        (title, facet) = open.conn.query_row(
                            "SELECT display_name,type_key FROM entity WHERE id=?1",
                            [id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )?;
                    }
                }
                hits.push(HitView {
                    subject_kind: kind,
                    subject_id: sid,
                    title,
                    facet,
                    snippet: format!("{role} evidence: {snippet}"),
                    superseded: false,
                    canonical_id: if request.search.resolve_merged.unwrap_or(true) {
                        canonical
                    } else {
                        None
                    },
                });
            }
        }
        for hit in hits {
            if !request.search.kinds.is_empty() && !request.search.kinds.contains(&hit.subject_kind)
                || !request.search.facets.is_empty() && !request.search.facets.contains(&hit.facet)
            {
                continue;
            }
            let key = format!(
                "{}:{}",
                hit.subject_kind,
                hit.canonical_id.as_ref().unwrap_or(&hit.subject_id)
            );
            let score = 1.0 / (60.0 + rank as f64);
            let entry = ranked.entry(key).or_insert((hit, 0.0));
            entry.1 = entry.1.max(score);
        }
    }
    if request.mode == "hybrid" {
        let mut lexical = request.search.clone();
        lexical.offset = 0;
        lexical.limit = Some(200);
        for (rank, hit) in crate::read::search(open, &lexical)?
            .hits
            .into_iter()
            .enumerate()
        {
            let key = format!(
                "{}:{}",
                hit.subject_kind,
                hit.canonical_id.as_ref().unwrap_or(&hit.subject_id)
            );
            ranked
                .entry(key)
                .and_modify(|v| v.1 += 1.0 / (60.0 + rank as f64))
                .or_insert((hit, 1.0 / (60.0 + rank as f64)));
        }
    }
    let mut ordered: Vec<_> = ranked.into_values().collect();
    ordered.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then_with(|| a.0.subject_kind.cmp(&b.0.subject_kind))
            .then_with(|| a.0.subject_id.cmp(&b.0.subject_id))
    });
    let total = ordered.len() as i64;
    let hits = ordered
        .into_iter()
        .skip(request.search.offset)
        .take(request.search.limit.unwrap_or(50).min(200))
        .map(|r| r.0)
        .collect();
    Ok(SemanticView{results:SearchView{hits,total,coverage:crate::read::coverage(&open.conn)?},indexed,candidate_limit:1000,truncated:indexed>1000||limited_links,notice:"Bounded retrieval: up to 1,000 nearest observations and 200 lexical rows, up to 256 linked records per observation, with the existing kind/facet/withdrawal filters and canonical entity grouping. Totals describe this candidate set, not all possible semantic matches. Similarity is not evidence or confidence.".into()})
}
