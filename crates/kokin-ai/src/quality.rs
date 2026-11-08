//! Synthetic retrieval gate is independent of case facts and model claims.
use crate::embedding::{EmbeddingProvider, Error};
pub const PASSAGES: &[&str] = &[
    "Elena repairs automobiles at a vehicle workshop in Tucson.",
    "Marco is employed by Tungsten Holdings as a financial analyst.",
    "The cargo ship departed Rotterdam harbour on Monday.",
    "A forest fire damaged the mountain village.",
    "The clinical researcher published a study on heart disease.",
    "Riverside bakery sells bread and pastries every morning.",
    "The university opened a new computer science laboratory.",
    "A satellite photograph shows a bridge crossing the river.",
    "A court filing lists Northstar as the owner of a warehouse.",
    "The domain registration record names a privacy service, not its beneficial owner.",
];
pub const QUERIES: &[(&str, usize)] = &[
    ("mechanic fixing cars", 0),
    ("who works as an investment specialist", 1),
    ("vessel leaving a port", 2),
    ("wildfire affecting a settlement", 3),
    ("medical investigation of cardiac illness", 4),
    ("shop making loaves and croissants", 5),
    ("higher education computing research facility", 6),
    ("overhead imagery of river infrastructure", 7),
];
pub fn evaluate(provider: &mut dyn EmbeddingProvider) -> Result<(usize, usize), Error> {
    let vectors = PASSAGES
        .iter()
        .map(|p| provider.embed(p, false))
        .collect::<Result<Vec<_>, _>>()?;
    let mut hits = 0;
    for (query, expected) in QUERIES {
        let q = provider.embed(query, true)?;
        let best = vectors
            .iter()
            .enumerate()
            .map(|(i, v)| {
                (
                    i,
                    v.iter()
                        .zip(&q)
                        .map(|(a, b)| f64::from(*a) * f64::from(*b))
                        .sum::<f64>(),
                )
            })
            .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
            .map(|v| v.0);
        if best == Some(*expected) {
            hits += 1;
        }
    }
    Ok((hits, QUERIES.len()))
}

/// The same FTS5 query compiler used by lexical case search, on synthetic text.
pub fn lexical_baseline() -> Result<usize, Error> {
    let conn = rusqlite::Connection::open_in_memory().map_err(|e| Error(e.to_string()))?;
    conn.execute_batch("CREATE VIRTUAL TABLE corpus USING fts5(body);")
        .map_err(|e| Error(e.to_string()))?;
    for p in PASSAGES {
        conn.execute("INSERT INTO corpus(body) VALUES(?1)", [p])
            .map_err(|e| Error(e.to_string()))?;
    }
    let mut correct = 0;
    for (q, expected) in QUERIES {
        if let Some(query) = kokin_search::Query::parse(q).to_fts5() {
            let mut stmt=conn.prepare("SELECT rowid FROM corpus WHERE corpus MATCH ?1 ORDER BY bm25(corpus),rowid LIMIT 1").map_err(|e|Error(e.to_string()))?;
            let rows = stmt
                .query_map([query], |r| r.get::<_, i64>(0))
                .map_err(|e| Error(e.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| Error(e.to_string()))?;
            if rows.first() == Some(&(*expected as i64 + 1)) {
                correct += 1;
            }
        }
    }
    Ok(correct)
}
