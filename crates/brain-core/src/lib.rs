//! brain-core — domain types, validation, no IO

use regex::Regex;
use serde::{Deserialize, Serialize};

pub const VALID_LAYERS: &[&str] = &[
    "arquitetura",
    "regras",
    "sessoes",
    "projetos",
    "estudos",
    "indexacao",
];

pub const LAYERS_WITH_SCOPE: &[&str] = &["arquitetura", "regras", "estudos"];
pub const VALID_SCOPES: &[&str] = &["projetos", "global"];

pub const EMBEDDING_DIM: usize = 768;

/// Target chunk width, in "tokens" (4 bytes each, as `chunk_text` counts them).
///
/// Owned here rather than spelled as a literal at each call site: the write-side
/// limits below are only meaningful relative to the chunk width they imply, and
/// a call site that silently uses a different width would validate against a
/// budget the reader never asked for.
pub const CHUNK_TARGET_TOKENS: usize = 4096;

/// Seconds per chunk assumed when sizing the write-side limits below.
///
/// Deliberately ~9x the ~0.045 s/chunk measured on the development box
/// (`nomic-embed-text`, warm, served serially with `OLLAMA_NUM_PARALLEL=1`:
/// 1 chunk 0.04 s, 32 chunks 1.4-1.8 s, 64 chunks 2.8-3.9 s). A ceiling sized to
/// the fastest measurement ever taken is not a ceiling — it is a snapshot of one
/// warm cache on one machine, and the first cold `nomic-embed-text` load (274 MB
/// from disk) or a CPU-only host breaks it. The limits are therefore sized
/// against this figure, which holds with room to spare on hardware ~9x slower
/// than the one measured.
pub const PESSIMISTIC_SECONDS_PER_CHUNK: u64 = 400;

/// Maximum accepted size of a note's `content`, in bytes.
///
/// Without a ceiling, one request is a denial of service against yourself:
/// `chunk_text` splits on `## ` with no bound on how many chunks come out, and
/// embedding is *serial* (Ollama's `OLLAMA_NUM_PARALLEL` defaults to 1). A note of
/// 6 MB split into 1000 chunks therefore holds a request open for minutes, and the
/// batch budget — a multiple of the wave count — for hours.
///
/// Sizing uses [`PESSIMISTIC_SECONDS_PER_CHUNK`], not the ~0.045 s/chunk this box
/// actually measures, because a limit sized to the fastest number observed is a
/// limit that stops being a limit on a cold start, a slower machine, or a larger
/// embedding model. The two constants below are sized against the pessimistic
/// figure and are ~9x looser than today's hardware requires.
///
/// Why 256 KiB, against the real corpus: the largest note in `data/brain.db` is
/// 6,286 B and the mean is 1,750 B, so this is 41.7x the largest real note and
/// 149x the mean. Frontmatter plus a dense architecture document or a long
/// session log stays two orders of magnitude below it, so nothing legitimate
/// comes near the wall, while the worst case drops from minutes to
/// `MAX_CHUNKS * PESSIMISTIC_SECONDS_PER_CHUNK` = ~26 s of *background* embedding
/// work (~3 s on this box).
pub const MAX_CONTENT_BYTES: usize = 256 * 1024;

/// Maximum accepted number of chunks per note.
///
/// This is the limit that actually bounds work, and it is deliberately separate
/// from [`MAX_CONTENT_BYTES`]: a 200 KiB note with a `## ` heading every 100
/// bytes is 2,000 chunks and 200 KiB, so the byte ceiling alone would let it
/// through. 64 is 6.4x the largest real note in the corpus (10 chunks) and
/// exactly `MAX_CONTENT_BYTES / CHUNK_TARGET_TOKENS`, so for a well-formed note
/// the two limits agree and only the pathological shape trips this one.
pub const MAX_CHUNKS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Note {
    pub path: String,
    pub layer: String,
    pub scope: Option<String>,
    pub content: String,
    pub project_id: Option<i64>,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub expires_at: Option<String>,
    pub version: i32,
}

impl Note {
    pub fn full_path(&self) -> String {
        if let Some(scope) = &self.scope {
            if LAYERS_WITH_SCOPE.contains(&self.layer.as_str()) {
                return format!("{}/{}/{}.md", self.layer, scope, self.path);
            }
        }
        format!("{}/{}.md", self.layer, self.path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchExplain {
    pub rrf_vec: f32,
    pub rrf_fts: f32,
    pub rrf_entity: f32,
    pub rrf_graph: f32,
    pub authority: f32,
    pub score: f32,
    pub rank_vec: Option<usize>,
    pub rank_fts: Option<usize>,
    pub rank_entity: Option<usize>,
    pub rank_graph: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub path: String,
    pub layer: String,
    pub scope: Option<String>,
    pub score: f32,
    pub snippet: String,
    pub chunk_index: i32,
    pub project: Option<String>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain: Option<SearchExplain>,
}

pub fn validate_layer(layer: &str) -> anyhow::Result<()> {
    if !VALID_LAYERS.contains(&layer) {
        anyhow::bail!("Invalid layer '{}'. Valid: {}", layer, VALID_LAYERS.join(", "));
    }
    Ok(())
}

pub fn validate_scope(scope: &str) -> anyhow::Result<()> {
    if !VALID_SCOPES.contains(&scope) {
        anyhow::bail!("Invalid scope '{}'. Valid: {}", scope, VALID_SCOPES.join(", "));
    }
    Ok(())
}

/// Rejects a note body that would cost unbounded embedding work.
///
/// Enforces both write-side limits, and it is deliberately a *pure* check on the
/// text: it runs before any `Store` is opened and before any network call, so a
/// note that is too big is refused in microseconds instead of after a
/// multi-minute embed of text nobody was ever going to read back.
///
/// Both numbers matter and neither subsumes the other — see [`MAX_CONTENT_BYTES`]
/// and [`MAX_CHUNKS`]. The message names the measured limit and what the caller
/// should do, because "request failed" with no bound is how an agent ends up
/// retrying a body it needs to split anyway.
pub fn validate_content_limits(content: &str) -> anyhow::Result<()> {
    if content.len() > MAX_CONTENT_BYTES {
        anyhow::bail!(
            "content too large: {} bytes, limit is {} bytes ({} KiB). Split it into several notes under one project instead of storing it as one.",
            content.len(),
            MAX_CONTENT_BYTES,
            MAX_CONTENT_BYTES / 1024
        );
    }
    let chunks = chunk_text(content, CHUNK_TARGET_TOKENS).len();
    if chunks > MAX_CHUNKS {
        anyhow::bail!(
            "content splits into {} chunks, limit is {}. Chunks are embedded serially (Ollama serves one \
             embedding at a time by default), so a note this size costs a lot of embedding work. Merge the \
             small sections or split it into several notes under one project.",
            chunks,
            MAX_CHUNKS
        );
    }
    Ok(())
}

pub fn sanitize_relative_path(raw: &str) -> anyhow::Result<String> {
    if raw.is_empty() {
        anyhow::bail!("Path cannot be empty");
    }
    if raw.starts_with('/') {
        anyhow::bail!("Absolute paths not allowed: {}", raw);
    }
    let parts: Vec<&str> = raw.split('/').collect();
    if parts.iter().any(|p| *p == "..") {
        anyhow::bail!("Path traversal detected: {}", raw);
    }
    Ok(raw.to_string())
}

pub fn parse_frontmatter(content: &str) -> (Option<String>, Vec<String>) {
    // manual parse without DOTALL regex complexity
    if !content.starts_with("---\n") { return (None, Vec::new()); }
    if let Some(end) = content[4..].find("\n---") {
        let block = &content[4..4+end];
        let mut project = None;
        let mut tags = Vec::new();
        for line in block.lines() {
            let l = line.trim();
            if let Some(val) = l.strip_prefix("project:") {
                let v = val.trim();
                if !v.is_empty() {
                    project = Some(v.to_string());
                }
            } else if let Some(val) = l.strip_prefix("tags:") {
                let v = val.trim();
                if v.starts_with('[') && v.ends_with(']') {
                    let inner = &v[1..v.len() - 1];
                    tags = inner.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
                } else if !v.is_empty() {
                    tags = v.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
                }
            }
        }
        return (project, tags);
    }
    (None, Vec::new())
}

pub fn chunk_text(text: &str, max_tokens: usize) -> Vec<String> {
    // split by lines starting with "## "
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if line.starts_with("## ") && !cur.trim().is_empty() {
            chunks.push(cur.trim().to_string());
            cur.clear();
        }
        cur.push_str(line);
        cur.push('\n');
    }
    if !cur.trim().is_empty() { chunks.push(cur.trim().to_string()); }
    if chunks.is_empty() {
        let t = if text.len() > max_tokens*4 { &text[..max_tokens*4] } else { text };
        chunks.push(t.to_string());
        return chunks;
    }
    // truncate oversize
    for c in &mut chunks {
        if c.len() > max_tokens*4 { c.truncate(max_tokens*4); }
    }
    chunks
}

pub fn extract_wikilinks(content: &str) -> Vec<String> {
    let re = Regex::new(r"\[\[([^\]]+)\]\]").unwrap();
    re.captures_iter(content).map(|c| c[1].to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn sanitize_ok() { assert_eq!(sanitize_relative_path("a/b").unwrap(), "a/b"); }
    #[test] fn sanitize_traversal() { assert!(sanitize_relative_path("../a").is_err()); }
    #[test] fn validate_layer_ok() { validate_layer("regras").unwrap(); }
    #[test] fn validate_layer_bad() { assert!(validate_layer("foo").is_err()); }
    #[test] fn frontmatter_parse() {
        let c = "---\nproject: myapp\ntags: [a, b]\n---\nbody";
        let (p,t) = parse_frontmatter(c);
        assert_eq!(p, Some("myapp".into())); assert_eq!(t, vec!["a","b"]);
    }
    #[test] fn chunk_basic() {
        let c = "## A\nfoo\n## B\nbar";
        let ch = chunk_text(c, 4096);
        assert_eq!(ch.len(), 2);
    }

    // ------------------------------------------------------------------
    // Write-side limits. A note with no size ceiling is a self-inflicted DoS:
    // embedding is serial and costs ~0.4s per chunk, so `chunk_text` splitting a
    // multi-megabyte body into 1000 chunks holds one request open for minutes.
    // ------------------------------------------------------------------

    #[test]
    fn content_limits_accept_a_dense_note_the_corpus_actually_contains() {
        // A realistic worst case: frontmatter + 60 `##` sections, which is 6x
        // the largest note in the real corpus (10 chunks, 6,286 bytes).
        let mut body = String::from("---\nproject: brain\ntags: [a, b]\n---\n\n");
        for i in 0..60 {
            body.push_str(&format!("## Section {}\nsome prose about the thing, padded a little\n\n", i));
        }
        assert!(body.len() > 1024, "fixture must be non-trivial, got {} bytes", body.len());
        assert!(body.len() < MAX_CONTENT_BYTES, "fixture must be under the byte limit");
        // 60 sections + the frontmatter block, which `chunk_text` emits as its
        // own chunk because it precedes the first `## ` heading.
        assert_eq!(chunk_text(&body, CHUNK_TARGET_TOKENS).len(), 61);
        validate_content_limits(&body).expect("a 60-chunk note is within budget");
    }

    #[test]
    fn content_limits_reject_an_oversized_body_and_name_the_limit() {
        let body = "x".repeat(MAX_CONTENT_BYTES + 1);
        let err = validate_content_limits(&body).expect_err("a body over the byte limit must be refused");
        let msg = err.to_string();
        assert!(msg.contains("content too large"), "got: {msg}");
        assert!(msg.contains(&MAX_CONTENT_BYTES.to_string()), "the message must name the limit: {msg}");
    }

    #[test]
    fn content_limits_reject_a_degenerate_chunk_count_even_when_the_bytes_fit() {
        // The case the byte ceiling cannot catch on its own: 200 KiB is under
        // MAX_CONTENT_BYTES, but a `## ` heading every 8 bytes is 20,000 chunks.
        let mut body = String::new();
        for i in 0..(MAX_CHUNKS + 5) {
            body.push_str(&format!("## {}\n", i));
        }
        assert!(body.len() < MAX_CONTENT_BYTES, "fixture must stay under the byte limit, got {}", body.len());
        let err = validate_content_limits(&body).expect_err("too many chunks must be refused even under the byte limit");
        let msg = err.to_string();
        assert!(msg.contains("chunks, limit is"), "got: {msg}");
    }

    #[test]
    fn content_limits_accept_exactly_the_maximum() {
        // Off-by-one guard: the boundary itself is legal, one byte / one chunk
        // past it is not.
        let mut at_limit = String::new();
        for i in 0..MAX_CHUNKS {
            at_limit.push_str(&format!("## {}\n", i));
        }
        at_limit.push_str(&"x".repeat(MAX_CONTENT_BYTES - at_limit.len()));
        assert_eq!(at_limit.len(), MAX_CONTENT_BYTES);
        validate_content_limits(&at_limit).expect("exactly at the limit is accepted");
        assert!(validate_content_limits(&format!("{}x", at_limit)).is_err());
    }
}
