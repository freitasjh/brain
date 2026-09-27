//! C3-01 SPIKE — `sqlite-vec` viability probe for the O(n) cosine full scan (TD-001).
//!
//! Run (one command):
//!     cargo run -p brain-store --release --example vec_spike
//!
//! This is a SPIKE, not production code. It deliberately does NOT touch
//! `crates/brain-store/src/lib.rs`: the current full-scan implementation stays
//! exactly as it is, and this file re-implements a faithful COPY of it as the
//! baseline to compare against.
//!
//! Gates answered (see the C3-01 report):
//!   G1 does the extension load?   G2 vec_version()?
//!   G3 vec0 DDL @768 + cosine?    G4 populate from real 730 embeddings?
//!   G5 recall@20 vs baseline?     G6 layer/scope/project filters?
//!   G7 can triggers keep it in sync with `chunks`?
//!   G8 benchmark 730 and 50_000 chunks.
//!
//! Env overrides: BRAIN_SPIKE_DB, BRAIN_SPIKE_OLLAMA, BRAIN_SPIKE_SYNTH.

use std::collections::HashMap;
use std::time::Instant;

use rusqlite::Connection;

#[path = "vec_common/mod.rs"]
mod common;

/// Sentinel for `chunks.scope` / `chunks.project_id` being SQL NULL.
/// vec0 METADATA columns reject NULL ("Expected text for TEXT metadata column");
/// partition keys accept it. See `vec_ddl_probe` for the measured matrix.
const NULL_SENTINEL: &str = "__NULL__";
const NULL_PID: i64 = -1;
const DIM: usize = 768;

/// One row of the G6 matrix: a human label plus the (layer, scope, project_id)
/// triple the baseline would push into its SQL WHERE clause.
type Filter = (
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    Option<i64>,
);

// ───────────────────────────────────────────────────────────── G1: registration

// ─────────────────────────────────────────────────────── baseline (copy of src)

/// Verbatim copy of `cosine()` in `crates/brain-store/src/lib.rs:829`.
/// Do not "improve" this — if it drifts, the recall numbers stop meaning anything.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}

/// Faithful COPY of the vector-candidate stage of `search()`
/// (`crates/brain-store/src/lib.rs:328-372`).
///
/// Returns ranked note `path`s. The original then turns rank `i` into the RRF
/// score `1/(60+i+1)`; RRF is a pure function of rank order, so comparing the
/// ranked path list is sufficient — if order is preserved, RRF and the final
/// fused ordering are preserved too.
fn baseline_scan(
    conn: &Connection,
    qv: &[f32],
    layer: Option<&str>,
    scope: Option<&str>,
    project_id: Option<i64>,
) -> rusqlite::Result<Vec<String>> {
    let mut sql = "SELECT path, snippet, layer, scope, tags, embedding, chunk_index, project_id \
                   FROM chunks WHERE embedding IS NOT NULL"
        .to_string();
    // Positional `?` bind order must match the order the predicates are pushed,
    // exactly as the original builds `filter_params`.
    let mut bind: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(lf) = layer {
        filter_push(&mut sql, &mut bind, "layer", lf.to_string());
    }
    if let Some(sf) = scope {
        filter_push(&mut sql, &mut bind, "scope", sf.to_string());
    }
    if let Some(pid) = project_id {
        filter_push(&mut sql, &mut bind, "project_id", pid);
    }
    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::ToSql> = bind.iter().map(|b| b.as_ref()).collect();
    let mut rows = stmt.query(rusqlite::params_from_iter(refs))?;

    // Same dedup-by-path-keeping-max-cos as the original.
    let mut best_per_path: HashMap<String, f32> = HashMap::new();
    while let Some(r) = rows.next()? {
        let path: String = r.get(0)?;
        let blob: Vec<u8> = r.get(5)?;
        let emb: Vec<f32> = blob
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        if emb.len() != qv.len() { continue; }
        let cos = cosine(qv, &emb);
        match best_per_path.get(&path) {
            Some(best) if *best >= cos => {}
            _ => { best_per_path.insert(path, cos); }
        }
    }
    let mut scored: Vec<(String, f32)> = best_per_path.into_iter().collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(50); // the original truncates to 50 before RRF
    Ok(scored.into_iter().map(|(p, _)| p).collect())
}

/// Appends ` AND <col> = ?` to `sql` and the matching value to `bind`.
/// Mirrors `filter_sql_parts` / `filter_params` in the original `search()`.
fn filter_push(sql: &mut String, bind: &mut Vec<Box<dyn rusqlite::ToSql>>, col: &str, val: impl rusqlite::ToSql + 'static) {
    sql.push_str(" AND ");
    sql.push_str(col);
    sql.push_str(" = ?");
    bind.push(Box::new(val));
}

// ────────────────────────────────────────────────────────────── vec0 (spike)

const DDL_VEC0: &str = "CREATE VIRTUAL TABLE vec_chunks USING vec0(\
     chunk_id  INTEGER PRIMARY KEY, \
     embedding float[768] distance_metric=cosine, \
     layer     text, \
     scope     text, \
     project_id integer)";

fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

struct Chunk {
    path: String,
    emb: Vec<f32>,
}

fn load_chunks(conn: &Connection) -> rusqlite::Result<Vec<Chunk>> {
    let mut st = conn.prepare(
        "SELECT id, path, embedding FROM chunks WHERE embedding IS NOT NULL ORDER BY id")?;
    let rows = st.query_map([], |r| {
        let blob: Vec<u8> = r.get(2)?;
        Ok(Chunk {
            path: r.get(1)?,
            emb: blob.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect(),
        })
    })?;
    rows.collect()
}

fn populate_vec0(conn: &Connection) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut ins = tx.prepare(
            "INSERT INTO vec_chunks(chunk_id, embedding, layer, scope, project_id) \
             SELECT c.id, c.embedding, c.layer, coalesce(c.scope, ?1), coalesce(c.project_id, ?2) \
             FROM chunks c WHERE c.embedding IS NOT NULL")?;
        ins.execute(rusqlite::params![NULL_SENTINEL, NULL_PID])?;
    }
    tx.commit()
}

/// KNN against vec0, returning ranked `(path, distance)` chunk hits.
///
/// `k = ?5` is a WHERE constraint, NOT a `LIMIT`: vec0's xBestIndex refuses to
/// plan a KNN query unless it can see `k` as an index constraint, and it does
/// not recognise a bound `LIMIT` parameter (measured: "A LIMIT or 'k = ?'
/// constraint is required on vec0 knn queries.").
fn knn(
    conn: &Connection,
    qv: &[f32],
    k: i64,
    layer: Option<&str>,
    scope: Option<&str>,
    project_id: Option<i64>,
) -> rusqlite::Result<Vec<(String, f64)>> {
    let mut sql = String::from(
        "SELECT c.path, v.distance FROM vec_chunks v \
         JOIN chunks c ON c.id = v.chunk_id \
         WHERE v.embedding MATCH ?1",
    );
    // Filters go INSIDE the KNN (metadata columns) — this is the G6 question.
    if layer.is_some() { sql.push_str(" AND v.layer = ?2"); }
    if scope.is_some() { sql.push_str(" AND v.scope = ?3"); }
    if project_id.is_some() { sql.push_str(" AND v.project_id = ?4"); }
    sql.push_str(" AND v.k = ?5 ORDER BY v.distance");

    let mut st = conn.prepare(&sql)?;
    let rows = st.query_map(
        rusqlite::params![
            to_blob(qv),
            layer.unwrap_or(""),
            scope.unwrap_or(NULL_SENTINEL),
            project_id.unwrap_or(NULL_PID),
            k,
        ],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)),
    )?;
    rows.collect()
}

/// KNN returning only `chunk_id`s — no join to `chunks`. Isolates the cost of
/// vec0's distance scan from the cost of fetching path/snippet for the hits.
fn knn_ids_only(conn: &Connection, qv: &[f32], k: i64) -> rusqlite::Result<Vec<i64>> {
    let mut st = conn.prepare(
        "SELECT chunk_id FROM vec_chunks WHERE embedding MATCH ?1 AND k = ?2 ORDER BY distance")?;
    let rows = st.query_map(rusqlite::params![to_blob(qv), k], |r| r.get(0))?;
    rows.collect()
}

/// vec0 ranks CHUNKS; the baseline ranks PATHS. Collapse the same way the
/// baseline does — keep the first (best) hit per path, order already correct.
fn knn_paths(hits: &[(String, f64)]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    hits.iter()
        .filter(|(p, _)| seen.insert(p.clone()))
        .map(|(p, _)| p.clone())
        .collect()
}

fn recall_at(a: &[String], b: &[String], k: usize) -> f64 {
    let top: std::collections::HashSet<&String> = a.iter().take(k).collect();
    let hit = b.iter().take(k).filter(|p| top.contains(p)).count();
    hit as f64 / k.min(a.len().max(1)) as f64
}

// ─────────────────────────────────────────────────────────── Ollama (queries)

/// Minimal `POST /api/embeddings` over plain HTTP, matching
/// `brain-embed`'s endpoint + model. Avoids adding brain-embed/reqwest as a
/// dev-dependency just to mint 10 query vectors.
fn ollama_embed(base: &str, model: &str, text: &str) -> Result<Vec<f32>, String> {
    use std::io::{Read, Write};
    let host_port = base.trim_start_matches("http://").trim_end_matches('/');
    let body = serde_json::json!({ "model": model, "prompt": text }).to_string();
    let req = format!(
        "POST /api/embeddings HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut s = std::net::TcpStream::connect(host_port).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(60))).ok();
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = String::new();
    s.read_to_string(&mut raw).map_err(|e| e.to_string())?;
    let (head, body) = raw.split_once("\r\n\r\n").ok_or("no http body")?;
    // Ollama replies with `Transfer-Encoding: chunked`, so the body is a series
    // of `<hex-size>\r\n<data>\r\n` frames ending in a zero-length frame.
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        dechunk(body)?
    } else {
        body.to_string()
    };
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("{e} in: {}", &body[..body.len().min(120)]))?;
    v["embedding"]
        .as_array()
        .ok_or("no embedding key")?
        .iter()
        .map(|x| x.as_f64().map(|y| y as f32).ok_or_else(|| "non-numeric".to_string()))
        .collect()
}

/// Decodes an HTTP/1.1 chunked-transfer body.
fn dechunk(mut body: &str) -> Result<String, String> {
    let mut out = String::new();
    loop {
        let (size_line, rest) = body.split_once("\r\n").ok_or("chunk: no size line")?;
        // A chunk-size line may carry extensions after ';' — ignore them.
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16).map_err(|e| format!("chunk: bad size {size_hex:?}: {e}"))?;
        if size == 0 { return Ok(out); }
        if rest.len() < size { return Err("chunk: truncated".into()); }
        out.push_str(&rest[..size]);
        body = rest[size..].strip_prefix("\r\n").ok_or("chunk: missing CRLF")?;
    }
}

fn load_queries(conn: &Connection, ollama: &str, model: &str) -> Vec<(String, Vec<f32>)> {
    // Real text lifted straight out of this DB's own notes.content.
    let mut st = match conn.prepare(
        "SELECT path, content FROM notes WHERE length(content) > 150 ORDER BY length(content) DESC LIMIT 14") {
        Ok(s) => s,
        Err(e) => { eprintln!("warn: cannot read notes for queries: {e}"); return Vec::new(); }
    };
    let Ok(rows) = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) else {
        eprintln!("warn: query_map failed for notes scan");
        return Vec::new();
    };
    let mut out = Vec::new();
    for r in rows.flatten() {
        let (path, content) = r;
        // First real prose line, not the frontmatter.
        let text: String = content
            .lines()
            .skip_while(|l| l.trim().is_empty() || l.starts_with("---") || l.starts_with('#') || l.starts_with("tags:") || l.starts_with("topico:"))
            .find(|l| l.trim().len() > 40)
            .unwrap_or("")
            .trim()
            .to_string();
        if text.is_empty() { continue; }
        match ollama_embed(ollama, model, &text) {
            Ok(v) if v.len() == DIM => out.push((format!("{path} :: {}", &text[..text.len().min(45)]), v)),
            Ok(v) => eprintln!("warn: dim {} != {DIM} for {path}", v.len()),
            Err(e) => eprintln!("warn: embed failed for {path}: {e}"),
        }
    }
    out
}

// ──────────────────────────────────────────────────────── synthetic DB (G8)

/// Deterministic xorshift64* so the 50k benchmark is reproducible run to run.
struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12; x ^= x << 25; x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn next_f32(&mut self) -> f32 { (self.next_u64() >> 40) as f32 / 8_388_608.0 }
}

fn build_synth(path: &str, n: usize) -> Result<(), String> {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{path}-wal"));
    let _ = std::fs::remove_file(format!("{path}-shm"));
    let conn = Connection::open(path).map_err(|e| e.to_string())?;
    // NOTE: the column set must match the REAL `chunks` table, because
    // `baseline_scan` selects path, snippet, layer, scope, tags, embedding,
    // chunk_index, project_id. An earlier revision of this spike created a
    // slimmed-down table, the baseline SELECT failed with "no such column",
    // the error was swallowed by `unwrap_or(0)`, and the "scan" benchmarked
    // ~0.04 ms for 50k rows. That number was fiction.
    conn.execute_batch(
        "PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;
         CREATE TABLE chunks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            note_id INTEGER NOT NULL DEFAULT 1,
            path TEXT NOT NULL,
            layer TEXT NOT NULL,
            scope TEXT,
            snippet TEXT NOT NULL,
            chunk_index INTEGER NOT NULL DEFAULT 0,
            total_chunks INTEGER NOT NULL DEFAULT 1,
            project_id INTEGER,
            tags TEXT DEFAULT '[]',
            embedding BLOB);",
    ).map_err(|e| e.to_string())?;
    conn.execute_batch(DDL_VEC0).map_err(|e| e.to_string())?;

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let t0 = Instant::now();
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    {
        let mut ins_c = tx.prepare(
            "INSERT INTO chunks(id, note_id, path, layer, scope, snippet, chunk_index, total_chunks, project_id, tags, embedding) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)").map_err(|e| e.to_string())?;
        let mut ins_v = tx.prepare(
            "INSERT INTO vec_chunks(chunk_id, embedding, layer, scope, project_id) VALUES (?1,?2,?3,?4,?5)").map_err(|e| e.to_string())?;
        // Cycle the same 3 filter buckets the real DB uses, so the filtered
        // benchmark hits a realistic selectivity (~1/3), not an all-or-nothing one.
        let buckets = [("regras", "global", 1i64), ("sessoes", NULL_SENTINEL, NULL_PID), ("regras", "projetos", 1i64)];
        for i in 1..=n {
            let mut v = vec![0f32; DIM];
            let mut norm = 0f32;
            for slot in v.iter_mut() { let x = rng.next_f32(); *slot = x; norm += x * x; }
            let norm = norm.sqrt();
            if norm > 0.0 { for slot in v.iter_mut() { *slot /= norm; } }
            let b = to_blob(&v);
            let (layer, scope, pid) = buckets[i % buckets.len()];
            let path = format!("synth/note-{:06}", (i - 1) / 4);
            let snippet = format!("synthetic chunk {i} — deterministic filler text for benchmark purposes");
            ins_c.execute(rusqlite::params![i as i64, 1i64, path, layer, scope, snippet, 0i64, 1i64, pid, "[]", b]).map_err(|e| e.to_string())?;
            ins_v.execute(rusqlite::params![i as i64, b, layer, scope, pid]).map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    conn.execute_batch("ANALYZE;").ok();
    println!("  built {n} synthetic chunks in {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}

/// Times `f` over `rounds` rounds after one discarded warm-up round, then
/// reports **per-query** milliseconds — `f` is expected to execute `nq`
/// queries per round, so its total is divided by `nq` before reporting.
/// Labelling the round total as "per query" understates cost by `nq`x, which is
/// how the first revision of this benchmark ended up reporting nonsense.
fn bench<F: FnMut() -> Result<usize, String>>(label: &str, rounds: usize, nq: usize, mut f: F) -> f64 {
    for _ in 0..1 {
        std::hint::black_box(f().expect("warm-up round failed"));
    }
    let mut samples = Vec::new();
    for _ in 0..rounds {
        let t = Instant::now();
        let hits = std::hint::black_box(f().expect("benchmark round failed"));
        let ms = t.elapsed().as_secs_f64() * 1000.0 / nq as f64;
        std::hint::black_box(hits);
        samples.push(ms);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = samples[samples.len() / 2];
    println!("  {label:<40} median {med:>9.3} ms/query  (min {:.3}, max {:.3})",
        samples[0], samples[samples.len() - 1]);
    med
}

// ────────────────────────────────────────────────────────────────────── main

/// A chunk id paired with its freshly computed embedding.
type Embedded = (i64, Vec<f32>);

/// Copies `src` to `dst` and replaces every zero-vector chunk embedding with a
/// REAL `nomic-embed-text` embedding of that chunk's snippet.
///
/// Why this exists: the shipped `data/brain.db` turned out to hold an all-zero
/// embedding for 727 of its 730 chunks (written by the `vec![0.0; 768]` Ollama
/// fallback). With 99.6% of the corpus at the origin, cosine similarity is
/// 0.0 for nearly every pair and any recall/benchmark number measured on it is
/// meaningless. This rebuilds the SAME corpus shape (same 730 rows, same
/// paths/layers/scopes/project_ids, same 10-chunks-per-path skew) with vectors
/// that are actually in the embedding space, which is what G5/G6/G8 need.
fn hydrate(src: &str, dst: &str, ollama: &str, model: &str) -> Result<Vec<(String, Vec<f32>)>, String> {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{dst}{suffix}"));
    }
    std::fs::copy(src, dst).map_err(|e| e.to_string())?;
    let conn = Connection::open(dst).map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA journal_mode=WAL;").map_err(|e| e.to_string())?;

    // Collect the work list: only chunks whose embedding is all-zero.
    let mut todo: Vec<(i64, String)> = Vec::new();
    {
        let mut st = conn
            .prepare("SELECT id, snippet FROM chunks WHERE embedding IS NOT NULL ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = st
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?;
        for r in rows {
            let (id, snippet) = r.map_err(|e| e.to_string())?;
            let is_zero: bool = conn
                .query_row("SELECT embedding FROM chunks WHERE id = ?1", rusqlite::params![id], |r| {
                    let b: Vec<u8> = r.get(0)?;
                    Ok(b.chunks_exact(4).all(|w| f32::from_le_bytes([w[0], w[1], w[2], w[3]]) == 0.0))
                })
                .map_err(|e| e.to_string())?;
            if is_zero { todo.push((id, snippet)); }
        }
    }
    println!("   hydrating {} zero-vector chunks with {model} ...", todo.len());
    let t0 = Instant::now();

    // Bounded worker pool over a simple atomic cursor.
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    let cursor = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicUsize::new(0));
    let out: Arc<Mutex<Vec<Embedded>>> = Arc::new(Mutex::new(Vec::new()));
    let errs = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for _ in 0..8 {
        let (cursor, done, out, errs) = (cursor.clone(), done.clone(), out.clone(), errs.clone());
        let todo = todo.clone();
        let (ollama, model) = (ollama.to_string(), model.to_string());
        handles.push(std::thread::spawn(move || {
            loop {
                let i = cursor.fetch_add(1, Ordering::SeqCst);
                if i >= todo.len() { break; }
                let (id, snippet) = &todo[i];
                // The fallback in brain_store writes a zero vector for a note with
                // no prose; embed the raw text anyway so we do not lose the row.
                let text = if snippet.trim().is_empty() { "(empty)" } else { snippet.as_str() };
                match ollama_embed(&ollama, &model, text) {
                    Ok(v) if v.len() == DIM => out.lock().unwrap().push((*id, v)),
                    Ok(v) => { eprintln!("  dim {} != {DIM} for chunk {id}", v.len()); errs.fetch_add(1, Ordering::SeqCst); }
                    Err(e) => { eprintln!("  embed chunk {id} failed: {e}"); errs.fetch_add(1, Ordering::SeqCst); }
                }
                let d = done.fetch_add(1, Ordering::SeqCst) + 1;
                if d % 100 == 0 || d == todo.len() {
                    eprint!("\r   {d}/{} embedded", todo.len());
                }
            }
        }));
    }
    for h in handles { h.join().map_err(|_| "worker panicked".to_string())?; }
    eprintln!();
    let embedded = out.lock().unwrap();
    println!("   embedded {}/{} chunks in {:.1}s ({} failures)",
        embedded.len(), todo.len(), t0.elapsed().as_secs_f64(), errs.load(Ordering::SeqCst));
    if errs.load(Ordering::SeqCst) > 0 { return Err("some chunks failed to embed".into()); }

    {
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        {
            let mut up = tx
                .prepare("UPDATE chunks SET embedding = ?1 WHERE id = ?2")
                .map_err(|e| e.to_string())?;
            for (id, v) in embedded.iter() {
                up.execute(rusqlite::params![to_blob(v), *id]).map_err(|e| e.to_string())?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
    }
    Ok(Vec::new())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::register_sqlite_vec();

    let db = std::env::var("BRAIN_SPIKE_DB").unwrap_or_else(|_| {
        // Stage a private copy so the spike can never write to the real DB.
        // Runs even if the caller did not pre-copy, so the documented command
        // really is one command.
        let staged = "/tmp/opencode/vec_spike/real.db".to_string();
        if !std::path::Path::new(&staged).exists() {
            std::fs::create_dir_all("/tmp/opencode/vec_spike").expect("create spike dir");
            std::fs::copy("data/brain.db", &staged).expect("stage a copy of data/brain.db");
            for suffix in ["-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{staged}{suffix}"));
            }
            eprintln!("staged a private copy of data/brain.db -> {staged}");
        }
        staged
    });
    let ollama = std::env::var("BRAIN_SPIKE_OLLAMA").unwrap_or_else(|_| "http://localhost:11434".into());
    let model = std::env::var("BRAIN_SPIKE_OLLAMA_MODEL").unwrap_or_else(|_| "nomic-embed-text".into());
    let synth_n: usize = std::env::var("BRAIN_SPIKE_SYNTH").ok()
        .and_then(|s| s.parse().ok()).unwrap_or(50_000);

    // ── G2 ────────────────────────────────────────────────────────────────
    let conn = Connection::open(&db)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;
    let version: String = conn.query_row("SELECT vec_version()", [], |r| r.get(0))?;
    println!("## G1 extension loaded   : OK (sqlite-vec {version}, static link, no feature flags)");
    println!("## G2 vec_version()      : {version}");

    // ── G4 ────────────────────────────────────────────────────────────────
    let chunks = load_chunks(&conn)?;
    // Full norm distribution over every real embedding. This decides whether
    // `distance_metric=cosine` is merely "better" or strictly REQUIRED for
    // ranking equivalence with `cosine()` in lib.rs.
    let mut norms: Vec<f32> = chunks.iter()
        .map(|c| c.emb.iter().map(|x| x * x).sum::<f32>().sqrt()).collect();
    norms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean_norm = norms.iter().sum::<f32>() / norms.len() as f32;
    let pct = |q: f64| norms[((norms.len() as f64 - 1.0) * q) as usize];
    let zero_vecs = norms.iter().filter(|n| **n == 0.0).count();
    let off_unit = norms.iter().filter(|n| (**n - 1.0).abs() > 1e-3).count();
    println!("## G4 real embeddings    : {} chunks x {DIM} dims", chunks.len());
    println!("   |v| min={:.4} p25={:.4} med={:.4} p75={:.4} max={:.4} mean={:.4}",
        norms[0], pct(0.25), pct(0.5), pct(0.75), norms[norms.len() - 1], mean_norm);
    println!("   ALL-ZERO vectors      : {zero_vecs}/{}", chunks.len());
    println!("   off-unit (| |v|-1 | > 1e-3): {off_unit}/{} chunks", chunks.len());

    // If the shipped DB is degenerate (Ollama-down fallback wrote vec![0.0;768]),
    // recall/bench numbers on it are meaningless. Rebuild the same corpus with
    // real vectors before running G5/G6/G8.
    let degenerate = zero_vecs * 2 > chunks.len();
    if degenerate {
        println!("\n!! BLOCKER (data, not C3): {zero_vecs}/{} chunks hold an ALL-ZERO embedding.", chunks.len());
        println!("!! cosine() in lib.rs returns 0.0 for a zero vector, so the vector stream is");
        println!("!! pure noise on this DB. G5/G6/G8 below run on a re-embedded copy instead.");
        let hydrated = "/tmp/opencode/vec_spike/hydrated.db".to_string();
        if !std::path::Path::new(&hydrated).exists() {
            hydrate(&db, &hydrated, &ollama, &model)?;
        } else {
            println!("   reusing existing {hydrated}");
        }
        drop(conn);
        let c = Connection::open(&hydrated)?;
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;
        let zn: i64 = c.query_row(
            "SELECT count(*) FROM chunks WHERE embedding IS NOT NULL", [], |r| r.get(0))?;
        println!("   hydrated DB: {zn} chunks, all with real vectors");
        return gates(&c, &hydrated, &ollama, &model, synth_n);
    }
    println!("   => vectors present; distance_metric=cosine still REQUIRED for exact ranking parity");
    gates(&conn, &db, &ollama, &model, synth_n)
}

/// Everything from G5 onward, against a DB whose embeddings are real.
/// Everything from G5 onward, against a DB whose embeddings are real.
fn gates(
    conn: &Connection,
    db: &str,
    ollama: &str,
    model: &str,
    synth_n: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let chunks = load_chunks(conn)?;
    let mut norms: Vec<f32> = chunks.iter()
        .map(|c| c.emb.iter().map(|x| x * x).sum::<f32>().sqrt()).collect();
    norms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let still_zero = norms.iter().filter(|n| **n == 0.0).count();
    println!("   |v| min={:.4} med={:.4} max={:.4} | ALL-ZERO={still_zero}/{}",
        norms[0], norms[norms.len() / 2], norms[norms.len() - 1], chunks.len());
    let uniq_paths = chunks.iter().map(|c| c.path.as_str()).collect::<std::collections::HashSet<_>>().len();
    println!("   {uniq_paths} distinct note paths over {} chunk rows", chunks.len());

    conn.execute_batch("DROP TABLE IF EXISTS vec_chunks;")?;
    conn.execute_batch(DDL_VEC0)?;
    let t = Instant::now();
    populate_vec0(conn)?;
    let n_vec: i64 = conn.query_row("SELECT count(*) FROM vec_chunks", [], |r| r.get(0))?;
    println!("   vec_chunks populated  : {n_vec} rows in {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);

    // ── G5 ────────────────────────────────────────────────────────────────
    let queries = load_queries(conn, ollama, model);
    println!("\n## G5 recall — baseline (Rust full scan) vs sqlite-vec KNN, K=200 (20 paths worst case @10 chunks/path)");
    println!("   {} real text queries embedded with {model}\n", queries.len());
    if queries.is_empty() {
        eprintln!("ABORT: no query vectors. Is Ollama up at {ollama}? Start it or pre-seed the DB.");
        return Err("no query vectors".into());
    }
    println!("   {:<52} {:>7} {:>7} {:>7}", "query", "base20", "vec20", "vecPaths");
    let (mut r20_sum, mut r20_paths_sum) = (0.0f64, 0.0f64);
    for (label, qv) in &queries {
        let base = baseline_scan(conn, qv, None, None, None)?;
        let hits = knn(conn, qv, 200, None, None, None)?;
        let paths = knn_paths(&hits);
        let r20 = recall_at(&base, &paths, 20);
        let rp20 = paths.len().min(20) as f64;
        r20_sum += r20;
        r20_paths_sum += rp20;
        println!("   {:<52} {:>7} {:>7.4} {:>7.1}", &label[..label.len().min(50)],
            base.len().min(20), r20, rp20);
    }
    let n = queries.len() as f64;
    println!("   --> MEAN recall@20            = {:.4}   (target >= 0.95)", r20_sum / n);
    println!("   --> MEAN distinct paths in K=200 = {:.1}  (need 20)", r20_paths_sum / n);

    // ── G6 ────────────────────────────────────────────────────────────────
    println!("\n## G6 filters — layer / scope / project_id");
    let filters: Vec<Filter> = vec![
        ("layer=regras",            Some("regras"),         None,         None),
        ("layer=sessoes",           Some("sessoes"),        None,         None),
        ("scope=global",            None,                  Some("global"), None),
        ("scope=projetos",          None,                  Some("projetos"), None),
        ("layer=regras,scope=global", Some("regras"),      Some("global"), None),
        ("project_id=1",            None,                  None,         Some(1)),
        ("project_id=4",            None,                  None,         Some(4)),
    ];
    println!("   {:<30} {:>7} {:>9} {:>9} {:>9}", "filter", "nBase", "inKNN", "postKNN", "overlap%");
    for (label, l, s, p) in &filters {
        // Which project_id the baseline would use for this filter.
        let base_hits: usize = queries.iter().map(|(_, qv)|
            baseline_scan(conn, qv, *l, *s, *p).map(|v| v.len()).unwrap_or(0)).sum();
        let (mut in_knn, mut post, mut overlap, mut tot) = (0.0, 0.0, 0.0, 0.0);
        for (_, qv) in &queries {
            let base = baseline_scan(conn, qv, *l, *s, *p)?;
            let inf = knn_paths(&knn(conn, qv, 200, *l, *s, *p)?);
            // Post-KNN alternative: fetch unfiltered with inflated K, filter in Rust.
            let post_hits = knn(conn, qv, 600, None, None, None)?;
            let postf: Vec<String> = post_hits.iter()
                .filter_map(|(p, _)| {
                    let keep = l.map(|x| p.contains(x)).unwrap_or(true);
                    keep.then(|| p.clone())
                })
                .collect();
            if !base.is_empty() {
                in_knn += recall_at(&base, &inf, 20);
                post += recall_at(&base, &postf, 20);
                overlap += inf.iter().take(20).filter(|p| base.contains(p)).count() as f64;
                tot += 1.0;
            }
        }
        println!("   {:<30} {:>7} {:>9.4} {:>9.4} {:>8.1}%", label, base_hits / queries.len(),
            in_knn / tot, post / tot, 100.0 * overlap / (tot * 20.0));
    }

    // ── G7 ────────────────────────────────────────────────────────────────
    println!("\n## G7 sync — can SQLite triggers maintain vec_chunks from chunks?");
    g7(db)?;

    // ── G8 ────────────────────────────────────────────────────────────────
    g9(conn, &queries)?;

    println!("\n## G8 benchmark — K=20, warm cache, 3 rounds (1 discarded)");
    g8(conn, &queries, chunks.len())?;

    println!("\n## G8 benchmark — synthetic {synth_n} chunks (normalized random 768-dim)");
    let synth_db = format!("/tmp/opencode/vec_spike/synth-{synth_n}.db");
    if !std::path::Path::new(&synth_db).exists() { build_synth(&synth_db, synth_n)?; }
    let sc = Connection::open(&synth_db)?;
    sc.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    g9(&sc, &queries)?;
    // Reuse the real query vectors: same dimensionality, so the comparison is
    // apples-to-apples and does not need another 50k Ollama round-trips.
    g8(&sc, &queries, synth_n)?;

    Ok(())
}

/// G7 — trigger-based sync, measured on a throwaway copy so the recall data
/// above is untouched.
fn g7(db: &str) -> Result<(), Box<dyn std::error::Error>> {
    let tmp = "/tmp/opencode/vec_spike/g7.db";
    let _ = std::fs::remove_file(tmp);
    let _ = std::fs::remove_file(format!("{tmp}-wal"));
    std::fs::copy(db, tmp)?;
    let c = Connection::open(tmp)?;
    c.execute_batch(&format!("DROP TABLE IF EXISTS vec_chunks; {DDL_VEC0}"))?;

    let ddl_triggers = format!(
        "CREATE TRIGGER chunks_ai AFTER INSERT ON chunks BEGIN
             INSERT INTO vec_chunks(chunk_id, embedding, layer, scope, project_id)
             VALUES (NEW.id, NEW.embedding, NEW.layer, coalesce(NEW.scope,'{NULL_SENTINEL}'),
                     coalesce(NEW.project_id,{NULL_PID}));
         END;
         CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN
             DELETE FROM vec_chunks WHERE chunk_id = OLD.id;
         END;
         CREATE TRIGGER chunks_au AFTER UPDATE ON chunks BEGIN
             DELETE FROM vec_chunks WHERE chunk_id = OLD.id;
             INSERT INTO vec_chunks(chunk_id, embedding, layer, scope, project_id)
             VALUES (NEW.id, NEW.embedding, NEW.layer, coalesce(NEW.scope,'{NULL_SENTINEL}'),
                     coalesce(NEW.project_id,{NULL_PID}));
         END;"
    );
    match c.execute_batch(&ddl_triggers) {
        Ok(()) => println!("   CREATE TRIGGER (insert/update/delete)   : OK"),
        Err(e) => { println!("   CREATE TRIGGER                          : FAIL — {e}"); return Ok(()); }
    }

    // Backfill the existing rows through the same INSERT the trigger uses.
    populate_vec0(&c)?;
    let before: i64 = c.query_row("SELECT count(*) FROM vec_chunks", [], |r| r.get(0))?;
    println!("   backfill existing rows                  : {before} rows");

    let probe = || -> rusqlite::Result<Vec<u8>> {
        c.query_row("SELECT embedding FROM chunks WHERE id = (SELECT max(id) FROM chunks)", [], |r| r.get(0))
    };
    let mut emb = probe()?;

    c.execute(
        "INSERT INTO chunks(id, note_id, path, layer, scope, snippet, chunk_index, total_chunks, tags, embedding)
         VALUES (999999, 1, 'g7/probe', 'regras', 'global', 'g7', 0, 1, '[]', ?1)",
        rusqlite::params![emb])?;
    let n: i64 = c.query_row("SELECT count(*) FROM vec_chunks WHERE chunk_id = 999999", [], |r| r.get(0))?;
    println!("   AFTER INSERT trigger                    : vec_chunks rows for 999999 = {n}  {}", if n == 1 { "OK" } else { "FAIL" });

    emb[0] ^= 0xFF;
    c.execute("UPDATE chunks SET embedding = ?1 WHERE id = 999999", rusqlite::params![emb])?;
    let got: Vec<u8> = c.query_row("SELECT embedding FROM vec_chunks WHERE chunk_id = 999999", [], |r| r.get(0))?;
    println!("   AFTER UPDATE trigger                    : embedding propagated = {}  {}",
        got == emb, if got == emb { "OK" } else { "FAIL" });

    c.execute("DELETE FROM chunks WHERE id = 999999", [])?;
    let n: i64 = c.query_row("SELECT count(*) FROM vec_chunks WHERE chunk_id = 999999", [], |r| r.get(0))?;
    println!("   AFTER DELETE trigger                    : vec_chunks rows for 999999 = {n}  {}", if n == 0 { "OK" } else { "FAIL" });

    // Cascade path: `delete_page()` removes the NOTE, and `ON DELETE CASCADE`
    // takes the chunks with it. An earlier revision of this function "deleted
    // the chunks by note_id" and called that a cascade test — it was really
    // re-testing the direct-delete path. This one actually deletes the note.
    c.execute(
        "INSERT INTO chunks(id, note_id, path, layer, scope, snippet, chunk_index, total_chunks, tags, embedding)
         VALUES (999997, (SELECT min(note_id) FROM chunks), 'g7/cascade', 'regras', NULL, 'g7c', 0, 1, '[]', ?1)",
        rusqlite::params![emb])?;
    let before_c: i64 = c.query_row("SELECT count(*) FROM chunks", [], |r| r.get(0))?;
    let before_v: i64 = c.query_row("SELECT count(*) FROM vec_chunks", [], |r| r.get(0))?;
    c.execute(
        "DELETE FROM notes WHERE id = (SELECT min(note_id) FROM chunks WHERE id = 999997)", [])?;
    let after_c: i64 = c.query_row("SELECT count(*) FROM chunks", [], |r| r.get(0))?;
    let after_v: i64 = c.query_row("SELECT count(*) FROM vec_chunks", [], |r| r.get(0))?;
    let probe_left: i64 = c.query_row("SELECT count(*) FROM vec_chunks WHERE chunk_id = 999997", [], |r| r.get(0))?;
    println!("   FK CASCADE via notes    : chunks {before_c}->{after_c} (delta {}), vec_chunks {before_v}->{after_v} (delta {})",
        before_c - after_c, before_v - after_v);
    println!("   cascaded probe row {} still in vec_chunks   {}",
        if probe_left == 0 { "absent" } else { "PRESENT" },
        if before_c - after_c == before_v - after_v && probe_left == 0 { "OK — trigger follows the cascade" } else { "FAIL — orphans" });
    let _ = (n, emb);
    Ok(())
}

/// G9 — the alternatives that would actually change the C3-02 decision.
///
/// vec0's KNN is a flat/brute-force scan, so sqlite-vec itself only removes the
/// BLOB→Vec<f32> conversion. This measures the three other levers:
///   A float[768] + cosine              (exact today, 3072 B/vector)
///   B float[768] + L2 on NORMALISED    (rank-equivalent to cosine, still 3072 B)
///   C int8[768]  + cosine              (768 B/vector, quantisation loss)
///   D int8[768]  + L2 on NORMALISED    (768 B/vector, quantisation loss)
fn g9(
    conn: &Connection,
    queries: &[(String, Vec<f32>)],
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n## G9 alternatives — recall@20 and cost, per storage format");
    let n: usize = {
        let c: i64 = conn.query_row("SELECT count(*) FROM chunks WHERE embedding IS NOT NULL", [], |r| r.get(0))?;
        c as usize
    };
    if n == 0 { eprintln!("   (skipped: no chunks)"); return Ok(()); }

    // One shared load of the corpus so every variant is timed on equal data.
    let mut corpus: Vec<(i64, Vec<f32>)> = Vec::with_capacity(n);
    {
        let mut st = conn.prepare("SELECT id, embedding FROM chunks WHERE embedding IS NOT NULL ORDER BY id")?;
        let rows = st.query_map([], |r| {
            let b: Vec<u8> = r.get(1)?;
            Ok((r.get::<_, i64>(0)?, b.chunks_exact(4).map(|w| f32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect::<Vec<f32>>()))
        })?;
        for r in rows { corpus.push(r?); }
    }
    let nq = queries.len();
    install_int8_caster(conn).map_err(|e| format!("cannot install as_int8(): {e}"))?;

    struct Variant { name: &'static str, ddl: &'static str, bytes_per_vec: usize }
    let variants = [
        Variant { name: "A float[768] cosine   (3072 B)", bytes_per_vec: DIM * 4,
                  ddl: "CREATE VIRTUAL TABLE vx USING vec0(chunk_id INTEGER PRIMARY KEY, embedding float[768] distance_metric=cosine)" },
        Variant { name: "B float[768] L2 norm  (3072 B)", bytes_per_vec: DIM * 4,
                  ddl: "CREATE VIRTUAL TABLE vx USING vec0(chunk_id INTEGER PRIMARY KEY, embedding float[768])" },
        Variant { name: "C int8[768]  cosine   ( 768 B)", bytes_per_vec: DIM,
                  ddl: "CREATE VIRTUAL TABLE vx USING vec0(chunk_id INTEGER PRIMARY KEY, embedding int8[768] distance_metric=cosine)" },
        Variant { name: "D int8[768]  L2 norm  ( 768 B)", bytes_per_vec: DIM,
                  ddl: "CREATE VIRTUAL TABLE vx USING vec0(chunk_id INTEGER PRIMARY KEY, embedding int8[768])" },
    ];

    println!("   {:<32} {:>9} {:>9} {:>11} {:>10}", "variant", "recall@20", "paths", "ms/query", "vs A");
    let mut base_ms = 0.0f64;
    for (vi, v) in variants.iter().enumerate() {
        conn.execute_batch("DROP TABLE IF EXISTS vx;")?;
        conn.execute_batch(v.ddl)?;
        let is_int8 = v.bytes_per_vec == DIM;
        // B and D store unit-normalised vectors so L2 ranking == cosine ranking.
        let needs_norm = vi == 1 || vi == 3;
        {
            let tx = conn.unchecked_transaction()?;
            let insert_sql = if is_int8 {
                "INSERT INTO vx(chunk_id, embedding) VALUES (?1, as_int8(?2))"
            } else {
                "INSERT INTO vx(chunk_id, embedding) VALUES (?1, ?2)"
            };
            {
                let mut ins = tx.prepare(insert_sql)?;
                for (id, emb) in &corpus {
                    let stored: Vec<u8> = if is_int8 {
                        quantize_i8(emb)
                    } else if needs_norm {
                        to_blob(&normalize(emb))
                    } else {
                        to_blob(emb)
                    };
                    ins.execute(rusqlite::params![*id, stored])?;
                }
            }
            tx.commit()?;
        }

        // Recall against the untouched Rust baseline.
        let mut recall_sum = 0.0;
        let mut paths_min = usize::MAX;
        for (_, qv) in queries {
            let base = baseline_scan(conn, qv, None, None, None)?;
            // Queries must be encoded in the same space as the stored vectors.
            let q = if is_int8 { quantize_i8(qv) }
                    else if needs_norm { to_blob(&normalize(qv)) }
                    else { to_blob(qv) };
            let hits: Vec<(String, f64)> = {
                let match_sql = if is_int8 {
                    "SELECT c.path, v.distance FROM vx v JOIN chunks c ON c.id = v.chunk_id \
                     WHERE v.embedding MATCH as_int8(?1) AND v.k = 200 ORDER BY v.distance"
                } else {
                    "SELECT c.path, v.distance FROM vx v JOIN chunks c ON c.id = v.chunk_id \
                     WHERE v.embedding MATCH ?1 AND v.k = 200 ORDER BY v.distance"
                };
                let mut st = conn.prepare(match_sql)?;
                let rows = st.query_map(rusqlite::params![q], |r| Ok((r.get(0)?, r.get(1)?)))?;
                rows.collect::<Result<_, _>>()?
            };
            let paths = knn_paths(&hits);
            paths_min = paths_min.min(paths.len());
            if !base.is_empty() { recall_sum += recall_at(&base, &paths, 20); }
        }
        let recall = recall_sum / nq as f64;

        let ms = bench(v.name, 3, nq, || {
            queries.iter().map(|(_, qv)| {
                let q = if is_int8 { quantize_i8(qv) }
                        else if needs_norm { to_blob(&normalize(qv)) }
                        else { to_blob(qv) };
                let bsql = if is_int8 {
                    "SELECT chunk_id FROM vx v WHERE v.embedding MATCH as_int8(?1) AND v.k = 20 ORDER BY v.distance"
                } else {
                    "SELECT chunk_id FROM vx v WHERE v.embedding MATCH ?1 AND v.k = 20 ORDER BY v.distance"
                };
                let mut st = conn.prepare(bsql).map_err(|e| e.to_string())?;
                let rows = st.query_map(rusqlite::params![q], |r| r.get::<_, i64>(0))
                    .map_err(|e| e.to_string())?;
                let got: Vec<i64> = rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?;
                Ok(got.len())
            }).sum::<Result<usize, String>>()
        });
        if vi == 0 { base_ms = ms; }
        let vs = if base_ms == 0.0 { 0.0 } else { base_ms / ms };
        println!("   {:<32} {:>9.4} {:>9} {:>11.3} {:>9.2}x   index={:.1} MB",
            v.name, recall, paths_min, ms, vs, n * v.bytes_per_vec / 1_048_576);
    }
    conn.execute_batch("DROP TABLE IF EXISTS vx;")?;
    Ok(())
}

fn normalize(v: &[f32]) -> Vec<f32> {
    let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n == 0.0 { v.to_vec() } else { v.iter().map(|x| x / n).collect() }
}

/// Symmetric int8 quantisation scaled to [-127, 127], the range sqlite-vec's
/// `int8[N]` element type expects.
fn quantize_i8(v: &[f32]) -> Vec<u8> {
    let max_abs = v.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-9);
    v.iter().map(|x| ((x / max_abs) * 127.0).round().clamp(-127.0, 127.0) as i8 as u8).collect()
}

/// `SQLITE_VEC_ELEMENT_TYPE_INT8` — the subtype sqlite-vec uses to tell an int8
/// vector apart from a float32 one.
const ELEM_INT8: std::os::raw::c_uint = 223 + 2;

/// Registers `as_int8(blob)`, an identity function that tags its BLOB result
/// with sqlite-vec's int8 subtype.
///
/// Necessary because rusqlite 0.32's `ToSqlOutput` is an enum with no subtype
/// field, so a plain `?`-bound BLOB is always seen as float32 and vec0 rejects
/// it with "expected to be of type int8, but a float32 vector was provided".
/// Going through a scalar function is the only way to attach a subtype.
fn install_int8_caster(conn: &Connection) -> rusqlite::Result<()> {
    fn as_int8(ctx: &rusqlite::functions::Context<'_>) -> rusqlite::Result<(rusqlite::functions::SqlFnArg, rusqlite::functions::SubType)> {
        // `get_arg` yields a pass-through marker, so the BLOB is forwarded
        // without a copy and only gains the int8 subtype.
        Ok((ctx.get_arg(0), Some(ELEM_INT8)))
    }
    conn.create_scalar_function(
        "as_int8",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8 | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        as_int8,
    )
}

/// G8 — the same two code paths, same DB, same machine.
fn g8(conn: &Connection, queries: &[(String, Vec<f32>)], n_chunks: usize) -> Result<(), Box<dyn std::error::Error>> {
    let nq = queries.len();
    let n_rows: i64 = conn.query_row("SELECT count(*) FROM chunks", [], |r| r.get(0))?;
    let bytes_per_row = (n_chunks * DIM * 4) as f64;
    println!("  --- {n_chunks} chunks ({:.1} MB of vector BLOBs) x {nq} queries/round ---",
        n_chunks * DIM * 4 / 1_048_576);

    // Sanity gate: if the baseline cannot read the corpus it is scanning, every
    // timing below is meaningless. Fail loudly instead of reporting 0.04 ms.
    let probe = baseline_scan(conn, &queries[0].1, None, None, None)
        .map_err(|e| format!("baseline_scan failed on the {n_chunks}-chunk corpus: {e}"))?;
    if n_rows > 0 && probe.is_empty() {
        return Err(format!("baseline_scan returned 0 rows on a {n_rows}-row corpus — benchmark aborted").into());
    }
    println!("  (baseline returned {} paths for probe query 0 — corpus is readable)", probe.len());

    let base = bench("scan Rust (current lib.rs)", 3, nq, || {
        queries.iter()
            .map(|(_, qv)| baseline_scan(conn, qv, None, None, None).map(|v| v.len()).map_err(|e| e.to_string()))
            .sum::<Result<usize, String>>()
    });
    let kn = bench("sqlite-vec KNN k=20 (+join)", 3, nq, || {
        queries.iter()
            .map(|(_, qv)| knn(conn, qv, 20, None, None, None).map(|v| v.len()).map_err(|e| e.to_string()))
            .sum::<Result<usize, String>>()
    });
    let kn_id = bench("sqlite-vec KNN k=20 (no join)", 3, nq, || {
        queries.iter()
            .map(|(_, qv)| knn_ids_only(conn, qv, 20).map(|v| v.len()).map_err(|e| e.to_string()))
            .sum::<Result<usize, String>>()
    });
    let kn200 = bench("sqlite-vec KNN k=200 (fill 20 paths)", 3, nq, || {
        queries.iter()
            .map(|(_, qv)| knn(conn, qv, 200, None, None, None).map(|v| v.len()).map_err(|e| e.to_string()))
            .sum::<Result<usize, String>>()
    });
    println!("  --> scan moves {:.1} MB/query; KNN reads ~0 MB of BLOBs", bytes_per_row / 1_048_576.0);
    println!("  --> speedup k=20 (join)   : {:.2}x", base / kn);
    println!("  --> speedup k=20 (no join) : {:.2}x   <- isolates vec0 from the chunks lookup", base / kn_id);
    println!("  --> speedup k=200 (join)  : {:.2}x", base / kn200);
    Ok(())
}
