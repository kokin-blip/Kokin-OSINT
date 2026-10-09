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
    let hops = hops.clamp(1, 4);
    let node_limit = node_limit.clamp(1, 300);
    let per_node = per_node.clamp(1, 128);
    let deadline = Instant::now() + Duration::from_millis(500);
    let node = |id: &str, depth| -> Result<Node> {
        conn.query_row("SELECT e.id,e.display_name,COALESCE(m.canonical_id,e.id) FROM entity e LEFT JOIN er_merge_map m ON m.entity_id=e.id AND m.active=1 WHERE e.id=?1",[id],|r|Ok(Node{id:r.get(0)?,name:r.get(1)?,canonical_id:r.get(2)?,depth})).map_err(GraphError::from)
    };
    let mut nodes = BTreeMap::new();
    nodes.insert(seed.to_owned(), node(seed, 0)?);
    let mut frontier = vec![seed.to_owned()];
    let mut edges = BTreeMap::new();
    let mut expanded = HashSet::new();
    let mut truncated = false;
    let mut completed_hops = 0;
    'hops: for depth in 1..=hops {
        let mut next = Vec::new();
        for current in &frontier {
            if cancel.load(Ordering::SeqCst) {
                return Err(GraphError::ExpansionCancelled);
            }
            if Instant::now() > deadline {
                truncated = true;
                break 'hops;
            }
            if !expanded.insert(current.clone()) {
                continue;
            }
            for direction in ["from_entity", "to_entity"] {
                let sql=format!("SELECT id,from_entity,to_entity,kind FROM relationship WHERE {direction}=?1 ORDER BY rowid LIMIT ?2");
                let mut statement = conn.prepare_cached(&sql)?;
                let rows = statement.query_map(params![current, (per_node + 1) as i64], |r| {
                    Ok(Edge {
                        id: r.get(0)?,
                        from: r.get(1)?,
                        to: r.get(2)?,
                        kind: r.get(3)?,
                    })
                })?;
                for (index, edge) in rows.enumerate() {
                    if cancel.load(Ordering::SeqCst) {
                        return Err(GraphError::ExpansionCancelled);
                    }
                    if Instant::now() > deadline {
                        truncated = true;
                        break 'hops;
                    }
                    let edge = edge?;
                    if index == per_node {
                        truncated = true;
                        break;
                    }
                    let other = if edge.from == *current {
                        &edge.to
                    } else {
                        &edge.from
                    };
                    if !nodes.contains_key(other) {
                        if nodes.len() >= node_limit {
                            truncated = true;
                            continue;
                        }
                        nodes.insert(other.clone(), node(other, depth)?);
                        next.push(other.clone());
                    }
                    if edges.len() >= 1000 && !edges.contains_key(&edge.id) {
                        truncated = true;
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
    })
}
