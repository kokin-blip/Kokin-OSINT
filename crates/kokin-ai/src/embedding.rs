//! Local, pinned bge-small int8 embeddings. No download or network feature.
use ort::{session::Session, value::Tensor};
use std::path::Path;
use tokenizers::{Tokenizer, TruncationParams};
#[derive(Debug, thiserror::Error)]
#[error("embedding engine/resource unavailable or invalid output: {0}")]
pub struct Error(pub String);
pub trait EmbeddingProvider {
    fn embed(&mut self, text: &str, query: bool) -> Result<Vec<f32>, Error>;
    fn identity(&self) -> &str;
}
pub struct BgeSmall {
    session: Session,
    tokenizer: Tokenizer,
    identity: String,
}
impl BgeSmall {
    pub fn open(
        runtime: &Path,
        model: &Path,
        tokenizer: &Path,
        identity: &str,
    ) -> Result<Self, Error> {
        let runtime = runtime
            .to_str()
            .ok_or_else(|| Error("runtime path".into()))?;
        ort::init_from(runtime)
            .with_name("kokin-local-embeddings")
            .with_telemetry(false)
            .commit()
            .map_err(|e| Error(e.to_string()))?;
        let session = Session::builder()
            .and_then(|b| b.with_intra_threads(2))
            .and_then(|b| b.commit_from_file(model))
            .map_err(|e| Error(e.to_string()))?;
        let mut tokenizer = Tokenizer::from_file(tokenizer).map_err(|e| Error(e.to_string()))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: 512,
                ..Default::default()
            }))
            .map_err(|e| Error(e.to_string()))?;
        Ok(Self {
            session,
            tokenizer,
            identity: identity.into(),
        })
    }
}
impl EmbeddingProvider for BgeSmall {
    fn identity(&self) -> &str {
        &self.identity
    }
    fn embed(&mut self, text: &str, query: bool) -> Result<Vec<f32>, Error> {
        if text.len() > 64 * 1024 || text.trim().is_empty() {
            return Err(Error("empty or oversized text".into()));
        }
        let text = if query {
            format!("Represent this sentence for searching relevant passages: {text}")
        } else {
            text.to_owned()
        };
        let tokens = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| Error(e.to_string()))?;
        let n = tokens.len();
        let tensor = |values: Vec<i64>| {
            Tensor::from_array(([1, n], values)).map_err(|e| Error(e.to_string()))
        };
        let ids = tensor(tokens.get_ids().iter().map(|v| i64::from(*v)).collect())?;
        let mask = tensor(
            tokens
                .get_attention_mask()
                .iter()
                .map(|v| i64::from(*v))
                .collect(),
        )?;
        let types = tensor(
            tokens
                .get_type_ids()
                .iter()
                .map(|v| i64::from(*v))
                .collect(),
        )?;
        let outputs = self
            .session
            .run(ort::inputs!["input_ids"=>ids,"attention_mask"=>mask,"token_type_ids"=>types])
            .map_err(|e| Error(e.to_string()))?;
        let (shape, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| Error(e.to_string()))?;
        if shape.last() != Some(&384) || data.len() < 384 {
            return Err(Error("expected 384-dimensional hidden state".into()));
        }
        // bge uses CLS pooling, then unit normalization; not mean pooling.
        let mut vector = data[..384].to_vec();
        normalize(&mut vector)?;
        Ok(vector)
    }
}
pub fn normalize(vector: &mut [f32]) -> Result<(), Error> {
    if vector.len() != 384 || vector.iter().any(|v| !v.is_finite()) {
        return Err(Error("invalid vector".into()));
    }
    let norm = vector
        .iter()
        .map(|v| f64::from(*v).powi(2))
        .sum::<f64>()
        .sqrt();
    if norm < 1e-12 {
        return Err(Error("zero vector".into()));
    }
    for v in vector {
        *v = (f64::from(*v) / norm) as f32;
    }
    Ok(())
}
