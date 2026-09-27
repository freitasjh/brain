//! C3-01 SPIKE — read-only inspection of a brain DB copy.
//!
//! Answers the questions the full spike needs before it can be written:
//!   - what is the real `chunks` schema we must mirror into vec0?
//!   - are the 768-dim embeddings ALREADY unit-normalized?
//!     (decides whether `distance_metric=cosine` is equivalent to the
//!      `cosine()` in brain-store/src/lib.rs:829, or whether we must normalize)
//!   - how are layer/scope/project_id distributed across chunks?
//!   - are the BLOBs uniformly 3072 bytes (768 x f32 LE)?
//!
//! Run: `cargo run -p brain-store --example vec_inspect -- /tmp/opencode/vec_spike/real.db`

use rusqlite::Connection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/opencode/vec_spike/real.db".to_string());
    let conn = Connection::open(&path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

    println!("== DB: {path}");
    let v: String = conn.query_row("SELECT value FROM _meta WHERE key='version'", [], |r| r.get(0))?;
    let dim: String = conn.query_row("SELECT value FROM _meta WHERE key='embedding_dim'", [], |r| r.get(0))?;
    println!("_meta.version={v}  _meta.embedding_dim={dim}");

    println!("\n== chunks schema");
    let mut st = conn.prepare("SELECT sql FROM sqlite_master WHERE name='chunks'")?;
    let rows = st.query_map([], |r| r.get::<_, String>(0))?;
    for s in rows { println!("{}", s?); }

    println!("\n== counts");
    let (total, with_emb, paths, notes): (i64, i64, i64, i64) = conn.query_row(
        "SELECT (SELECT count(*) FROM chunks),
                (SELECT count(*) FROM chunks WHERE embedding IS NOT NULL),
                (SELECT count(DISTINCT path) FROM chunks),
                (SELECT count(*) FROM notes)",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    println!("chunks={total} with_embedding={with_emb} distinct_paths={paths} notes={notes}");

    println!("\n== BLOB length distribution");
    let mut st = conn.prepare(
        "SELECT length(embedding) AS len, count(*) FROM chunks WHERE embedding IS NOT NULL GROUP BY len ORDER BY 2 DESC")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    for r in rows { let (len, n) = r?; println!("  len={len} bytes -> {n} chunks ({:.1} dims)", len as f64 / 4.0); }

    println!("\n== normalization check (first 20 chunks) — cosine vs L2 equivalence needs |v|=1");
    let mut st = conn.prepare(
        "SELECT id, embedding FROM chunks WHERE embedding IS NOT NULL ORDER BY id LIMIT 20")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
    let mut max_dev = 0.0f32;
    let mut any = false;
    for r in rows {
        let (id, blob) = r?;
        let emb: Vec<f32> = blob.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        max_dev = max_dev.max((norm - 1.0).abs());
        if !any { println!("  chunk id={id} dims={} norm={norm:.6}", emb.len()); any = true; }
    }
    println!("  max |norm-1| over 20 chunks = {max_dev:.6}  => {}",
        if max_dev < 1e-3 { "NORMALIZED (cosine == L2 on unit sphere)" } else { "NOT normalized (must use distance_metric=cosine)" });

    println!("\n== layer / scope / project_id distribution (the G6 filter dimensions)");
    let mut st = conn.prepare(
        "SELECT coalesce(layer,'<null>'), coalesce(scope,'<null>'), coalesce(cast(project_id AS TEXT),'<null>'), count(*)
         FROM chunks GROUP BY 1,2,3 ORDER BY 4 DESC")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?)))?;
    for r in rows { let (l, s, p, n) = r?; println!("  layer={l:<14} scope={s:<10} project_id={p:<8} chunks={n}"); }

    println!("\n== candidate real text queries (from notes.content, for G5 recall)");
    let mut st = conn.prepare(
        "SELECT substr(content, instr(content, char(10))+1, 60) FROM notes
         WHERE length(content) > 200 ORDER BY length(content) DESC LIMIT 12")?;
    let rows = st.query_map([], |r| r.get::<_, String>(0))?;
    for (i, s) in rows.enumerate() {
        let t = s?.replace('\n', " ").trim().to_string();
        println!("  q{:02}: {}", i + 1, &t[..t.len().min(58)]);
    }

    Ok(())
}
