//! Explicitly authorized, evidence-first research. Network work runs outside case locks.
use crate::{operations::Operations, CommandError, ErrorCode, Session};
use kokin_connectors::research::{
    self, CheckOutcome, CheckPlan, ResearchPlan, ResearchRequest, ResearchResult, Selector,
    SelectorKind,
};
use kokin_net::HttpCapability;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
fn invalid(s: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidValue, s)
}
fn encode<T: serde::Serialize>(v: T) -> Result<Value, CommandError> {
    serde_json::to_value(v).map_err(|_| invalid("research encoding failed"))
}
struct Prepared {
    plan: ResearchPlan,
    checks: Vec<CheckPlan>,
    digest: String,
    expires: Instant,
}
#[derive(Default)]
pub struct ResearchPlans {
    plans: Mutex<HashMap<String, Prepared>>,
}
impl ResearchPlans {
    pub fn clear(&self) {
        if let Ok(mut p) = self.plans.lock() {
            p.clear();
        }
    }
    fn take(&self, token: &str, digest: &str, generation: u64) -> Result<Prepared, CommandError> {
        let p = self
            .plans
            .lock()
            .map_err(|_| invalid("plan registry unavailable"))?
            .remove(token)
            .ok_or_else(|| invalid("plan expired or already used"))?;
        if p.expires < Instant::now() || p.plan.generation != generation || p.digest != digest {
            return Err(invalid(
                "plan changed or belongs to a closed case; review again",
            ));
        }
        Ok(p)
    }
}
#[tauri::command]
pub fn research_import_session(
    session: tauri::State<'_, Session>,
    generation: u64,
    id: String,
    offset: usize,
) -> Result<Value, CommandError> {
    session.with_generation(generation, |open| {
        crate::research_import::state(open, &id, offset)
    })
}
/// First-party fallback: a bounded phrase search over this case's existing index.
/// No socket, credential lookup, model or external corpus participates.
#[tauri::command]
pub fn research_local_search(
    session: tauri::State<'_, Session>,
    generation: u64,
    selectors: Vec<Selector>,
    offset: usize,
) -> Result<Value, CommandError> {
    session.with_generation(generation, |open| local_search(open, selectors, offset))
}
pub fn local_search(
    open: &kokin_store::OpenCase,
    selectors: Vec<Selector>,
    offset: usize,
) -> Result<Value, CommandError> {
    if selectors.is_empty() || selectors.len() > 15 || offset > 1_000_000 {
        return Err(invalid("choose 1–15 local queries within the page bound"));
    }

    let mut results = vec![];
    for raw in selectors {
        let selector =
            Selector::parse(raw.kind, &raw.original).map_err(|e| invalid(e.to_string()))?;
        if selector.normalized != raw.normalized {
            return Err(invalid("selector changed; validate again"));
        }
        if selector.kind == SelectorKind::Image {
            results.push(json!({"selector":selector,"status":"unsupported","hits":[],"total":0,"limitation":"Use the existing local media/face workspace. Text search cannot compare image content."}));
            continue;
        }
        let request = crate::SearchRequest {
            query: format!("\"{}\"", selector.normalized.replace('"', " ")),
            offset,
            limit: Some(50),
            ..Default::default()
        };
        let found = crate::read::search(open, &request)?;
        let mut hits = vec![];
        for hit in found.hits {
            let artifact: Option<String> = if hit.subject_kind == "observation" {
                open.conn
                    .query_row(
                        "SELECT artifact_id FROM observation WHERE id=?1",
                        [&hit.subject_id],
                        |r| r.get(0),
                    )
                    .ok()
            } else {
                None
            };
            hits.push(json!({"subject_kind":hit.subject_kind,"subject_id":hit.subject_id,"title":hit.title,"snippet":hit.snippet,"artifact_id":artifact}));
        }
        results.push(json!({"selector":selector,"status":"local_leads","hits":hits,"total":found.total,"coverage":found.coverage,"limitation":"Tokenized phrase matches in this unlocked case only. Index leads require source inspection; no account, breach membership, identity or web-wide absence is inferred."}));
    }
    if offset == 0 {
        let selectors = results
            .iter()
            .map(|r| r["selector"].clone())
            .collect::<Vec<_>>();
        kokin_store::research::save_document(
            &open.conn,
            &kokin_store::research::Document {
                id: crate::operations::random_id()?,
                revision: 0,
                kind: "saved_search".into(),
                name: "Local research query".into(),
                payload: json!({"version":1,"purpose":"local_query","selectors":selectors,"external_requests":0}),
                archived: false,
                updated_utc: String::new(),
            },
        )?;
    }
    Ok(
        json!({"version":1,"delivery":"native","scope":"current encrypted case index","external_requests":0,"offset":offset,"limit":50,"queries":results}),
    )
}

#[tauri::command]
pub fn research_registry() -> Result<Value, CommandError> {
    encode(research::registry())
}
#[tauri::command]
pub fn research_directory() -> Result<Value, CommandError> {
    encode(kokin_connectors::directory::tools())
}
#[tauri::command]
pub fn research_selector(kind: SelectorKind, value: String) -> Result<Value, CommandError> {
    encode(Selector::parse(kind, &value).map_err(|e| invalid(e.to_string()))?)
}
#[tauri::command]
pub fn research_bulk(text: String, kind: SelectorKind) -> Result<Value, CommandError> {
    encode(research::parse_bulk(&text, kind).map_err(|e| invalid(e.to_string()))?)
}
#[tauri::command]
pub fn research_table(text: String, format: String, header: bool) -> Result<Value, CommandError> {
    encode(
        kokin_connectors::tabular::parse(&text, &format, header)
            .map_err(|e| invalid(e.to_string()))?,
    )
}
#[tauri::command]
pub fn research_table_map(
    table: kokin_connectors::tabular::Table,
    column: usize,
    kind: SelectorKind,
    columns: Option<Vec<usize>>,
) -> Result<Value, CommandError> {
    // Revalidate inbound preview size before mapping; previews never authorize a request.
    let size = serde_json::to_vec(&table)
        .map_err(|_| invalid("invalid table"))?
        .len();
    if size > 4 * 1024 * 1024 {
        return Err(invalid("table preview exceeds bound"));
    }
    encode(
        kokin_connectors::tabular::mapped_columns(
            &table,
            &columns.unwrap_or_else(|| vec![column]),
            kind,
        )
        .map_err(|e| invalid(e.to_string()))?,
    )
}
#[tauri::command]
pub fn research_table_suggestions(
    table: kokin_connectors::tabular::Table,
) -> Result<Value, CommandError> {
    encode(kokin_connectors::tabular::suggestions(&table).map_err(|e| invalid(e.to_string()))?)
}
#[tauri::command]
pub fn research_table_candidates(
    table: kokin_connectors::tabular::Table,
    column: usize,
) -> Result<Value, CommandError> {
    encode(
        kokin_connectors::tabular::candidates(&table, column)
            .map_err(|e| invalid(e.to_string()))?,
    )
}
#[tauri::command]
pub fn research_candidates(text: String) -> Result<Value, CommandError> {
    encode(
        kokin_connectors::candidates::extract(&[kokin_connectors::candidates::Segment {
            origin_id: "local-paste".into(),
            text,
        }])
        .map_err(|e| invalid(e.to_string()))?,
    )
}
#[tauri::command]
pub async fn research_evidence_preview(
    app: AppHandle,
    generation: u64,
    artifact_id: String,
) -> Result<Value, CommandError> {
    tauri::async_runtime::spawn_blocking(move || {
        let session = app.state::<Session>();
        let input = session.with_generation(generation, |open| {
            crate::research_preview::prepare(open, &artifact_id)
        })?;
        let output = crate::research_preview::process(input)?;
        session.with_generation(generation, |open| {
            crate::research_preview::validate_publication(open, &output)?;
            encode(output.preview)
        })
    })
    .await
    .map_err(|_| invalid("evidence preview worker failed"))?
}
#[tauri::command]
pub fn research_import_preview(
    app: AppHandle,
    generation: u64,
    selection: String,
    format: String,
    header: bool,
) -> Result<Value, CommandError> {
    use std::io::Read;
    app.state::<Session>()
        .with_generation(generation, |_| Ok(()))?;
    let path = app
        .state::<Operations>()
        .consume(&selection, "input", generation)?;
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .map_err(|_| invalid("selected file unavailable"))?
        .take(2_000_001)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid("selected file unreadable"))?;
    if bytes.len() > 2_000_000 {
        return Err(invalid("bulk preview exceeds 2 MB"));
    }
    let text = String::from_utf8(bytes).map_err(|_| {
        invalid("text import must be UTF-8; PDF text must be processed through Evidence")
    })?;
    app.state::<Session>()
        .with_generation(generation, |_| Ok(()))?;
    research_table(text, format, header)
}
#[tauri::command]
pub fn research_open_url(url: String) -> Result<(), CommandError> {
    let url = kokin_net::browser_handoff(&url).map_err(|_| {
        invalid("only HTTP(S) destinations without credentials can open externally")
    })?;
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("/usr/bin/open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = std::process::Command::new("xdg-open");
    let status = command
        .arg(url)
        .status()
        .map_err(|_| invalid("external browser could not be opened"))?;
    if !status.success() {
        return Err(invalid("external browser could not be opened"));
    }
    Ok(())
}
#[tauri::command]
pub fn research_provider_status(provider: String) -> Result<Value, CommandError> {
    kokin_net::provider::endpoint(&provider).map_err(|_| invalid("unknown provider"))?;
    Ok(
        json!({"provider":provider,"configured":kokin_net::provider::credential(&provider).is_ok(),"credential_account":if provider=="osint_industries_credits"{"osint_industries"}else{provider.as_str()},"supported":cfg!(target_os="macos"),"usage_state":"not_checked","eligibility_state":"unverified","capability_state":"offline_contract_only","service":"com.kokin.osint","storage":"macOS Keychain; API keys never enter the webview"}),
    )
}
#[tauri::command]
pub fn research_prepare(
    app: AppHandle,
    generation: u64,
    request: ResearchRequest,
    input_origins: Option<Vec<research::InputOrigin>>,
    parent_import: Option<research::ImportParent>,
) -> Result<Value, CommandError> {
    app.state::<Session>()
        .with_generation(generation, |_| Ok(()))?;
    let (mut plan, mut checks) =
        research::checks(&request, generation).map_err(|e| invalid(e.to_string()))?;
    if let Some(parent) = &parent_import {
        app.state::<Session>().with_generation(generation, |o| {
            crate::research_import::validate_parent(o, parent, &plan.selectors)
        })?;
    }
    plan.parent_import = parent_import;
    if plan
        .sources
        .iter()
        .any(|source| source == "osint_industries_credits")
    {
        plan.limitation.push_str(" The credit-balance lookup makes one authenticated GET per batch and sends no selector or image. It does not search these queries, reserve credits or establish account/region eligibility.");
    }
    if plan.sources.iter().any(|source| source == "searxng") {
        plan.limitation.push_str(" Local SearXNG sends the query to its configured upstream engines. This plan bounds app-to-service calls only; engine fanout and any user-configured upstream billing are outside that ceiling. Destination pages are not collected by this query.");
    }
    plan.input_origins = input_origins.unwrap_or_default();
    app.state::<Session>().with_generation(generation, |open| {
        crate::research_preview::validate_origins(open, &plan.selectors, &plan.input_origins)
    })?;
    for (index, c) in checks.iter_mut().enumerate() {
        if c.manifest.delivery == research::Delivery::ProviderBacked {
            kokin_net::provider::credential(&c.source).map_err(|_|invalid(format!("Add the {} credential to macOS Keychain using service com.kokin.osint and account {} before preparing this request",c.manifest.title,c.source)))?;
            if matches!(c.source.as_str(), "lenso" | "eyematch")
                && c.selector.kind == SelectorKind::Image
            {
                app.state::<Session>().with_generation(generation, |o| {
                    let outgoing=image_request(o,c)?;
                    plan.request_digests[index]=outgoing.fixture_key().map_err(|_|invalid("image request digest unavailable"))?;
                    if !plan.image_disclosures.iter().any(|v|v.artifact_id==c.selector.normalized){
                     let image=o.conn.query_row("SELECT id,content_hash,media_type,byte_length FROM artifact WHERE id=?1",[&c.selector.normalized],|r|Ok(research::ImageDisclosure{artifact_id:r.get(0)?,content_hash:r.get(1)?,media_type:r.get(2)?,byte_length:r.get::<_,i64>(3)? as u64}))?;
                     plan.image_disclosures.push(image);
                    }
                    Ok(())
                })?;
            }
        }
        c.authorized_request_digest = Some(plan.request_digests[index].clone());
    }
    let digest = kokin_store::blake3_hex(
        &serde_json::to_vec(&plan).map_err(|_| invalid("plan encoding failed"))?,
    );
    let token = crate::operations::random_id()?;
    let state = app.state::<ResearchPlans>();
    let mut plans = state
        .plans
        .lock()
        .map_err(|_| invalid("plan registry unavailable"))?;
    // Check while holding the plan registry; close_case clears it again after key disposal.
    app.state::<Session>()
        .with_generation(generation, |_| Ok(()))?;
    plans.retain(|_, p| p.expires > Instant::now() && p.plan.generation == generation);
    if plans.len() >= 16 {
        return Err(invalid("too many pending plans"));
    }
    plans.insert(
        token.clone(),
        Prepared {
            plan: plan.clone(),
            checks,
            digest: digest.clone(),
            expires: Instant::now() + Duration::from_secs(300),
        },
    );
    Ok(json!({"token":token,"digest":digest,"plan":plan}))
}
#[tauri::command]
pub fn research_runs(
    session: tauri::State<'_, Session>,
    generation: u64,
    offset: usize,
) -> Result<Value, CommandError> {
    session.with_generation(generation, |o| {
        encode(kokin_store::research::runs(&o.conn, offset)?)
    })
}
#[tauri::command]
pub fn research_results(
    session: tauri::State<'_, Session>,
    generation: u64,
    run_id: String,
    offset: usize,
    filter: Option<kokin_store::research::ResultFilter>,
) -> Result<Value, CommandError> {
    session.with_generation(generation, |o| {
        encode(kokin_store::research::filtered_results(
            &o.conn,
            &run_id,
            offset,
            &filter.unwrap_or_default(),
        )?)
    })
}
/// Prepare a grounded proposal only; analytical writes remain in the existing review form.
pub fn review_candidate(
    open: &kokin_store::OpenCase,
    result_id: &str,
) -> Result<Value, CommandError> {
    let row = kokin_store::research::selected(&open.conn, &[result_id.to_owned()])?
        .into_iter()
        .next()
        .ok_or_else(|| invalid("research result is unavailable"))?;
    let result: ResearchResult = serde_json::from_value(row)
        .map_err(|_| invalid("research result contract is unsupported"))?;
    if result.outcome != CheckOutcome::Found {
        return Err(invalid(
            "only a grounded positive result can stage this proposal",
        ));
    }
    let (key, entity_type, namespace) = match result.source.as_str() {
        "github" | "codeberg" => (
            "login",
            "social_account",
            format!("{}:username", result.source),
        ),
        "hackernews" => ("id", "social_account", "hackernews:username".into()),
        "rdap" => ("ldhName", "domain", "domain".into()),
        _ => {
            return Err(invalid(
                "this source has no reviewed proposal adapter; inspect its evidence manually",
            ))
        }
    };
    let identity = result.fields[key]
        .as_str()
        .ok_or_else(|| invalid("source identity field is unavailable"))?;
    let artifact = result
        .artifact_id
        .as_deref()
        .ok_or_else(|| invalid("captured source is required"))?;
    let pointer = format!("/{key}");
    let mut query=open.conn.prepare("SELECT o.id FROM observation o WHERE o.artifact_id=?1 AND o.kind='json.value' AND o.value=?2 AND json_extract(o.locator_json,'$.pointer')=?3 AND NOT EXISTS(SELECT 1 FROM observation_supersession s WHERE s.superseded_id=o.id) ORDER BY o.rowid DESC LIMIT 1")?;
    let oid: String = query
        .query_row(rusqlite::params![artifact, identity, pointer], |r| r.get(0))
        .map_err(|_| {
            invalid(
                "current located identity observation is missing; inspect or re-extract the source",
            )
        })?;
    let observation = crate::read::observation(open, &oid)?;
    if !matches!(
        observation.quote,
        crate::views::QuoteView::StructuredExact { .. }
    ) {
        return Err(invalid(
            "the source identity observation cannot be authenticated",
        ));
    }
    Ok(
        json!({"result_id":result_id,"proposed_name":if entity_type=="domain" {identity.to_owned()}else{format!("{} account · {identity}",result.source)},"proposed_type":entity_type,"namespace":namespace,"value":identity,"source":result.source,"url":result.url,"cited_observation_ids":[oid],"limitation":"Public source assertion for human review. This stages no identity link, entity, identifier or relationship; decide its meaning in the existing review form."}),
    )
}
/// Stage one provider profile, preserving its raw located identifier assertion.
pub fn review_card_candidate(
    open: &kokin_store::OpenCase,
    result_id: &str,
    index: usize,
) -> Result<Value, CommandError> {
    let row = kokin_store::research::selected(&open.conn, &[result_id.into()])?
        .into_iter()
        .next()
        .ok_or_else(|| invalid("result unavailable"))?;
    if row["source"] != "osint_industries" || row["outcome"] != "found" || index >= 200 {
        return Err(invalid(
            "this result has no reviewed provider account adapter",
        ));
    }
    let card = &row["fields"]["cards"][index];
    if card["kind"] != "account" || card["provider_assertion"] != true {
        return Err(invalid("select a typed provider account assertion"));
    }
    let module = card["module"]
        .as_str()
        .ok_or_else(|| invalid("provider module unavailable"))?;
    let (key, identity) = ["username", "id", "email", "phone"]
        .iter()
        .find_map(|key| {
            let v = &card["fields"][*key];
            v.as_str()
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .or_else(|| v.as_i64().map(|n| n.to_string()))
                .map(|v| (*key, v))
        })
        .ok_or_else(|| {
            invalid("no supported located account identifier; inspect the evidence manually")
        })?;
    let pointer = card["origins"][key]
        .as_str()
        .ok_or_else(|| invalid("provider field lacks an evidence pointer"))?;
    let artifact = row["artifact_id"]
        .as_str()
        .ok_or_else(|| invalid("captured source required"))?;
    let oid:String=open.conn.query_row("SELECT o.id FROM observation o WHERE o.artifact_id=?1 AND o.kind='json.value' AND o.value=?2 AND json_extract(o.locator_json,'$.pointer')=?3 AND NOT EXISTS(SELECT 1 FROM observation_supersession s WHERE s.superseded_id=o.id) ORDER BY o.rowid DESC LIMIT 1",rusqlite::params![artifact,identity,pointer],|r|r.get(0)).map_err(|_|invalid("current located provider identifier is not indexed; inspect raw evidence"))?;
    if !matches!(
        crate::read::observation(open, &oid)?.quote,
        crate::views::QuoteView::StructuredExact { .. }
    ) {
        return Err(invalid("provider identifier cannot be authenticated"));
    }
    Ok(
        json!({"result_id":result_id,"proposed_name":format!("{module} account · {identity}"),"proposed_type":"social_account","namespace":format!("{module}:{key}"),"value":identity,"source":"osint_industries","url":card["fields"]["profile_url"].as_str().unwrap_or(""),"cited_observation_ids":[oid],"limitation":"Provider assertion for human review only. The account assertion does not establish ownership, a person, breach membership or an identity link. Staging performs no analytical write."}),
    )
}
#[tauri::command]
pub fn research_review_candidate(
    session: tauri::State<'_, Session>,
    generation: u64,
    result_id: String,
    card_index: Option<usize>,
) -> Result<Value, CommandError> {
    session.with_generation(generation, |open| {
        if let Some(index) = card_index {
            review_card_candidate(open, &result_id, index)
        } else {
            review_candidate(open, &result_id)
        }
    })
}
#[tauri::command]
pub fn research_documents(
    session: tauri::State<'_, Session>,
    generation: u64,
    kind: String,
) -> Result<Value, CommandError> {
    session.with_generation(generation, |o| {
        encode(kokin_store::research::documents(&o.conn, &kind)?)
    })
}
#[tauri::command]
pub fn research_document_save(
    session: tauri::State<'_, Session>,
    generation: u64,
    document: kokin_store::research::Document,
) -> Result<Value, CommandError> {
    session.with_generation(generation, |o| {
        if document.payload["purpose"] == "import_session" {
            crate::research_import::validate(o, &document)?;
        }
        encode(kokin_store::research::save_document(&o.conn, &document)?)
    })
}
pub fn cache_key(c: &CheckPlan) -> String {
    kokin_store::blake3_hex(
        format!(
            "research-v3:{}:{:?}:{}:{}:{}",
            c.source,
            c.selector.kind,
            c.selector.normalized,
            serde_json::to_string(&c.request).unwrap_or_default(),
            serde_json::to_string(&c.manifest.profile_rule).unwrap_or_default()
        )
        .as_bytes(),
    )
}
/// Reusable answers require their captured encrypted source to authenticate now.
pub fn authenticated_cache(
    open: &kokin_store::OpenCase,
    key: &str,
) -> Result<Option<Value>, CommandError> {
    authenticated_cache_for(open, key, 8 * 3600)
}
fn authenticated_cache_for(
    open: &kokin_store::OpenCase,
    key: &str,
    max_age_seconds: u32,
) -> Result<Option<Value>, CommandError> {
    let Some(value) = kokin_store::research::cached_for(&open.conn, key, max_age_seconds)? else {
        return Ok(None);
    };
    let Some(artifact) = value["artifact_id"].as_str() else {
        return Ok(None);
    };
    let hash: String = open.conn.query_row(
        "SELECT content_hash FROM artifact WHERE id=?1",
        [artifact],
        |r| r.get(0),
    )?;
    let kokin_store::BlobAccess::Readable(reference) = kokin_store::blob_access(&open.conn, &hash)?
    else {
        return Ok(None);
    };
    if reference.size > 8 * 1024 * 1024 {
        return Ok(None);
    };
    if kokin_blob::BlobStore::new(open.paths.blobs())
        .get(&reference, &open.cmk)
        .is_err()
    {
        return Ok(None);
    };
    Ok(Some(value))
}
fn image_request(
    open: &mut kokin_store::OpenCase,
    c: &CheckPlan,
) -> Result<kokin_net::HttpRequest, CommandError> {
    use base64::Engine;
    let (media, hash): (String, String) = open
        .conn
        .query_row(
            "SELECT media_type,content_hash FROM artifact WHERE id=?1",
            [&c.selector.normalized],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(kokin_store::StoreError::from)?;
    if !matches!(media.as_str(), "image/jpeg" | "image/png" | "image/webp") {
        return Err(invalid("provider upload requires an imported JPEG, PNG or WEBP; prepare other formats locally first"));
    }
    let kokin_store::BlobAccess::Readable(reference) = kokin_store::blob_access(&open.conn, &hash)?
    else {
        return Err(invalid("query image is unavailable or shredded"));
    };
    if reference.size > 10 * 1024 * 1024 {
        return Err(invalid(
            "provider query image exceeds 10 MiB; prepare a smaller derived image",
        ));
    }
    let bytes = kokin_blob::BlobStore::new(open.paths.blobs())
        .get(&reference, &open.cmk)
        .map_err(|_| invalid("query image could not be authenticated"))?;
    let mut request = c.request.clone();
    let mut body: Value = serde_json::from_str(request.body.as_deref().unwrap_or("{}"))
        .map_err(|_| invalid("image request invalid"))?;
    body.as_object_mut()
        .ok_or_else(|| invalid("image request is not an object"))?
        .remove("image_artifact_id");
    body["image"] = base64::engine::general_purpose::STANDARD
        .encode(bytes)
        .into();
    request.body = Some(body.to_string());
    Ok(request)
}
/// Offline contract entry point; capture always precedes classification.
pub fn publish_response(
    open: &mut kokin_store::OpenCase,
    run: &str,
    check: &CheckPlan,
    response: &kokin_net::HttpResponse,
) -> Result<ResearchResult, CommandError> {
    let prepared = prepare_response(check, response);
    commit_response(open, run, check, response, prepared)
}
struct PreparedResponse {
    parsed: Option<kokin_extract::json::Parsed>,
    outcome: CheckOutcome,
    url: String,
    fields: Value,
}
fn prepare_response(check: &CheckPlan, response: &kokin_net::HttpResponse) -> PreparedResponse {
    let bytes = response
        .body_bytes
        .as_deref()
        .unwrap_or(response.body.as_bytes());
    let parsed = kokin_extract::json::parse(bytes).ok();
    let (outcome, url, mut fields) = research::classify(&check.source, response, &check.selector);
    let cards = kokin_connectors::enrichment::native_cards(&check.source, &fields);
    if !cards.is_empty() && outcome == CheckOutcome::Found {
        fields["cards"] = cards.into();
    }
    if fields.to_string().len() > 64 * 1024 {
        fields = json!({"coverage":"Structured result exceeds the 64 KiB card limit. Inspect the complete captured source evidence."});
    }
    PreparedResponse {
        parsed,
        outcome,
        url,
        fields,
    }
}
fn commit_response(
    open: &mut kokin_store::OpenCase,
    run: &str,
    check: &CheckPlan,
    response: &kokin_net::HttpResponse,
    prepared: PreparedResponse,
) -> Result<ResearchResult, CommandError> {
    let blobs = kokin_blob::BlobStore::new(open.paths.blobs());
    let context = kokin_ingest::IngestContext {
        connector: &check.source,
        job_id: run,
    };
    let captured = if matches!(check.source.as_str(), "lenso" | "eyematch")
        && check.selector.kind == SelectorKind::Image
    {
        kokin_ingest::ingest_image_response(
            &mut open.conn,
            &blobs,
            &open.cmk,
            &check.request,
            response,
            &context,
            &check.selector.normalized,
        )?
    } else {
        kokin_ingest::ingest_response(
            &mut open.conn,
            &blobs,
            &open.cmk,
            &check.request,
            response,
            &context,
            "structured_response_only",
        )?
    };
    let extracted = prepared.parsed.as_ref().is_some_and(|parsed| {
        kokin_extract::json::commit(&mut open.conn, &captured.artifact_id, parsed).is_ok()
    });
    let PreparedResponse {
        outcome,
        url,
        fields,
        ..
    } = prepared;
    let limitation = format!(
        "{}{}{}",
        check.manifest.limitation,
        if response.from_fixture {
            " Fixture replay, not live."
        } else {
            ""
        },
        if extracted {
            ""
        } else {
            " Raw evidence preserved; JSON extraction unavailable."
        }
    );
    let result = ResearchResult {
        id: crate::operations::random_id()?,
        run_id: run.into(),
        selector: check.selector.clone(),
        source: check.source.clone(),
        outcome,
        url,
        fields,
        observed_utc: kokin_store::now_utc_rfc3339(),
        source_reported_utc: None,
        indexed_utc: None,
        artifact_id: Some(captured.artifact_id),
        from_cache: false,
        limitation,
    };
    kokin_store::research::append(
        &open.conn,
        run,
        &result.id,
        &cache_key(check),
        &encode(&result)?,
    )?;
    Ok(result)
}
fn execute(
    app: &AppHandle,
    run: &str,
    generation: u64,
    c: &CheckPlan,
    rescan: bool,
    cancel: &Arc<AtomicBool>,
    online: &Arc<AtomicBool>,
) -> Result<(), CommandError> {
    if cancel.load(Ordering::SeqCst) || !online.load(Ordering::SeqCst) {
        return Err(invalid("research cancelled or offline"));
    }
    if let Some(reason) = &c.validation_error {
        return app.state::<Session>().with_generation(generation, |o| {
            let result = ResearchResult {
                id: crate::operations::random_id()?,
                run_id: run.into(),
                selector: c.selector.clone(),
                source: c.source.clone(),
                outcome: CheckOutcome::Invalid,
                url: String::new(),
                fields: json!({}),
                observed_utc: kokin_store::now_utc_rfc3339(),
                source_reported_utc: None,
                indexed_utc: None,
                artifact_id: None,
                from_cache: false,
                limitation: reason.clone(),
            };
            kokin_store::research::append(
                &o.conn,
                run,
                &result.id,
                &cache_key(c),
                &encode(result.clone())?,
            )?;
            Ok(())
        });
    }
    if !rescan && c.manifest.delivery == research::Delivery::Native {
        if let Some(mut v) = app.state::<Session>().with_generation(generation, |o| {
            authenticated_cache_for(o, &cache_key(c), c.manifest.cache_max_age_seconds)
        })? {
            v["id"] = crate::operations::random_id()?.into();
            v["run_id"] = run.into();
            v["from_cache"] = true.into();
            return app.state::<Session>().with_generation(generation, |o| {
                if cancel.load(Ordering::SeqCst) {
                    return Err(invalid("cancelled"));
                }
                kokin_store::research::append(
                    &o.conn,
                    run,
                    v["id"]
                        .as_str()
                        .ok_or_else(|| invalid("cached result ID"))?,
                    &cache_key(c),
                    &v,
                )?;
                Ok(())
            });
        }
    }
    let hosts = c
        .manifest
        .destinations
        .iter()
        .map(|v| {
            kokin_net::normalise(v)
                .map(|u| u.host)
                .map_err(|_| invalid("declared origin invalid"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let policy = kokin_net::EgressPolicy {
        allowed_hosts: hosts,
        max_response_bytes: c.manifest.max_response_bytes,
        requests_per_second: 1.0,
        burst: 1,
    };
    let case_id = app
        .state::<Session>()
        .view()?
        .case
        .ok_or_else(CommandError::no_case_open)?
        .case_id;
    let live = kokin_net::live::LiveHttp::new(
        policy.clone(),
        kokin_net::Attribution {
            connector: c.source.clone(),
            job_id: run.into(),
            case_id: case_id.clone(),
        },
        online.clone(),
        cancel.clone(),
        Arc::new(Mutex::new(kokin_net::RateLimiter::new(&c.source, &policy))),
    )
    .without_redirects();
    #[cfg(feature = "desktop-test")]
    let fixture = app
        .state::<Operations>()
        .test_state
        .lock()
        .map_err(|_| invalid("test fixtures unavailable"))?
        .replay
        .clone();
    #[cfg(not(feature = "desktop-test"))]
    let fixture: Option<std::path::PathBuf> = None;
    let outgoing = if c.manifest.delivery == research::Delivery::ProviderBacked
        && matches!(c.source.as_str(), "lenso" | "eyematch")
        && c.selector.kind == SelectorKind::Image
    {
        app.state::<Session>()
            .with_generation(generation, |o| image_request(o, c))?
    } else {
        c.request.clone()
    };
    if c.authorized_request_digest
        .as_ref()
        .is_none_or(|digest| outgoing.fixture_key().as_ref() != Ok(digest))
    {
        return Err(invalid(
            "transmitted request differs from the authorized plan; review again",
        ));
    }
    let response = if let Some(path) = fixture {
        kokin_net::ReplayHttp::new(path).send(&outgoing)
    } else if c.source == "searxng" {
        let url = tauri::Url::parse(&outgoing.url)
            .map_err(|_| invalid("local discovery URL is invalid"))?;
        let query = url
            .query_pairs()
            .find(|(k, _)| k == "q")
            .map(|(_, v)| v.into_owned())
            .ok_or_else(|| invalid("local discovery query is missing"))?;
        kokin_net::discovery::search_response(&query, 1, cancel, online)
            .map(|(_, response)| response)
            .map_err(|_| {
                kokin_net::HttpError::Refused(
                    "local SearXNG is unavailable or refused the request".into(),
                )
            })
    } else if c.manifest.delivery == research::Delivery::ProviderBacked {
        kokin_net::provider::ProviderHttp {
            provider: c.source.clone(),
            attribution: kokin_net::Attribution {
                connector: c.source.clone(),
                job_id: run.into(),
                case_id,
            },
            online: online.clone(),
            cancel: cancel.clone(),
        }
        .send(&outgoing)
    } else {
        live.send(&outgoing)
    };
    if cancel.load(Ordering::SeqCst) || !online.load(Ordering::SeqCst) {
        return Err(invalid("cancelled before publication"));
    }
    let processed = response
        .as_ref()
        .ok()
        .map(|response| prepare_response(c, response));
    app.state::<Session>().with_generation(generation,|o|{
  if cancel.load(Ordering::SeqCst){return Err(invalid("cancelled before commit"));}
  match response{Ok(response)=>{let mut sent=c.clone();sent.request=outgoing;commit_response(o,run,&sent,&response,processed.ok_or_else(||invalid("response processing unavailable"))?)?;},Err(_)=>{
   let r=ResearchResult{id:crate::operations::random_id()?,run_id:run.into(),selector:c.selector.clone(),source:c.source.clone(),outcome:CheckOutcome::Error,url:String::new(),fields:json!({}),observed_utc:kokin_store::now_utc_rfc3339(),source_reported_utc:None,indexed_utc:None,artifact_id:None,from_cache:false,limitation:"Request failed, timed out, exceeded limits, or was refused. No existence assertion or automatic retry.".into()};
   kokin_store::research::append(&o.conn,run,&r.id,&cache_key(c),&encode(&r)?)?;
  }}Ok(())
 })
}
pub fn export(
    app: &AppHandle,
    job: &str,
    generation: u64,
    args: &Value,
    cancel: &Arc<AtomicBool>,
) -> Result<Value, CommandError> {
    if args["plaintext_consent"] != true {
        return Err(invalid(
            "confirm the selected findings will leave encryption as plaintext",
        ));
    }
    let ids: Vec<String> = serde_json::from_value(args["result_ids"].clone())
        .map_err(|_| invalid("select result IDs"))?;
    let format = args["format"].as_str().unwrap_or("");
    let selection = args["destination"]
        .as_str()
        .ok_or_else(|| invalid("choose an export destination"))?;
    let path = app
        .state::<Operations>()
        .consume(selection, "output", generation)?;
    let bytes = app.state::<Session>().with_generation(generation, |o| {
        kokin_export::research::render(&o.conn, &ids, format).map_err(|e| invalid(e.to_string()))
    })?;
    if cancel.load(Ordering::SeqCst) {
        return Err(invalid("export cancelled"));
    }
    app.state::<Operations>().begin_publish(job)?;
    kokin_export::research::publish(&path, &bytes).map_err(|e| invalid(e.to_string()))?;
    Ok(
        json!({"complete":true,"format":format,"results":ids.len(),"bytes":bytes.len(),"plaintext":true}),
    )
}
pub fn run(
    app: &AppHandle,
    job: &str,
    generation: u64,
    args: Value,
    cancel: &Arc<AtomicBool>,
) -> Result<Value, CommandError> {
    if args["consent"] != true {
        return Err(invalid("authorize the reviewed plan"));
    }
    let p = app.state::<ResearchPlans>().take(
        args["token"].as_str().unwrap_or(""),
        args["digest"].as_str().unwrap_or(""),
        generation,
    )?;
    let online = app.state::<Operations>().online_flag();
    if !online.load(Ordering::SeqCst) {
        return Err(invalid("enable online mode"));
    }
    app.state::<Session>().with_generation(generation, |o| {
        crate::research_preview::validate_origins(o, &p.plan.selectors, &p.plan.input_origins)?;
        if let Some(parent) = &p.plan.parent_import {
            crate::research_import::validate_parent(o, parent, &p.plan.selectors)?;
        }
        kokin_store::research::begin(&o.conn, job, &encode(&p.plan)?, p.checks.len())?;
        Ok(())
    })?;
    let queue = Mutex::new(VecDeque::from(p.checks));
    let failed = AtomicBool::new(false);
    let rescan = p.plan.rescan;
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let queue = &queue;
            let failed = &failed;
            let online = &online;
            scope.spawn(move || loop {
                if cancel.load(Ordering::SeqCst) || !online.load(Ordering::SeqCst) {
                    break;
                }
                let next = queue.lock().ok().and_then(|mut q| q.pop_front());
                let Some(check) = next else { break };
                app.state::<Operations>().update(
                    job,
                    &format!(
                        "Checking {} · completed sources remain saved",
                        check.manifest.title
                    ),
                    true,
                );
                if execute(app, job, generation, &check, rescan, cancel, online).is_err() {
                    failed.store(true, Ordering::SeqCst);
                }
            });
        }
    });
    let status = if cancel.load(Ordering::SeqCst) || !online.load(Ordering::SeqCst) {
        "cancelled"
    } else if failed.load(Ordering::SeqCst)
        || app
            .state::<Session>()
            .with_generation(generation, |o| {
                Ok(kokin_store::research::has_failures(&o.conn, job)?)
            })
            .unwrap_or(true)
    {
        "partial"
    } else {
        "completed"
    };
    app.state::<Session>().with_generation(generation, |o| {
        kokin_store::research::finish(&o.conn, job, status)?;
        Ok(())
    })?;
    Ok(json!({"run_id":job,"complete":status=="completed","status":status}))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn plan(generation: u64) -> Prepared {
        let request = ResearchRequest {
            version: 1,
            selectors: vec![Selector::parse(SelectorKind::Username, "example").unwrap()],
            sources: vec!["github".into()],
            rescan: false,
            safe_mode: true,
            options: Default::default(),
        };
        let (plan, checks) = research::checks(&request, generation).unwrap();
        Prepared {
            plan,
            checks,
            digest: "authorized".into(),
            expires: Instant::now() + Duration::from_secs(300),
        }
    }
    #[test]
    fn authorizations_are_single_use_expiring_and_case_bound() {
        let state = ResearchPlans::default();
        state.plans.lock().unwrap().insert("valid".into(), plan(7));
        assert!(state.take("valid", "authorized", 7).is_ok());
        assert!(state.take("valid", "authorized", 7).is_err());
        state
            .plans
            .lock()
            .unwrap()
            .insert("wrong-case".into(), plan(7));
        assert!(state.take("wrong-case", "authorized", 8).is_err());
        state
            .plans
            .lock()
            .unwrap()
            .insert("wrong-digest".into(), plan(7));
        assert!(state.take("wrong-digest", "changed", 7).is_err());
        let mut expired = plan(7);
        expired.expires = Instant::now() - Duration::from_secs(1);
        state
            .plans
            .lock()
            .unwrap()
            .insert("expired".into(), expired);
        assert!(state.take("expired", "authorized", 7).is_err());
        state.plans.lock().unwrap().insert("clear".into(), plan(7));
        state.clear();
        assert!(state.take("clear", "authorized", 7).is_err());
    }
}
