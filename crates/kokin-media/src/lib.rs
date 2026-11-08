//! Bounded media-worker protocol. No case credentials or filesystem paths cross it.
use serde::{Deserialize, Serialize};
pub const MAX_INPUT: usize = 64 * 1024 * 1024;
pub const MAX_OUTPUT: usize = 16 * 1024 * 1024;
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub media_type: String,
    pub page: u16,
    pub ocr: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_recipe: Option<image_query::Recipe>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub kind: String,
    pub value: String,
    pub locator: serde_json::Value,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Result {
    pub version: u32,
    pub engine: String,
    pub observations: Vec<Observation>,
    pub thumbnail_png: Vec<u8>,
    pub perceptual_hash: Option<String>,
    pub pages: u16,
    pub gaps: Vec<String>,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("media processing failed: {0}")]
    Processing(String),
    #[error("media exceeds a processing bound")]
    Limit,
    #[error("unsupported media format")]
    Unsupported,
}

#[cfg(feature = "engine")]
pub mod engine;

pub mod worker;

pub mod face_worker;

pub mod image_query;
