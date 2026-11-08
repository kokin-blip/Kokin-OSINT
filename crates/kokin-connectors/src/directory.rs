//! Pinned MIT directory data. A catalog entry is never an executable connector.
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub id: String,
    pub name: String,
    pub category: String,
    pub url: String,
    pub description: String,
    pub status: String,
    pub pricing: String,
    pub input: String,
    pub output: String,
    pub best_for: String,
    pub opsec: String,
    pub opsec_note: String,
    pub api: bool,
    pub registration: bool,
    pub local_install: bool,
    pub google_dork: bool,
    pub edit_url: bool,
    pub invitation_only: bool,
    pub deprecated: bool,
}
pub fn tools() -> &'static [Tool] {
    static DATA: OnceLock<Vec<Tool>> = OnceLock::new();
    DATA.get_or_init(|| {
        fn walk(node: &serde_json::Value, path: &str, rows: &mut Vec<Tool>) {
            if node["url"].is_string() {
                let text = |key: &str| node[key].as_str().unwrap_or("").to_owned();
                let name = text("name");
                let flag = |key: &str, marker: &str| {
                    node[key].as_bool().unwrap_or(false) || name.contains(marker)
                };
                rows.push(Tool {
                    id: format!("tool-{:04}", rows.len() + 1),
                    name: name.clone(),
                    category: path.into(),
                    url: text("url"),
                    description: text("description"),
                    status: text("status"),
                    pricing: text("pricing"),
                    input: text("input"),
                    output: text("output"),
                    best_for: text("bestFor"),
                    opsec: text("opsec"),
                    opsec_note: text("opsecNote"),
                    api: flag("api", "(API)"),
                    registration: flag("registration", "(R)"),
                    local_install: flag("localInstall", "(T)"),
                    google_dork: flag("googleDork", "(D)"),
                    edit_url: flag("editUrl", "(M)"),
                    invitation_only: flag("invitationOnly", "(invite)"),
                    deprecated: flag("deprecated", "(deprecated)"),
                });
            } else if let Some(children) = node["children"].as_array() {
                let name = node["name"].as_str().unwrap_or("");
                let next = if path.is_empty() {
                    name.to_owned()
                } else {
                    format!("{path} > {name}")
                };
                for child in children {
                    walk(child, &next, rows);
                }
            }
        }
        // Catalog bytes are pinned and validated by the catalog contract test.
        let root: serde_json::Value = serde_json::from_str(include_str!(
            "../../../resources/osint-framework-ff4107ba.json"
        ))
        .unwrap_or_default();
        let mut rows = Vec::new();
        if let Some(children) = root["children"].as_array() {
            for child in children {
                walk(child, "", &mut rows);
            }
        }
        rows
    })
}
#[cfg(test)]
mod tests {
    #[test]
    fn catalog_is_complete_and_not_executable() {
        let rows = super::tools();
        assert_eq!(rows.len(), 1166);
        assert!(rows.iter().all(|t| !t.id.is_empty() && !t.url.is_empty()));
        assert_eq!(
            rows.iter()
                .map(|r| r.category.split(" > ").next().unwrap_or(""))
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            33
        );
    }
}
