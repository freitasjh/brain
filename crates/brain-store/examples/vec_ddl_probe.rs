//! C3-01 SPIKE — vec0 DDL / filter-shape matrix (feeds G6).
//!
//! The current brain scan applies layer/scope/project_id as a SQL WHERE filter
//! BEFORE the similarity math. To keep ranking semantics identical, vec0 has to
//! apply the same filter INSIDE the KNN, not after it. This probe tries the
//! candidate vec0 shapes and reports, for each, whether the filter is honoured
//! inside `WHERE embedding MATCH ? AND <filter> AND k = ?`.
//!
//! Run: `cargo run -p brain-store --example vec_ddl_probe`

use rusqlite::Connection;

#[path = "vec_common/mod.rs"]
mod common;

fn unit(seed: usize) -> Vec<f32> {
    // Deterministic pseudo-unit vector: 8 dims, easy to reason about by hand.
    let base = [
        [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
    ];
    base[seed % base.len()].to_vec()
}

fn blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn try_ddl(conn: &Connection, label: &str, ddl: &str) -> bool {
    match conn.execute_batch(ddl) {
        Ok(()) => {
            println!("  [DDL ok] {label}");
            true
        }
        Err(e) => {
            println!("  [DDL FAIL] {label}: {e}");
            false
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::register_sqlite_vec();
    let conn = Connection::open_in_memory()?;

    println!("== DDL shape matrix (dim 8 for legibility, semantics identical at 768)");

    // Shape A: metadata columns only (no partition key).
    try_ddl(
        &conn,
        "A  layer text / scope text / project_id integer  (metadata)",
        "CREATE VIRTUAL TABLE t_a USING vec0(\
             chunk_id INTEGER PRIMARY KEY, \
             embedding float[8] distance_metric=cosine, \
             layer text, scope text, project_id integer)",
    );

    // Shape B: layer as partition key + the other two as metadata.
    try_ddl(
        &conn,
        "B  layer text partition key / scope text / project_id integer",
        "CREATE VIRTUAL TABLE t_b USING vec0(\
             chunk_id INTEGER PRIMARY KEY, \
             embedding float[8] distance_metric=cosine, \
             layer text partition key, \
             scope text, project_id integer)",
    );

    // Shape C: all three partition keys (max is 4).
    try_ddl(
        &conn,
        "C  all three as partition keys",
        "CREATE VIRTUAL TABLE t_c USING vec0(\
             chunk_id INTEGER PRIMARY KEY, \
             embedding float[8] distance_metric=cosine, \
             layer text partition key, scope text partition key, project_id integer partition key)",
    );

    println!("\n== does NULL work for metadata/partition columns? (scope/project_id are NULLable in chunks)");
    for t in ["t_a", "t_b", "t_c"] {
        let r = conn.execute(
            &format!("INSERT INTO {t}(chunk_id, embedding, layer, scope, project_id) VALUES (1, ?1, 'regras', NULL, NULL)"),
            rusqlite::params![blob(&unit(0))],
        );
        println!("  {t} INSERT with NULL scope/project_id -> {}", match r { Ok(_) => "OK".to_string(), Err(e) => format!("FAIL: {e}") });
    }

    println!("\n== populate t_a with sentinel for NULL, then test in-KNN filtering");
    conn.execute_batch("DELETE FROM t_a")?;
    // 6 rows: layer ∈ {regras, sessoes}; sentinel '__NULL__' for missing scope/project.
    let rows: Vec<(i64, usize, &str, &str, i64)> = vec![
        (1, 0, "regras", "global", 1),
        (2, 1, "regras", "global", 2),
        (3, 2, "sessoes", "__NULL__", 1),
        (4, 3, "regras", "projetos", 1),
        (5, 0, "sessoes", "__NULL__", 1),
        (6, 1, "regras", "global", 1),
    ];
    for (id, s, layer, scope, pid) in &rows {
        conn.execute(
            "INSERT INTO t_a(chunk_id, embedding, layer, scope, project_id) VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![*id, blob(&unit(*s)), layer, scope, *pid],
        )?;
    }
    let q = blob(&unit(0));

    let mut st = conn.prepare(
        "SELECT chunk_id, distance FROM t_a WHERE embedding MATCH ?1 AND k = ?2 ORDER BY distance")?;
    let all: Vec<(i64, f64)> = st
        .query_map(rusqlite::params![q.clone(), 6i64], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    println!("  unfiltered k=6 -> {:?}", all.iter().map(|(i, _)| *i).collect::<Vec<_>>());

    // Now the same query with a layer filter INSIDE the KNN.
    match conn.prepare(
        "SELECT chunk_id FROM t_a WHERE embedding MATCH ?1 AND layer = 'regras' AND k = 6 ORDER BY distance") {
        Ok(mut st) => {
            let got: Vec<i64> = st
                .query_map(rusqlite::params![q.clone()], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            println!("  layer='regras' in-KNN  -> {got:?}  (expect only 1,2,4,6)");
        }
        Err(e) => println!("  layer='regras' in-KNN  -> PREPARE FAIL: {e}"),
    }

    match conn.prepare(
        "SELECT chunk_id FROM t_a WHERE embedding MATCH ?1 AND project_id = 2 AND k = 6 ORDER BY distance") {
        Ok(mut st) => {
            let got: Vec<i64> = st
                .query_map(rusqlite::params![q.clone()], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            println!("  project_id=2 in-KNN   -> {got:?}  (expect only 2)");
        }
        Err(e) => println!("  project_id=2 in-KNN   -> PREPARE FAIL: {e}"),
    }

    // The G6 trap: post-KNN filtering with a small K.
    let post: Vec<i64> = st
        .query_map(rusqlite::params![q.clone(), 2i64], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    println!("  post-filter w/ k=2     -> {post:?} then filter project_id=2 => (see spike)");

    Ok(())
}
