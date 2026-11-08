//! Rebuildable vector projection lives inside the existing SQLCipher database.
use crate::embedding::{normalize, Error};
use rusqlite::{params, Connection};
pub trait VectorIndex {
    fn nearest(&self, query: &[f32], limit: usize) -> Result<Vec<(String, f64)>, Error>;
}
pub struct SqliteVectorIndex<'a> {
    pub conn: &'a Connection,
}
pub fn encode(values: &[f32]) -> Result<Vec<u8>, Error> {
    let mut values = values.to_vec();
    normalize(&mut values)?;
    Ok(values.iter().flat_map(|v| v.to_le_bytes()).collect())
}
pub fn rebuild_projection(conn: &Connection) -> Result<(), Error> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| Error(e.to_string()))?;
    populate_projection(&tx)?;
    tx.commit().map_err(|e| Error(e.to_string()))
}
pub fn populate_projection(conn: &Connection) -> Result<(), Error> {
    conn.execute_batch("DROP TABLE IF EXISTS kokin_vectors; CREATE VIRTUAL TABLE kokin_vectors USING vec0(observation_id TEXT PRIMARY KEY, embedding float[384] distance_metric=cosine); INSERT INTO kokin_vectors(observation_id,embedding) SELECT observation_id,vector FROM observation_embedding;").map_err(|e|Error(e.to_string()))
}
impl VectorIndex for SqliteVectorIndex<'_> {
    fn nearest(&self, query: &[f32], limit: usize) -> Result<Vec<(String, f64)>, Error> {
        if limit == 0 || limit > 1000 {
            return Err(Error("query limit".into()));
        }
        let mut stmt=self.conn.prepare("WITH nearest AS MATERIALIZED (SELECT observation_id,distance FROM kokin_vectors WHERE embedding MATCH ?1 AND k=?2 ORDER BY distance) SELECT observation_id,distance FROM nearest ORDER BY distance,observation_id").map_err(|e|Error(e.to_string()))?;
        let rows = stmt
            .query_map(params![encode(query)?, limit as i64], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(|e| Error(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| Error(e.to_string()))?;
        Ok(rows)
    }
}
