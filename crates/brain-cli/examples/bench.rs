use brain_store::Store;
use std::time::Instant;

/// Where the report goes when nothing says otherwise.
///
/// B5 said `/tmp/bench.json`, and that was the whole problem: the artifact the
/// acceptance criterion rests on lived in a directory the next reboot clears, so
/// the number cited in `VERIFY.md` was not reproducible by anyone. The default is
/// therefore **inside the repository**, and the ephemeral path is what an operator
/// has to ask for by name.
const DEFAULT_OUT: &str = "bench-hybrid.json";

/// The report destination, from `BRAIN_BENCH_OUT`.
fn out_path() -> String {
    std::env::var("BRAIN_BENCH_OUT").ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| DEFAULT_OUT.to_string())
}

fn main() -> anyhow::Result<()> {
    let db = std::env::var("BRAIN_DB_PATH").unwrap_or("/tmp/bench.db".into());
    // The bench wipes its database and rebuilds a 5-note synthetic corpus, so
    // pointing `BRAIN_DB_PATH` at a real one destroys it. That footgun was
    // inherited from the first version of this file and it is the kind of thing a
    // person does exactly once, on their real path, out of habit. A populated
    // database is refused instead: an empty path (or a missing file) is the only
    // case the bench can honestly claim.
    //
    // Two things this guard must not be, and both were wrong once:
    //
    //   * It must not **open for writing** to look. `Store::open` runs
    //     `init_schema`, so inspecting a database with it created tables and
    //     wrote the `_meta` version row into the operator's real file — on the
    //     production `brain.db` with 1,027 chunks, the most expensive thing a
    //     guard could do while claiming to protect it.
    //   * It must not treat **"could not read it"** as "nothing to protect". A
    //     corrupted file, a database this user cannot open, a WAL whose `-shm` is
    //     unreadable — every one of those used to fall out of `if let Ok(s)` and
    //     land in the `remove_file` below, so the *hardest* case to recover from
    //     was the one that got silently deleted. `let Some(..) else` refuses to
    //     continue instead.
    if std::path::Path::new(&db).exists() {
        let Ok(s) = Store::open_read_only(&db) else {
            anyhow::bail!(
                "refusing to run: {db} exists but cannot be opened read-only, so what it holds is \
                 unknown. The bench DELETES its database, and a database it cannot read is exactly \
                 the one it must not delete — it may be corrupt, or owned by another user. Inspect \
                 it by hand, or set BRAIN_DB_PATH to a scratch path."
            );
        };
        let notes = s.count_notes()?;
        if notes > 0 {
            anyhow::bail!(
                "{db} already holds {notes} note(s). This bench DELETES its database and writes a synthetic \
                 5-note corpus, so it refuses to run against a populated one. Set BRAIN_DB_PATH to a \
                 scratch path (or unset it for /tmp/bench.db)."
            );
        }
    }
    let _ = std::fs::remove_file(&db);
    let store = Store::open(&db)?;
    // 5 notas python/rust/memoria
    let notes = vec![
        ("regras/global/python-mem", "regras", Some("global"), "## Python memoria\nPython gerencia memoria via GC e reference counting. Memoria eficiente."),
        ("regras/global/rust-mem", "regras", Some("global"), "## Rust memoria\nRust ownership borrow checker zero-cost memoria sem GC."),
        ("regras/global/memoria-cache", "regras", Some("global"), "## Cache memoria\nCache em memoria Redis acelera leitura. Memoria volatil."),
        ("arquitetura/projetos/app-stack", "arquitetura", Some("projetos"), "## Stack memoria\nStack usa memoria heap para alocar vetores."),
        ("sessoes/app-2026-09-17", "sessoes", None, "## Sessao\nDiscutimos memoria e embeddings."),
    ];
    for (path, layer, scope, content) in notes {
        let nid = store.note_upsert(path, layer, scope, content, None, &[], false, None)?;
        // NULL, not a zero vector. This bench used to insert BLOB-of-zeros and
        // then measure a "hybrid" search against them, which meant `hybrid_ms`
        // never exercised the vector path at all: cosine() short-circuits a
        // zero-norm vector to 0.0, so the scan produced no ranking signal and the
        // number reported the FTS cost, not a hybrid one.
        for (i, ch) in brain_core::chunk_text(content, 4096).iter().enumerate() {
            store.chunk_insert(nid, path, layer, scope, ch, i as i32, 1, None, &[], None)?;
        }
    }
    // 100x search "memoria"
    let start = Instant::now();
    for _ in 0..100 {
        let _ = store.search("memoria", None, None, None, None, None, 5, false)?;
    }
    let elapsed = start.elapsed();
    let fts_ms = elapsed.as_secs_f64() * 1000.0 / 100.0;
    // Vector stream: no chunk has an embedding, so this measures the vector
    // branch's setup cost only. To bench a real vector scan, store real vectors
    // first and confirm `embedding_coverage.embedded` is non-zero below —
    // otherwise `hybrid_ms` is not a hybrid measurement.
    let start2 = Instant::now();
    let dummy = vec![0.01; 768];
    for _ in 0..100 {
        let _ = store.search("memoria", Some(&dummy), None, None, None, None, 5, false)?;
    }
    let elapsed2 = start2.elapsed();
    let hybrid_ms = elapsed2.as_secs_f64() * 1000.0 / 100.0;
    // recall: expect at least 3 results contain memoria
    let res = store.search("memoria", None, None, None, None, None, 5, false)?;
    let recall = res.iter().filter(|r| r.snippet.to_lowercase().contains("memoria") || r.path.contains("memoria")).count() as f32 / res.len().max(1) as f32;
    let cov = store.embedding_coverage()?;
    let bench = serde_json::json!({
        "fts_ms": fts_ms,
        "hybrid_ms": hybrid_ms,
        "recall": recall,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "notes": store.count_notes()?,
        "chunks": store.count_chunks()?,
        // Unit of every field above is documented next to it; `hybrid_ms` is only
        // a hybrid measurement when `chunks_embedded` > 0.
        "chunks_embedded": cov.embedded,
        "chunks_without_embedding": cov.without_embedding,
        "hybrid_is_real_vector_scan": cov.embedded > 0,
        // The corpus this ran against, so a reader can tell a 5-note run from a
        // 250-note one without guessing from the timings.
        "corpus": "synthetic: 5 hard-coded notes about memory, 1 chunk each",
        "runs_per_measurement": 100,
        "build": "cargo run --release -p brain-cli --example bench",
        "schema_version": store.schema_version()?,
        "format": "brain-bench/2"
    });
    let out = out_path();
    let json = serde_json::to_string_pretty(&bench)?;
    // Never clobber silently. This file is the evidence a recall number is quoted
    // from; overwriting it with a run over a different corpus, without saying so,
    // is how a stale number starts looking current. `BRAIN_BENCH_FORCE=1` is the
    // explicit override, and the previous file is kept as `.prev`.
    if std::fs::symlink_metadata(&out).is_ok() && std::env::var("BRAIN_BENCH_FORCE").as_deref() != Ok("1") {
        let prev = format!("{out}.prev");
        std::fs::copy(&out, &prev).map_err(|e| anyhow::anyhow!("cannot keep the previous report at {prev}: {e}"))?;
        eprintln!(
            "bench: {out} already exists; the old one is at {prev}. Set BRAIN_BENCH_FORCE=1 to overwrite in place."
        );
    }
    std::fs::write(&out, &json)?;
    println!("{json}");
    eprintln!("bench: wrote {out}");
    Ok(())
}
