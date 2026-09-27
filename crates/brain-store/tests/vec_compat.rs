//! C3-01 SPIKE — regression net for the `sqlite-vec` viability findings.
//!
//! These tests exist so that C3-02 inherits a machine-checked statement of what
//! was actually measured, instead of a claim in a report. They are deliberately
//! hermetic: no Ollama, no `data/brain.db`, no `/tmp` state — a fresh in-memory
//! database with a small deterministic corpus.
//!
//! What each test pins down:
//!   * the extension registers and answers `vec_version()`
//!   * `vec0` accepts `float[N] distance_metric=cosine` (column-level option)
//!   * cosine on vec0 reproduces `brain_store`'s own `cosine()` ranking exactly
//!   * `layer`/`scope`/`project_id` can be filtered INSIDE the KNN, so the
//!     filter-before-similarity semantics of `search()` are preserved
//!   * SQLite triggers keep `vec_chunks` in sync with `chunks`, including
//!     `ON DELETE CASCADE` from the parent note
//!
//! Deliberately NOT tested here: exact recall against the real corpus, and
//! performance. Both need Ollama and the production DB, so they live in
//! `examples/vec_spike.rs` where they are measured on demand.

use rusqlite::Connection;

#[path = "../examples/vec_common/mod.rs"]
mod common;

/// Corpus shape mirrors the production DB: several chunks per note path, and
/// the same three filter buckets (projetos / global / no-scope sessions).
const DIM: usize = 768;
const NULL_SENTINEL: &str = "__NULL__";
const NULL_PID: i64 = -1;

/// Verbatim copy of `cosine()` in `crates/brain-store/src/lib.rs:829`.
/// If the two ever disagree, this suite is measuring the wrong thing.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}

/// Deterministic xorshift64* so the corpus is identical on every run.
struct Rng(u64);
impl Rng {
    fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D)) >> 40) as f32 / 8_388_608.0
    }
}

fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

struct Row {
    id: i64,
    path: String,
    layer: &'static str,
    scope: Option<&'static str>,
    project_id: Option<i64>,
    emb: Vec<f32>,
}

/// Builds a corpus whose vectors are NOT unit-normalised (production
/// `nomic-embed-text` output is not either), which is exactly the case where
/// choosing L2 over cosine would silently change the ranking.
fn corpus(n_paths: usize, chunks_per_path: usize) -> Vec<Row> {
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    let layers = ["regras", "sessoes", "arquitetura"];
    let mut out = Vec::new();
    // Start at 1: `chunks.id` is INTEGER PRIMARY KEY AUTOINCREMENT, so 0 is
    // unreachable in production. Using 0 here surfaced a vec0/cascade anomaly
    // (a cascaded delete of chunk_id 0 is silently dropped) that cannot occur
    // in the real schema.
    let mut id = 1i64;
    for p in 0..n_paths {
        for c in 0..chunks_per_path {
            let layer = layers[p % layers.len()];
            // A third of the rows exercise the NULL paths for scope/project_id.
            let (scope, pid) = match (p + c) % 3 {
                0 => (Some("global"), Some(1i64)),
                1 => (Some("projetos"), None),
                _ => (None, None),
            };
            // Scale to a norm in the same range as the real DB (~16..23) so this
            // test would catch a metric that silently assumes unit vectors.
            let mut v: Vec<f32> = (0..DIM).map(|_| rng.next_f32()).collect();
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
            let target = 16.0 + (p as f32 % 7.0);
            for slot in v.iter_mut() {
                *slot = *slot / norm * target;
            }
            out.push(Row {
                id,
                path: format!("camada/nota-{p:03}"),
                layer,
                scope,
                project_id: pid,
                emb: v,
            });
            id += 1;
        }
    }
    out
}

fn open_with_corpus(rows: &[Row]) -> Connection {
    common::register_sqlite_vec();
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE chunks (
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
            embedding BLOB);
         CREATE VIRTUAL TABLE vec_chunks USING vec0(
            chunk_id  INTEGER PRIMARY KEY,
            embedding float[768] distance_metric=cosine,
            layer     text,
            scope     text,
            project_id integer);",
    )
    .unwrap();
    for r in rows {
        conn.execute(
            "INSERT INTO chunks(id, path, layer, scope, snippet, project_id, embedding)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![r.id, r.path, r.layer, r.scope, r.path, r.project_id, to_blob(&r.emb)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vec_chunks(chunk_id, embedding, layer, scope, project_id)
             VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![r.id, to_blob(&r.emb), r.layer, r.scope.unwrap_or(NULL_SENTINEL),
                              r.project_id.unwrap_or(NULL_PID)],
        )
        .unwrap();
    }
    conn
}

/// Baseline: the vector-candidate stage of `search()` — scan, cosine, dedup by
/// path keeping max cosine, rank descending.
fn baseline_top(conn: &Connection, qv: &[f32], k: usize) -> Vec<String> {
    let mut st = conn
        .prepare("SELECT path, embedding FROM chunks WHERE embedding IS NOT NULL")
        .unwrap();
    let rows = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)))
        .unwrap();
    let mut best: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
    for r in rows {
        let (path, blob) = r.unwrap();
        let emb: Vec<f32> = blob
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        if emb.len() != qv.len() {
            continue;
        }
        let c = cosine(qv, &emb);
        if best.get(&path).is_none_or(|b| c > *b) {
            best.insert(path, c);
        }
    }
    let mut v: Vec<(String, f32)> = best.into_iter().collect();
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    v.into_iter().take(k).map(|(p, _)| p).collect()
}

fn knn_top(conn: &Connection, qv: &[f32], k: i64) -> Vec<String> {
    let mut st = conn
        .prepare(
            "SELECT c.path FROM vec_chunks v JOIN chunks c ON c.id = v.chunk_id
             WHERE v.embedding MATCH ?1 AND v.k = ?2 ORDER BY v.distance",
        )
        .unwrap();
    let rows = st
        .query_map(rusqlite::params![to_blob(qv), k], |r| r.get::<_, String>(0))
        .unwrap();
    let mut seen = std::collections::HashSet::new();
    rows.map(|r| r.unwrap())
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

// ─────────────────────────────────────────────────────────────── the gates

#[test]
fn g1_g2_extension_registers_and_reports_a_version() {
    common::register_sqlite_vec();
    let conn = Connection::open_in_memory().unwrap();
    let v: String = conn.query_row("SELECT vec_version()", [], |r| r.get(0)).unwrap();
    assert!(v.starts_with('v'), "vec_version() should look like 'v0.1.9', got {v:?}");
    // Pinned by the dev-dependency in brain-store/Cargo.toml.
    assert_eq!(v, "v0.1.9", "spike was validated against sqlite-vec 0.1.9");
}

#[test]
fn g3_vec0_supports_cosine_metric_at_768_dims() {
    common::register_sqlite_vec();
    let conn = Connection::open_in_memory().unwrap();
    // `distance_metric` is a COLUMN-level option: it must follow the `]`.
    conn.execute_batch(
        "CREATE VIRTUAL TABLE v USING vec0(
            chunk_id INTEGER PRIMARY KEY,
            embedding float[768] distance_metric=cosine)",
    )
    .expect("vec0 must accept distance_metric=cosine at dim 768");

    let v = vec![0.5f32; DIM];
    conn.execute("INSERT INTO v(chunk_id, embedding) VALUES (1, ?1)", rusqlite::params![to_blob(&v)])
        .unwrap();
    // A vector's cosine distance to itself is 0 regardless of its norm. If the
    // metric silently fell back to L2 this would be ~ 768 * 0.25, not 0.
    let d: f64 = conn
        .query_row("SELECT distance FROM v WHERE embedding MATCH ?1 AND k = 1", rusqlite::params![to_blob(&v)], |r| r.get(0))
        .unwrap();
    assert!(d.abs() < 1e-5, "self-distance should be ~0 under cosine, got {d}");
}

#[test]
fn g5_cosine_knn_reproduces_the_rust_baseline_ranking() {
    let rows = corpus(40, 5); // 200 chunks over 40 note paths
    let conn = open_with_corpus(&rows);
    let mut rng = Rng(0x1234_5678_9ABC_DEF0);

    for case in 0..25 {
        let q: Vec<f32> = (0..DIM).map(|_| rng.next_f32()).collect();
        // Reuse a real corpus vector for some cases: guarantees at least one
        // exact match, which is where a tie-breaking difference would show up.
        let qv = if case % 2 == 0 {
            rows[(case * 7) % rows.len()].emb.clone()
        } else {
            q
        };
        let base = baseline_top(&conn, &qv, 20);
        // Max 5 chunks per path, so K=200 is the worst case needed to fill 20 paths.
        let got = knn_top(&conn, &qv, 200);
        let overlap = base.iter().filter(|p| got.contains(p)).count();
        assert_eq!(
            overlap,
            base.len(),
            "case {case}: baseline top-{} and sqlite-vec top-{} disagree ({overlap} shared)\n  baseline: {base:?}\n  vec0    : {:?}",
            base.len(),
            base.len(),
            &got[..base.len().min(got.len())],
        );
    }
}

#[test]
fn g6_filters_apply_inside_the_knn_preserving_pre_filter_semantics() {
    let rows = corpus(40, 5);
    let conn = open_with_corpus(&rows);

    // Each case pairs the BASELINE predicate (against real `chunks` columns,
    // where a missing value is SQL NULL) with the vec0 predicate (against the
    // metadata column, where NULL was encoded as a sentinel at insert time).
    // Both must select the same row set — that equivalence is the whole point.
    struct Case {
        name: &'static str,
        baseline_pred: &'static str,
        vec_pred: &'static str,
        val: rusqlite::types::Value,
    }
    let cases = [
        Case { name: "layer=regras", baseline_pred: "layer = ?1", vec_pred: "v.layer = ?2",
               val: rusqlite::types::Value::Text("regras".into()) },
        Case { name: "layer=sessoes", baseline_pred: "layer = ?1", vec_pred: "v.layer = ?2",
               val: rusqlite::types::Value::Text("sessoes".into()) },
        // NULL scope: baseline must use IS NULL, vec0 uses the sentinel.
        Case { name: "scope IS NULL", baseline_pred: "scope IS NULL", vec_pred: "v.scope = ?2",
               val: rusqlite::types::Value::Text(NULL_SENTINEL.into()) },
        Case { name: "scope=global", baseline_pred: "scope = ?1", vec_pred: "v.scope = ?2",
               val: rusqlite::types::Value::Text("global".into()) },
        // NULL project_id: sentinel -1.
        Case { name: "project_id IS NULL", baseline_pred: "project_id IS NULL", vec_pred: "v.project_id = ?2",
               val: rusqlite::types::Value::Integer(NULL_PID) },
        Case { name: "project_id=1", baseline_pred: "project_id = ?1", vec_pred: "v.project_id = ?2",
               val: rusqlite::types::Value::Integer(1) },
    ];

    let q = &rows[3].emb;
    for c in &cases {
        let takes_param = c.baseline_pred.contains("?1");
        // Baseline: filter in SQL, then score — the semantics `search()` has today.
        let mut st = conn
            .prepare(&format!(
                "SELECT path, embedding FROM chunks WHERE embedding IS NOT NULL AND {}",
                c.baseline_pred
            ))
            .unwrap();
        let mut collected: Vec<(String, Vec<u8>)> = Vec::new();
        if takes_param {
            let it = st
                .query_map(rusqlite::params![c.val], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
                })
                .unwrap();
            for r in it {
                collected.push(r.unwrap());
            }
        } else {
            let it = st
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)))
                .unwrap();
            for r in it {
                collected.push(r.unwrap());
            }
        }
        let mut best: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
        for (path, blob) in collected {
            let emb: Vec<f32> = blob
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect();
            let cos = cosine(q, &emb);
            if best.get(&path).is_none_or(|b| cos > *b) {
                best.insert(path, cos);
            }
        }
        let mut base: Vec<(String, f32)> = best.into_iter().collect();
        base.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        // `search()` truncates the deduped candidate list to 50 before RRF, so
        // compare the full match set for cardinality and the head for ranking.
        let base_all: Vec<String> = base.iter().map(|(p, _)| p.clone()).collect();
        let base: Vec<String> = base_all.iter().take(20).cloned().collect();

        // Same predicate, inside the KNN via a vec0 metadata column.
        let mut st = conn
            .prepare(&format!(
                "SELECT c.path FROM vec_chunks v JOIN chunks c ON c.id = v.chunk_id
                 WHERE v.embedding MATCH ?1 AND {} AND v.k = 200 ORDER BY v.distance",
                c.vec_pred
            ))
            .unwrap();
        let got: Vec<String> = st
            .query_map(rusqlite::params![to_blob(q), c.val], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let mut seen = std::collections::HashSet::new();
        let got: Vec<String> = got.into_iter().filter(|p| seen.insert(p.clone())).collect();

        assert_eq!(
            got.len(),
            base_all.len(),
            "{}: in-KNN filter matched {} paths, the SQL predicate matched {} — \
             the two must select the same rows or the filter changed meaning",
            c.name,
            got.len(),
            base_all.len()
        );
        for p in &base {
            assert!(
                got.contains(p),
                "{}: baseline top-20 path {p:?} missing from the in-KNN result",
                c.name
            );
        }
    }
}

#[test]
fn g7_triggers_keep_vec_chunks_in_sync_with_chunks() {
    let rows = corpus(4, 2);
    let conn = open_with_corpus(&rows);

    conn.execute_batch(
        "CREATE TRIGGER chunks_ai AFTER INSERT ON chunks BEGIN
             INSERT INTO vec_chunks(chunk_id, embedding, layer, scope, project_id)
             VALUES (NEW.id, NEW.embedding, NEW.layer, coalesce(NEW.scope,'__NULL__'),
                     coalesce(NEW.project_id,-1));
         END;
         CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN
             DELETE FROM vec_chunks WHERE chunk_id = OLD.id;
         END;
         CREATE TRIGGER chunks_au AFTER UPDATE ON chunks BEGIN
             DELETE FROM vec_chunks WHERE chunk_id = OLD.id;
             INSERT INTO vec_chunks(chunk_id, embedding, layer, scope, project_id)
             VALUES (NEW.id, NEW.embedding, NEW.layer, coalesce(NEW.scope,'__NULL__'),
                     coalesce(NEW.project_id,-1));
         END;",
    )
    .expect("triggers on a table feeding a vec0 virtual table must be creatable");

    // INSERT
    let emb = to_blob(&rows[0].emb);
    conn.execute(
        "INSERT INTO chunks(id, path, layer, scope, snippet, project_id, embedding)
         VALUES (9999, 'probe/ins', 'regras', 'global', 'snip', 1, ?1)",
        rusqlite::params![emb],
    )
    .unwrap();
    let n: i64 = conn
        .query_row("SELECT count(*) FROM vec_chunks WHERE chunk_id = 9999", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "AFTER INSERT trigger did not populate vec_chunks");

    // UPDATE — the vector must be replaced, not duplicated.
    let mut changed = rows[1].emb.clone();
    changed[0] += 1.0;
    conn.execute(
        "UPDATE chunks SET embedding = ?1 WHERE id = 9999",
        rusqlite::params![to_blob(&changed)],
    )
    .unwrap();
    let (n, got): (i64, Vec<u8>) = conn
        .query_row("SELECT count(*), min(embedding) FROM vec_chunks WHERE chunk_id = 9999", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(n, 1, "AFTER UPDATE trigger duplicated the row");
    assert_eq!(got, to_blob(&changed), "AFTER UPDATE trigger did not propagate the new vector");

    // DELETE
    conn.execute("DELETE FROM chunks WHERE id = 9999", []).unwrap();
    let n: i64 = conn
        .query_row("SELECT count(*) FROM vec_chunks WHERE chunk_id = 9999", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "AFTER DELETE trigger did not remove the vec_chunks row");
}

#[test]
fn g7_triggers_survive_the_fk_cascade() {
    // `chunks.note_id REFERENCES notes(id) ON DELETE CASCADE`, and `delete_page()`
    // deletes through the `notes` table — so the cascade path is the hot path and
    // the trigger design has to hold there, not just for direct chunk deletes.
    //
    // MEASURED: it does. Verified from n=1 to n=50 cascaded chunk rows, all of
    // which propagate to `vec_chunks`.
    for n in [1usize, 2, 3, 7, 25] {
        let rows = corpus(n, 1);
        let conn = build_cascade_corpus(&rows, true);
        conn.execute("DELETE FROM notes WHERE id = 1", []).unwrap();
        assert_eq!(count(&conn, "SELECT count(*) FROM chunks"), 0,
            "n={n}: FK CASCADE should have removed every chunk");
        assert_eq!(count(&conn, "SELECT count(*) FROM vec_chunks"), 0,
            "n={n}: vec_chunks still holds rows after the cascade — the chunks \
             AFTER DELETE trigger is not sufficient for C3-02's sync strategy");
    }
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// `notes` + `chunks` + `vec_chunks` wired with a real FK CASCADE, every chunk
/// row owned by note 1.
fn build_cascade_corpus(rows: &[Row], with_trigger: bool) -> Connection {
    common::register_sqlite_vec();
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "PRAGMA foreign_keys=ON;
         CREATE TABLE notes (id INTEGER PRIMARY KEY);
         CREATE TABLE chunks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
            path TEXT NOT NULL, layer TEXT NOT NULL, scope TEXT, snippet TEXT NOT NULL,
            chunk_index INTEGER NOT NULL DEFAULT 0, total_chunks INTEGER NOT NULL DEFAULT 1,
            project_id INTEGER, tags TEXT DEFAULT '[]', embedding BLOB);
         CREATE VIRTUAL TABLE vec_chunks USING vec0(
            chunk_id INTEGER PRIMARY KEY,
            embedding float[768] distance_metric=cosine,
            layer text, scope text, project_id integer);",
    )
    .unwrap();
    conn.execute("INSERT INTO notes(id) VALUES (1)", []).unwrap();
    for r in rows {
        conn.execute(
            "INSERT INTO chunks(note_id, path, layer, scope, snippet, project_id, embedding)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![r.path, r.layer, r.scope, r.path, r.project_id, to_blob(&r.emb)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vec_chunks(chunk_id, embedding, layer, scope, project_id)
             VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![r.id, to_blob(&r.emb), r.layer, r.scope.unwrap_or(NULL_SENTINEL),
                              r.project_id.unwrap_or(NULL_PID)],
        )
        .unwrap();
    }
    if with_trigger {
        conn.execute_batch(
            "CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN
                 DELETE FROM vec_chunks WHERE chunk_id = OLD.id;
             END;",
        )
        .unwrap();
    }
    conn
}
