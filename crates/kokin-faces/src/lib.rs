//! Case-local face comparisons. Scores are similarities, never identity probabilities.
use serde::{Deserialize, Serialize};
pub const DIMENSIONS: usize = 128;
pub const CANDIDATE_THRESHOLD: f32 = 0.363;
pub const GROUP_THRESHOLD: f32 = 0.5;
pub const MAX_VIDEO: usize = 512 * 1024 * 1024;
pub const MAX_FACES: usize = 10_000;
pub const MAX_OUTPUT: usize = 512 * 1024 * 1024;
#[derive(Debug, thiserror::Error)]
#[error("face processing: {0}")]
pub struct Error(pub String);
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub media_type: String,
    pub page: u16,
    pub start_ms: u64,
    pub end_ms: Option<u64>,
    #[serde(default)]
    pub preview: bool,
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.start_ms > 86_400_000
            || self
                .end_ms
                .is_some_and(|e| e <= self.start_ms || e > 86_400_000)
        {
            return Err(Error("invalid version or time range".into()));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Detection {
    pub bbox: [f32; 4],
    pub landmarks: [[f32; 2]; 5],
    pub image_size: [u32; 2],
    pub detection_score: f32,
    pub vector: Vec<f32>,
    #[serde(
        serialize_with = "png_encoding::serialize",
        deserialize_with = "png_encoding::crop"
    )]
    pub crop_png: Vec<u8>,
    pub page: u16,
    pub timestamp_ms: Option<u64>,
}
impl Detection {
    pub fn validate(&self) -> Result<()> {
        let [x, y, w, h] = self.bbox;
        if self.image_size.contains(&0)
            || self.bbox.iter().any(|v| !v.is_finite())
            || x < 0.
            || y < 0.
            || w <= 0.
            || h <= 0.
            || x + w > self.image_size[0] as f32 + 1.
            || y + h > self.image_size[1] as f32 + 1.
            || self.landmarks.iter().flatten().any(|v| !v.is_finite())
            || !self.detection_score.is_finite()
            || !(0.9..=1.).contains(&self.detection_score)
            || self.crop_png.len() > 128 * 1024
        {
            return Err(Error("invalid detection geometry or crop".into()));
        }
        let norm: f32 = self.vector.iter().map(|v| v * v).sum();
        if self.vector.len() != DIMENSIONS
            || self.vector.iter().any(|v| !v.is_finite())
            || (norm - 1.).abs() > 0.001
        {
            return Err(Error("invalid face embedding".into()));
        }
        Ok(())
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub version: u32,
    pub detections: Vec<Detection>,
    #[serde(
        default,
        serialize_with = "png_encoding::serialize",
        deserialize_with = "png_encoding::preview"
    )]
    pub preview_png: Vec<u8>,
    pub sampled_frames: usize,
    #[serde(default)]
    pub last_sample_ms: Option<u64>,
    #[serde(default)]
    pub sampled_timestamps_ms: Vec<u64>,
    pub complete: bool,
    pub limitations: Vec<String>,
}
pub fn normalize(v: &mut [f32]) -> Result<()> {
    if v.len() != DIMENSIONS || v.iter().any(|x| !x.is_finite()) {
        return Err(Error("invalid embedding".into()));
    }
    let norm = v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    if norm < 1e-12 {
        return Err(Error("zero embedding".into()));
    }
    for x in v {
        *x = (f64::from(*x) / norm) as f32;
    }
    Ok(())
}
pub fn cosine(a: &[f32], b: &[f32]) -> Result<f32> {
    if a.len() != DIMENSIONS || b.len() != DIMENSIONS || a.iter().chain(b).any(|v| !v.is_finite()) {
        return Err(Error("incompatible embeddings".into()));
    }
    Ok(a.iter()
        .zip(b)
        .map(|(a, b)| a * b)
        .sum::<f32>()
        .clamp(-1., 1.))
}
pub fn encode(v: &[f32]) -> Result<Vec<u8>> {
    cosine(v, v)?;
    Ok(v.iter().flat_map(|v| v.to_le_bytes()).collect())
}
pub fn decode(bytes: &[u8]) -> Result<Vec<f32>> {
    if bytes.len() != DIMENSIONS * 4 {
        return Err(Error("invalid vector size".into()));
    }
    let v: Vec<_> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    cosine(&v, &v)?;
    Ok(v)
}
/// Incremental complete-link groups: every pair in a group must meet the threshold.
/// Input must be sorted by stable face ID; returned members are input indices.
pub fn groups(vectors: &[Vec<f32>], threshold: f32) -> Result<Vec<Vec<usize>>> {
    groups_controlled(vectors, threshold, || false)
}
pub fn groups_controlled(
    vectors: &[Vec<f32>],
    threshold: f32,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<Vec<usize>>> {
    if vectors.len() > MAX_FACES || !threshold.is_finite() || !(0.363..=1.).contains(&threshold) {
        return Err(Error("grouping bound or threshold".into()));
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, v) in vectors.iter().enumerate() {
        if cancelled() {
            return Err(Error("grouping cancelled".into()));
        }
        cosine(v, v)?;
        let mut destination = None;
        for (j, g) in groups.iter().enumerate() {
            let mut compatible = true;
            for (index, k) in g.iter().enumerate() {
                if index % 32 == 0 && cancelled() {
                    return Err(Error("grouping cancelled".into()));
                }
                if cosine(v, &vectors[*k])? < threshold {
                    compatible = false;
                    break;
                }
            }
            if compatible {
                destination = Some(j);
                break;
            }
        }
        if let Some(j) = destination {
            groups[j].push(i)
        } else {
            groups.push(vec![i])
        }
    }
    Ok(groups.into_iter().filter(|g| g.len() > 1).collect())
}
#[cfg(feature = "inference")]
pub mod inference;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn v(angle: f32) -> Vec<f32> {
        let mut v = vec![0.; 128];
        v[0] = angle.cos();
        v[1] = angle.sin();
        v
    }
    #[test]
    fn complete_link_does_not_chain() {
        let input = vec![v(0.), v(0.8), v(1.6)];
        assert_eq!(groups(&input, 0.5).unwrap(), vec![vec![0, 1]]);
    }
    #[test]
    fn vector_validation() {
        assert!(normalize(&mut vec![0.; 128]).is_err());
        assert!(decode(&[0; 20]).is_err());
        let a = v(0.);
        assert_eq!(decode(&encode(&a).unwrap()).unwrap(), a);
        assert!(cosine(&[f32::NAN; 128], &a).is_err());
    }
    #[test]
    fn sampling_ranges() {
        assert!(Request {
            version: 1,
            media_type: "video/mp4".into(),
            page: 0,
            start_ms: 10,
            end_ms: Some(10),
            preview: false
        }
        .validate()
        .is_err());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod cancellation_tests {
    #[test]
    fn grouping_checks_cancellation_before_pairwise_work() {
        let mut v = vec![0.; 128];
        v[0] = 1.;
        assert!(super::groups_controlled(&[v], 0.5, || true).is_err());
    }
}

// PNGs use bounded base64 instead of decimal byte arrays in the private worker
// protocol. A long video must not multiply thumbnail payloads fourfold.
mod png_encoding {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(
        bytes: &[u8],
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }
    fn decode<'de, D: Deserializer<'de>>(
        deserializer: D,
        limit: usize,
    ) -> std::result::Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.len() > limit.div_ceil(3) * 4 {
            return Err(serde::de::Error::custom(
                "PNG payload exceeds its processing bound",
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(serde::de::Error::custom)?;
        if bytes.len() > limit {
            return Err(serde::de::Error::custom(
                "PNG payload exceeds its processing bound",
            ));
        }
        Ok(bytes)
    }
    pub fn crop<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Vec<u8>, D::Error> {
        decode(deserializer, 128 * 1024)
    }
    pub fn preview<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Vec<u8>, D::Error> {
        decode(deserializer, 8 * 1024 * 1024)
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod protocol_tests {
    #[test]
    fn crop_messages_are_compact_bounded_and_roundtrip() {
        let mut vector = vec![0.; 128];
        vector[0] = 1.;
        let crop: Vec<u8> = (0..30000).map(|i| (i % 251) as u8).collect();
        let face = super::Detection {
            bbox: [1., 1., 50., 50.],
            landmarks: [[10., 10.]; 5],
            image_size: [100, 100],
            detection_score: 0.95,
            vector,
            crop_png: crop.clone(),
            page: 0,
            timestamp_ms: None,
        };
        let encoded = serde_json::to_vec(&face).unwrap();
        assert!(encoded.len() < crop.len() * 2);
        let decoded: super::Detection = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.crop_png, crop);
        decoded.validate().unwrap();
        let mut malformed = serde_json::to_value(&face).unwrap();
        malformed["crop_png"] = serde_json::Value::String("X".repeat(180000));
        assert!(serde_json::from_value::<super::Detection>(malformed).is_err());
    }
}
