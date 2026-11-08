//! Indexed adjacency with limits applied during every hop. No confidence is derived.
use crate::{GraphError, Result};
use rusqlite::{params, Connection};
use std::{
    collections::{BTreeMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub canonical_id: String,
    pub depth: usize,
}
#[derive(Debug, Clone)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: String,
}
#[derive(Debug)]
pub struct Expansion {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub truncated: bool,
    pub completed_hops: usize,
    pub truncation_reasons: Vec<String>,
}
/// Stored records are traversed; aliases are labeled and open canonical details.
pub fn expand(
    conn: &Connection,
    seed: &str,
    hops: usize,
    node_limit: usize,
    per_node: usize,
    cancel: &AtomicBool,
) -> Result<Expansion> {
    expand_multi(
        conn,
        &[seed.to_owned()],
        ExpansionOptions {
            hops,
            node_limit,
            per_node,
            direction: "both",
            kinds: &[],
        },
        cancel,
    )
}
/// Multi-seed expansion shares a single budget and deadline across all roots.
pub struct ExpansionOptions<'a> {
    pub hops: usize,
    pub node_limit: usize,
    pub per_node: usize,
    pub direction: &'a str,
    pub kinds: &'a [String],
}
pub fn expand_multi(
    conn: &Connection,
    seeds: &[String],
    options: ExpansionOptions<'_>,
    cancel: &AtomicBool,
) -> Result<Expansion> {
    let ExpansionOptions {
        hops,
        node_limit,
        per_node,
        direction,
        kinds,
    } = options;
    let hops = hops.clamp(1, 4);
    let node_limit = node_limit.clamp(1, 300);
    let per_node = per_node.clamp(1, 128);
    let deadline = Instant::now() + Duration::from_millis(500);
    let node = |id: &str, depth| -> Result<Node> {
        conn.query_row("SELECT e.id,e.display_name,COALESCE(m.canonical_id,e.id) FROM entity e LEFT JOIN er_merge_map m ON m.entity_id=e.id AND m.active=1 WHERE e.id=?1",[id],|r|Ok(Node{id:r.get(0)?,name:r.get(1)?,canonical_id:r.get(2)?,depth})).map_err(GraphError::from)
    };
    let group = |id: &str| -> Result<Vec<String>> {
        let canonical = node(id, 0)?.canonical_id;
        let mut statement=conn.prepare_cached("SELECT e.id FROM entity e WHERE e.id=?1 OR e.id IN (SELECT entity_id FROM er_merge_map WHERE canonical_id=?1 AND active=1) ORDER BY CASE WHEN e.id=?1 THEN 0 ELSE 1 END,e.rowid LIMIT 301")?;
        let ids = statement
            .query_map([canonical], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(ids)
    };
    let mut nodes = BTreeMap::new();
    let mut frontier = Vec::new();
    let mut reasons = std::collections::BTreeSet::new();
    'roots: for seed in seeds.iter().take(300) {
        for id in group(seed)? {
            if nodes.contains_key(&id) {
                continue;
            }
            if nodes.len() >= node_limit {
                reasons.insert("node_limit".to_owned());
                break 'roots;
            }
            if cancel.load(Ordering::SeqCst) {
                return Err(GraphError::ExpansionCancelled);
            }
            if Instant::now() > deadline {
                reasons.insert("time_limit".to_owned());
                break 'roots;
            }
            nodes.insert(id.clone(), node(&id, 0)?);
            frontier.push(id);
        }
    }

    let mut edges = BTreeMap::new();
    let mut expanded = HashSet::new();
    let mut truncated = !reasons.is_empty();
    let mut completed_hops = 0;
    'hops: for depth in 1..=hops {
        let mut next = Vec::new();
        for current in &frontier {
            if cancel.load(Ordering::SeqCst) {
                return Err(GraphError::ExpansionCancelled);
            }
            if Instant::now() > deadline {
                truncated = true;
                reasons.insert("time_limit".to_owned());
                break 'hops;
            }
            if !expanded.insert(current.clone()) {
                continue;
            }
            for column in ["from_entity", "to_entity"] {
                if (direction == "incoming" && column == "from_entity")
                    || (direction == "outgoing" && column == "to_entity")
                {
                    continue;
                }

                let sql=format!("SELECT id,from_entity,to_entity,kind FROM relationship WHERE {column}=?1 AND (?3='[]' OR kind IN (SELECT value FROM json_each(?3))) ORDER BY rowid LIMIT ?2");
                let mut statement = conn.prepare_cached(&sql)?;
                let rows = statement.query_map(
                    params![
                        current,
                        (per_node + 1) as i64,
                        serde_json::to_string(kinds).unwrap_or_else(|_| "[]".into())
                    ],
                    |r| {
                        Ok(Edge {
                            id: r.get(0)?,
                            from: r.get(1)?,
                            to: r.get(2)?,
                            kind: r.get(3)?,
                        })
                    },
                )?;
                for (index, edge) in rows.enumerate() {
                    if cancel.load(Ordering::SeqCst) {
                        return Err(GraphError::ExpansionCancelled);
                    }
                    if Instant::now() > deadline {
                        truncated = true;
                        reasons.insert("time_limit".to_owned());
                        break 'hops;
                    }
                    let edge = edge?;
                    if index == per_node {
                        truncated = true;
                        reasons.insert("per_node_limit".to_owned());
                        break;
                    }
                    let other = if edge.from == *current {
                        &edge.to
                    } else {
                        &edge.from
                    };
                    if !nodes.contains_key(other) {
                        for id in group(other)? {
                            if nodes.contains_key(&id) {
                                continue;
                            }
                            if nodes.len() >= node_limit {
                                truncated = true;
                                reasons.insert("node_limit".to_owned());
                                break;
                            }
                            nodes.insert(id.clone(), node(&id, depth)?);
                            next.push(id);
                        }
                    }
                    if !nodes.contains_key(other) {
                        continue;
                    }
                    if edges.len() >= 1000 && !edges.contains_key(&edge.id) {
                        truncated = true;
                        reasons.insert("edge_limit".to_owned());
                        break 'hops;
                    }
                    edges.insert(edge.id.clone(), edge);
                }
            }
        }
        completed_hops = depth;
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    Ok(Expansion {
        nodes: nodes.into_values().collect(),
        edges: edges.into_values().collect(),
        truncated,
        completed_hops,
        truncation_reasons: reasons.into_iter().collect(),
    })
}
