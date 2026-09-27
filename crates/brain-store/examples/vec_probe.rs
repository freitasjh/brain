//! C3-01 SPIKE — minimal link/DDL probe for `sqlite-vec` (TD-001).
//!
//! Isolates G1 (does the extension load at all?), G2 (`vec_version()`),
//! and G3 (can we CREATE VIRTUAL TABLE ... USING vec0 with a cosine metric?).
//! Kept separate from `vec_spike.rs` on purpose: if static linking of the
//! sqlite-vec amalgamation is ever rejected, this is the file that fails, and
//! it fails in 20 lines with an unambiguous error.
//!
//! Run: `cargo run -p brain-store --example vec_probe`

use rusqlite::Connection;

#[path = "vec_common/mod.rs"]
mod common;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::register_sqlite_vec();
    let conn = Connection::open_in_memory()?;

    // G2 — does the extension answer?
    let version: String = conn.query_row("SELECT vec_version()", [], |r| r.get(0))?;
    println!("G1 extension loaded      : OK");
    println!("G2 vec_version()         : {version}");

    // G3a — plain vec0, 768 dims, default (L2) metric.
    conn.execute(
        "CREATE VIRTUAL TABLE vec_l2 USING vec0(chunk_id INTEGER PRIMARY KEY, embedding float[768])",
        [],
    )?;
    println!("G3a vec0 float[768] L2   : OK");

    // G3b — the critical one: cosine metric. `distance_metric` is a
    // *column-level* option, so it goes after the `]` of the column def.
    match conn.execute(
        "CREATE VIRTUAL TABLE vec_cos USING vec0(\
             chunk_id INTEGER PRIMARY KEY, \
             embedding float[768] distance_metric=cosine)",
        [],
    ) {
        Ok(_) => println!("G3b vec0 + distance_metric=cosine : OK"),
        Err(e) => {
            println!("G3b vec0 + distance_metric=cosine : FAIL — {e}");
            return Err(e.into());
        }
    }

    // Sanity: a 768-float row inserts and a KNN query with k= returns.
    let vec: Vec<f32> = (0..768).map(|i| (i as f32) / 768.0).collect();
    let bytes: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();
    conn.execute(
        "INSERT INTO vec_cos(chunk_id, embedding) VALUES (?1, ?2)",
        rusqlite::params![1i64, bytes],
    )?;
    let n: i64 = conn.query_row("SELECT count(*) FROM vec_cos", [], |r| r.get(0))?;
    let (dist,): (f64,) = conn.query_row(
        "SELECT distance FROM vec_cos WHERE embedding MATCH ?1 AND k = 1",
        rusqlite::params![bytes],
        |r| Ok((r.get(0)?,)),
    )?;
    println!("G3c insert+KNN roundtrip : OK (rows={n}, self_distance={dist:.6})");

    Ok(())
}
