//! First-party public sources. Planning is read-only; collection requires consent.
use crate::{
    views::{SourceDisclosureView, SourceRequest},
    CommandError, ErrorCode,
};
pub(crate) fn plan(request: &SourceRequest) -> Result<kokin_connectors::Plan, CommandError> {
    if request.version != 1 {
        return Err(CommandError::new(
            ErrorCode::InvalidValue,
            "unsupported connector version",
        ));
    }
    kokin_connectors::plan(
        &request.connector,
        &request.input,
        &request.record_type,
        request.page,
        &request.timestamp,
    )
    .map_err(|e| CommandError::new(ErrorCode::InvalidValue, e.to_string()))
}
#[tauri::command]
pub fn source_disclosure(request: SourceRequest) -> Result<SourceDisclosureView, CommandError> {
    let p = plan(&request)?;
    Ok(SourceDisclosureView {
        connector: p.manifest.id,
        destinations: p.manifest.destinations,
        transmitted_fields: p.manifest.transmitted_fields,
        request_urls: p.requests.iter().map(|r| r.url.clone()).collect(),
        normalized_input: p.normalized_input,
        limitation: p.limitation,
        max_response_bytes: p.manifest.max_response_bytes,
        health: p.manifest.health,
        fixtures_version: p.manifest.fixtures_version,
    })
}
