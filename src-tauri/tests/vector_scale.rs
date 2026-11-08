//! Explicit vector correctness/latency benchmark; synthetic data, encrypted case.
#![allow(clippy::unwrap_used)]
#[allow(dead_code)]
#[path = "support/corpus.rs"]
mod corpus;
use kokin_ai::vector::{encode, SqliteVectorIndex, VectorIndex};
use kokin_osint_lib::Session;
#[test]
#[ignore = "100,000 x 384 vector reference benchmark; run release explicitly"]
fn encrypted_vectors_match_exact_reference_at_100k() {
    let root = std::env::temp_dir().join(format!("kokin-vector-scale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let session = Session::default();
    session
        .create(&root, "vector-scale", "synthetic benchmark secret")
        .unwrap();
    session.with_case(|open|{
 corpus::seed(open,100_000,0,0);let mut rng=0x93ab729df28371u64;let mut vectors=Vec::with_capacity(100_000);
 let tx=open.conn.transaction()?;for i in 0..100_000{let mut v=Vec::with_capacity(384);for _ in 0..384{rng^=rng<<13;rng^=rng>>7;rng^=rng<<17;v.push(((rng>>40)as f32/(1u32<<24)as f32)*2.0-1.0);}kokin_ai::embedding::normalize(&mut v).unwrap();tx.execute("INSERT INTO observation_embedding VALUES(?1,'synthetic-384','synthetic',?2)",rusqlite::params![format!("obs-{i}"),encode(&v).unwrap()])?;vectors.push(v);}tx.commit()?;
 kokin_ai::vector::rebuild_projection(&open.conn).unwrap();let index=SqliteVectorIndex{conn:&open.conn};let mut timings=Vec::new();
 for q in [0,7,301,9001,99999]{let query=&vectors[q];let mut exact:Vec<_>=vectors.iter().enumerate().map(|(i,v)|(format!("obs-{i}"),v.iter().zip(query).map(|(a,b)|f64::from(*a)*f64::from(*b)).sum::<f64>())).collect();exact.sort_by(|a,b|b.1.total_cmp(&a.1).then_with(||a.0.cmp(&b.0)));let expected:Vec<_>=exact.iter().take(10).map(|v|v.0.clone()).collect();let actual=index.nearest(query,10).unwrap();assert_eq!(actual.iter().map(|v|v.0.clone()).collect::<Vec<_>>(),expected);}
 for i in 0..30{let start=std::time::Instant::now();index.nearest(&vectors[(i*301)%vectors.len()],50).unwrap();timings.push(start.elapsed().as_secs_f64()*1000.0);}timings.sort_by(f64::total_cmp);println!("100k x 384 encrypted vec0 exact top10 recall=1.0; warm query n=30 p50={:.1}ms p95={:.1}ms p99={:.1}ms",timings[15],timings[28],timings[29]);Ok(())
 }).unwrap();
    session.close().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
