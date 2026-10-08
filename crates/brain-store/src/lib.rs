//! brain-store — SQLite-only actor, WAL, FTS5, vec via cosine in Rust (no sqlite-vec ext yet)
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use anyhow::{Context, Result};
use brain_core::{Project, SearchExplain, SearchResult, EMBEDDING_DIM};
use chrono::Utc;
use rusqlite::{OpenFlags, params, Connection, OptionalExtension};
use std::collections::HashMap;

pub const SCHEMA_VERSION: i32 = 4;

/// How long a connection waits for another connection's write lock, in ms.
///
/// X-01. Set explicitly in `init_schema` even though rusqlite 0.32 already
/// defaults new connections to 5000 ms, so that the wait a concurrent writer
/// depends on is this crate's decision rather than a dependency's default. It is
/// comfortably longer than any critical section here (a `SELECT` and an `UPDATE`,
/// both sub-millisecond), and bounded, because the alternative to waiting is
/// losing a session event outright.
const BUSY_TIMEOUT_MS: u32 = 5_000;

/// Bytes per stored embedding (`f32` little-endian).
const EMBED_BLOB_BYTES: usize = EMBEDDING_DIM * 4;

/// Whether a rusqlite error is a lock conflict, i.e. worth retrying.
///
/// X-01. `Error::SqliteFailure` carries the extended code, and both
/// `SQLITE_BUSY` (5) and `SQLITE_LOCKED` (6) mean "someone else has it, try
/// again" rather than "this will never work". The message is a fallback for a
/// failure whose payload rusqlite did not classify.
fn is_busy(e: &rusqlite::Error) -> bool {
    match e {
        rusqlite::Error::SqliteFailure(failure, _) => {
            matches!(failure.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
        }
        _ => {
            let msg = e.to_string();
            msg.contains("database is locked") || msg.contains("database table is locked")
        }
    }
}

/// Escapes the SQL `LIKE` wildcards in a literal prefix.///
/// A project named `a%` would otherwise match every note in the database, which
/// is a read amplification bug on a table that a session hook queries on every
/// tool result. Paired with `ESCAPE '\'` in the statement.
fn like_escaped(prefix: &str) -> String {
    prefix
        .chars()
        .flat_map(|c| match c {
            '%' | '_' | '\\' => ['\\', c].into_iter().collect::<Vec<_>>(),
            other => vec![other],
        })
        .collect()
}

/// Chars replaced with a space before a string becomes an FTS5 `MATCH` expr.
///
/// Single source of truth for [`fts5_match_expr`] and [`fts5_match_expr_joined`]
/// (the latter delegates to the former, so drift is structurally impossible).
/// `*\"():{}=-/\\` is the HEAD set; `?.,+!^~&|` is the hook-kill set: each breaks
/// `MATCH` as a loose term (`?` measured failing on a real question; `.` `,`
/// `+` `!` `^` `~` `&` `|` verified against FTS5 the same way — glued `emissao<c>`
/// fails for all of them, and spaced `a <c> b` fails for all but `+`/`^`, which
/// still fail glued e.g. `C++`). `/` was already in the HEAD set.
const FTS5_STRIP_CHARS: &str = "*\"():{}=-/\\?.,+!^~&|";

/// Build an FTS5 `MATCH` expression with the default (AND) conjunction.
///
/// Byte-identical in behaviour to the inline sanitisation in `Store::search` at
/// HEAD d5f8939, extended with the hook-kill set (see [`FTS5_STRIP_CHARS`]):
/// replace FTS5 specials with space, trim. Space-separated terms
/// are implicit AND in FTS5. No separator/quoting overhaul — that stays a
/// future decision.
pub fn fts5_match_expr(q: &str) -> String {
    q.replace(|c: char| FTS5_STRIP_CHARS.contains(c), " ")
        .trim()
        .to_string()
}

/// Same sanitisation as [`fts5_match_expr`], but terms joined with `conj`.
///
/// The strip charset is [`FTS5_STRIP_CHARS`], shared via delegation to
/// [`fts5_match_expr`] below — byte-identical between the two by construction,
/// never two literals to drift.
///
/// Only `"OR"` (case-insensitive) changes the output: terms joined with
/// `" OR "`. Any other `conj` returns the default AND form verbatim, so
/// existing callers keep HEAD behaviour.
///
/// A bare `OR`/`AND`/`NOT` token from the user is dropped before the join:
/// otherwise `"a OR b"` becomes `"a OR OR OR b"`, a second operator with no
/// operand that FTS5 rejects. The match is case-insensitive because FTS5
/// operators are.
pub fn fts5_match_expr_joined(q: &str, conj: &str) -> String {
    let base = fts5_match_expr(q);
    if conj.eq_ignore_ascii_case("OR") {
        base.split_whitespace()
            .filter(|t| {
                !t.eq_ignore_ascii_case("OR")
                    && !t.eq_ignore_ascii_case("AND")
                    && !t.eq_ignore_ascii_case("NOT")
            })
            .collect::<Vec<_>>()
            .join(" OR ")
    } else {
        base
    }
}

/// What [`Store::note_append_section`] did.
#[derive(Debug, Clone)]
pub struct Appended {
    /// The note the section actually landed in. Differs from the requested path
    /// only when the day's note was at its write limit and this event started a
    /// rotated part.
    pub path: String,
    /// Row id of that note.
    pub id: i64,
    /// Its content after the append, or unchanged when `appended` is `false`.
    pub content: String,
    /// `false` when the dedup marker was already present, so nothing was written.
    pub appended: bool,
    /// The write-limit error that forced a rotation, for the caller's log line.
    pub rotated_because: Option<String>,
}

/// Outcome of a chunk rebuild, reported by every path that re-chunks a note so a
/// lost semantic index is visible in command output instead of silent.
///
/// The `nulls` counter is the one that matters operationally: it is the number of
/// chunks a backfill pass still owes the vector stream. `diverged` and
/// `unmatched` say *why* a vector was not written, which is what makes a
/// reindex report diagnosable rather than merely countable.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ChunkSyncStats {
    /// Chunks written.
    pub total: usize,
    /// Chunks written with a real, non-zero vector: `rehydrated + preserved`.
    ///
    /// Kept as the aggregate every existing caller already prints; the two
    /// components are what `REINDEX_DONE` additionally breaks out, because
    /// "recovered by this run" and "survived from before" are different
    /// operational stories.
    pub embedded: usize,
    /// Chunks given a vector the caller had just computed.
    pub rehydrated: usize,
    /// Subset of `rehydrated` applied from text that only *passed* the reuse
    /// threshold rather than matching exactly. Zero at the default policy
    /// (`1.0`), non-zero only under an explicit `BRAIN_REUSE_SIMILARITY` opt-in.
    /// Counted because "fresh but not exact" is the one case where a reindexed
    /// vector is still approximate, and folding it into `rehydrated` alone would
    /// hide it.
    pub stale_fresh: usize,
    /// Chunks that kept a vector already stored for the same text.
    pub preserved: usize,
    /// Subset of `preserved` kept through the similarity fallback rather than an
    /// exact text match, i.e. the vector is slightly stale. Counted separately so
    /// "how often does this happen" is answerable without reading logs.
    pub stale_reused: usize,
    /// Chunks written with SQL `NULL` — indexed by FTS only, awaiting a backfill.
    pub nulls: usize,
    /// Caller-supplied vectors discarded because the chunk text no longer matches
    /// the text they were computed from: the note was edited between the embed
    /// pass and this write. Never applied — a vector for other text is a wrong
    /// answer, not a degraded one.
    pub diverged: usize,
    /// Stored vectors discarded because their chunk's text changed too much to
    /// reuse (below the similarity threshold) and no fresh vector was available.
    pub unmatched: usize,
}

/// Vectors previously stored for one note, keyed by `chunk_index`.
///
/// The value is `(snippet, embedding)`. The snippet is what makes reuse
/// *safe*: a vector is only carried over to text it still describes — see
/// [`chunk_text_matches`] for the exact rule.
pub type ChunkSnapshot = std::collections::HashMap<i32, (String, Option<Vec<f32>>)>;

/// A freshly computed vector paired with the exact chunk text it was computed
/// from.
///
/// The pairing is the whole point. Both embedding producers — the reindex
/// backfill and the background queue — run *outside* the write transaction, so
/// the note can be edited while they work; a bare `HashMap<i32, Vec<f32>>` could
/// not tell a vector that still describes the chunk from one that describes text
/// the note no longer has, and would silently attach the latter. Carrying the
/// source text turns that into a check at the single point where the write
/// happens.
pub type FreshVector = (String, Vec<f32>);
pub type FreshVectors = std::collections::HashMap<i32, FreshVector>;

/// One note's embedding batch, as computed outside the write transaction.
#[derive(Debug, Default, Clone)]
pub struct NoteEmbed {
    /// Chunk texts, positionally aligned with `vectors`.
    pub chunks: Vec<String>,
    /// Vectors, in `chunk_index` order.
    pub vectors: Vec<Vec<f32>>,
}

impl NoteEmbed {
    /// Pairs each vector with the chunk text it came from.
    ///
    /// A vector whose slot has no text is dropped rather than guessed: provenance
    /// cannot be established, and an unattributable vector is a wrong answer.
    pub fn fresh_vectors(&self) -> FreshVectors {
        let mut out = FreshVectors::new();
        for (i, v) in self.vectors.iter().enumerate() {
            if let Some(text) = self.chunks.get(i) {
                out.insert(i as i32, (text.clone(), v.clone()));
            } else {
                eprintln!(
                    "brain-store: dropping vector at index {i}: no chunk text to verify it against \
                     ({} vectors for {} chunks)",
                    self.vectors.len(),
                    self.chunks.len()
                );
            }
        }
        out
    }
}


/// Embedding coverage of the `chunks` table.
///
/// `coverage_pct` deliberately counts only *usable* vectors. A `NOT NULL` BLOB of
/// zeros would otherwise be reported as embedded while scoring `0.0` against
/// every query — which is exactly how 99.6% of the production index ended up
/// useless while the raw `NOT NULL` count looked healthy.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct EmbeddingCoverage {
    /// Every chunk row.
    pub total: i64,
    /// Chunks with a real, non-zero vector.
    pub embedded: i64,
    /// Chunks with SQL `NULL`.
    pub without_embedding: i64,
    /// Chunks whose BLOB is present but all-zero: stored, scored as `0.0`, useless.
    pub zero_vector: i64,
    /// `embedded / total * 100`, rounded to 2 decimals; `0.0` when there are no chunks.
    pub coverage_pct: f64,
}

/// True when `v` has a zero L2 norm, i.e. `cosine` would score it `0.0` against
/// anything. Such a vector carries no information and must never be persisted.
fn is_zero_vector(v: &[f32]) -> bool {
    v.iter().all(|f| *f == 0.0)
}

/// The single SQL definition of "this chunk has a usable vector", shared by
/// [`Store::embedding_coverage`] and [`Store::chunk_embedding_counts`].
///
/// `zeroblob(n)` is n zero bytes and blob equality is memcmp, so `= zeroblob(n)`
/// names the legacy BLOB-of-zeros rows exactly; the `length(embedding) = n` guard
/// keeps a short blob from being misreported as a real vector. `zero = true`
/// selects the unusable rows instead of the usable ones.
///
/// The point of sharing it is that "embedded" is a claim about quality, not about
/// `NOT NULL`. Two copies of this predicate is how a per-note badge ends up
/// calling a zero vector indexed while the global coverage calls it missing.
fn embedded_predicate(zero: bool) -> String {
    format!(
        "embedding IS NOT NULL AND length(embedding)={n} AND embedding {op} zeroblob({n})",
        n = EMBED_BLOB_BYTES,
        op = if zero { "=" } else { "<>" }
    )
}

/// Chunk counts for one note, as reported by [`Store::chunk_embedding_counts`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ChunkEmbeddingCounts {
    /// Every chunk of the note.
    pub total: i64,
    /// Chunks with a real, non-zero vector — same meaning as
    /// [`EmbeddingCoverage::embedded`].
    pub embedded: i64,
}

/// Identity used for the cross-process embed lock, unique per process and per
/// role (`brain:reindex:1234`, `brain:queue:1234`).
///
/// Owned here rather than in a caller because the lock is a store concept: an
/// owner string that is not unique per process would let two processes each
/// believe they hold the same lock, which is the exact case the lock exists to
/// prevent. See [`Store::try_acquire_embed_lock`].
pub fn embed_lock_owner(role: &str) -> String {
    format!("brain:{}:{}", role, std::process::id())
}

/// Snapshot of the cross-process embed lock, for status reporting.
///
/// One value rather than three calls so a status response cannot show a holder
/// from one instant and an age from another, and so every reader degrades to
/// `None` instead of failing the whole status.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EmbedLockState {
    /// Owner string, e.g. `brain:queue:1234`.
    pub holder: Option<String>,
    /// Seconds since the lock was taken. `None` for a legacy lock value that
    /// carried no `taken_at`.
    pub age_secs: Option<i64>,
    /// Seconds until it expires; at or below zero the lock is abandoned and the
    /// next acquirer may take it over.
    pub expires_in_secs: Option<i64>,
}

impl Store {
    /// Holder, age and countdown of the embed lock, in one read.
    pub fn embed_lock_state(&self) -> EmbedLockState {
        EmbedLockState {
            holder: self.embed_lock_holder().ok().flatten(),
            age_secs: self.embed_lock_age_secs().ok().flatten(),
            expires_in_secs: self.embed_lock_expires_in_secs().ok().flatten(),
        }
    }
}

/// Splits an `_meta.embed_lock` value into `(owner, taken_at, expires_at)`.
///
/// The field carries `taken_at` as well as `expires_at` because age cannot be
/// derived from an expiry alone: the TTL is a caller-supplied argument, so
/// `expires_at` alone does not say when the lock was taken, and "how long has the
/// queue been standing down" is the question a stuck lock raises. The second
/// field of a legacy two-field value is an expiry with no matching `taken_at`, so
/// it is reported as `"0"` and every age derived from it is `None` rather than
/// silently wrong.
///
/// This is a *value* format inside `_meta`, not a schema change: no DDL, no new
/// column, no new table.
fn parse_lock_value(v: &str) -> (&str, &str, &str) {
    let mut parts = v.split('|');
    let owner = parts.next().unwrap_or("");
    match (parts.next(), parts.next()) {
        (Some(taken), Some(expires)) => (owner, taken, expires),
        // Legacy `owner|expires`.
        (Some(expires), None) => (owner, "0", expires),
        (None, _) => (owner, "0", "0"),
    }
}

/// Default minimum token-set similarity for reusing a stored vector on changed
/// text. **`1.0` — exact match only.**
///
/// This was `0.90`, and the reason it no longer can be is measured, not
/// theoretical. With the very function below it scores:
///
/// ```text
/// "Todo serviço DEVE validar o token JWT" → "…NÃO DEVE validar…"   sim = 0.9615  REUSED
/// "O comando de release DEVE usar a tag"  → "…NÃO DEVE usar…"       sim = 0.9444  REUSED
/// ```
///
/// Inverting a rule costs **one token** (intersection 14, union 15), and negation
/// is the entire content of this project's `regras` layer. A stored vector for
/// "DEVE validar" answers a search about "NÃO DEVE validar" with the *opposite*
/// of what the note says — strictly worse than `NULL`, which is merely silent.
///
/// The justification the old constant carried ("keep the reformatting class")
/// does not survive contact with [`normalize_for_match`]: casefolding plus
/// whitespace collapsing already resolves reformatting at `sim == 1.0`, proven by
/// `test_chunks_sync_preserves_a_vector_across_a_whitespace_only_edit`
/// (`preserved = 1, stale_reused = 0`). A threshold below 1.0 therefore buys
/// nothing legitimate and costs a class of silent wrong answers.
///
/// Operators who knowingly prefer a stale vector over `NULL` can still ask for it
/// with `BRAIN_REUSE_SIMILARITY=0.9` — an explicit opt-in, resolved once per
/// [`Store`] open into a [`ReusePolicy`], never a hardcoded number at a call site.
pub const DEFAULT_REUSE_SIMILARITY: f32 = 1.0;

/// Env var that lowers [`DEFAULT_REUSE_SIMILARITY`]. Opt-in only.
pub const REUSE_SIMILARITY_ENV: &str = "BRAIN_REUSE_SIMILARITY";

/// Tokens that *remove* normative force from a claim.
///
/// Matched against the *normalised* text ([`normalize_for_match`]), so entries
/// are lowercase and whitespace-free; both the accented and unaccented spellings
/// are listed because normalisation lowercases but does not strip diacritics.
///
/// X-03. `proibido`/`proibida` used to sit in [`OBLIGATION_TOKENS`], which is a
/// polarity inversion rather than a near miss: "obrigatorio validar o token"
/// (+1) and "proibido validar o token" (+1) scored the same, so the guard saw
/// *no change* on the exact axis it exists to catch. The reviewer measured it:
/// `nf 1 vs 1`, `delta 0`, `Similar(0.6)`. It was not exploitable at the default
/// threshold only because 0.6 < 1.0 caught it by accident — the guard's own
/// claim was false.
///
/// The prohibition synonyms are listed together for the same reason: an
/// obligation flipped to any of them is the same edit.
const NEGATION_TOKENS: &[&str] = &[
    "nao", "não", "nunca", "jamais", "nenhum", "nenhuma", "sem", "salvo", "exceto", "except",
    "never", "not", "none", "neither", "nor", "without", "unless", "only", "apenas", "somente",
    "unicamente", "cannot", "isn't",
    // X-03: prohibition, not obligation.
    "proibido", "proibida", "proibidos", "proibidas", "vetado", "vetada", "forbidden",
];

/// Tokens that *carry* normative force: dropping one changes what the sentence
/// asserts, exactly as adding a negation does.
///
/// X-03 audit, both directions. Every entry here was checked for whether it
/// expresses *polarity* (the claim is now the opposite) or merely *intensity*
/// (the claim is the same, said more firmly), and the borderline cases are called
/// out rather than left to be discovered:
///
/// - `sempre` / `always` are **intensity, not polarity**, and are kept anyway.
///   Dropping "sempre" does not flip a claim, so they add no detection power here;
///   keeping them can only cause a *refusal* to reuse (a wasted re-embed, which is
///   visible in `stale_reused`/`unmatched`). Removing them would widen reuse for no
///   detection gain, so they stay. `sempre validar` → `nunca validar` is caught by
///   `nunca` regardless.
/// - `should` is kept as an obligation. In English it is advisory and its absence
///   does weaken a claim, which is the same class as dropping `must`; treating it as
///   neutral would make `should validate` → `validate` invisible.
/// - `required` / `requires` / `mandatory` / `shall` / `must` are unambiguous.
/// - The restrictive words (`only`, `apenas`, `somente`, `unicamente`) are in
///   [`NEGATION_TOKENS`] for the same conservative reason: they are intensity, and
///   the list errs toward refusing reuse.
const OBLIGATION_TOKENS: &[&str] = &[
    "deve", "devem", "deve-se", "deverao", "deverá", "obrigatorio", "obrigatoria",
    "obrigatório", "obrigatória", "must", "should", "shall",
    "required", "requires", "mandatory", "sempre", "always",
];

/// How much normative force a text carries: negations minus obligations.
///
/// A pure negation stop-list is not enough, and the reason is `deve`/`must`: they
/// appear on *both* sides of a polarity flip, so a stop-list of negation words
/// alone cannot see "DEVE validar" → "validar" (the obligation deleted) — it sees
/// no negation token on either side and waves the edit through. Counting both
/// directions catches both: an inserted negation and a deleted obligation each
/// move the score by one, and reuse is refused when the score changes at all.
fn normative_force(text: &str) -> i32 {
    let mut score = 0i32;
    for tok in text.split(' ') {
        if tok.is_empty() {
            continue;
        }
        if NEGATION_TOKENS.contains(&tok) {
            score -= 1;
        } else if OBLIGATION_TOKENS.contains(&tok) {
            score += 1;
        }
    }
    score
}

/// Canonical form of a chunk text for equality matching: case-folded, with every
/// run of whitespace collapsed to one space and the ends trimmed.
///
/// This is deliberately *only* used to decide whether a stored vector still
/// describes the chunk. The stored `snippet` keeps the original bytes, so what a
/// reader sees is never normalised behind their back.
fn normalize_for_match(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for word in s.split_whitespace() {
        if pending_space {
            out.push(' ');
        }
        out.extend(word.chars().flat_map(char::to_lowercase));
        pending_space = true;
    }
    out
}

/// Jaccard similarity over the token sets of two normalised chunk texts.
///
/// Token-based rather than character-based because the differences that must be
/// absorbed are *between* words (spacing, wrapping) while the differences that
/// must be caught are *which* words are present. A character-level metric reads
/// "RUST" vs "Rust rules" as ~0.7 similar, which is above any threshold worth
/// setting; the token metric reads it as 0.0, which is the honest answer.
fn token_similarity(a: &str, b: &str) -> f32 {
    use std::collections::HashSet;
    let ta: HashSet<&str> = a.split(' ').filter(|t| !t.is_empty()).collect();
    let tb: HashSet<&str> = b.split(' ').filter(|t| !t.is_empty()).collect();
    if ta.is_empty() && tb.is_empty() {
        return 1.0;
    }
    let inter = ta.intersection(&tb).count();
    let union = ta.union(&tb).count();
    if union == 0 { 0.0 } else { inter as f32 / union as f32 }
}

/// How a stored vector relates to the chunk text it would be reused for.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ChunkMatch {
    /// Normalised text is identical: the vector is exactly right.
    Exact,
    /// Text is similar enough to keep the vector, which becomes slightly stale.
    /// Only reachable when a [`ReusePolicy`] with `min_similarity < 1.0` is in
    /// force — see [`DEFAULT_REUSE_SIMILARITY`] for why the default is exact.
    Similar(f32),
    /// Text changed too much, or its normative force flipped: the vector must not
    /// be reused. The second reason is a *separate* check from the similarity one
    /// and is not overridable by a threshold.
    Different,
}

/// The single place a reuse threshold is resolved, so
/// [`Store::chunks_sync`] and [`Store::chunks_needing_embedding`] cannot drift
/// apart. They used to: the queue treated `Similar` as "already has a vector" and
/// so never re-embedded, while `chunks_sync` would go on reusing the stale one —
/// the error sustained itself and only a manual `reindex --all` cleared it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReusePolicy {
    /// Minimum token-set similarity for reuse. `1.0` = normalised-exact only.
    pub min_similarity: f32,
}

impl ReusePolicy {
    /// Exact-match-only: the default, and the only safe automatic choice.
    ///
    /// Derived from [`DEFAULT_REUSE_SIMILARITY`] rather than restating `1.0`, so
    /// the documented default and the enforced one cannot drift apart — a constant
    /// that only appears in a doc comment is a constant nobody tests.
    pub const EXACT: ReusePolicy = ReusePolicy { min_similarity: DEFAULT_REUSE_SIMILARITY };

    /// Reads `BRAIN_REUSE_SIMILARITY`, falling back to
    /// [`DEFAULT_REUSE_SIMILARITY`] on anything unparseable or out of range.
    ///
    /// A bad value warns rather than silently changing behaviour: an operator who
    /// typed `0,9` (comma) must not end up with either an error or a threshold
    /// nobody intended.
    pub fn from_env() -> Self {
        match std::env::var(REUSE_SIMILARITY_ENV) {
            Ok(raw) if !raw.trim().is_empty() => match raw.trim().parse::<f32>() {
                Ok(v) if v > 0.0 && v <= 1.0 => {
                    if v < DEFAULT_REUSE_SIMILARITY {
                        eprintln!(
                            "brain-store: {REUSE_SIMILARITY_ENV}={v} opts in to reusing vectors across text \
                             edits. A chunk whose rule was inverted (DEVE -> NAO DEVE) scores above 0.9 and \
                             will keep the wrong vector unless the negation guard catches it."
                        );
                    }
                    ReusePolicy { min_similarity: v }
                }
                _ => {
                    eprintln!(
                        "brain-store: ignoring {REUSE_SIMILARITY_ENV}={raw:?}: expected a number in (0, 1]. \
                         Using {DEFAULT_REUSE_SIMILARITY}."
                    );
                    ReusePolicy::EXACT
                }
            },
            _ => ReusePolicy::EXACT,
        }
    }
}

impl Default for ReusePolicy {
    fn default() -> Self {
        ReusePolicy::from_env()
    }
}

/// Classifies `stored` against `current` for vector reuse.
///
/// Two independent refusals, in this order:
///
/// 1. **Normative-force flip** ([`normative_force`]). Independent of the
///    threshold and *not* overridable by it, because the failure it prevents is
///    not "slightly stale" — it is a vector standing in for a claim the note no
///    longer makes. `DEVE validar` → `NÃO DEVE validar` inserts one token and
///    scores 0.96, which is above any threshold worth setting; the guard is what
///    stops it.
/// 2. **Similarity** against `policy.min_similarity`. At the default of 1.0 this
///    only ever admits text that is identical after normalisation, which is the
///    reformatting class that the old byte-equality match used to lose.
///
/// # What this guard does not do (X-03)
///
/// The old doc for this function claimed the guard prevents *"a vector that
/// answers the opposite of the note"*. That was **false**, and the review is what
/// showed it: `proibido` was listed as an obligation, so flipping an obligation
/// into a prohibition scored `+1` on both sides, `delta 0`, and the guard saw no
/// change at all on the one axis it exists for.
///
/// The honest, narrower claim: **this is a token-count heuristic over a fixed
/// word list, not an understanding of meaning.** It refuses reuse when the
/// difference in the count of listed negation and obligation tokens is non-zero.
/// That catches `DEVE` → `NÃO DEVE`, `obrigatório` → `proibido`, `must` →
/// `must not`, and it catches them regardless of the threshold. It does **not**
/// catch a polarity change expressed with none of the listed words — "sempre
/// validar" → "validar sempre que possível" is a different claim and scores
/// nothing — and it does not catch a change that adds and removes one token each,
/// which nets to zero.
///
/// The defence in depth is the default [`DEFAULT_REUSE_SIMILARITY`] of 1.0, which
/// refuses anything but normalised-exact text regardless. The guard exists so that
/// lowering that threshold, deliberately, does not re-open the hole.
fn chunk_text_matches(stored: &str, current: &str, policy: ReusePolicy) -> ChunkMatch {
    let a = normalize_for_match(stored);
    let b = normalize_for_match(current);
    if a == b {
        return ChunkMatch::Exact;
    }
    // Defence in depth: independent of the threshold, and deliberately checked
    // before it, so lowering `BRAIN_REUSE_SIMILARITY` cannot re-open this.
    if normative_force(&a) != normative_force(&b) {
        return ChunkMatch::Different;
    }
    let sim = token_similarity(&a, &b);
    if sim >= policy.min_similarity { ChunkMatch::Similar(sim) } else { ChunkMatch::Different }
}


/// Decodes a stored `embedding` BLOB back into an `Option<Vec<f32>>`.
///
/// `None` for SQL `NULL`. A BLOB of the wrong length decodes to whatever whole
/// `f32`s it contains; callers pair this with a length check because `search`
/// skips vectors whose width differs from the query vector.
fn decode_embedding(blob: Option<&[u8]>) -> Option<Vec<f32>> {
    blob.map(|b| {
        b.chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    })
}


pub struct Store {
    conn: Connection,
    /// The file this handle was opened from, for callers that need to act on the
    /// same database from outside the store (the embedding queue, `brain backup`).
    /// `":memory:"` for [`Store::open_in_memory`].
    path: String,
    /// How willing this store is to reuse a vector across a text edit.
    ///
    /// Held here rather than read from the environment at each decision so that
    /// `chunks_sync` (which reuses) and `chunks_needing_embedding` (which decides
    /// what to re-embed) provably apply the *same* rule — they were two separate
    /// call sites of a free function, and they drifted.
    reuse: ReusePolicy,
}

/// `Store` may be *held* across an `.await` but must never be *borrowed* across one.
///
/// `rusqlite::Connection` is `Send` but not `Sync`, so a `&Store` is not `Send` and
/// a handler that keeps one live across a network call stops being `Send` — axum
/// and rmcp then refuse to spawn it, with a type error rather than a runtime
/// surprise. Asserting `Send` here keeps the arrangement that makes the whole
/// codebase work legal: every phase opens its own `Store`, uses it synchronously,
/// and drops it before the next `.await`. If someone later adds a field that is
/// neither `Send` nor `Sync` (a shared cache, say), this fails to compile.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Store>();
};

impl Store {
    pub fn open(path: &str) -> Result<Self> {
        Self::open_with_reuse(path, ReusePolicy::from_env())
    }

    /// Opens `path` with an explicit reuse policy. Tests use this to exercise the
    /// opt-in stale-reuse branch without mutating the process environment.
    pub fn open_with_reuse(path: &str, reuse: ReusePolicy) -> Result<Self> {
        let conn = Connection::open(path).context("open db")?;
        let mut s = Self { conn, path: path.to_string(), reuse };
        s.init_schema()?;
        Ok(s)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::open_in_memory_with_reuse(ReusePolicy::from_env())
    }

    /// Opens `path` for **reading only**, without running [`Store::init_schema`].
    ///
    /// For guards that inspect a database before deciding what to do with it. The
    /// two properties that matter are the two [`Store::open_with_reuse`] lacks:
    ///
    /// - **No DDL.** `init_schema` creates tables, indexes and triggers and
    ///   writes the `_meta` version row, so a *guard* that called [`Store::open`]
    ///   was writing to the very database it had been asked to look at — on a
    ///   file the caller believed it was merely inspecting. Worse, on a
    ///   read-only or full disk it fails, and a guard that treats "could not
    ///   open" as "therefore nothing to protect" then takes the destructive
    ///   branch.
    /// - **No `sqlite3` write mode.** `SQLITE_OPEN_READ_ONLY` is enforced by the
    ///   driver, so a bug in the calling code cannot escalate to a write.
    ///
    /// Fails, rather than degrading to a fresh empty store, on anything it cannot
    /// read. Two layers are needed to deliver that, and the first is not enough:
    ///
    /// - `Connection::open_with_flags` is **lazy**. It installs a flag and hands
    ///   back a handle; SQLite does not read a single byte of the file until the
    ///   first statement runs. So a garbage file, or one whose header is
    ///   unreadable, opened successfully in a first revision of this function —
    ///   and a caller asking "may I delete this?" would have been told yes.
    /// - So one statement is forced here, against `sqlite_master`, which parses
    ///   the header and the schema. A file that fails it is refused at the open.
    ///
    /// A WAL database whose `-shm` sidecar is missing or unreadable is rejected
    /// this way even though the file itself is legible — correct, because a caller
    /// that cannot read a database must not be trusted to delete it.
    pub fn open_read_only(path: &str) -> Result<Self> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .context("open db read-only")?;
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))
            .context("read db schema (the file may be corrupt or unreadable)")?;
        Ok(Self { conn, path: path.to_string(), reuse: ReusePolicy::from_env() })
    }

    /// In-memory store with an explicit policy; the in-memory twin of
    /// [`Store::open_with_reuse`].
    pub fn open_in_memory_with_reuse(reuse: ReusePolicy) -> Result<Self> {
        let conn = Connection::open_in_memory().context("open in-mem")?;
        let mut s = Self { conn, path: ":memory:".to_string(), reuse };
        s.init_schema()?;
        Ok(s)
    }

    /// The database file this handle was opened from.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The reuse policy in force, for diagnostics and for a caller that has to
    /// mirror the store's own decision.
    pub fn reuse_policy(&self) -> ReusePolicy {
        self.reuse
    }

    /// Puts the file into WAL, retrying while another process holds the
    /// conversion.
    ///
    /// X-01, and the last statement that still needed handling. Converting the
    /// journal takes a lock that SQLite does **not** pass to the busy handler, so
    /// `busy_timeout` does not cover it — which is why a bounded retry is here
    /// rather than relying on the timeout. It is only reachable on a database that
    /// does not exist yet: every concurrent opener converts at once, and the
    /// losers only have to wait for the winner. Measured as the final
    /// `SQLITE_BUSY` under twelve simultaneous opens of a fresh file, once the
    /// DDL and the append had both been fixed.
    ///
    /// The sleep blocks the calling thread, and `Store::open` is called from async
    /// handlers. That is a deliberate, bounded trade: it is off the steady-state
    /// path (a WAL database answers the pragma as a query and returns on the first
    /// attempt without sleeping), it cannot exceed ~500 ms, and the alternative —
    /// failing a session event, or a write lock on every open — is worse.
    fn set_journal_mode_wal(&self) -> Result<()> {
        const ATTEMPTS: u32 = 25;
        const PAUSE: std::time::Duration = std::time::Duration::from_millis(20);
        let mut last = String::new();
        for attempt in 0..ATTEMPTS {
            match self.conn.execute_batch("PRAGMA journal_mode=WAL;") {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last = e.to_string();
                    let busy = is_busy(&e);
                    if !busy {
                        return Err(e).context("set journal_mode=WAL");
                    }
                    if attempt + 1 < ATTEMPTS {
                        std::thread::sleep(PAUSE);
                    }
                }
            }
        }
        anyhow::bail!("could not set journal_mode=WAL after {ATTEMPTS} attempts: {last}")
    }

    /// Whether the database on disk is already at [`SCHEMA_VERSION`].
    ///
    /// X-01. A read, and the reason the hook can be called on every tool result.
    /// `init_schema` used to re-run the whole DDL batch on *every* `Store::open`,
    /// which is every MCP request, every queue phase and twice per hook event — so
    /// it needed the write lock every single time, and a write lock taken by a
    /// read-only `brain status` is how twelve concurrent hooks turned into twelve
    /// `SQLITE_BUSY` deaths.
    ///
    /// `_meta` is written *inside* the DDL transaction, so "the version row says 4"
    /// implies "the transaction that built the schema at version 4 committed". A
    /// fresh database has no `_meta` at all, the query errors, and the DDL runs —
    /// which is also the migration path, since an older database's row says 3.
    fn schema_is_current(&self) -> bool {
        self.conn
            .query_row("SELECT value FROM _meta WHERE key='version'", [], |r| r.get::<_, String>(0))
            .map(|v| v.trim() == SCHEMA_VERSION.to_string())
            .unwrap_or(false)
    }

    /// The `_meta.version` value on disk, as read right now.
    ///
    /// `Ok(None)` means the database has no `_meta` row at all — a file that
    /// exists but was never initialised by this schema. That is a real state and
    /// not an error, so it is reported rather than folded into a default: a
    /// caller asking "what version is this?" is asking exactly this question, and
    /// answering [`SCHEMA_VERSION`] for an unknown database would be the shape of
    /// bug this whole accessor exists to rule out.
    ///
    /// US-01.1 AC2 ("create DB with schema v4") is the reason it is public. That
    /// AC is about what lands on disk, and a test that could only observe it
    /// through `Store::open` succeeding would pass against a schema that was one
    /// version behind, or half-created, as long as it did not error.
    pub fn schema_version(&self) -> Result<Option<String>> {
        match self.conn.query_row("SELECT value FROM _meta WHERE key='version'", [], |r| r.get::<_, String>(0)) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Every table present, in `sqlite_master` order.
    ///
    /// The counterpart to [`Self::schema_version`]: the version row is a single
    /// string a DDL batch writes, so asserting on it alone would pass for a
    /// database where the batch wrote the row and then failed to create the
    /// tables. Reading the names is what makes "schema v4" mean 15 tables rather
    /// than one string.
    pub fn table_names(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    fn init_schema(&mut self) -> Result<()> {
        // X-01: pin the busy timeout explicitly.
        //
        // Worth being precise about what this does and does not do. rusqlite
        // 0.32 already gives every new connection a 5000 ms busy timeout, and
        // this restates that value, so it is **not** what fixed the hook — the
        // things that did were the `IMMEDIATE` transactions below and the atomic
        // read-modify-write. It is here to make the wait a property of this crate
        // rather than of a dependency's current default, because the failure it
        // prevents is losing a session event: if rusqlite changed that default,
        // or a future `busy_timeout(0)` crept in, the `an_append_waits_for_a_
        // concurrent_writer_instead_of_failing` test is what would notice.
        self.conn.busy_timeout(std::time::Duration::from_millis(u64::from(BUSY_TIMEOUT_MS)))?;
        // `journal_mode` cannot run inside a transaction, and takes a lock only
        // when it is actually converting the file; on a database already in WAL
        // this is a query.
        self.set_journal_mode_wal()?;
        // Per-connection settings, no lock involved.
        self.conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;
        // X-01, step 2: nothing to do for a database already at this version, and
        // taking a write lock to re-create tables that exist would serialise every
        // reader in the system behind the DDL.
        if self.schema_is_current() {
            return Ok(());
        }
        // X-01, step 3: the DDL in one transaction, and `IMMEDIATE` rather than
        // the default `DEFERRED`.
        //
        // Concurrent openers used to interleave statement by statement and produce
        // `no such table: notes` and `trigger notes_fts_delete already exists` — a
        // process reading a schema another one was halfway through building. The
        // statements are still the same idempotent ones and the database file is
        // unchanged; wrapping them just means a second opener waits instead of
        // observing a half-built schema.
        //
        // `IMMEDIATE` is load-bearing, not a style choice: `DEFERRED` takes the
        // write lock lazily on the first write, and SQLite returns `SQLITE_BUSY`
        // *without consulting the busy handler* on that upgrade when a snapshot
        // would go stale — so the timeout installed above would be silently
        // ignored on exactly the concurrent path it exists for.
        let tx = rusqlite::Transaction::new_unchecked(&self.conn, rusqlite::TransactionBehavior::Immediate)
            .context("begin the schema transaction")?;
        tx.execute_batch(&format!(r#"
            CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                description TEXT DEFAULT '',
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                layer TEXT NOT NULL,
                scope TEXT,
                content TEXT NOT NULL,
                project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
                tags TEXT DEFAULT '[]',
                pinned INTEGER DEFAULT 0,
                expires_at TEXT,
                version INTEGER DEFAULT 1,
                created_at TEXT DEFAULT (datetime('now')),
                updated_at TEXT DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_notes_layer ON notes(layer);
            CREATE INDEX IF NOT EXISTS idx_notes_scope ON notes(scope);
            CREATE INDEX IF NOT EXISTS idx_notes_expires ON notes(expires_at);
            CREATE INDEX IF NOT EXISTS idx_notes_project ON notes(project_id);

            CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
                path TEXT NOT NULL,
                layer TEXT NOT NULL,
                scope TEXT,
                snippet TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                total_chunks INTEGER NOT NULL,
                project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
                tags TEXT DEFAULT '[]',
                embedding BLOB,
                UNIQUE(path, chunk_index)
            );
            CREATE INDEX IF NOT EXISTS idx_chunks_path ON chunks(path);
            CREATE INDEX IF NOT EXISTS idx_chunks_layer ON chunks(layer);

            CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(title, body, tokenize='porter unicode61');
            DROP TRIGGER IF EXISTS notes_fts_insert;
            DROP TRIGGER IF EXISTS notes_fts_delete;
            DROP TRIGGER IF EXISTS notes_fts_update;
            CREATE TRIGGER notes_fts_insert AFTER INSERT ON notes BEGIN
                INSERT INTO notes_fts(rowid, title, body) VALUES (new.id, new.path, new.content);
            END;
            CREATE TRIGGER notes_fts_delete AFTER DELETE ON notes BEGIN
                DELETE FROM notes_fts WHERE rowid=old.id;
            END;
            CREATE TRIGGER notes_fts_update AFTER UPDATE ON notes BEGIN
                DELETE FROM notes_fts WHERE rowid=old.id;
                INSERT INTO notes_fts(rowid, title, body) VALUES (new.id, new.path, new.content);
            END;

            CREATE TABLE IF NOT EXISTS links (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_path TEXT NOT NULL,
                to_path TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_links_from ON links(from_path);

            CREATE TABLE IF NOT EXISTS entities (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                normalized TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS entity_links (
                entity_id INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
                note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
                PRIMARY KEY(entity_id, note_id)
            );

            CREATE TABLE IF NOT EXISTS note_projects (
                note_path TEXT NOT NULL,
                project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                PRIMARY KEY(note_path, project_id)
            );
            CREATE INDEX IF NOT EXISTS idx_note_projects_path ON note_projects(note_path);
            CREATE INDEX IF NOT EXISTS idx_note_projects_project ON note_projects(project_id);

            CREATE TABLE IF NOT EXISTS audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                action TEXT NOT NULL,
                path TEXT NOT NULL,
                prev_content TEXT,
                at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS _meta (key TEXT PRIMARY KEY, value TEXT);
            INSERT OR IGNORE INTO _meta(key,value) VALUES ('version','{v}');
            INSERT OR IGNORE INTO _meta(key,value) VALUES ('embedding_dim','{d}');
        "#, v=SCHEMA_VERSION, d=EMBEDDING_DIM))?;
        // ensure version updated
        tx.execute("UPDATE _meta SET value=? WHERE key='version'", params![SCHEMA_VERSION.to_string()])?;
        // migrate legacy: create note_projects if not exists (already via IF NOT EXISTS)
        tx.commit()?;
        Ok(())
    }

    // projects
    pub fn project_create(&self, name: &str, description: &str) -> Result<Project> {
        let name = name.trim();
        if name.is_empty() { anyhow::bail!("project name empty"); }
        self.conn.execute("INSERT INTO projects(name, description) VALUES (?1, ?2)", params![name, description])
            .map_err(|e| anyhow::anyhow!("project exists: {}", e))?;
        let _id = self.conn.last_insert_rowid();
        self.project_get(name)?.ok_or_else(|| anyhow::anyhow!("not found after create"))
    }

    pub fn project_get(&self, name: &str) -> Result<Option<Project>> {
        let row = self.conn.query_row("SELECT id, name, description, created_at FROM projects WHERE name=?1", params![name.trim()],
            |r| Ok(Project{ id: r.get(0)?, name: r.get(1)?, description: r.get(2)?, created_at: r.get(3)? })).optional()?;
        Ok(row)
    }

    pub fn project_list(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare("SELECT id, name, description, created_at FROM projects ORDER BY name")?;
        let rows = stmt.query_map([], |r| Ok(Project{ id: r.get(0)?, name: r.get(1)?, description: r.get(2)?, created_at: r.get(3)? }))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn project_delete(&self, name: &str) -> Result<bool> {
        let n = self.conn.execute("DELETE FROM projects WHERE name=?1", params![name.trim()])?;
        Ok(n>0)
    }

    pub fn project_notes(&self, name: &str) -> Result<Vec<brain_core::Note>> {
        let proj = self.project_get(name)?.ok_or_else(|| anyhow::anyhow!("project not found: {}", name))?;
        let now = Utc::now().to_rfc3339();
        // owned notes
        let mut stmt = self.conn.prepare("SELECT path, layer, scope, content, project_id, tags, pinned, expires_at, version FROM notes WHERE project_id=?1 AND (expires_at IS NULL OR expires_at > ?2) ORDER BY updated_at DESC")?;
        let owned = stmt.query_map(params![proj.id, now], |r| {
            let tags_s: String = r.get(5)?;
            let tags: Vec<String> = serde_json::from_str(&tags_s).unwrap_or_default();
            Ok(brain_core::Note{ path: r.get(0)?, layer: r.get(1)?, scope: r.get(2)?, content: r.get(3)?, project_id: r.get(4)?, tags, pinned: r.get::<_,i32>(6)?!=0, expires_at: r.get(7)?, version: r.get(8)? })
        })?.collect::<Result<Vec<_>, _>>()?;
        // linked notes via note_projects
        let mut stmt2 = self.conn.prepare("SELECT n.path, n.layer, n.scope, n.content, n.project_id, n.tags, n.pinned, n.expires_at, n.version FROM notes n JOIN note_projects np ON np.note_path=n.path WHERE np.project_id=?1 AND (n.expires_at IS NULL OR n.expires_at > ?2) ORDER BY n.updated_at DESC")?;
        let linked = stmt2.query_map(params![proj.id, now], |r| {
            let tags_s: String = r.get(5)?;
            let tags: Vec<String> = serde_json::from_str(&tags_s).unwrap_or_default();
            Ok(brain_core::Note{ path: r.get(0)?, layer: r.get(1)?, scope: r.get(2)?, content: r.get(3)?, project_id: r.get(4)?, tags, pinned: r.get::<_,i32>(6)?!=0, expires_at: r.get(7)?, version: r.get(8)? })
        })?.collect::<Result<Vec<_>, _>>()?;
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for n in owned.into_iter().chain(linked.into_iter()) {
            if seen.insert(n.path.clone()) { out.push(n); }
        }
        Ok(out)
    }

    pub fn note_link_project(&self, note_path: &str, project_name: &str) -> Result<()> {
        let proj = self.project_get(project_name)?.ok_or_else(|| anyhow::anyhow!("project not found: {}", project_name))?;
        // ensure note exists
        let exists: Option<i64> = self.conn.query_row("SELECT id FROM notes WHERE path=?1", params![note_path], |r| r.get(0)).optional()?;
        if exists.is_none() { anyhow::bail!("note not found: {}", note_path); }
        self.conn.execute("INSERT OR IGNORE INTO note_projects(note_path, project_id) VALUES (?1, ?2)", params![note_path, proj.id])?;
        Ok(())
    }

    pub fn note_unlink_project(&self, note_path: &str, project_name: &str) -> Result<bool> {
        let proj = self.project_get(project_name)?.ok_or_else(|| anyhow::anyhow!("project not found: {}", project_name))?;
        let n = self.conn.execute("DELETE FROM note_projects WHERE note_path=?1 AND project_id=?2", params![note_path, proj.id])?;
        Ok(n>0)
    }

    // notes upsert (path = layer/scope/path or layer/path)
    pub fn note_upsert(&self, path: &str, layer: &str, scope: Option<&str>, content: &str, project_id: Option<i64>, tags: &[String], pinned: bool, expires_at: Option<&str>) -> Result<i64> {
        Self::note_upsert_on(&self.conn, path, layer, scope, content, project_id, tags, pinned, expires_at)
    }

    /// [`Store::note_upsert`] on an explicit connection, so the identical write can
    /// also run inside a caller's transaction.
    ///
    /// X-01. An associated function over `&Connection` rather than a `&self`
    /// method, because `rusqlite::Transaction` derefs to the `Connection` it was
    /// opened on: this lets [`Store::note_append_section`] read a note and write
    /// it back *inside one transaction* without duplicating a single statement of
    /// the upsert, so the two paths cannot drift.
    #[allow(clippy::too_many_arguments)]
    fn note_upsert_on(conn: &Connection, path: &str, layer: &str, scope: Option<&str>, content: &str, project_id: Option<i64>, tags: &[String], pinned: bool, expires_at: Option<&str>) -> Result<i64> {
        let tags_json = serde_json::to_string(tags).unwrap_or("[]".into());
        let pinned_i = if pinned {1} else {0};
        let existing: Option<(i64, String)> = conn.query_row("SELECT id, content FROM notes WHERE path=?1", params![path], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        if let Some((id, prev)) = existing {
            conn.execute("INSERT INTO audit_log(action, path, prev_content) VALUES ('update', ?1, ?2)", params![path, prev])?;
            conn.execute("UPDATE notes SET layer=?1, scope=?2, content=?3, project_id=?4, tags=?5, pinned=?6, expires_at=?7, version=version+1, updated_at=datetime('now') WHERE id=?8",
                params![layer, scope, content, project_id, tags_json, pinned_i, expires_at, id])?;
            // NB: the note's chunk rows are deliberately NOT deleted here.
            //
            // They used to be, which made the vectors unrecoverable: `restore_audit`
            // snapshots the note's vectors to decide whether they still apply, but by
            // the time it ran, the update that preceded it had already thrown them
            // away. Preserving the rows across the text change and letting
            // `chunks_sync` compare snippets is what lets a vector outlive an edit
            // to unrelated parts of a note.
            conn.execute("DELETE FROM links WHERE from_path=?1", params![path])?;
            conn.execute("DELETE FROM entity_links WHERE note_id=?1", params![id])?;
            Self::insert_links_and_entities(conn, id, path, content, tags)?;
            Ok(id)
        } else {
            conn.execute("INSERT INTO notes(path, layer, scope, content, project_id, tags, pinned, expires_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![path, layer, scope, content, project_id, tags_json, pinned_i, expires_at])?;
            let id = conn.last_insert_rowid();
            conn.execute("INSERT INTO audit_log(action, path) VALUES ('create', ?1)", params![path])?;
            Self::insert_links_and_entities(conn, id, path, content, tags)?;
            Ok(id)
        }
    }

    fn insert_links_and_entities(conn: &Connection, note_id: i64, path: &str, content: &str, tags: &[String]) -> Result<()> {
        for link in brain_core::extract_wikilinks(content) {
            conn.execute("INSERT INTO links(from_path, to_path) VALUES (?1, ?2)", params![path, link])?;
        }
        for tag in tags {
            let norm = tag.to_lowercase();
            conn.execute("INSERT OR IGNORE INTO entities(name, normalized) VALUES (?1, ?2)", params![tag, norm])?;
            let eid: i64 = conn.query_row("SELECT id FROM entities WHERE name=?1", params![tag], |r| r.get(0))?;
            conn.execute("INSERT OR IGNORE INTO entity_links(entity_id, note_id) VALUES (?1, ?2)", params![eid, note_id])?;
        }
        Ok(())
    }

    /// Appends `section` to the note at `path` as one indivisible operation.
    ///
    /// # Why this method exists
    ///
    /// The session hook appends one section per tool result, and an agent's tool
    /// results overlap by design — several run before the previous one has
    /// finished. The hook used to do that append in three steps on one
    /// connection: `note_get`, build `previous + section` in the caller's memory,
    /// then `note_upsert` the whole thing. Three steps is a read-modify-write
    /// with no lock anywhere, so two processes interleaving between the read and
    /// the write both won: the second `note_upsert` wrote its own copy of the old
    /// content plus its own section over the first's, and the first event's
    /// section was gone. Measured at 2 of 8 and 7 of 8 events lost across
    /// concurrent hooks, with the *sequential* control losing none.
    ///
    /// # What "indivisible" means here
    ///
    /// One `BEGIN IMMEDIATE` transaction around the read *and* the write. SQLite
    /// grants that lock to one writer at a time, so the second hook's read
    /// necessarily observes the first hook's committed section and appends after
    /// it. There is no retry loop and no compare-and-swap because there is
    /// nothing to retry: the interleaving cannot be constructed.
    ///
    /// # Why not the alternatives
    ///
    /// - *Hold the spool's `flock` across the append.* The lock is on the spool
    ///   file, which is shared by every project and every agent on the host, and
    ///   it would have to be held across the embed. The hook is called on every
    ///   tool result of every agent, so that serialises unrelated projects behind
    ///   each other's embedding. Rejected on contention grounds, not correctness.
    /// - *Optimistic `UPDATE ... WHERE version = ?` with a retry.* Correct, but it
    ///   needs the same lock to make progress, a bounded retry count, and a
    ///   caller-side loop to re-derive the content. The transaction gets the same
    ///   serialisation in one statement pair and cannot run out of retries.
    ///
    /// Rotation is resolved inside the same transaction, so two hooks that both
    /// find the day's note full also both land in the new part rather than one
    /// overwriting the other.
    ///
    /// `dedup_marker` is the caller's idempotency token: if the note already
    /// contains it, nothing is written and [`Appended::appended`] is `false`. This
    /// is a substring test because the hook's marker is an event id embedded in a
    /// rendered markdown section, and it has to be decided against the same
    /// snapshot the write is based on — deciding it before the transaction is how
    /// the dedup and the append disagree under concurrency.
    #[allow(clippy::too_many_arguments)]
    pub fn note_append_section(
        &self,
        path: &str,
        layer: &str,
        scope: Option<&str>,
        section: &str,
        dedup_marker: &str,
        new_note_header: &str,
        rotated_header: &dyn Fn(u32, &str) -> String,
        project_id: Option<i64>,
        tags: &[String],
    ) -> Result<Appended> {
        let tx = rusqlite::Transaction::new_unchecked(&self.conn, rusqlite::TransactionBehavior::Immediate)
            .context("begin the append transaction")?;
        let existing: Option<(i64, String)> =
            tx.query_row("SELECT id, content FROM notes WHERE path=?1", params![path], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
        let (full, content, rotated_because) = match existing {
            Some((id, prev)) => {
                if !dedup_marker.is_empty() && prev.contains(dedup_marker) {
                    tx.rollback()?;
                    return Ok(Appended { path: path.to_string(), id, content: prev, appended: false, rotated_because: None });
                }
                let grown = format!("{}\n{}", prev.trim_end(), section);
                match brain_core::validate_content_limits(&grown) {
                    Ok(()) => (path.to_string(), grown, None),
                    Err(e) => {
                        // The day's note cannot take this section. Start a new one
                        // rather than refuse, and let the caller's log say so.
                        let part = Self::next_part_number(&tx, path)?;
                        let full = format!("{path}-{part}");
                        (full, rotated_header(part, &e.to_string()), Some(e.to_string()))
                    }
                }
            }
            None => (path.to_string(), format!("{}\n{}", new_note_header.trim_end(), section), None),
        };
        let id = Self::note_upsert_on(&tx, &full, layer, scope, &content, project_id, tags, false, None)?;
        tx.commit()?;
        Ok(Appended { path: full, id, content, appended: true, rotated_because })
    }

    /// Next rotated part number for `base`: the highest existing `{base}-N` plus
    /// one, starting at 2.
    ///
    /// Highest-plus-one rather than first-free, so two appends racing on the same
    /// day cannot both pick `-2` and have the second overwrite the first's
    /// section. Called from inside [`Store::note_append_section`]'s transaction,
    /// where it is exact: `BEGIN IMMEDIATE` means every writer that could have
    /// created a `-N` has already committed and is visible to this read.
    fn next_part_number(conn: &Connection, base: &str) -> Result<u32> {
        let prefix = format!("{base}-");
        let mut stmt = conn.prepare("SELECT path FROM notes WHERE path LIKE ?1 ESCAPE '\\' ORDER BY id")?;
        let rows = stmt.query_map(params![format!("{}%", like_escaped(&prefix))], |r| r.get::<_, String>(0))?;
        let mut highest = 1u32;
        for p in rows {
            let p = p?;
            // The date component contains dashes too, so the part number is
            // whatever follows the *last* one.
            if let Some(n) = p.rsplit('-').next().and_then(|s| s.parse::<u32>().ok()) {
                highest = highest.max(n);
            }
        }
        Ok(highest + 1)
    }

    pub fn note_get(&self, path: &str) -> Result<Option<brain_core::Note>> {
        let row = self.conn.query_row("SELECT path, layer, scope, content, project_id, tags, pinned, expires_at, version FROM notes WHERE path=?1", params![path],
            |r| {
                let tags_s: String = r.get(5)?;
                let tags: Vec<String> = serde_json::from_str(&tags_s).unwrap_or_default();
                Ok(brain_core::Note{ path: r.get(0)?, layer: r.get(1)?, scope: r.get(2)?, content: r.get(3)?, project_id: r.get(4)?, tags, pinned: r.get::<_,i32>(6)?!=0, expires_at: r.get(7)?, version: r.get(8)? })
            }).optional()?;
        Ok(row)
    }

    /// Row id of a note, if it exists.
    ///
    /// `brain_core::Note` deliberately carries no id — it is the public shape of a
    /// note, and ids are storage detail. Callers that re-chunk an existing note
    /// (the embedding queue) still need the row id, and this is the narrow way to
    /// get it without leaking `id` into the shared type.
    pub fn note_id(&self, path: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT id FROM notes WHERE path=?1", params![path], |r| r.get(0))
            .optional()?)
    }

    pub fn note_delete(&self, path: &str) -> Result<bool> {
        let prev: Option<String> = self.conn.query_row("SELECT content FROM notes WHERE path=?1", params![path], |r| r.get(0)).optional()?;
        if let Some(prev) = prev {
            self.conn.execute("INSERT INTO audit_log(action, path, prev_content) VALUES ('delete', ?1, ?2)", params![path, prev])?;
        }
        let n = self.conn.execute("DELETE FROM notes WHERE path=?1", params![path])?;
        // also clean note_projects links
        let _ = self.conn.execute("DELETE FROM note_projects WHERE note_path=?1", params![path]);
        Ok(n>0)
    }

    /// Inserts one chunk row.
    ///
    /// `embedding` is `Option<&[f32]>` and `None` is stored as SQL `NULL`, which
    /// is the *only* correct representation of "this chunk was not embedded":
    /// - `search` already filters `WHERE embedding IS NOT NULL`, so a `NULL`
    ///   chunk simply does not enter the vector stream, leaving FTS/entity/graph
    ///   ranking intact.
    /// - A BLOB of zeros would do the opposite. `cosine` returns `0.0` for a
    ///   zero-norm vector, so every zero chunk ties with every other zero chunk
    ///   and the whole lot still competes for the `truncate(50)` candidate budget,
    ///   evicting genuinely relevant notes. A zero vector is therefore *rejected*
    ///   here rather than stored: silence at this boundary is what allowed 735 of
    ///   738 chunks in the production index to be dead weight.
    ///
    /// A vector of the wrong width is rejected too — `search` discards mismatched
    /// widths silently, so a bad length would look like a missing vector with no
    /// trace of the cause.
    pub fn chunk_insert(&self, note_id: i64, path: &str, layer: &str, scope: Option<&str>, snippet: &str, chunk_index: i32, total_chunks: i32, project_id: Option<i64>, tags: &[String], embedding: Option<&[f32]>) -> Result<()> {
        let tags_json = serde_json::to_string(tags).unwrap_or("[]".into());
        let blob: Option<Vec<u8>> = match embedding {
            None => None,
            Some(v) => {
                if v.len() != EMBEDDING_DIM {
                    anyhow::bail!("embedding dim mismatch: got {}, expected {}", v.len(), EMBEDDING_DIM);
                }
                if is_zero_vector(v) {
                    anyhow::bail!("refusing to store a zero-norm embedding for {} chunk {}: it scores 0.0 for every query; pass None instead", path, chunk_index);
                }
                Some(v.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>())
            }
        };
        // OR REPLACE: chunk rows are uniquely keyed by (path, chunk_index) and
        // every rebuild path re-inserts the same key. A plain INSERT would make
        // the second write of an unchanged note fail on the unique index.
        self.conn.execute("INSERT OR REPLACE INTO chunks(note_id, path, layer, scope, snippet, chunk_index, total_chunks, project_id, tags, embedding) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![note_id, path, layer, scope, snippet, chunk_index, total_chunks, project_id, tags_json, blob])?;
        Ok(())
    }

    /// Captures the vectors currently stored for `path` so a rebuild can reuse
    /// them. Call this *before* anything that deletes the note's chunks.
    pub fn chunk_snapshot(&self, path: &str) -> Result<ChunkSnapshot> {
        let mut stmt = self.conn.prepare("SELECT chunk_index, snippet, embedding FROM chunks WHERE path=?1")?;
        let rows = stmt.query_map(params![path], |r| {
            let idx: i32 = r.get(0)?;
            let snippet: String = r.get(1)?;
            let blob: Option<Vec<u8>> = r.get(2)?;
            Ok((idx, (snippet, decode_embedding(blob.as_deref()))))
        })?;
        let mut out = ChunkSnapshot::new();
        for r in rows {
            let (i, v) = r?;
            out.insert(i, v);
        }
        Ok(out)
    }

    /// Rebuilds the chunk rows for one note, reusing vectors wherever possible.
    ///
    /// This is the single owner of the chunk lifecycle. `note_upsert` leaves a
    /// note's existing chunk rows in place so their vectors survive the text
    /// change; this method is what reconciles them against the new text, and
    /// every caller that writes note content goes through it — MCP store, CLI
    /// store, the session hook, `migrate`, `restore_audit` and `reindex_all` — so
    /// none of them can reintroduce a zero-vector path.
    ///
    /// Reconciliation is upsert-then-prune rather than delete-then-insert:
    /// - each chunk is written with `INSERT OR REPLACE` on `(path, chunk_index)`,
    ///   so an unchanged chunk's vector is read back out of the table it is
    ///   replacing instead of being deleted first;
    /// - rows at or past the new chunk count are pruned, so a note that shrank
    ///   does not keep chunks that no longer correspond to any text.
    ///
    /// Vector precedence, highest first:
    /// 1. `fresh` — a vector the caller just computed, **only** when the text it
    ///    was computed from still matches this chunk (see [`FreshVector`]). A
    ///    mismatch means the note was edited while the caller was embedding, and
    ///    the vector is discarded and counted in `diverged`: the note now has
    ///    text nobody has embedded yet, which is a fact worth reporting, not a
    ///    gap worth papering over with a vector for other text.
    /// 2. the row currently stored at the same `chunk_index`, or an entry of
    ///    `snapshot`, when the text is unchanged after normalisation, or close
    ///    enough to pass [`MIN_REUSE_SIMILARITY`];
    /// 3. `None` — SQL `NULL`, awaiting a backfill.
    ///
    /// Returns per-chunk counts so the caller can report how much of the
    /// semantic index survived the rebuild, and why the rest did not.
    pub fn chunks_sync(
        &self,
        note_id: i64,
        path: &str,
        layer: &str,
        scope: Option<&str>,
        content: &str,
        project_id: Option<i64>,
        tags: &[String],
        fresh: &FreshVectors,
        snapshot: &ChunkSnapshot,
    ) -> Result<ChunkSyncStats> {
        // Rows already in the table are the freshest evidence, so they take
        // precedence over a caller-supplied (older) snapshot.
        let mut prior = self.chunk_snapshot(path)?;
        for (idx, entry) in snapshot {
            prior.entry(*idx).or_insert_with(|| entry.clone());
        }
        let chunks = brain_core::chunk_text(content, brain_core::CHUNK_TARGET_TOKENS);
        let mut stats = ChunkSyncStats { total: chunks.len(), ..Default::default() };
        for (i, ch) in chunks.iter().enumerate() {
            let idx = i as i32;
            // What the stored rows can offer for this chunk's *current* text.
            // Decided first, but counted only on the branch actually taken below,
            // so `preserved` and `rehydrated` can never both claim one chunk.
            let reuse: Option<(ChunkMatch, &Vec<f32>)> = prior
                .get(&idx)
                .and_then(|(prev_snippet, prev_emb)| {
                    let v = prev_emb.as_ref()?;
                    // A legacy BLOB-of-zeros is inherited data, not caller intent:
                    // treat it as "no vector" instead of failing, so a reindex can
                    // clean the production index up rather than refusing to run.
                    if v.len() != EMBEDDING_DIM || is_zero_vector(v) {
                        return None;
                    }
                    Some((chunk_text_matches(prev_snippet, ch, self.reuse), v))
                });

            // Counted and reported on the reuse path only.
            let take_reuse = |stats: &mut ChunkSyncStats| -> Option<&[f32]> {
                match reuse {
                    Some((ChunkMatch::Exact, v)) => { stats.preserved += 1; Some(v.as_slice()) }
                    Some((ChunkMatch::Similar(sim), v)) => {
                        stats.preserved += 1;
                        stats.stale_reused += 1;
                        eprintln!(
                            "brain-store: {} chunk {} keeps a stale vector: the text changed but its token \
                             similarity is {sim:.2} (>= {}). Only reachable because {REUSE_SIMILARITY_ENV} \
                             asked for it; unset that to require an exact match. Rerun `brain reindex --all` \
                             to refresh it.",
                            path, idx, self.reuse.min_similarity
                        );
                        Some(v.as_slice())
                    }
                    Some((ChunkMatch::Different, _)) => {
                        stats.unmatched += 1;
                        eprintln!(
                            "brain-store: {} chunk {} lost its vector: the text changed past the reuse \
                             threshold ({}) or its normative force flipped, and no fresh vector applied, so \
                             the chunk is now NULL and FTS-only until a backfill.",
                            path, idx, self.reuse.min_similarity
                        );
                        None
                    }
                    // Nothing stored for this index: a genuinely new chunk, not a
                    // loss, so it is not reported as one.
                    None => None,
                }
            };

            let emb: Option<&[f32]> = match fresh.get(&idx) {
                // Caller-supplied vector: a zero or wrong-width one is a bug in
                // the caller, so it fails loudly rather than degrading quietly.
                Some((src_text, v)) => {
                    if v.len() != EMBEDDING_DIM || is_zero_vector(v) {
                        anyhow::bail!("refusing to write a degenerate embedding for {} chunk {} (len={}, zero={})", path, idx, v.len(), is_zero_vector(v));
                    }
                    match chunk_text_matches(src_text, ch, self.reuse) {
                        ok @ (ChunkMatch::Exact | ChunkMatch::Similar(_)) => {
                            // **Fresh always wins.** It was computed from this
                            // text moments ago; the stored vector is by
                            // definition older. The previous code demanded
                            // `Exact` here while the stored branch accepted
                            // anything above the threshold, so a fresh vector
                            // computed from 96%-identical text was *discarded*
                            // in favour of a stored one from 85%-identical text —
                            // preferring the worse answer. Both branches now ask
                            // the same question of the same threshold.
                            if matches!(ok, ChunkMatch::Similar(_)) {
                                stats.stale_fresh += 1;
                            }
                            stats.rehydrated += 1;
                            Some(v.as_slice())
                        }
                        ChunkMatch::Different => {
                            // The note moved under the embed pass. The vector is
                            // now for text the note does not have; fall back to
                            // whatever is already stored for the current text.
                            stats.diverged += 1;
                            eprintln!(
                                "brain-store: {} chunk {} diverged — the vector was computed from different \
                                 text, so it was NOT applied; the note was edited during the embed pass. \
                                 Falling back to the stored vector for the current text (NULL if there is none).",
                                path, idx
                            );
                            take_reuse(&mut stats)
                        }
                    }
                }
                None => take_reuse(&mut stats),
            };
            self.chunk_insert(note_id, path, layer, scope, ch, idx, chunks.len() as i32, project_id, tags, emb)?;
            if emb.is_some() { stats.embedded += 1; } else { stats.nulls += 1; }
        }
        // Prune only after every new chunk is written, so a failure mid-loop
        // cannot leave the note with fewer chunks than before.
        self.prune_past_the_note(path, chunks.len())?;
        Ok(stats)
    }

    /// Y-04: delete the rows of `path` that the note no longer has a chunk for.
    ///
    /// The bound is **not** `own_len` — the chunk count of the content *this
    /// caller* was handed. That content is a snapshot read before the embed pass,
    /// and a session note grows on every tool result, so a writer holding an
    /// older snapshot can land *after* a writer holding a newer one. Pruning at
    /// its own count then deletes rows a later writer added, for a note that still
    /// has those chunks. Two `tool-result` hooks on the same note are enough:
    /// A captures 3 chunks, B captures 4, B's write-back lands, then A's does and
    /// removes index 3. B has already returned, so nothing rewrites the note and
    /// the semantic index is silently short until the next event or the next
    /// boot's `recover()`.
    ///
    /// So the bound is re-derived from the note as the **database** currently
    /// holds it, and it is `max(own, stored)`:
    ///
    /// - `stored` is the authoritative count of what the note *is*. Any row below
    ///   it belongs to the note and must not be deleted, whoever wrote it.
    /// - `max` with `own` keeps the old behaviour for the case where the caller
    ///   legitimately knows about more chunks than the note row records, so this
    ///   can only ever prune *less* than before, never more. A note that
    ///   genuinely shrank is still pruned, because then `stored` is the small
    ///   number.
    ///
    /// Reading `stored` and deleting are in **one transaction**, because the pair
    /// is a read-modify-write: with the read outside it, a writer committing
    /// between the two would reintroduce exactly the race. `BEGIN IMMEDIATE` when
    /// the connection is in autocommit, and *not* when it is not — `reindex`
    /// calls `chunks_sync` inside its own transaction, and a nested `BEGIN`
    /// fails outright. In that case we are already inside an exclusive
    /// transaction, which is the property `IMMEDIATE` was wanted for.
    fn prune_past_the_note(&self, path: &str, own_len: usize) -> Result<()> {
        const DELETE_TAIL: &str = "DELETE FROM chunks WHERE path=?1 AND chunk_index >= ?2";
        if self.conn.is_autocommit() {
            let tx = rusqlite::Transaction::new_unchecked(&self.conn, rusqlite::TransactionBehavior::Immediate)
                .context("begin the chunk prune transaction")?;
            let bound = self.prune_bound(path, own_len)?;
            tx.execute(DELETE_TAIL, params![path, bound])?;
            tx.commit()?;
        } else {
            let bound = self.prune_bound(path, own_len)?;
            self.conn.execute(DELETE_TAIL, params![path, bound])?;
        }
        Ok(())
    }

    /// The first `chunk_index` that is not a chunk of the note any more.
    ///
    /// `max` of what the caller is syncing and what the note currently holds, so
    /// the prune never removes a row for a chunk the note still has — see
    /// [`Store::prune_past_the_note`] for why the caller's own count is not
    /// enough.
    fn prune_bound(&self, path: &str, own_len: usize) -> Result<i32> {
        let stored: Option<String> = self
            .conn
            .query_row("SELECT content FROM notes WHERE path=?1", params![path], |r| r.get(0))
            .optional()?;
        let stored_len = match stored {
            // A note with no row: nothing else can own chunks under this path, so
            // the caller's own count stands.
            None => 0,
            Some(content) => brain_core::chunk_text(&content, brain_core::CHUNK_TARGET_TOKENS).len(),
        };
        Ok(own_len.max(stored_len) as i32)
    }

    /// `(chunk_index, chunk text)` for every chunk of `path` that has no usable
    /// vector for its **current** text.
    ///
    /// The work list of the background queue, and the reason the queue is cheap
    /// rather than merely asynchronous: it is a diff, not a rebuild. A session
    /// note grows by one section per tool result, so re-embedding the whole
    /// accumulated document on every write would be quadratic work for vectors
    /// that already exist — and it is exactly that pattern the previous inline
    /// embed performed.
    ///
    /// The reuse decision here uses the store's own [`ReusePolicy`] — the same
    /// value `chunks_sync` applies, by construction rather than by convention.
    /// When they disagreed, the queue treated a stale vector as "done" and never
    /// re-embedded it, so an inverted rule (`DEVE` → `NÃO DEVE`) stayed inverted
    /// until someone ran `reindex --all` by hand. Whatever `chunks_sync` refuses
    /// to reuse, this reports as owed.
    pub fn chunks_needing_embedding(&self, path: &str, content: &str) -> Result<Vec<(i32, String)>> {
        let stored = self.chunk_snapshot(path)?;
        let mut out = Vec::new();
        for (i, ch) in brain_core::chunk_text(content, brain_core::CHUNK_TARGET_TOKENS).iter().enumerate() {
            let idx = i as i32;
            let usable = stored.get(&idx).and_then(|(snippet, emb)| {
                let v = emb.as_ref()?;
                if v.len() != EMBEDDING_DIM || is_zero_vector(v) {
                    return None;
                }
                (!matches!(chunk_text_matches(snippet, ch, self.reuse), ChunkMatch::Different)).then_some(())
            });
            if usable.is_none() {
                out.push((idx, ch.clone()));
            }
        }
        Ok(out)
    }

    /// Every note in the database that still owes at least one vector, as
    /// `(path, missing chunk count)`.
    ///
    /// Exists for **boot recovery** (W-04a). The background queue lives in
    /// memory, so a restart with chunks in flight — a deploy, an OOM, a
    /// `kill -9` — loses the work outright: nothing re-reads the queue, and
    /// `coverage_pct` sits below 100% until a human notices or runs
    /// `reindex --all`. The owed work is recoverable from the database alone,
    /// because `NULL` is the record of it.
    ///
    /// Every note is scanned, not just the recent ones, for the same reason
    /// [`Store::all_notes`] exists: a TTL-expired note awaiting its sweep still
    /// holds chunks, and a boot scan that skipped it would never be retried.
    pub fn notes_needing_embedding(&self) -> Result<Vec<(String, usize)>> {
        let mut out = Vec::new();
        for (path, content) in self.all_notes()? {
            let missing = self.chunks_needing_embedding(&path, &content)?.len();
            if missing > 0 {
                out.push((path, missing));
            }
        }
        Ok(out)
    }

    /// Note paths starting with `prefix`, in insertion order.
    ///
    /// Narrower than [`Store::all_notes`], which pulls every note's *content* and is
    /// therefore a full scan. The session hook needs this on every event to pick
    /// the next rotated part name, and it needs it to be cheap: a full-corpus read
    /// on every `tool-result` would be a cost the hook exists to avoid.
    pub fn note_paths_with_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM notes WHERE path LIKE ?1 ESCAPE '\\' ORDER BY id")?;
        let rows = stmt.query_map(params![format!("{}%", like_escaped(prefix))], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Every note's `(path, content)`.
    ///
    /// Separate from [`Store::recent`] because a reindex must cover *every*
    /// note, including TTL-expired ones that have not been swept yet — `recent`
    /// filters those out, and a reindex that skipped them would drop their
    /// vectors and their FTS rows.
    pub fn all_notes(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare("SELECT path, content FROM notes ORDER BY id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Embedding coverage of the `chunks` table, splitting unusable zero vectors
    /// out from real ones. See [`EmbeddingCoverage`].
    pub fn embedding_coverage(&self) -> Result<EmbeddingCoverage> {
        let total: i64 = self.count_chunks()?;
        // `zeroblob(n)` is n zero bytes, and blob equality is memcmp, so this
        // identifies the legacy BLOB-of-zeros rows exactly. Restricted to the
        // expected width so a short blob is not misreported.
        let embedded: i64 = self.conn.query_row(
            &format!("SELECT count(*) FROM chunks WHERE {}", embedded_predicate(false)),
            [], |r| r.get(0))?;
        let nulls: i64 = self.conn.query_row("SELECT count(*) FROM chunks WHERE embedding IS NULL", [], |r| r.get(0))?;
        let zero: i64 = self.conn.query_row(
            &format!("SELECT count(*) FROM chunks WHERE {}", embedded_predicate(true)),
            [], |r| r.get(0))?;
        let pct = if total > 0 { (embedded as f64 / total as f64 * 100.0 * 100.0).round() / 100.0 } else { 0.0 };
        Ok(EmbeddingCoverage { total, embedded, without_embedding: nulls, zero_vector: zero, coverage_pct: pct })
    }

    /// Per-note chunk embedding counts, for surfaces that have to say *which*
    /// notes are semantically searchable — the viewer's browse list is the one
    /// that matters today.
    ///
    /// **One query, not one per note.** A `GROUP BY path` over `chunks` is the
    /// whole aggregation, so the cost is one scan of `idx_chunks_path` plus one
    /// pass over the rows, independent of how many notes the caller lists. The
    /// N+1 shape is deliberately absent: `recent` already returns every note, and
    /// a per-note `count` would make the endpoint's cost `O(notes x chunks)`.
    ///
    /// `embedded` uses [`embedded_predicate`], i.e. literally the same
    /// definition [`Self::embedding_coverage`] reports, so a per-note badge and
    /// the global `embedding_coverage_pct` cannot disagree about the same chunk.
    /// A note with no rows in `chunks` is **absent** from the map rather than
    /// mapped to zero — callers distinguish "no chunks" from "chunks pending".
    pub fn chunk_embedding_counts(&self) -> Result<HashMap<String, ChunkEmbeddingCounts>> {
        let sql = format!(
            "SELECT path, count(*), sum(CASE WHEN {} THEN 1 ELSE 0 END) FROM chunks GROUP BY path",
            embedded_predicate(false)
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            let path: String = r.get(0)?;
            let total: i64 = r.get(1)?;
            let embedded: i64 = r.get(2)?;
            Ok((path, ChunkEmbeddingCounts { total, embedded }))
        })?;
        let mut out = HashMap::new();
        for row in rows {
            let (path, counts) = row?;
            out.insert(path, counts);
        }
        Ok(out)
    }

    /// The `(path, layer, scope)` projection of [`Self::recent`], without the
    /// note bodies.
    ///
    /// `recent` selects `content` for every row, which on the production corpus
    /// is ~516 KiB across 278 notes — and the browse view renders path, layer and
    /// scope, never the body. Selecting the column and discarding it in the
    /// handler is the more expensive way to return nothing; this is the same
    /// rows without the bytes. The TTL filter and the `updated_at DESC` ordering
    /// are copied verbatim from `recent` so the two cannot disagree about which
    /// notes are listed.
    pub fn recent_paths(&self, top_k: usize) -> Result<Vec<(String, String, Option<String>)>> {
        let now = Utc::now().to_rfc3339();
        let mut stmt = self
            .conn
            .prepare("SELECT path, layer, scope FROM notes WHERE (expires_at IS NULL OR expires_at > ?1) ORDER BY updated_at DESC LIMIT ?2")?;
        let rows = stmt.query_map(params![now, top_k as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn count_notes(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT count(*) FROM notes", [], |r| r.get(0))?)
    }
    pub fn count_chunks(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT count(*) FROM chunks", [], |r| r.get(0))?)
    }

    /// Run one FTS5 `MATCH` and return `(title, rank)` pairs, unsorted.
    ///
    /// When `project_id` is `Some`, the `project` filter is applied **inside**
    /// the FTS query (join against `notes` + `note_projects`) before the
    /// `LIMIT 50`, so a project-relevant note past the global top-50 still
    /// enters the RRF. Ownership (`notes.project_id`) and links
    /// (`note_projects`) are the same two sources the post-filter accepts, so
    /// the candidate set and the filter cannot disagree. No DDL: the join
    /// reads the existing tables.
    fn fts_candidates(&self, match_expr: &str, project_id: Option<i64>) -> Result<Vec<(String, f64)>> {
        if let Some(pid) = project_id {
            let mut stmt = self.conn.prepare(
                "SELECT f.title, f.rank FROM notes_fts f JOIN notes n ON n.path = f.title WHERE notes_fts MATCH ?1 AND (n.project_id = ?2 OR n.path IN (SELECT note_path FROM note_projects WHERE project_id = ?3)) ORDER BY rank LIMIT 50")?;
            let rows = stmt.query_map(params![match_expr, pid, pid], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
            })?;
            let ranked = rows.collect::<Result<Vec<_>, _>>()?;
            return Ok(ranked);
        }
        let mut stmt = self
            .conn
            .prepare("SELECT title, rank FROM notes_fts WHERE notes_fts MATCH ?1 ORDER BY rank LIMIT 50")?;
        let rows = stmt.query_map(params![match_expr], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })?;
        let ranked = rows.collect::<Result<Vec<_>, _>>()?;
        Ok(ranked)
    }

    // search: FTS + vector cosine (Rust-side) + entity + graph RRF — batch optimized (TD-001 C1/C2/C3/A1)
    pub fn search(&self, query: &str, query_vec: Option<&[f32]>, layer: Option<&str>, scope: Option<&str>, project: Option<&str>, tag: Option<&str>, top_k: usize, explain: bool) -> Result<Vec<SearchResult>> {
        use std::collections::HashSet;
        let now = Utc::now().to_rfc3339();
        // Resolve the project filter once, up front, so the FTS candidate
        // query, the vector scan and the post-filter all agree on what
        // `project=X` means: owned (`notes.project_id`) OR linked
        // (`note_projects`). A name with no row keeps the old contract:
        // every stream empties and the search returns no rows, without error.
        let project_id_filter: Option<i64> = if let Some(pname) = project {
            self.conn.query_row("SELECT id FROM projects WHERE name=?1", params![pname], |r| r.get(0)).optional()?.flatten()
        } else { None };
        let missing_project = project.is_some() && project_id_filter.is_none();
        // TD-F2: early-return instead of threading `!missing_project` through
        // the FTS (below) and vector (vec-scan) gates. Equivalent: with an
        // unknown project every stream empties anyway — FTS/vec scans are
        // skipped, and the post-filter drops all entity/graph candidates
        // (`project_id_filter_opt` is `None` → every path `continue`s).
        if missing_project {
            return Ok(Vec::new());
        }
        // 1. FTS candidates (project filter applied inside the query, before LIMIT)
        let mut fts_scores: HashMap<String, f32> = HashMap::new();
        let mut fts_rank: HashMap<String, usize> = HashMap::new();
        if !query.trim().is_empty() {
            // A1: escape FTS5 specials + SQL ops to prevent syntax error / injection
            let sanitized = fts5_match_expr(query);
            if !sanitized.is_empty() {
                match self.fts_candidates(&sanitized, project_id_filter) {
                    Ok(mut ranked) => {
                        ranked.sort_by(|a,b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
                        for (i, (p,_)) in ranked.iter().enumerate() {
                            let score = 1.0/(60.0 + (i as f32 +1.0));
                            fts_scores.insert(p.clone(), score);
                            fts_rank.insert(p.clone(), i+1);
                        }
                    }
                    Err(e) => {
                        eprintln!("warn fts query syntax error for '{}': {}", sanitized, e);
                    }
                }
            }
        }
        // 2. vector cosine candidates (scan chunks) — pre-filter by layer/scope/project.
        // The project pre-filter is path-based (notes ownership UNION note_projects
        // links), never `chunks.project_id = ?`: a linked note's chunks carry the
        // owner's id (or NULL), so an owned-only predicate discarded every linked
        // vector before the truncate(50) and the post-filter never saw them.
        let mut vec_scores: HashMap<String, f32> = HashMap::new();
        let mut vec_rank: HashMap<String, usize> = HashMap::new();
        if let Some(qv) = query_vec {
            // warn if large scan without filter
            if layer.is_none() && scope.is_none() && project.is_none() {
                if let Ok(cnt) = self.conn.query_row("SELECT count(*) FROM chunks WHERE embedding IS NOT NULL", [], |r| r.get::<_,i64>(0)) {
                    if cnt > 10000 {
                        eprintln!("WARN large scan {} chunks without filter, consider layer/scope/project", cnt);
                    }
                }
            }
            // TD-F2: the old `if !missing_project` gate stood here. It is
            // unreachable-gated by the early return above, so the scan below
            // runs unconditionally once `qv` exists — no nesting left.
                let mut sql = "SELECT path, snippet, layer, scope, tags, embedding, chunk_index, project_id FROM chunks WHERE embedding IS NOT NULL".to_string();
                // TD-F1: pid binds as typed i64 (Value::Integer), not via
                // `pid.to_string()`. String binding relied on SQLite's loose
                // affinity to compare INTEGER = TEXT; the typed bind compares
                // INTEGER = INTEGER on both UNION branches.
                let mut filter_params: Vec<rusqlite::types::Value> = Vec::new();
                let mut filter_sql_parts: Vec<String> = Vec::new();
                if let Some(lf) = layer { filter_sql_parts.push("layer = ?".to_string()); filter_params.push(rusqlite::types::Value::Text(lf.to_string())); }
                if let Some(sf) = scope { filter_sql_parts.push("scope = ?".to_string()); filter_params.push(rusqlite::types::Value::Text(sf.to_string())); }
                if let Some(pid) = project_id_filter { filter_sql_parts.push("(path IN (SELECT path FROM notes WHERE project_id = ? UNION SELECT note_path FROM note_projects WHERE project_id = ?))".to_string()); filter_params.push(rusqlite::types::Value::Integer(pid)); filter_params.push(rusqlite::types::Value::Integer(pid)); }
                if !filter_sql_parts.is_empty() {
                    sql.push_str(" AND ");
                    sql.push_str(&filter_sql_parts.join(" AND "));
                }
                // use params_from_iter for dynamic params
                let mut stmt = self.conn.prepare(&sql)?;
                let param_refs: Vec<&dyn rusqlite::ToSql> = filter_params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
                // if no filter params, query without params
                let mut rows = if param_refs.is_empty() {
                    stmt.query([])?
                } else {
                    stmt.query(rusqlite::params_from_iter(param_refs))?
                };
                let mut scored: Vec<(String, f32, String, String, Option<String>, String, i32)> = Vec::new();
                while let Some(r) = rows.next()? {
                    let path: String = r.get(0)?;
                    let snippet: String = r.get(1)?;
                    let _layer: String = r.get(2)?;
                    let _scope: Option<String> = r.get(3)?;
                    let tags_s: String = r.get(4)?;
                    let blob: Option<Vec<u8>> = r.get(5)?;
                    let cidx: i32 = r.get(6)?;
                    let Some(emb) = decode_embedding(blob.as_deref()) else { continue };
                    if emb.len()!=qv.len() { continue; }
                    let cos = cosine(qv, &emb);
                    // keep all for ranking but store path->cos mapping (deduplicate later by max cos per path)
                    scored.push((path, cos, snippet, _layer, _scope, tags_s, cidx));
                }
                // deduplicate by path keeping max cos per path before truncate
                let mut best_per_path: HashMap<String, (f32, String, String, Option<String>, String, i32)> = HashMap::new();
                for (p, cos, snippet, l, s, tags_s, cidx) in scored {
                    match best_per_path.get(&p) {
                        Some((best_cos, _,_,_,_,_)) if *best_cos >= cos => {},
                        _ => { best_per_path.insert(p, (cos, snippet, l, s, tags_s, cidx)); }
                    }
                }
                let mut scored_dedup: Vec<(String, f32)> = best_per_path.into_iter().map(|(p,(cos,_,_,_,_,_))| (p, cos)).collect();
                scored_dedup.sort_by(|a,b| b.1.partial_cmp(&a.1).unwrap());
                scored_dedup.truncate(50);
                for (i, (p, _cos)) in scored_dedup.iter().enumerate() {
                    let score = 1.0/(60.0 + (i as f32 +1.0));
                    vec_scores.insert(p.clone(), score);
                    vec_rank.insert(p.clone(), i+1);
                }
            }
        // 3. entity stream: tag filter => boost
        let mut ent_scores: HashMap<String, f32> = HashMap::new();
        let mut ent_rank: HashMap<String, usize> = HashMap::new();
        if let Some(t) = tag {
            let mut stmt = self.conn.prepare("SELECT n.path FROM notes n JOIN entity_links el ON el.note_id=n.id JOIN entities e ON e.id=el.entity_id WHERE e.normalized=?1")?;
            let rows = stmt.query_map(params![t.to_lowercase()], |r| r.get::<_,String>(0))?;
            for (i, p) in rows.enumerate() {
                let p = p?;
                ent_scores.insert(p.clone(), 1.0/(60.0 + (i as f32+1.0)));
                ent_rank.insert(p, i+1);
            }
        }
        // 4. graph: links neighbors of FTS hits
        let mut graph_scores: HashMap<String, f32> = HashMap::new();
        let mut graph_rank: HashMap<String, usize> = HashMap::new();
        let fts_paths: Vec<String> = fts_scores.keys().cloned().collect();
        for fp in &fts_paths {
            let mut stmt = self.conn.prepare("SELECT to_path FROM links WHERE from_path=?1 LIMIT 10")?;
            let rows = stmt.query_map(params![fp], |r| r.get::<_,String>(0))?;
            for (idx, r) in rows.enumerate() { let tp = r?; if !graph_scores.contains_key(&tp) { graph_scores.insert(tp.clone(), 1.0/61.0); graph_rank.insert(tp, idx+1); } }
        }

        // fuse — batch optimized: single IN query for filter (C1) 300→1 query for 100 paths
        let all_paths: Vec<String> = std::collections::HashSet::<String>::from_iter(
            fts_scores.keys().cloned().chain(vec_scores.keys().cloned()).chain(ent_scores.keys().cloned()).chain(graph_scores.keys().cloned())
        ).into_iter().collect();
        if all_paths.is_empty() {
            return Ok(Vec::new());
        }

        // C1 batch filter: single SELECT IN (...) → HashMap path→(layer,scope,expires_at,tags,project_id,pinned)
        // Previously: loop per path → 1-4 queries per path (300q/100 paths). Now: ≤3 queries total.
        let placeholders = vec!["?"; all_paths.len()].join(",");
        let sql = format!("SELECT path, layer, scope, expires_at, tags, project_id, pinned FROM notes WHERE path IN ({})", placeholders);
        let mut stmt = self.conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = all_paths.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
        let mut rows = stmt.query(rusqlite::params_from_iter(param_refs))?;
        let mut meta_map: HashMap<String, (String, Option<String>, Option<String>, String, Option<i64>, i32)> = HashMap::new();
        while let Some(r) = rows.next()? {
            let path: String = r.get(0)?;
            let layer_v: String = r.get(1)?;
            let scope_v: Option<String> = r.get(2)?;
            let expires_v: Option<String> = r.get(3)?;
            let tags_s: String = r.get(4)?;
            let pid: Option<i64> = r.get(5)?;
            let pinned: i32 = r.get(6)?;
            meta_map.insert(path, (layer_v, scope_v, expires_v, tags_s, pid, pinned));
        }
        // project linked batch: if project filter present, fetch linked set in one query.
        // Reuses the `project_id_filter` resolved once at the top of `search`,
        // so the candidate queries and this post-filter agree by construction.
        let mut linked_set: HashSet<String> = HashSet::new();
        let project_id_filter_opt: Option<i64> = project_id_filter;
        if project.is_some() && project_id_filter_opt.is_some() && !all_paths.is_empty() {
                let placeholders3 = vec!["?"; all_paths.len()].join(",");
                let sql3 = format!("SELECT note_path, project_id FROM note_projects WHERE note_path IN ({})", placeholders3);
                let mut stmt3 = self.conn.prepare(&sql3)?;
                let param_refs3: Vec<&dyn rusqlite::ToSql> = all_paths.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
                let mut rows3 = stmt3.query(rusqlite::params_from_iter(param_refs3))?;
                while let Some(r) = rows3.next()? {
                    let np: String = r.get(0)?;
                    let pid2: i64 = r.get(1)?;
                    if Some(pid2) == project_id_filter_opt {
                        linked_set.insert(np);
                    }
                }
        }
        let mut filtered_paths = Vec::new();
        for p in &all_paths {
            let Some((layer_v, scope_v, expires_v, tags_s, pid_owned, _pinned)) = meta_map.get(p) else { continue; };
            if let Some(lf) = layer { if lf != layer_v { continue; } }
            if let Some(sf) = scope { if Some(sf.to_string()) != *scope_v { continue; } }
            if let Some(exp) = expires_v { if exp.as_str() < now.as_str() { continue; } }
            if let Some(tf) = tag { let tags: Vec<String> = serde_json::from_str(tags_s).unwrap_or_default(); if !tags.iter().any(|t| t.to_lowercase()==tf.to_lowercase()) { continue; } }
            if let Some(_pf) = project {
                let Some(pid_filter) = project_id_filter_opt else { continue; };
                let owned_match = pid_owned.map(|id| id == pid_filter).unwrap_or(false);
                let linked_match = linked_set.contains(p);
                if !owned_match && !linked_match { continue; }
            }
            filtered_paths.push(p.clone());
        }
        let all_paths = filtered_paths;
        if all_paths.is_empty() {
            return Ok(Vec::new());
        }

        // ranking without N+1 — use meta_map for layer/pinned (C1 reuse)
        let mut results: Vec<(String, f32, f32, f32, f32, f32, f32)> = all_paths.into_iter().map(|p| {
            let rrf_vec = vec_scores.get(&p).copied().unwrap_or(0.0);
            let rrf_fts = fts_scores.get(&p).copied().unwrap_or(0.0);
            let rrf_ent = ent_scores.get(&p).copied().unwrap_or(0.0);
            let rrf_graph = graph_scores.get(&p).copied().unwrap_or(0.0);
            let base = rrf_vec + rrf_fts + rrf_ent + rrf_graph;
            let (layer_v, _scope_v, _expires_v, _tags_s, _pid, pinned) = meta_map.get(&p).map(|(l,s,e,t,pid,pinned)| (l.clone(), s.clone(), e.clone(), t.clone(), *pid, *pinned)).unwrap_or(("".to_string(), None, None, "[]".to_string(), None, 0));
            let mut boost = 0.0;
            if ["arquitetura","regras"].contains(&layer_v.as_str()) { boost += 0.15; }
            if pinned!=0 { boost += 0.1; }
            (p, base+boost, rrf_vec, rrf_fts, rrf_ent, rrf_graph, boost)
        }).collect();
        results.sort_by(|a,b| b.1.partial_cmp(&a.1).unwrap());
        results.truncate(top_k);
        if results.is_empty() {
            return Ok(Vec::new());
        }

        // C2 batch assembly: chunks IN (...) + notes fallback IN (...) + projects IN (...) → ≤3 queries for 60 paths (was 60q)
        let result_paths: Vec<String> = results.iter().map(|(p,_,_,_,_,_,_)| p.clone()).collect();
        // batch chunks
        let placeholders_c = vec!["?"; result_paths.len()].join(",");
        let sql_c = format!("SELECT path, layer, scope, snippet, tags, chunk_index, project_id FROM chunks WHERE path IN ({}) ORDER BY chunk_index", placeholders_c);
        let mut stmt_c = self.conn.prepare(&sql_c)?;
        let param_refs_c: Vec<&dyn rusqlite::ToSql> = result_paths.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
        let mut rows_c = stmt_c.query(rusqlite::params_from_iter(param_refs_c))?;
        let mut chunk_map: HashMap<String, (String, Option<String>, String, String, i32, Option<i64>)> = HashMap::new();
        while let Some(r) = rows_c.next()? {
            let path: String = r.get(0)?;
            if chunk_map.contains_key(&path) { continue; } // keep first (lowest chunk_index)
            let layer: String = r.get(1)?;
            let scope: Option<String> = r.get(2)?;
            let snippet: String = r.get(3)?;
            let tags_s: String = r.get(4)?;
            let cidx: i32 = r.get(5)?;
            let pid: Option<i64> = r.get(6)?;
            chunk_map.insert(path, (layer, scope, snippet, tags_s, cidx, pid));
        }
        // fallback for paths without chunk
        let mut fallback_map: HashMap<String, (String, Option<String>, String, String, Option<i64>)> = HashMap::new();
        let missing_paths: Vec<String> = result_paths.iter().filter(|p| !chunk_map.contains_key(*p)).cloned().collect();
        if !missing_paths.is_empty() {
            let placeholders_n = vec!["?"; missing_paths.len()].join(",");
            let sql_n = format!("SELECT path, layer, scope, content, tags, project_id FROM notes WHERE path IN ({})", placeholders_n);
            let mut stmt_n = self.conn.prepare(&sql_n)?;
            let param_refs_n: Vec<&dyn rusqlite::ToSql> = missing_paths.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
            let mut rows_n = stmt_n.query(rusqlite::params_from_iter(param_refs_n))?;
            while let Some(r) = rows_n.next()? {
                let path: String = r.get(0)?;
                let layer: String = r.get(1)?;
                let scope: Option<String> = r.get(2)?;
                let content: String = r.get(3)?;
                let tags_s: String = r.get(4)?;
                let pid: Option<i64> = r.get(5)?;
                fallback_map.insert(path, (layer, scope, content, tags_s, pid));
            }
        }
        // batch project names
        let all_pids: HashSet<i64> = chunk_map.values().filter_map(|(_,_,_,_,_,pid)| *pid).chain(fallback_map.values().filter_map(|(_,_,_,_,pid)| *pid)).collect();
        let mut proj_map: HashMap<i64, String> = HashMap::new();
        if !all_pids.is_empty() {
            let pids_vec: Vec<i64> = all_pids.into_iter().collect();
            let placeholders_p = vec!["?"; pids_vec.len()].join(",");
            let sql_p = format!("SELECT id, name FROM projects WHERE id IN ({})", placeholders_p);
            let mut stmt_p = self.conn.prepare(&sql_p)?;
            let param_refs_p: Vec<&dyn rusqlite::ToSql> = pids_vec.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
            let mut rows_p = stmt_p.query(rusqlite::params_from_iter(param_refs_p))?;
            while let Some(r) = rows_p.next()? {
                let id: i64 = r.get(0)?;
                let name: String = r.get(1)?;
                proj_map.insert(id, name);
            }
        }

        let mut out = Vec::new();
        for (path, score, rrf_vec, rrf_fts, rrf_ent, rrf_graph, authority) in results {
            let (layer, scope, snippet, tags_s, cidx, pid) = if let Some((l,s,snip,tags,ci,pid)) = chunk_map.get(&path) {
                (l.clone(), s.clone(), snip.clone(), tags.clone(), *ci, *pid)
            } else if let Some((l,s,content,tags,pid)) = fallback_map.get(&path) {
                (l.clone(), s.clone(), content.chars().take(200).collect::<String>(), tags.clone(), 0, *pid)
            } else {
                // should not happen, fallback to meta_map
                let (l,s,_,tags,pid,_) = meta_map.get(&path).cloned().unwrap_or(("".to_string(), None, None, "[]".to_string(), None, 0));
                (l, s, "".to_string(), tags, 0, pid)
            };
            let tags: Vec<String> = serde_json::from_str(&tags_s).unwrap_or_default();
            let project: Option<String> = pid.and_then(|id| proj_map.get(&id).cloned());
            let explain_obj = if explain {
                Some(SearchExplain{
                    rrf_vec, rrf_fts, rrf_entity: rrf_ent, rrf_graph, authority, score,
                    rank_vec: vec_rank.get(&path).copied(),
                    rank_fts: fts_rank.get(&path).copied(),
                    rank_entity: ent_rank.get(&path).copied(),
                    rank_graph: graph_rank.get(&path).copied(),
                })
            } else { None };
            out.push(SearchResult{ path, layer, scope, score, snippet, chunk_index: cidx, project, tags, explain: explain_obj });
        }
        Ok(out)
    }

    pub fn recent(&self, top_k: usize) -> Result<Vec<(String,String,Option<String>,String)>> {
        let now = Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare("SELECT path, layer, scope, content FROM notes WHERE (expires_at IS NULL OR expires_at > ?1) ORDER BY updated_at DESC LIMIT ?2")?;
        let rows = stmt.query_map(params![now, top_k as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        Ok(rows.collect::<Result<Vec<_>,_>>()?)
    }

    pub fn checkpoints(&self, limit: usize) -> Result<Vec<(i64,String,String,String)>> {
        let mut stmt = self.conn.prepare("SELECT id, action, path, at FROM audit_log ORDER BY at DESC LIMIT ?1")?;
        Ok(stmt.query_map(params![limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<Vec<_>,_>>()?)
    }

    pub fn audit_get(&self, id: i64) -> Result<Option<(String,String,Option<String>,String)>> {
        Ok(self.conn.query_row("SELECT action, path, prev_content, at FROM audit_log WHERE id=?1", params![id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?)
    }

    pub fn restore_audit(&self, audit_id: i64) -> Result<bool> {
        let Some((action, path, prev, _at)) = self.audit_get(audit_id)? else { return Ok(false); };
        match action.as_str() {
            "create" => { self.note_delete(&path)?; Ok(true) },
            "update" | "delete" => {
                if let Some(prev_content) = prev {
                    let parts: Vec<&str> = path.split('/').collect();
                    let (layer, scope, _rel) = if parts.len()>=3 && brain_core::VALID_SCOPES.contains(&parts[1]) {
                        (parts[0].to_string(), Some(parts[1].to_string()), parts[2..].join("/"))
                    } else if parts.len()>=2 { (parts[0].to_string(), None, parts[1..].join("/")) } else { (parts[0].to_string(), None, "".to_string()) };
                    self.note_upsert(&path, &layer, scope.as_deref(), &prev_content, None, &[], false, None)?;
                    let nid: i64 = self.conn.query_row("SELECT id FROM notes WHERE path=?1", params![path], |r| r.get(0))?;
                    // `chunks_sync` reads the note's still-stored chunk rows itself
                    // (`note_upsert` no longer deletes them), so restoring the
                    // original text recovers the original vectors. Restoring
                    // *different* text leaves them NULL rather than attaching a
                    // vector computed from something else. Before this, every
                    // restore zeroed the note's whole semantic index.
                    let stats = self.chunks_sync(nid, &path, &layer, scope.as_deref(), &prev_content, None, &[], &std::collections::HashMap::new(), &ChunkSnapshot::new())?;
                    if stats.nulls > 0 {
                        eprintln!("brain-store: restore {} left {}/{} chunks without a vector (text changed, or never embedded); rerun `brain reindex --all` with Ollama reachable to backfill", path, stats.nulls, stats.total);
                    }
                    Ok(true)
                } else { Ok(false) }
            },
            _ => Ok(false),
        }
    }

    /// Rebuilds every note's chunks, preserving existing vectors.
    ///
    /// Kept for callers that have no vectors to contribute (it passes an empty
    /// `embeds`); see [`Store::reindex_all_with`].
    pub fn reindex_all(&self) -> Result<usize> {
        let empty: std::collections::HashMap<String, NoteEmbed> = std::collections::HashMap::new();
        self.reindex_all_with(&empty).map(|(notes, _)| notes)
    }

    /// Rebuilds every note's chunks inside a single transaction, reusing
    /// vectors from two sources, highest precedence first:
    /// 1. `embeds` — vectors the caller computed before opening the write
    ///    transaction, each paired with the chunk text it was computed from;
    /// 2. the vector already stored at that `(path, chunk_index)`, when the
    ///    text is unchanged or similar enough;
    /// 3. SQL `NULL`.
    ///
    /// Non-destructive by construction. The previous implementation ran
    /// `DELETE FROM chunks` and re-inserted every row with a zero vector, which
    /// meant `brain reindex --all` silently destroyed the entire vector index —
    /// the three surviving vectors in the production DB survived only because
    /// nobody had run it. The delete and the re-insert share one transaction so
    /// an error mid-way cannot leave the index empty.
    ///
    /// `embeds` is keyed by note path and each [`NoteEmbed`] pairs its vectors
    /// with the chunk texts they were computed from. That pairing is what makes
    /// this safe against a concurrent write: embedding the whole corpus takes
    /// minutes against a serial Ollama, and a `brain_store` landing in that
    /// window would otherwise receive the *old* text's vectors written onto the
    /// *new* text's chunks. `chunks_sync` checks every fresh vector against the
    /// chunk text and drops the mismatched ones into
    /// [`ChunkSyncStats::diverged`], so the report says "N diverged" instead of
    /// quietly attaching a vector that describes something else. A caller must
    /// not pad a chunk list with placeholder vectors to fill a gap: a vector
    /// without text cannot be attributed, and `NoteEmbed::fresh_vectors` drops
    /// it rather than guessing.
    ///
    /// Returns the note count (unchanged from the historical return value) and
    /// the [`ChunkSyncStats`] breakdown of what was rehydrated, preserved, left
    /// `NULL`, diverged or lost.
    pub fn reindex_all_with(&self, embeds: &std::collections::HashMap<String, NoteEmbed>) -> Result<(usize, ChunkSyncStats)> {
        let mut stmt = self.conn.prepare("SELECT id, path, layer, scope, content, project_id, tags FROM notes")?;
        let rows = stmt.query_map([], |r| {
            let tags_s: String = r.get(6)?;
            Ok((r.get::<_,i64>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?, r.get::<_,Option<String>>(3)?, r.get::<_,String>(4)?, r.get::<_,Option<i64>>(5)?, tags_s))
        })?.collect::<Result<Vec<_>, _>>()?;

        // No `DELETE FROM chunks` here, and that is the entire point: the old
        // implementation cleared the table and re-inserted every row with a zero
        // vector, so `brain reindex --all` destroyed the whole vector index — the
        // three surviving vectors in the production DB survived only because
        // nobody had run it. `chunks_sync` instead upserts each chunk (reading its
        // current vector out of the row it replaces) and prunes only what no
        // longer corresponds to text. One transaction, so an error mid-way cannot
        // leave the index half-rebuilt.
        let tx = self.conn.unchecked_transaction()?;
        let mut total = ChunkSyncStats::default();
        for (nid, path, layer, scope, content, pid, tags_s) in rows {
            let tags: Vec<String> = serde_json::from_str(&tags_s).unwrap_or_default();
            let fresh = embeds.get(&path).map(NoteEmbed::fresh_vectors).unwrap_or_default();
            let stats = self.chunks_sync(nid, &path, &layer, scope.as_deref(), &content, pid, &tags, &fresh, &ChunkSnapshot::new())?;
            total.total += stats.total;
            total.embedded += stats.embedded;
            total.rehydrated += stats.rehydrated;
            total.stale_fresh += stats.stale_fresh;
            total.preserved += stats.preserved;
            total.stale_reused += stats.stale_reused;
            total.nulls += stats.nulls;
            total.diverged += stats.diverged;
            total.unmatched += stats.unmatched;
        }
        tx.commit()?;
        let notes = self.count_notes()? as usize;
        Ok((notes, total))
    }

    // ---------------------------------------------------------------- locks --

    /// Cross-process advisory lock serialising embedding work, held in `_meta`.
    ///
    /// Why it lives in the database rather than in a `Mutex`: the two producers
    /// that would otherwise embed the same chunks twice are a long-lived server
    /// (the MCP background queue) and a one-shot `brain reindex` process. An
    /// in-process mutex cannot see the other one, and `brain reindex` exists
    /// precisely to be run against a live server.
    ///
    /// Semantics: the first caller wins, everyone else is told to skip. A lock
    /// older than `ttl_secs` is treated as abandoned and can be taken over, so a
    /// process killed mid-backfill cannot wedge embedding forever. Re-entrant for
    /// the same `owner`. A lost race costs duplicated CPU and nothing else — every
    /// write is idempotent and re-verified against chunk text — so this is
    /// best-effort mutual exclusion, not a correctness mechanism.
    ///
    /// The check and the write happen inside one `BEGIN IMMEDIATE`. In autocommit
    /// they were two independent statements, so two processes could both read
    /// "no lock" and both insert: the `SELECT` is not serialised against the
    /// `INSERT OR REPLACE`, and a writer arriving between them is invisible to
    /// the loser. `IMMEDIATE` takes the write lock at `BEGIN`, which makes the
    /// pair atomic against every other writer in every process. The damage was
    /// bounded (duplicated CPU, never a wrong write) but it was one line of
    /// `BEGIN IMMEDIATE` away from being free.
    pub fn try_acquire_embed_lock(&self, owner: &str, ttl_secs: i64) -> Result<bool> {
        // `IMMEDIATE` fails with SQLITE_BUSY if another writer holds the DB, which
        // is the answer we want: someone else is mid-transaction, so we did not
        // get the lock. The busy timeout installed by `init_schema` absorbs the
        // short overlaps a normal write produces.
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let now: i64 = match self
            .conn
            .query_row("SELECT CAST(strftime('%s','now') AS INTEGER)", [], |r| r.get(0))
        {
            Ok(v) => v,
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                return Err(e.into());
            }
        };
        let cur: Option<String> = match self
            .conn
            .query_row("SELECT value FROM _meta WHERE key='embed_lock'", [], |r| r.get(0))
            .optional()
        {
            Ok(v) => v,
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                return Err(e.into());
            }
        };
        let mut acquired = true;
        if let Some(v) = &cur {
            let (held_by, taken_at, expires) = parse_lock_value(v);
            let expires: i64 = expires.parse().unwrap_or(0);
            if held_by != owner && expires > now {
                acquired = false;
            }
            // Read-only check: still release the transaction below.
            let _ = taken_at;
        }
        if acquired {
            let stamp = format!("{}|{}|{}", owner, now, now + ttl_secs);
            if let Err(e) = self
                .conn
                .execute("INSERT OR REPLACE INTO _meta(key, value) VALUES('embed_lock', ?1)", params![stamp])
            {
                let _ = self.conn.execute_batch("ROLLBACK");
                return Err(e.into());
            }
        }
        self.conn.execute_batch("COMMIT")?;
        Ok(acquired)
    }

    /// Releases a lock held by `owner`. Returns whether this call released it.
    pub fn release_embed_lock(&self, owner: &str) -> Result<bool> {
        let n = self.conn.execute(
            "DELETE FROM _meta WHERE key='embed_lock' AND value LIKE ?1",
            params![format!("{}|%", owner.replace(['%', '_'], ""))],
        )?;
        Ok(n > 0)
    }

    /// Whether an embedding lock is currently held, and by whom. Diagnostic only.
    pub fn embed_lock_holder(&self) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM _meta WHERE key='embed_lock'", [], |r| r.get::<_, String>(0))
            .optional()?
            .map(|v| parse_lock_value(&v).0.to_string()))
    }

    /// Seconds since the held embed lock was taken, or `None` when free.
    ///
    /// This is the number that answers "is the queue wedged, or is another embed
    /// legitimately still working?" — the holder name alone cannot. A lock that
    /// has been held for 2 s is a reindex in progress; one held for 14 minutes is
    /// either a very large corpus or a process that died without releasing, and
    /// the two want different responses. Requires the `taken_at` stamp written by
    /// [`Store::try_acquire_embed_lock`]; a legacy two-field value reports `None`
    /// rather than a wrong age.
    pub fn embed_lock_age_secs(&self) -> Result<Option<i64>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT value FROM _meta WHERE key='embed_lock'", [], |r| r.get(0))
            .optional()?;
        let Some(raw) = raw else { return Ok(None) };
        let (_, taken_at, _) = parse_lock_value(&raw);
        // `"0"` is the sentinel for a legacy value that carried only an expiry.
        let Ok(taken_at) = taken_at.parse::<i64>() else { return Ok(None) };
        if taken_at <= 0 {
            return Ok(None);
        }
        let now: i64 = self.conn.query_row("SELECT CAST(strftime('%s','now') AS INTEGER)", [], |r| r.get(0))?;
        Ok(Some((now - taken_at).max(0)))
    }

    /// Seconds until the held embed lock expires, or `None` when free.
    ///
    /// The countdown half of [`Store::embed_lock_age_secs`]: a value at or below
    /// zero means the lock is abandoned and the next acquirer may take it over.
    pub fn embed_lock_expires_in_secs(&self) -> Result<Option<i64>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT value FROM _meta WHERE key='embed_lock'", [], |r| r.get(0))
            .optional()?;
        let Some(raw) = raw else { return Ok(None) };
        let (_, _, expires) = parse_lock_value(&raw);
        let Ok(expires) = expires.parse::<i64>() else { return Ok(None) };
        let now: i64 = self.conn.query_row("SELECT CAST(strftime('%s','now') AS INTEGER)", [], |r| r.get(0))?;
        Ok(Some(expires - now))
    }

    pub fn forget_sweep(&self, dry_run: bool) -> Result<Vec<String>> {
        let now = Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare("SELECT path, expires_at, pinned FROM notes WHERE expires_at IS NOT NULL AND expires_at <= ?1")?;
        let rows = stmt.query_map(params![now], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,i32>(2)?)))?;
        let mut expired: Vec<String> = Vec::new();
        for r in rows { let (p,_,pinned) = r?; if pinned!=0 { eprintln!("warn pinned+expiring {} (TTL wins)", p); } expired.push(p); }
        if !dry_run {
            for p in &expired { let _ = self.note_delete(p); }
        }
        Ok(expired)
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x,y)| x*y).sum();
    let na: f32 = a.iter().map(|x| x*x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x*x).sum::<f32>().sqrt();
    if na==0.0||nb==0.0 {0.0} else {dot/(na*nb)}
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::chunk_text;
    use std::path::PathBuf;

    fn test_store() -> Store {
        Store::open_in_memory().expect("open_in_memory")
    }

    /// A scratch database path that removes itself, so a test that writes to it
    /// cannot leave anything behind for another one to trip over.
    struct ScratchDb(PathBuf);

    impl ScratchDb {
        fn new(tag: &str) -> ScratchDb {
            let p = std::env::temp_dir()
                .join(format!("brain-ro-{}-{tag}.db", std::process::id()));
            let _ = std::fs::remove_file(&p);
            ScratchDb(p)
        }

        fn as_str(&self) -> &str {
            self.0.to_str().expect("utf-8 scratch path")
        }
    }

    impl Drop for ScratchDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            // WAL sidecars, if the connection left any.
            let _ = std::fs::remove_file(self.0.with_extension("db-wal"));
            let _ = std::fs::remove_file(self.0.with_extension("db-shm"));
        }
    }

    /// `open_read_only` reads a real database, and reports its counts.
    #[test]
    fn open_read_only_reads_an_existing_store() {
        let p = ScratchDb::new("reads");
        {
            let s = Store::open(p.as_str()).expect("open for write");
            s.note_upsert("regras/global/alpha", "regras", Some("global"), "## alpha", None, &[], false, None)
                .unwrap();
        }
        let s = Store::open_read_only(p.as_str()).expect("open read-only");
        assert_eq!(s.count_notes().unwrap(), 1);
        assert_eq!(s.path(), p.as_str());
    }

    /// The property that makes it a *guard* API: it creates nothing.
    ///
    /// `Store::open` runs `init_schema`, so a guard that used it would write
    /// tables, indexes, triggers and the `_meta` version row into the very file
    /// it was asked to look at. Asserted by opening a database that is not a
    /// brain database and checking it is still not one afterwards.
    #[test]
    fn open_read_only_does_not_run_ddl_on_a_foreign_database() {
        let p = ScratchDb::new("noddl");
        {
            let c = Connection::open(p.as_str()).unwrap();
            c.execute_batch("CREATE TABLE something_else (x INTEGER);").unwrap();
        }
        // It opens: a read-only handle on a valid SQLite file is not an error.
        Store::open_read_only(p.as_str()).expect("a foreign database is still readable");
        let c = Connection::open(p.as_str()).unwrap();
        let tables: Vec<String> = c
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            tables,
            vec!["something_else".to_string()],
            "open_read_only created schema in a database it was only asked to inspect"
        );
    }

    /// A read-only handle cannot write, so a bug in a caller cannot escalate.
    #[test]
    fn open_read_only_refuses_to_write() {
        let p = ScratchDb::new("nowrite");
        Store::open(p.as_str()).expect("open for write");
        let s = Store::open_read_only(p.as_str()).expect("open read-only");
        let err = s
            .note_upsert("regras/global/nope", "regras", Some("global"), "## x", None, &[], false, None)
            .expect_err("a read-only handle must refuse a write");
        assert!(
            err.to_string().to_lowercase().contains("readonly")
                || err.to_string().to_lowercase().contains("read-only"),
            "expected a read-only error, got: {err}"
        );
    }

    /// It **fails** on input it cannot read, rather than degrading to an empty
    /// store. A guard that treats "could not read" as "nothing to protect" is the
    /// failure mode this API exists to make impossible — so the failure has to be
    /// an `Err`, never an `Ok` over zero rows.
    #[test]
    fn open_read_only_fails_rather_than_returning_an_empty_store() {
        let missing = ScratchDb::new("missing");
        assert!(
            Store::open_read_only(missing.as_str()).is_err(),
            "a nonexistent path must be an error, not an empty store"
        );

        let garbage = ScratchDb::new("garbage");
        std::fs::write(&garbage.0, b"this is not a sqlite database, not even close").unwrap();
        assert!(
            Store::open_read_only(garbage.as_str()).is_err(),
            "a corrupt file must be an error, not an empty store"
        );
    }

    #[test]
    fn test_note_crud() {
        let s = test_store();
        let path = "regras/global/test-crud";
        let nid = s.note_upsert(path, "regras", Some("global"), "## hello world", None, &[], false, None).unwrap();
        assert!(nid > 0);
        let note = s.note_get(path).unwrap().unwrap();
        assert_eq!(note.layer, "regras");
        assert_eq!(note.scope.as_deref(), Some("global"));
        // update
        s.note_upsert(path, "regras", Some("global"), "## updated content", None, &[], false, None).unwrap();
        let note2 = s.note_get(path).unwrap().unwrap();
        assert_eq!(note2.content, "## updated content");
        assert_eq!(note2.version, 2);
        // delete
        assert!(s.note_delete(path).unwrap());
        assert!(s.note_get(path).unwrap().is_none());
    }

    #[test]
    fn test_search_fts_only() {
        let s = test_store();
        s.note_upsert("regras/global/alpha", "regras", Some("global"), "## alpha unique token xyz123", None, &[], false, None).unwrap();
        s.note_upsert("regras/global/beta", "regras", Some("global"), "## beta other content", None, &[], false, None).unwrap();
        // insert chunks for FTS recall (chunks not needed for FTS, but search uses notes_fts)
        let res = s.search("xyz123", None, None, None, None, None, 5, false).unwrap();
        assert!(res.iter().any(|r| r.path.contains("alpha")), "fts should find alpha, got {:?}", res.iter().map(|r| &r.path).collect::<Vec<_>>());
    }

    #[test]
    fn test_fts5_match_expr_joined_or_differs_from_and() {
        let q = "kebab-case nos caminhos";
        let and_default = fts5_match_expr(q);
        let and_joined = fts5_match_expr_joined(q, "AND");
        let or_joined = fts5_match_expr_joined(q, "OR");
        // Default AND preserved: joined AND == plain expr (HEAD byte-identical).
        assert_eq!(and_joined, and_default);
        // OR differs: terms joined with OR vs implicit AND (spaces).
        assert_ne!(or_joined, and_default);
        assert!(or_joined.contains(" OR "));
        assert!(!and_default.contains(" OR "));
        // Same sanitisation: FTS5 specials stripped in both.
        let tricky = "a:b (c)";
        assert!(!fts5_match_expr(tricky).contains([':', '(', ')']));
        assert!(!fts5_match_expr_joined(tricky, "OR").contains([':', '(', ')']));
        // Hook-kill set: each of ?.,+!^~&| stripped in BOTH fns (glued form,
        // the way a real hook question carries them). `/` was HEAD already.
        for c in ['?', '.', ',', '+', '!', '^', '~', '&', '|'] {
            let glued = format!("emissao{c}");
            assert!(!fts5_match_expr(&glued).contains(c), "plain keeps {c:?}");
            assert!(!fts5_match_expr_joined(&glued, "OR").contains(c), "joined keeps {c:?}");
        }
        // A user-typed operator must not double: "a OR b" joins to one OR, not three.
        assert_eq!(fts5_match_expr_joined("a OR b", "OR"), "a OR b");
        assert_eq!(fts5_match_expr_joined("a or AND not b", "OR"), "a OR b");
    }

    #[test]
    fn test_fts5_hook_question_mark_searchable_with_valid_rank() {
        // Real hook failure: "...emissão?" kept `?`, FTS5 errored
        // (`syntax error near "?"`), the FTS stream emptied. Now the expr
        // carries no `?` and the MATCH runs clean with a valid rank.
        let q = "como faco emissao?";
        let expr = fts5_match_expr(q);
        assert!(!expr.contains('?'), "expr keeps '?': {expr:?}");
        assert_eq!(fts5_match_expr_joined(q, "AND"), expr);
        let s = test_store();
        s.note_upsert(
            "regras/global/emissao",
            "regras",
            Some("global"),
            "## como faco emissao de nota fiscal",
            None,
            &[],
            false,
            None,
        )
        .unwrap();
        // Raw `?` form errors at the SQLite layer — proves the strip is load-bearing.
        assert!(s.fts_candidates(q, None).is_err(), "expected FTS5 to reject '?', it did not");
        // Sanitised form: no error, non-empty, finite rank.
        let cands = s.fts_candidates(&expr, None).unwrap();
        assert!(!cands.is_empty(), "FTS found nothing for {expr:?}");
        assert!(cands.iter().any(|(t, _)| t.contains("emissao")), "got {:?}", cands);
        for (t, rank) in &cands {
            assert!(rank.is_finite(), "non-finite rank for {t}: {rank}");
        }
        // End-to-end FTS-only search (no vector) recalls the note.
        let res = s.search(q, None, None, None, None, None, 5, false).unwrap();
        assert!(res.iter().any(|r| r.path.contains("emissao")), "got {:?}", res.iter().map(|r| &r.path).collect::<Vec<_>>());
        // OR contract intact: user "a OR b" stays exactly that.
        assert_eq!(fts5_match_expr_joined("a OR b", "OR"), "a OR b");
    }

    #[test]
    fn test_search_hybrid_recall() {
        let s = test_store();
        let nid = s.note_upsert("regras/global/hybrid", "regras", Some("global"), "## hybrid vector test", None, &[], false, None).unwrap();
        let chunks = chunk_text("## hybrid vector test", 4096);
        s.chunk_insert(nid, "regras/global/hybrid", "regras", Some("global"), &chunks[0], 0, 1, None, &[], Some(&vec![1.0; 768])).unwrap();
        let res = s.search("hybrid", Some(&vec![1.0; 768]), None, None, None, None, 5, false).unwrap();
        assert!(res.iter().any(|r| r.path == "regras/global/hybrid"));
    }

    #[test]
    fn test_forget_sweep_ttl() {
        let s = test_store();
        // expired note
        s.note_upsert("regras/global/expired", "regras", Some("global"), "old", None, &[], false, Some("2000-01-01T00:00:00Z")).unwrap();
        s.note_upsert("regras/global/valid", "regras", Some("global"), "new", None, &[], false, Some("2099-01-01T00:00:00Z")).unwrap();
        let expired = s.forget_sweep(true).unwrap();
        assert!(expired.contains(&"regras/global/expired".to_string()));
        assert!(!expired.contains(&"regras/global/valid".to_string()));
        // actual sweep deletes
        let deleted = s.forget_sweep(false).unwrap();
        assert!(deleted.contains(&"regras/global/expired".to_string()));
        assert!(s.note_get("regras/global/expired").unwrap().is_none());
        assert!(s.note_get("regras/global/valid").unwrap().is_some());
    }

    #[test]
    fn test_audit_restore() {
        let s = test_store();
        let path = "regras/global/audit";
        s.note_upsert(path, "regras", Some("global"), "v1", None, &[], false, None).unwrap();
        s.note_upsert(path, "regras", Some("global"), "v2", None, &[], false, None).unwrap();
        let cps = s.checkpoints(10).unwrap();
        assert!(cps.len() >= 2);
        // restore first create (id 1) should delete? we test update restore
        let update_id = cps.iter().find(|(_, act, p, _)| act=="update" && p==path).map(|(id,_,_,_)| *id).unwrap();
        assert!(s.restore_audit(update_id).unwrap());
        let note = s.note_get(path).unwrap().unwrap();
        assert_eq!(note.content, "v1");
    }

    #[test]
    fn test_project_crud_link_unlink() {
        let s = test_store();
        let proj = s.project_create("proj-a", "desc").unwrap();
        assert_eq!(proj.name, "proj-a");
        s.note_upsert("regras/global/linked", "regras", Some("global"), "hello", None, &[], false, None).unwrap();
        s.note_link_project("regras/global/linked", "proj-a").unwrap();
        let notes = s.project_notes("proj-a").unwrap();
        assert!(notes.iter().any(|n| n.path=="regras/global/linked"));
        assert!(s.note_unlink_project("regras/global/linked", "proj-a").unwrap());
        let notes2 = s.project_notes("proj-a").unwrap();
        assert!(!notes2.iter().any(|n| n.path=="regras/global/linked"));
        assert!(s.project_delete("proj-a").unwrap());
    }

    #[test]
    fn test_batch_filter_correctness() {
        let s = test_store();
        for i in 0..10 {
            let layer = if i % 2 == 0 { "regras" } else { "arquitetura" };
            let scope = if i % 3 == 0 { "global" } else { "projetos" };
            let path = format!("{}/{}/batch-{}", layer, scope, i);
            s.note_upsert(&path, layer, Some(scope), &format!("## content {} token_batch_{}", i, i), None, &[], false, None).unwrap();
        }
        // search with layer filter should only return that layer
        let res = s.search("token_batch", None, Some("regras"), None, None, None, 10, false).unwrap();
        for r in &res {
            assert_eq!(r.layer, "regras", "batch filter layer failed: {:?}", r.path);
        }
        let res2 = s.search("token_batch", None, None, Some("global"), None, None, 10, false).unwrap();
        for r in &res2 {
            assert_eq!(r.scope.as_deref(), Some("global"));
        }
    }

    #[test]
    fn test_search_with_project_filter() {
        let s = test_store();
        let proj = s.project_create("myproj", "").unwrap();
        s.note_upsert("regras/global/proj-note", "regras", Some("global"), "## project filtered content", Some(proj.id), &[], false, None).unwrap();
        s.note_upsert("regras/global/other-note", "regras", Some("global"), "## project filtered content", None, &[], false, None).unwrap();
        let res = s.search("project filtered", None, None, None, Some("myproj"), None, 10, false).unwrap();
        assert!(res.iter().any(|r| r.path=="regras/global/proj-note"));
        assert!(!res.iter().any(|r| r.path=="regras/global/other-note"));
        // linked project
        s.note_upsert("regras/global/linked-proj", "regras", Some("global"), "## project filtered content linked", None, &[], false, None).unwrap();
        s.note_link_project("regras/global/linked-proj", "myproj").unwrap();
        let res2 = s.search("project filtered", None, None, None, Some("myproj"), None, 10, false).unwrap();
        assert!(res2.iter().any(|r| r.path=="regras/global/linked-proj"));
    }

    /// T1 — the vector stream sees linked notes, not just owned ones.
    ///
    /// The query text matches nothing (`nomatch-zzz-vector-only` is in no
    /// note), so the FTS stream is empty and any hit must come from the
    /// vector stream. The linked note's chunks carry the *owner's*
    /// `project_id`, so the old `chunks.project_id = ?` pre-filter discarded
    /// them before `truncate(50)`. Mutation: restoring `project_id = ?`
    /// returns only the owned note and this test fails on the linked assert.
    #[test]
    fn project_filter_vector_stream_includes_linked_notes() {
        let s = test_store();
        let proj_a = s.project_create("projvec-a", "").unwrap();
        let proj_b = s.project_create("projvec-b", "").unwrap();
        let qv = fixture_vec(5);
        let nid_o = s.note_upsert("regras/global/vec-owned", "regras", Some("global"), "## alpha owned token", Some(proj_a.id), &[], false, None).unwrap();
        s.chunk_insert(nid_o, "regras/global/vec-owned", "regras", Some("global"), "alpha owned token", 0, 1, Some(proj_a.id), &[], Some(&qv)).unwrap();
        let nid_l = s.note_upsert("regras/global/vec-linked", "regras", Some("global"), "## beta linked token", Some(proj_b.id), &[], false, None).unwrap();
        s.chunk_insert(nid_l, "regras/global/vec-linked", "regras", Some("global"), "beta linked token", 0, 1, Some(proj_b.id), &[], Some(&qv)).unwrap();
        s.note_link_project("regras/global/vec-linked", "projvec-a").unwrap();
        let res = s.search("nomatch-zzz-vector-only", Some(&qv), None, None, Some("projvec-a"), None, 10, true).unwrap();
        let paths: Vec<&str> = res.iter().map(|r| r.path.as_str()).collect();
        assert!(paths.contains(&"regras/global/vec-owned"), "owned note missing from vector stream: {paths:?}");
        assert!(paths.contains(&"regras/global/vec-linked"), "linked note missing from vector stream: {paths:?}");
        for want in ["regras/global/vec-owned", "regras/global/vec-linked"] {
            let hit = res.iter().find(|r| r.path == want).unwrap();
            let vec_score = hit.explain.as_ref().map(|e| e.rrf_vec).unwrap_or(0.0);
            assert!(vec_score > 0.0, "{want} arrived without the vector stream (rrf_vec={vec_score})");
        }
    }

    /// T2 — a `sessoes` note with `project_id NULL` is invisible under a
    /// `project` filter until it is linked via `note_projects`. Ownership is
    /// never assigned silently; the test links explicitly, which is the only
    /// supported way for a session note to join a project.
    #[test]
    fn project_filter_session_note_needs_a_link_to_appear() {
        let s = test_store();
        s.project_create("proj-sess", "").unwrap();
        s.note_upsert("sessoes/proj-sess/2026-01-01", "sessoes", None, "## sessao token unico s2 alvo", None, &[], false, None).unwrap();
        let hidden = s.search("sessao unico alvo", None, None, None, Some("proj-sess"), None, 10, false).unwrap();
        assert!(!hidden.iter().any(|r| r.path == "sessoes/proj-sess/2026-01-01"), "unlinked session note leaked into project filter: {:?}", hidden.iter().map(|r| &r.path).collect::<Vec<_>>());
        s.note_link_project("sessoes/proj-sess/2026-01-01", "proj-sess").unwrap();
        let shown = s.search("sessao unico alvo", None, None, None, Some("proj-sess"), None, 10, false).unwrap();
        assert!(shown.iter().any(|r| r.path == "sessoes/proj-sess/2026-01-01"), "linked session note missing from project filter");
    }

    /// T3 — a `project` name with no row is an empty result, not an error,
    /// on both the FTS and the vector streams.
    #[test]
    fn project_filter_unknown_project_returns_empty_without_error() {
        let s = test_store();
        let proj = s.project_create("proj-real", "").unwrap();
        let nid = s.note_upsert("regras/global/real-note", "regras", Some("global"), "## real content token", Some(proj.id), &[], false, None).unwrap();
        let qv = fixture_vec(1);
        s.chunk_insert(nid, "regras/global/real-note", "regras", Some("global"), "real content token", 0, 1, Some(proj.id), &[], Some(&qv)).unwrap();
        let res = s.search("real content", Some(&qv), None, None, Some("no-such-proj"), None, 10, false).unwrap();
        assert!(res.is_empty(), "unknown project must match nothing, got {:?}", res.iter().map(|r| &r.path).collect::<Vec<_>>());
    }

    /// T4 — isolation: a note owned by another project with no link never
    /// appears, even when both its text and its vector match the query.
    /// `mobile-t4` vs `progaterp-t4` stand in for the production pair
    /// `mobile` (id 2) vs `progaterp` (id 6), which share vocabulary but no
    /// ownership.
    #[test]
    fn project_filter_never_leaks_an_unlinked_owner() {
        let s = test_store();
        s.project_create("mobile-t4", "").unwrap();
        let proga = s.project_create("progaterp-t4", "").unwrap();
        let qv = fixture_vec(9);
        let nid = s.note_upsert("regras/global/secret-t4", "regras", Some("global"), "## mobile devolucao offline token t4", Some(proga.id), &[], false, None).unwrap();
        s.chunk_insert(nid, "regras/global/secret-t4", "regras", Some("global"), "mobile devolucao offline token t4", 0, 1, Some(proga.id), &[], Some(&qv)).unwrap();
        let res = s.search("mobile devolucao offline token t4", Some(&qv), None, None, Some("mobile-t4"), None, 10, true).unwrap();
        assert!(!res.iter().any(|r| r.path == "regras/global/secret-t4"), "unlinked owner leaked across projects: {:?}", res.iter().map(|r| &r.path).collect::<Vec<_>>());
        let sanity = s.search("mobile devolucao offline token t4", Some(&qv), None, None, Some("progaterp-t4"), None, 10, false).unwrap();
        assert!(sanity.iter().any(|r| r.path == "regras/global/secret-t4"), "owner project lost its own note");
    }

    /// T5 — the `project` filter is applied inside the FTS query, before
    /// `LIMIT 50`. Sixty distractors share the query token but belong to no
    /// project; the target is inserted last (highest rowid), so a global
    /// top-50 followed by a post-filter would cut it and return nothing.
    #[test]
    fn project_filter_fts_finds_a_target_past_the_global_top_50() {
        let s = test_store();
        let proj = s.project_create("proj-fts50", "").unwrap();
        for i in 0..60 {
            s.note_upsert(&format!("regras/global/distra-{i:02}"), "regras", Some("global"), "## tokencomum distrator", None, &[], false, None).unwrap();
        }
        s.note_upsert("regras/global/alvo-fts50", "regras", Some("global"), "## tokencomum alvo", Some(proj.id), &[], false, None).unwrap();
        let res = s.search("tokencomum", None, None, None, Some("proj-fts50"), None, 10, true).unwrap();
        assert!(res.iter().any(|r| r.path == "regras/global/alvo-fts50"), "project target past the global top-50 was cut");
        assert!(!res.iter().any(|r| r.path.starts_with("regras/global/distra-")), "distractor without project leaked into filtered FTS");
        let hit = res.iter().find(|r| r.path == "regras/global/alvo-fts50").unwrap();
        assert!(hit.explain.as_ref().map(|e| e.rrf_fts).unwrap_or(0.0) > 0.0, "target arrived without the FTS stream");
    }

    #[test]
    fn test_fts_injection_escaped() {
        let s = test_store();
        s.note_upsert("regras/global/inject", "regras", Some("global"), "## safe content", None, &[], false, None).unwrap();
        // query with FTS specials should not panic or inject
        let res = s.search("\" * : ( )", None, None, None, None, None, 5, false).unwrap();
        // should not error, may return empty
        assert!(res.len() <= 5);
        let res2 = s.search("safe \" OR 1=1 --", None, None, None, None, None, 5, false).unwrap();
        assert!(res2.len() <= 5);
    }

    #[test]
    fn test_search_batch_vs_single_consistency() {
        // ensure batch filter returns same as before for 5 notes with mixed filters
        let s = test_store();
        for i in 0..5 {
            s.note_upsert(&format!("regras/global/consist-{}", i), "regras", Some("global"), &format!("## consist token {}", i), None, &["tag-a".to_string()], false, None).unwrap();
        }
        let res = s.search("consist", None, Some("regras"), Some("global"), None, Some("tag-a"), 10, false).unwrap();
        assert_eq!(res.len(), 5);
    }

    #[test]
    fn test_shared_note_links_both_projects() {
        // SH-02: hook sessoes/shared/* auto-link erp+mobile (single-source, idempotente)
        let s = test_store();
        let full = "sessoes/shared/sh02-e2e";
        let content = "## shared session hooktesttoken";
        let nid = s.note_upsert(full, "sessoes", None, content, None, &[], false, None).unwrap();
        assert!(nid > 0);
        // simula hook_handle shared: ensure + link (idempotente)
        for pname in ["erp", "mobile"] {
            if s.project_get(pname).unwrap().is_none() {
                let _ = s.project_create(pname, "");
            }
            s.note_link_project(full, pname).unwrap();
        }
        // idempotência: relink não duplica nem falha
        s.note_link_project(full, "erp").unwrap();
        s.note_link_project(full, "mobile").unwrap();
        // ambos project_notes veem a nota
        let erp_notes = s.project_notes("erp").unwrap();
        assert!(erp_notes.iter().any(|n| n.path == full), "erp deve ver shared");
        let mob_notes = s.project_notes("mobile").unwrap();
        assert!(mob_notes.iter().any(|n| n.path == full), "mobile deve ver shared");
        // search --project ambos retornam
        let r_erp = s.search("hooktesttoken", None, None, None, Some("erp"), None, 10, false).unwrap();
        assert!(r_erp.iter().any(|r| r.path == full), "search erp deve achar shared");
        let r_mob = s.search("hooktesttoken", None, None, None, Some("mobile"), None, 10, false).unwrap();
        assert!(r_mob.iter().any(|r| r.path == full), "search mobile deve achar shared");
    }

    // ------------------------------------------------------------------
    // P0-ZV: NULL instead of BLOB-of-zeros, and non-destructive rebuilds.
    //
    // Context: 735 of 738 chunks in the production index held a BLOB of zeros.
    // `cosine` scores a zero-norm vector 0.0 against everything, so the vector
    // half of the RRF was pure noise while `chunks`/`notes` counts stayed
    // healthy. These tests pin the two properties that make that unrepeatable:
    // a zero vector can never be stored, and a rebuild never destroys one.
    // ------------------------------------------------------------------

    /// A deterministic, non-zero, correctly sized vector.
    fn fixture_vec(seed: u32) -> Vec<f32> {
        (0..EMBEDDING_DIM).map(|i| ((i as u32 + seed) % 17 + 1) as f32 / 16.0).collect()
    }

    /// Stores `content` as one note with `embedding` on its single chunk.
    fn store_with_chunk(s: &Store, path: &str, content: &str, embedding: Option<&[f32]>) -> i64 {
        let nid = s.note_upsert(path, "regras", Some("global"), content, None, &[], false, None).unwrap();
        let chunks = chunk_text(content, 4096);
        assert_eq!(chunks.len(), 1, "fixture content must be a single chunk");
        s.chunk_insert(nid, path, "regras", Some("global"), &chunks[0], 0, 1, None, &[], embedding).unwrap();
        nid
    }

    fn embedded_count(s: &Store) -> i64 {
        s.embedding_coverage().unwrap().embedded
    }

    fn null_blob_len(s: &Store) -> Option<i64> {
        s.conn
            .query_row("SELECT length(embedding) FROM chunks WHERE embedding IS NOT NULL LIMIT 1", [], |r| r.get(0))
            .optional()
            .unwrap()
    }

    /// Number of chunks holding any BLOB at all. `length(NULL)` is NULL in
    /// SQLite, not 0, so counting rows is the only way to ask this.
    fn blob_rows(s: &Store) -> i64 {
        s.conn.query_row("SELECT count(*) FROM chunks WHERE embedding IS NOT NULL", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn test_chunk_insert_none_stores_null_and_search_ignores_the_chunk() {
        let s = test_store();
        store_with_chunk(&s, "regras/global/nonull", "## unique nevertoken", None);
        // NULL, not a 3072-byte BLOB of zeros.
        assert_eq!(blob_rows(&s), 0, "no chunk should hold any BLOB at all");
        assert_eq!(null_blob_len(&s), None, "a NULL chunk has no BLOB length to read");
        let cov = s.embedding_coverage().unwrap();
        assert_eq!((cov.total, cov.embedded, cov.without_embedding, cov.zero_vector), (1, 0, 1, 0));
        assert_eq!(cov.coverage_pct, 0.0);
        // The note is still FTS-searchable, and the vector stream contributes
        // nothing rather than contributing noise.
        let res = s.search("nevertoken", Some(&fixture_vec(1)), None, None, None, None, 5, true).unwrap();
        let hit = res.iter().find(|r| r.path == "regras/global/nonull").expect("FTS must still find the note");
        assert_eq!(hit.explain.as_ref().unwrap().rrf_vec, 0.0, "a NULL chunk must not enter the vector stream");
        assert!(hit.explain.as_ref().unwrap().rrf_fts > 0.0, "FTS stream must still score");
    }

    #[test]
    fn test_chunk_insert_some_stores_a_3072_byte_blob() {
        let s = test_store();
        let v = fixture_vec(3);
        store_with_chunk(&s, "regras/global/some", "## blobwidth", Some(&v));
        let len: i64 = s.conn.query_row("SELECT length(embedding) FROM chunks", [], |r| r.get(0)).unwrap();
        assert_eq!(len, (EMBEDDING_DIM * 4) as i64, "768 f32 little-endian = 3072 bytes");
        let cov = s.embedding_coverage().unwrap();
        assert_eq!((cov.embedded, cov.without_embedding, cov.zero_vector, cov.coverage_pct), (1, 0, 0, 100.0));
    }

    #[test]
    fn test_chunk_insert_rejects_a_zero_vector() {
        // The guard that would have turned 735 zero BLOBs into 735 loud errors.
        let s = test_store();
        let err = s.chunk_insert(1, "regras/global/zero", "regras", Some("global"), "## z", 0, 1, None, &[], Some(&vec![0.0; EMBEDDING_DIM]))
            .expect_err("a zero-norm vector must be refused");
        assert!(err.to_string().contains("zero-norm"), "got: {err}");
    }

    #[test]
    fn test_chunk_insert_rejects_a_wrong_dimension_vector() {
        // `search` silently discards mismatched widths, so a bad length would
        // look like a missing vector with no trace of the cause.
        let s = test_store();
        let err = s.chunk_insert(1, "regras/global/dim", "regras", Some("global"), "## d", 0, 1, None, &[], Some(&vec![1.0; 384]))
            .expect_err("wrong dimension must be refused");
        assert!(err.to_string().contains("dim mismatch"), "got: {err}");
    }

    #[test]
    fn test_reindex_all_preserves_the_embedding_of_an_unchanged_chunk() {
        let s = test_store();
        let content = "## stable content for reindex";
        store_with_chunk(&s, "regras/global/keep", content, Some(&fixture_vec(2)));
        assert_eq!(embedded_count(&s), 1);
        s.reindex_all().unwrap();
        assert_eq!(embedded_count(&s), 1, "reindex must not drop a vector for unchanged text");
        assert_eq!(s.embedding_coverage().unwrap().zero_vector, 0);
    }

    #[test]
    fn test_reindex_all_nulls_a_changed_chunk_instead_of_zeroing_it() {
        let s = test_store();
        store_with_chunk(&s, "regras/global/mutate", "## version one", Some(&fixture_vec(4)));
        s.note_upsert("regras/global/mutate", "regras", Some("global"), "## version two entirely different", None, &[], false, None).unwrap();
        s.reindex_all().unwrap();
        let cov = s.embedding_coverage().unwrap();
        assert_eq!(cov.zero_vector, 0, "a changed chunk must never become a zero BLOB");
        assert_eq!(cov.embedded, 0, "text changed, so the old vector no longer applies");
        assert_eq!(cov.without_embedding, 1, "the changed chunk is NULL and awaits a backfill");
    }

    #[test]
    fn test_reindex_all_is_not_destructive_over_many_notes() {
        // Direct regression on the reported bug: `brain reindex --all` used to
        // DELETE FROM chunks and re-insert every row with a zero vector, so a
        // fully-embedded index came back 100% dead.
        let s = test_store();
        for i in 0..5 {
            let path = format!("regras/global/many-{}", i);
            store_with_chunk(&s, &path, &format!("## note number {} withtoken", i), Some(&fixture_vec(i as u32)));
        }
        assert_eq!(embedded_count(&s), 5);
        s.reindex_all().unwrap();
        let cov = s.embedding_coverage().unwrap();
        assert_eq!(cov.embedded, 5, "every vector must survive a reindex");
        assert_eq!(cov.zero_vector, 0, "no zero vectors may be introduced");
        assert_eq!(cov.coverage_pct, 100.0);
    }

    #[test]
    fn test_reindex_all_with_hydrates_chunks_from_supplied_vectors() {
        let s = test_store();
        let content = "## hydrate me from above";
        store_with_chunk(&s, "regras/global/hydrate", content, None);
        assert_eq!(embedded_count(&s), 0);
        let mut embeds = std::collections::HashMap::new();
        embeds.insert("regras/global/hydrate".to_string(), NoteEmbed { chunks: vec![content.to_string()], vectors: vec![fixture_vec(6)] });
        let (notes, stats) = s.reindex_all_with(&embeds).unwrap();
        assert_eq!(notes, 1);
        assert_eq!((stats.total, stats.embedded, stats.nulls), (1, 1, 0));
        assert_eq!(stats.rehydrated, 1, "the supplied vector must be counted as rehydrated, not preserved");
        assert_eq!(stats.preserved, 0);
        assert_eq!(embedded_count(&s), 1);
    }

    #[test]
    fn test_restore_audit_recovers_the_embedding_of_identical_content() {
        let s = test_store();
        let path = "regras/global/restore-same";
        let original = "## content that will be restored unchanged";
        store_with_chunk(&s, path, original, Some(&fixture_vec(5)));
        assert_eq!(embedded_count(&s), 1);
        // Re-write the *same* content. `note_upsert` deliberately leaves the chunk
        // rows alone so their vectors survive an unrelated edit.
        s.note_upsert(path, "regras", Some("global"), original, None, &[], false, None).unwrap();
        assert_eq!(embedded_count(&s), 1, "an edit must not discard a still-valid vector");
        let update_id = s.checkpoints(20).unwrap().into_iter()
            .find(|(_, a, p, _)| a == "update" && p == path).map(|(id, _, _, _)| id).unwrap();
        assert!(s.restore_audit(update_id).unwrap());
        assert_eq!(embedded_count(&s), 1, "restoring identical content must recover the vector");
        assert_eq!(s.embedding_coverage().unwrap().zero_vector, 0);
        assert_eq!(s.note_get(path).unwrap().unwrap().content, original);
    }

    #[test]
    fn test_restore_audit_nulls_the_embedding_of_changed_content() {
        // A deleted note takes its chunks with it (FK CASCADE), so restoring it
        // has no prior vector to reuse. The correct outcome is NULL — the old
        // code wrote a zero vector here, which is how a restored note ended up
        // scoring 0.0 against every query while looking indexed.
        let s = test_store();
        let path = "regras/global/restore-diff";
        store_with_chunk(&s, path, "## the original text", Some(&fixture_vec(7)));
        assert_eq!(embedded_count(&s), 1);
        assert!(s.note_delete(path).unwrap());
        assert_eq!(embedded_count(&s), 0, "deleting a note drops its chunks");
        let del_id = s.checkpoints(20).unwrap().into_iter()
            .find(|(_, a, p, _)| a == "delete" && p == path).map(|(id, _, _, _)| id).unwrap();
        assert!(s.restore_audit(del_id).unwrap());
        assert_eq!(s.note_get(path).unwrap().unwrap().content, "## the original text");
        let cov = s.embedding_coverage().unwrap();
        assert_eq!(cov.zero_vector, 0, "restore must not zero a vector");
        assert_eq!((cov.embedded, cov.without_embedding), (0, 1), "no reusable vector existed, so the chunk is NULL");
    }

    #[test]
    fn test_chunks_sync_nulls_a_vector_whose_text_changed() {
        // The reuse precondition in isolation: a stored vector is only carried
        // over to byte-identical text. A vector computed from other text is worse
        // than no vector, so it is dropped rather than misattributed.
        let s = test_store();
        let path = "regras/global/sync-change";
        let v = fixture_vec(9);
        let nid = s.note_upsert(path, "regras", Some("global"), "## text A", None, &[], false, None).unwrap();
        let fresh = FreshVectors::from([(0i32, ("## text A".to_string(), v))]);
        let st = s.chunks_sync(nid, path, "regras", Some("global"), "## text A", None, &[], &fresh, &ChunkSnapshot::new()).unwrap();
        assert_eq!((st.embedded, st.rehydrated, st.nulls), (1, 1, 0));
        // Same note, different text, no fresh vector available.
        let st = s.chunks_sync(nid, path, "regras", Some("global"), "## text B, entirely different", None, &[], &FreshVectors::new(), &ChunkSnapshot::new()).unwrap();
        assert_eq!((st.embedded, st.nulls), (0, 1), "changed text must not inherit the old vector");
        assert_eq!(st.unmatched, 1, "losing a vector to a substantive edit must be counted, not silent");
        assert_eq!(s.embedding_coverage().unwrap().zero_vector, 0);
    }

    #[test]
    fn test_embedding_coverage_splits_usable_null_and_zero_vectors() {
        let s = test_store();
        store_with_chunk(&s, "regras/global/cov-ok", "## good", Some(&fixture_vec(1)));
        store_with_chunk(&s, "regras/global/cov-null", "## absent", None);
        // A legacy BLOB-of-zeros, written the way the old code wrote them. The
        // guard in `chunk_insert` refuses to create one, so the only way to
        // reproduce the production state is to inject it directly — which is
        // exactly what makes this assertion meaningful.
        s.conn.execute("INSERT INTO chunks(note_id, path, layer, scope, snippet, chunk_index, total_chunks, tags, embedding) SELECT id, 'regras/global/cov-zero', 'regras', 'global', '## legacy', 0, 1, '[]', zeroblob(3072) FROM notes WHERE path='regras/global/cov-null'", []).unwrap();
        let cov = s.embedding_coverage().unwrap();
        assert_eq!(cov.total, 3);
        assert_eq!(cov.embedded, 1, "only the real vector counts as embedded");
        assert_eq!(cov.without_embedding, 1, "the NULL chunk");
        assert_eq!(cov.zero_vector, 1, "the legacy zero BLOB must be reported as its own failure mode");
        // 1 of 3 = 33.33%, not 66.67% — a NOT NULL count would have hidden the
        // dead vector entirely.
        assert_eq!(cov.coverage_pct, 33.33);
    }

    #[test]
    fn test_embedding_coverage_of_an_empty_index_is_zero_not_a_division_error() {
        let s = test_store();
        let cov = s.embedding_coverage().unwrap();
        assert_eq!((cov.total, cov.embedded, cov.coverage_pct), (0, 0, 0.0));
    }

    /// The per-note aggregate must be the *same measurement* as the global one,
    /// decomposed. They are two queries written against two `format!`s, and the
    /// whole reason `embedded_predicate` is shared is that a drifted copy would
    /// let a note's badge contradict `embedding_coverage_pct` about the very same
    /// chunk — the failure mode where "indexed" and "missing" are both true.
    ///
    /// The BLOB-of-zeros rows are the interesting half: they are `NOT NULL` and
    /// they score `0.0`, so a plain `IS NOT NULL` count would put them in
    /// `embedded` and this equality would fail.
    #[test]
    fn test_chunk_embedding_counts_sum_to_the_global_coverage() {
        let s = test_store();
        let v = fixture_vec(11);
        store_with_chunk(&s, "regras/global/embedded", "## embedded", Some(&v));
        store_with_chunk(&s, "regras/global/pending", "## pending", None);
        // A legacy all-zero BLOB: stored, `NOT NULL`, useless. Planted with raw
        // SQL because `chunk_insert` refuses a zero-norm vector — which is the
        // point: these rows exist only from before that guard, and a test that
        // could not reproduce them could not prove the predicate still spots them.
        let nid = s.note_upsert("regras/global/zero", "regras", Some("global"), "## zero", None, &[], false, None).unwrap();
        s.chunk_insert(nid, "regras/global/zero", "regras", Some("global"), "## zero", 0, 1, None, &[], None).unwrap();
        s.conn.execute("UPDATE chunks SET embedding=zeroblob(3072) WHERE path='regras/global/zero'", []).unwrap();
        // A note with no chunk rows at all: absent from the map, not zeroed.
        s.note_upsert("regras/global/bare", "regras", Some("global"), "## bare", None, &[], false, None).unwrap();

        let cov = s.embedding_coverage().unwrap();
        let per_note = s.chunk_embedding_counts().unwrap();

        let sum_total: i64 = per_note.values().map(|c| c.total).sum();
        let sum_embedded: i64 = per_note.values().map(|c| c.embedded).sum();
        assert_eq!(sum_total, cov.total, "per-note totals must reconstruct the global total");
        assert_eq!(sum_embedded, cov.embedded, "per-note embedded must reconstruct the global embedded");

        assert_eq!(per_note["regras/global/embedded"], ChunkEmbeddingCounts { total: 1, embedded: 1 });
        assert_eq!(per_note["regras/global/pending"], ChunkEmbeddingCounts { total: 1, embedded: 0 });
        // Present and all-zero -> counted as neither embedded nor NULL: the
        // global report calls it `zero_vector`, and a badge that called it
        // indexed would be the exact lie TD-008 exists to prevent.
        assert_eq!(per_note["regras/global/zero"], ChunkEmbeddingCounts { total: 1, embedded: 0 });
        assert!(
            !per_note.contains_key("regras/global/bare"),
            "a note with no chunks must be absent from the map so callers can tell 'no chunks' from 'chunks pending'"
        );
    }

    /// `recent_paths` exists to drop the note bodies, so it must return the same
    /// rows `recent` does — same TTL filter, same ordering, same limit. If the
    /// two disagree the browse list shows a different set of notes than
    /// `brain recent`, with no error anywhere to explain it.
    #[test]
    fn test_recent_paths_agrees_with_recent_minus_the_bodies() {
        let s = test_store();
        let v = fixture_vec(12);
        store_with_chunk(&s, "regras/global/keep", "## keep", Some(&v));
        store_with_chunk(&s, "regras/global/live", "## live", None);
        // Expired: filtered out of both.
        let nid = s.note_upsert("regras/global/gone", "regras", Some("global"), "## gone", None, &[], false, Some("2000-01-01T00:00:00Z")).unwrap();
        s.chunk_insert(nid, "regras/global/gone", "regras", Some("global"), "## gone", 0, 1, None, &[], None).unwrap();

        let full = s.recent(1000).unwrap();
        let paths = s.recent_paths(1000).unwrap();
        assert_eq!(paths.len(), full.len(), "row count must match");
        let full_ids: Vec<_> = full.iter().map(|(p, l, sc, _)| (p.clone(), l.clone(), sc.clone())).collect();
        assert_eq!(paths, full_ids, "same rows, same order, bodies omitted");
        assert!(
            !paths.iter().any(|(p, _, _)| p == "regras/global/gone"),
            "an expired note must not be listed"
        );

        // The limit is honoured, matching `recent`.
        assert_eq!(s.recent_paths(1).unwrap().len(), 1);
        assert_eq!(s.recent_paths(0).unwrap().len(), 0);
    }

    #[test]
    fn test_chunks_sync_reuses_a_vector_only_for_byte_identical_text() {
        let s = test_store();
        let path = "regras/global/sync-reuse";
        let v = fixture_vec(8);
        let nid = s.note_upsert(path, "regras", Some("global"), "## alpha beta gamma", None, &[], false, None).unwrap();
        let fresh = FreshVectors::from([(0i32, ("## alpha beta gamma".to_string(), v.clone()))]);
        let snap = ChunkSnapshot::new();
        let st = s.chunks_sync(nid, path, "regras", Some("global"), "## alpha beta gamma", None, &[], &fresh, &snap).unwrap();
        assert_eq!((st.embedded, st.rehydrated, st.nulls), (1, 1, 0));

        // Same text, snapshot carrying the vector -> reused.
        let mut snap = ChunkSnapshot::new();
        snap.insert(0, ("## alpha beta gamma".to_string(), Some(v.clone())));
        let st = s.chunks_sync(nid, path, "regras", Some("global"), "## alpha beta gamma", None, &[], &FreshVectors::new(), &snap).unwrap();
        assert_eq!((st.embedded, st.preserved, st.nulls), (1, 1, 0), "identical snippet must reuse the stored vector");

        // A caller-supplied snapshot whose snippet differs is not reusable. Run on
        // a fresh path so the table holds no competing row and the caller's
        // snapshot is the only candidate.
        let path2 = "regras/global/sync-reuse-2";
        let nid2 = s.note_upsert(path2, "regras", Some("global"), "## alpha beta gamma", None, &[], false, None).unwrap();
        let mut snap = ChunkSnapshot::new();
        snap.insert(0, ("## some other text".to_string(), Some(v)));
        let st = s.chunks_sync(nid2, path2, "regras", Some("global"), "## alpha beta gamma", None, &[], &FreshVectors::new(), &snap).unwrap();
        assert_eq!((st.embedded, st.nulls), (0, 1), "a vector computed from other text must not be reused");
    }

    #[test]
    fn test_chunks_sync_refuses_to_write_a_degenerate_fresh_vector() {
        let s = test_store();
        let path = "regras/global/sync-degenerate";
        let nid = s.note_upsert(path, "regras", Some("global"), "## body", None, &[], false, None).unwrap();
        let fresh = FreshVectors::from([(0i32, ("## body".to_string(), vec![0.0; EMBEDDING_DIM]))]);
        let err = s.chunks_sync(nid, path, "regras", Some("global"), "## body", None, &[], &fresh, &ChunkSnapshot::new())
            .expect_err("a zero fresh vector must be refused, not stored");
        assert!(err.to_string().contains("degenerate"), "got: {err}");
    }

    #[test]
    fn test_all_notes_covers_ttl_expired_notes_that_recent_hides() {
        // A reindex built on `recent` would silently drop the vectors and FTS
        // rows of everything awaiting a sweep.
        let s = test_store();
        s.note_upsert("regras/global/live", "regras", Some("global"), "## live", None, &[], false, None).unwrap();
        s.note_upsert("regras/global/stale", "regras", Some("global"), "## stale", None, &[], false, Some("2000-01-01T00:00:00Z")).unwrap();
        assert_eq!(s.recent(10).unwrap().len(), 1, "recent filters expired notes");
        assert_eq!(s.all_notes().unwrap().len(), 2, "all_notes must not");
    }

    // ------------------------------------------------------------------
    // V-02: the reindex must not write vectors computed from text a note no
    // longer has (TOCTOU between the embed pass and the write).
    //
    // The backfill embeds the whole corpus *before* opening its write
    // transaction — measured at ~2 minutes for 815 chunks against a serial
    // Ollama. Every `brain_store` in that window writes the note's new text.
    // Positional alignment alone cannot see this: chunk 0 of the old text and
    // chunk 0 of the new text are both "index 0", so the old vector landed on
    // the new text and the index reported success.
    // ------------------------------------------------------------------

    /// Snapshot of a note's chunks, as a caller takes before an embed pass.
    fn snapshot_chunks(content: &str) -> Vec<String> {
        brain_core::chunk_text(content, brain_core::CHUNK_TARGET_TOKENS)
    }

    #[test]
    fn test_reindex_refuses_to_apply_a_vector_computed_from_superseded_text() {
        let s = test_store();
        let path = "regras/global/toctou";
        let old_text = "## original section text";
        let new_text = "## an entirely different section text";
        s.note_upsert(path, "regras", Some("global"), old_text, None, &[], false, None).unwrap();

        // Pass 1: read the corpus and embed it, exactly as `run_reindex` does.
        let snapshot: Vec<String> = s.all_notes().unwrap().into_iter().map(|(_, c)| c).collect();
        let mut embeds = std::collections::HashMap::new();
        embeds.insert(
            path.to_string(),
            NoteEmbed { chunks: snapshot_chunks(&snapshot[0]), vectors: vec![fixture_vec(11)] },
        );

        // An agent stores the note while the embed pass is still running.
        s.note_upsert(path, "regras", Some("global"), new_text, None, &[], false, None).unwrap();

        // Pass 2: write. The vector describes the OLD text and must not land.
        let (_notes, st) = s.reindex_all_with(&embeds).unwrap();
        assert_eq!(st.diverged, 1, "the stale vector must be counted, not silently applied");
        assert_eq!(st.rehydrated, 0, "nothing was rehydrated: the text it described is gone");
        assert_eq!(st.nulls, 1, "the new text has no vector, and NULL is the honest answer");

        let stored = s.chunk_snapshot(path).unwrap();
        assert_eq!(stored[&0].0, new_text, "reindex must reconcile the index against the CURRENT text");
        assert!(stored[&0].1.is_none(), "a vector computed from other text must never be attached to this chunk");
        assert_eq!(s.embedding_coverage().unwrap().zero_vector, 0);
    }

    #[test]
    fn test_reindex_applies_a_vector_whose_text_still_matches() {
        // The control for the case above: same snapshot, nobody edited the note.
        let s = test_store();
        let path = "regras/global/toctou-ok";
        let text = "## stable text for a clean reindex";
        s.note_upsert(path, "regras", Some("global"), text, None, &[], false, None).unwrap();
        let snapshot: Vec<String> = s.all_notes().unwrap().into_iter().map(|(_, c)| c).collect();
        let mut embeds = std::collections::HashMap::new();
        embeds.insert(
            path.to_string(),
            NoteEmbed { chunks: snapshot_chunks(&snapshot[0]), vectors: vec![fixture_vec(12)] },
        );
        let (_n, st) = s.reindex_all_with(&embeds).unwrap();
        assert_eq!((st.diverged, st.rehydrated, st.nulls), (0, 1, 0));
        assert_eq!(embedded_count(&s), 1);
    }

    #[test]
    fn test_reindex_preserves_the_untouched_notes_around_one_that_diverged() {
        // Divergence must be scoped to the note that moved. A single concurrent
        // `brain_store` must not cost the rest of the corpus its vectors.
        let s = test_store();
        let moved = "regras/global/moved";
        let stable = "regras/global/stable";
        s.note_upsert(moved, "regras", Some("global"), "## before the edit", None, &[], false, None).unwrap();
        s.note_upsert(stable, "regras", Some("global"), "## never touched", None, &[], false, None).unwrap();
        let snapshot: Vec<(String, String)> = s.all_notes().unwrap();
        let mut embeds = std::collections::HashMap::new();
        for (p, c) in &snapshot {
            embeds.insert(p.clone(), NoteEmbed { chunks: snapshot_chunks(c), vectors: vec![fixture_vec(13)] });
        }
        s.note_upsert(moved, "regras", Some("global"), "## after the edit, entirely new", None, &[], false, None).unwrap();
        let (_n, st) = s.reindex_all_with(&embeds).unwrap();
        assert_eq!((st.diverged, st.rehydrated), (1, 1), "one note diverged, the other rehydrated");
        let cov = s.embedding_coverage().unwrap();
        assert_eq!((cov.embedded, cov.without_embedding, cov.zero_vector), (1, 1, 0));
        assert!(s.chunk_snapshot(stable).unwrap()[&0].1.is_some(), "the untouched note keeps its vector");
    }

    #[test]
    fn test_note_embed_drops_a_vector_it_cannot_attribute() {
        // 3 vectors, 2 chunks: the last vector has no text to be verified against.
        // Writing it would be a guess, and a guessed vector is a wrong one.
        let ne = NoteEmbed {
            chunks: vec!["## a".to_string(), "## b".to_string()],
            vectors: vec![fixture_vec(1), fixture_vec(2), fixture_vec(3)],
        };
        let fresh = ne.fresh_vectors();
        assert_eq!(fresh.len(), 2, "an unattributable vector must be dropped, not guessed");
        assert_eq!(fresh[&0].0, "## a");
        assert_eq!(fresh[&1].0, "## b");
    }

    // ------------------------------------------------------------------
    // V-03: matching. Byte equality meant any cosmetic edit silently reset a
    // chunk to NULL with no warning and no recovery short of a full reindex.
    // ------------------------------------------------------------------

    #[test]
    fn test_normalisation_ignores_case_and_whitespace_runs() {
        assert_eq!(normalize_for_match("## A  b\n\n  c  "), normalize_for_match("## a b c"));
        assert_eq!(normalize_for_match("  leading and trailing  "), "leading and trailing");
        assert_ne!(normalize_for_match("## a b"), normalize_for_match("## a c"));
    }

    #[test]
    fn test_token_similarity_separates_reformatting_from_rewriting() {
        let reformat = token_similarity(&normalize_for_match("## alpha beta gamma"), &normalize_for_match("## alpha beta gamma"));
        assert!((reformat - 1.0).abs() < 1e-6, "identical token sets must score 1.0, got {reformat}");
        // A single changed word in a 5-token chunk: 4 of 6 tokens shared.
        let rewritten = token_similarity(&normalize_for_match("## alpha beta gamma"), &normalize_for_match("## alpha beta delta"));
        assert!(rewritten < DEFAULT_REUSE_SIMILARITY, "a substituted word must not qualify, got {rewritten}");
        // A rewritten paragraph shares almost nothing.
        let unrelated = token_similarity(&normalize_for_match("## rust sqlite wal fts5"), &normalize_for_match("## deploy nginx with tls"));
        assert!(unrelated < 0.2, "unrelated text must score near zero, got {unrelated}");
    }

    // ------------------------------------------------------------------
    // W-02: the reuse threshold must not recycle a semantic inversion.
    //
    // `DEVE validar` -> `NÃO DEVE validar` costs one token and scores 0.96, so the
    // old 0.90 threshold kept the *old* vector for the *inverted* rule. Each
    // guarantee below names the test that fails if it is broken again.
    // ------------------------------------------------------------------

    /// The measured numbers from the review, asserted so the threshold's
    /// justification cannot be quietly re-tuned to a value that recycles them.
    #[test]
    fn test_inverting_a_rule_scores_above_the_old_threshold() {
        let a = normalize_for_match("Todo serviço DEVE validar o token JWT em todas as requisições antes de responder ao cliente");
        let b = normalize_for_match("Todo serviço NAO DEVE validar o token JWT em todas as requisições antes de responder ao cliente");
        let sim = token_similarity(&a, &b);
        assert!(sim > 0.90, "the inversion is one token in a 16-token sentence, so it must score above the old threshold; got {sim}");
        assert!(sim < DEFAULT_REUSE_SIMILARITY, "and below the default, which is the whole point: {sim}");
    }

    /// The reuse rule itself: identical normative force, below the threshold.
    fn reuse_of(stored: &str, current: &str, policy: ReusePolicy) -> ChunkMatch {
        chunk_text_matches(stored, current, policy)
    }

    // ------------------------------------------------------------------
    // X-03: `proibido` was on the wrong side of the polarity ledger.
    //
    // It was listed as an *obligation*, so "obrigatorio validar o token" (+1) and
    // "proibido validar o token" (+1) scored identically, `delta 0`, and the
    // normative-force guard saw no change on the one axis it exists to police. The
    // review measured it as `nf 1 vs 1 | delta 0 | Similar(0.6)`.
    //
    // The token-count arithmetic is asserted first because that is what was wrong,
    // and it is a cheaper thing to break than the store path.
    // ------------------------------------------------------------------

    /// The numbers that were wrong, pinned.
    #[test]
    fn an_obligation_and_a_prohibition_score_as_opposites() {
        // The review's exact pair, and the exact numbers it reported.
        assert_eq!(normative_force(&normalize_for_match("obrigatorio validar o token")), 1);
        assert_eq!(normative_force(&normalize_for_match("proibido validar o token")), -1);
        let before = normalize_for_match("obrigatorio validar o token");
        let after = normalize_for_match("proibido validar o token");
        assert_ne!(
            normative_force(&before),
            normative_force(&after),
            "flipping an obligation into a prohibition must move the score, or the guard is blind on this axis"
        );
        // And it must survive similarity: the texts are close enough that a
        // threshold is the only thing that could wave this through.
        let sim = token_similarity(&before, &after);
        assert!(sim > 0.5, "the pair is close enough to be a realistic edit, not a rewrite: {sim}");
    }

    /// The load-bearing property: the inversion is refused **at every threshold**,
    /// including the opt-in lenient one.
    ///
    /// The lenient policy is the point. `BRAIN_REUSE_SIMILARITY=0.9` is a
    /// deliberate, documented loosening — the guard is what keeps that loosening
    /// from also permitting a polarity flip, so if the guard could be talked out of
    /// its verdict by a threshold then the threshold is the vulnerability, not the
    /// guard. This test uses `ReusePolicy` directly rather than the environment
    /// variable, so it cannot be affected by — or leak into — another test.
    #[test]
    fn an_inversion_from_obligation_to_prohibition_is_refused_under_the_lenient_opt_in() {
        // 22 tokens with exactly one of them swapped, so similarity is 0.909: above
        // every threshold in the loop below. That is deliberate — the *only* reason
        // these are refused is the polarity, which is the claim being made.
        let stored = "## autenticacao\n\nTodo servico obrigatorio validar o token jwt em todas as requisicoes antes de responder ao cliente com o escopo correto";
        let inverted = "## autenticacao\n\nTodo servico proibido validar o token jwt em todas as requisicoes antes de responder ao cliente com o escopo correto";

        // Same threshold the review measured 0.6 against, and the documented opt-in.
        for min_similarity in [0.5f32, 0.6, 0.75, 0.9, 1.0] {
            let policy = ReusePolicy { min_similarity };
            assert_eq!(
                reuse_of(stored, inverted, policy),
                ChunkMatch::Different,
                "at min_similarity {min_similarity} the obligation->prohibition flip must be refused, not \
                 reused. The guard is independent of the threshold by design."
            );
        }

        // The control: without the flip, the same threshold *does* allow reuse, so
        // the assertions above are about the polarity and not about the threshold
        // refusing everything.
        // Also one token, also 0.909 — the sole difference from `inverted` is
        // *which* word it is. Same similarity, opposite verdict, so the verdict can
        // only have come from the polarity.
        let benign = "## autenticacao\n\nTodo servico deve validar o token jwt em todas as requisicoes antes de responder ao cliente com o escopo correto";
        match reuse_of(stored, benign, ReusePolicy { min_similarity: 0.9 }) {
            ChunkMatch::Similar(sim) => {
                assert!((0.9..1.0).contains(&sim), "a non-polarity edit must be reusable under the opt-in: {sim}")
            }
            other => panic!("a non-polarity edit must be reusable under the opt-in, got {other:?}"),
        }
    }

    /// End to end through `chunks_sync`: the stored vector is dropped, and the
    /// report says the chunk is owed a fresh one.
    ///
    /// The unit assertions above cover the decision; this covers the consequence,
    /// because "the predicate returns `Different`" and "the old vector is not
    /// reused" are separate claims and only the second one is the guarantee.
    #[test]
    fn an_inverted_rule_does_not_keep_its_vector_under_the_lenient_opt_in() {
        let original = "## autenticacao\n\nTodo servico obrigatorio validar o token jwt em todas as requisicoes antes de responder ao cliente com o escopo correto";
        let inverted = "## autenticacao\n\nTodo servico proibido validar o token jwt em todas as requisicoes antes de responder ao cliente com o escopo correto";
        let lenient = Store::open_in_memory_with_reuse(ReusePolicy { min_similarity: 0.90 }).unwrap();
        let path = "regras/global/inverted";
        store_with_chunk(&lenient, path, original, Some(&fixture_vec(22)));
        let nid = lenient.note_upsert(path, "regras", Some("global"), inverted, None, &[], false, None).unwrap();
        let st = lenient
            .chunks_sync(nid, path, "regras", Some("global"), inverted, None, &[], &FreshVectors::new(), &ChunkSnapshot::new())
            .unwrap();
        assert_eq!(st.stale_reused, 0, "the inverted rule must not keep the old vector: {st:?}");
        assert_eq!(st.embedded, 0, "and no vector may be claimed: {st:?}");
        assert_eq!(st.nulls, 1, "the chunk is owed a fresh vector: {st:?}");
        assert_eq!(embedded_count(&lenient), 0, "nothing is embedded for the inverted rule");
    }

    /// The audit, as a test.
    ///
    /// Every token in either list is asserted to be on the side of the ledger the
    /// doc claims, and the borderline entries are pinned with a reason. The point
    /// is that "which list is a token in" is a decision someone will revisit, and
    /// the failure mode when they get it wrong is a guard that silently stops
    /// guarding — which is exactly what happened to `proibido`.
    #[test]
    fn the_polarity_ledger_has_no_token_on_the_wrong_side() {
        // X-03: these are the tokens that were wrong. If one is ever moved back,
        // this fails and the doc has to be updated with it.
        for prohibition in ["proibido", "proibida", "proibidos", "proibidas", "vetado", "vetada", "forbidden"] {
            assert!(
                NEGATION_TOKENS.contains(&prohibition),
                "{prohibition} expresses a prohibition; on the obligation side it makes an inversion score 0"
            );
            assert!(
                !OBLIGATION_TOKENS.contains(&prohibition),
                "{prohibition} must not be in both lists: the first match wins, so the entry would be dead"
            );
        }
        // The obligations stay obligations.
        for obligation in ["deve", "devem", "deve-se", "must", "should", "shall", "required", "requires", "mandatory", "obrigatorio"] {
            assert!(OBLIGATION_TOKENS.contains(&obligation), "{obligation} expresses an obligation and belongs on the positive side");
            assert!(!NEGATION_TOKENS.contains(&obligation), "{obligation} must not be in both lists");
        }
        // Intensity, not polarity, and deliberately kept anyway — pinned with the
        // reason so a future reader does not "fix" them and silently widen reuse.
        for intensity in ["sempre", "always", "only", "apenas", "somente", "unicamente"] {
            let side = if OBLIGATION_TOKENS.contains(&intensity) { "obligation" } else { "negation" };
            assert!(
                OBLIGATION_TOKENS.contains(&intensity) || NEGATION_TOKENS.contains(&intensity),
                "{intensity} is listed, and this test is where the choice is recorded"
            );
            assert_ne!(side, "unlisted", "{intensity} must be on exactly one side");
        }
        // No token may appear on both sides, and none may appear in neither by
        // accident: `normative_force` uses `else if`, so a duplicate is dead code.
        for tok in OBLIGATION_TOKENS {
            assert!(!NEGATION_TOKENS.contains(tok), "{tok} is in both lists, so the OBLIGATION branch is unreachable for it");
        }
        for tok in NEGATION_TOKENS {
            assert!(!OBLIGATION_TOKENS.contains(tok), "{tok} is in both lists, so the NEGATION branch is unreachable for it");
        }
    }

    #[test]
    fn test_an_inserted_negation_is_refused_reuse_at_every_threshold() {
        for policy in [ReusePolicy::EXACT, ReusePolicy { min_similarity: 0.90 }, ReusePolicy { min_similarity: 0.5 }] {
            assert_eq!(
                reuse_of("todo serviço DEVE validar o token JWT", "todo serviço NAO DEVE validar o token JWT", policy),
                ChunkMatch::Different,
                "an inserted negation must be refused even at similarity {}",
                policy.min_similarity
            );
        }
    }

    #[test]
    fn test_a_removed_negation_is_refused_reuse_at_every_threshold() {
        // The other direction: dropping `NAO` restores the obligation, and the
        // stored vector is for the forbidden version. A stop-list checked only for
        // *insertions* would miss this.
        assert_eq!(
            reuse_of("todo serviço NAO DEVE validar o token JWT", "todo serviço DEVE validar o token JWT", ReusePolicy { min_similarity: 0.5 }),
            ChunkMatch::Different
        );
    }

    #[test]
    fn test_a_deleted_obligation_is_refused_reuse_even_with_no_negation_token() {
        // Why this is polarity *counting* and not a negation stop-list: `deve`
        // appears on both sides here, so no negation word is inserted or removed —
        // only the obligation is gone. "DEVE validar o token" becoming "validar o
        // token" is as much a change of claim as inserting `NÃO`.
        let stored = "o serviço DEVE validar o token JWT";
        let current = "o serviço validar o token JWT";
        assert!(
            !NEGATION_TOKENS.iter().any(|t| current.split(' ').any(|w| w == *t)),
            "this fixture must contain no negation token on either side, or it is not testing what it claims"
        );
        assert_eq!(reuse_of(stored, current, ReusePolicy { min_similarity: 0.5 }), ChunkMatch::Different);
    }

    #[test]
    fn test_reformatting_still_reuses_and_never_counts_as_stale() {
        // The legitimate case the threshold is not allowed to break. At 1.0,
        // normalisation alone carries it.
        let a = "## Alpha Beta Gamma\n\nthe quick brown fox jumps";
        let b = "## alpha beta gamma   \n\nthe quick\nbrown fox\njumps\n";
        assert_eq!(reuse_of(a, b, ReusePolicy::EXACT), ChunkMatch::Exact);
    }

    #[test]
    fn a_renumbered_heading_costs_the_vector_under_the_exact_default() {
        // `## 1` -> `## 2` is the edit the old 0.90 threshold existed for, and it is
        // a real change to the text, so at the default it costs the vector. The chunk
        // is long on purpose: one changed token in a short heading scores 0.67 and
        // would be refused by *any* threshold, which would make this test blind to
        // the default's value. Here it scores ~0.96, so the assertion below only
        // holds while the default is 1.0 — which is what makes it a guard.
        let s = test_store();
        let path = "regras/global/renumber";
        let mut original = String::from("## 1\n\n");
        for i in 0..40 {
            original.push_str(&format!("token{i} filler{i}\n"));
        }
        let edited = original.replacen("## 1", "## 2", 1);
        store_with_chunk(&s, path, &original, Some(&fixture_vec(41)));
        let nid = s.note_upsert(path, "regras", Some("global"), &edited, None, &[], false, None).unwrap();
        let st = s.chunks_sync(nid, path, "regras", Some("global"), &edited, None, &[], &FreshVectors::new(), &ChunkSnapshot::new()).unwrap();
        assert_eq!((st.embedded, st.nulls, st.unmatched), (0, 1, 1), "the default is exact-match, so a renumbered heading costs the vector");
        assert_eq!(st.stale_reused, 0, "and nothing is reused behind the operator's back");
    }

    #[test]
    fn test_renumbering_preserves_the_vector_under_an_explicit_stale_opt_in() {
        // The same class of edit under the opt-in policy: one changed token in a
        // long chunk scores above 0.90, the vector is kept, and the report says it
        // is approximate. This is the branch that used to be the *default*, and it
        // still works when asked for by name.
        let s = Store::open_in_memory_with_reuse(ReusePolicy { min_similarity: 0.90 }).unwrap();
        let path = "regras/global/renumber-optin";
        let mut original = String::from("## 1\n\n");
        for i in 0..40 {
            original.push_str(&format!("token{i} filler{i}\n"));
        }
        let edited = original.replacen("## 1", "## 2", 1);
        store_with_chunk(&s, path, &original, Some(&fixture_vec(42)));
        let nid = s.note_upsert(path, "regras", Some("global"), &edited, None, &[], false, None).unwrap();
        let st = s.chunks_sync(nid, path, "regras", Some("global"), &edited, None, &[], &FreshVectors::new(), &ChunkSnapshot::new()).unwrap();
        assert_eq!((st.embedded, st.preserved, st.stale_reused, st.nulls), (1, 1, 1, 0), "the opt-in keeps the vector and says it is approximate");
    }

    #[test]
    fn test_a_fresh_vector_beats_a_stored_one_even_when_it_is_less_similar() {
        // The inverted precedence. Previously the fresh branch demanded an *exact*
        // match while the stored branch accepted anything above the threshold, so a
        // fresh vector computed from 96%-identical text was thrown away in favour of
        // a stored one describing 85%-identical text — preferring the worse answer,
        // and reporting it as `diverged` when nothing had diverged.
        //
        // The three texts: `S` is what the table holds, `C` is the current content,
        // and `F` is what the in-flight embed pass actually saw (one token behind
        // `C`, because a benign edit landed mid-pass).
        let s = Store::open_in_memory_with_reuse(ReusePolicy { min_similarity: 0.90 }).unwrap();
        let path = "regras/global/fresh-wins";
        let mut stored_text = String::from("## alpha\n\n");
        for i in 0..20 {
            stored_text.push_str(&format!("w{i}\n"));
        }
        let mut current = String::from("## beta\n\n");
        for i in 0..20 {
            current.push_str(&format!("x{i}\n"));
        }
        let embed_seen = format!("{current}extra\n");
        let stored_vec = fixture_vec(43);
        let fresh_vec = fixture_vec(44);

        store_with_chunk(&s, path, &stored_text, Some(&stored_vec));
        let nid = s.note_upsert(path, "regras", Some("global"), &current, None, &[], false, None).unwrap();
        // Provenance is the chunk text the pass saw, one token behind the current one.
        let seen_chunk = chunk_text(&embed_seen, brain_core::CHUNK_TARGET_TOKENS)[0].clone();
        let fresh = FreshVectors::from([(0i32, (seen_chunk, fresh_vec.clone()))]);
        let st = s.chunks_sync(nid, path, "regras", Some("global"), &current, None, &[], &fresh, &ChunkSnapshot::new()).unwrap();

        assert_eq!(st.diverged, 0, "nothing diverged: the pass saw text 96% identical to the current one");
        assert_eq!(st.preserved, 0, "and the stored vector describes text the note no longer has");
        assert_eq!((st.rehydrated, st.nulls), (1, 0), "so the fresh vector must be applied rather than discarded");
        assert_eq!(st.stale_fresh, 1, "and it is reported as not-exact rather than hidden inside `rehydrated`");
        let snap = s.chunk_snapshot(path).unwrap();
        assert_eq!(snap[&0].1.as_ref().unwrap(), &fresh_vec, "the stored vector must be the fresh one, byte for byte");
    }

    #[test]
    fn test_the_queue_and_the_write_path_apply_the_same_threshold() {
        // W-02.3. These were two independent call sites of a free function and they
        // drifted: `chunks_needing_embedding` treated a `Similar` chunk as done, so
        // a vector the write path would have refused to reuse was never refreshed
        // and the error sustained itself. Both now read the store's own policy, so
        // the assertion below is a property of the code, not of a shared constant.
        //
        // X-05.6. The write-path half used to be recomputed here as
        // `chunk_text_matches(&original, &edited, s.reuse_policy())`, which is a
        // tautology waiting to happen: it asserts that the free function agrees
        // with itself, and a refactor that moved the real decision somewhere else
        // would leave the assertion passing while the two paths drifted apart again
        // — the exact bug this test exists to prevent. It now observes
        // `chunks_sync`, which is the decision the write path actually makes.
        for policy in [ReusePolicy::EXACT, ReusePolicy { min_similarity: 0.90 }] {
            let s = Store::open_in_memory_with_reuse(policy).unwrap();
            let path = "regras/global/aligned";
            let mut original = String::from("## long section\n\n");
            for i in 0..40 {
                original.push_str(&format!("token{i} filler{i}\n"));
            }
            let edited = format!("{original}one more sentence\n");
            store_with_chunk(&s, path, &original, Some(&fixture_vec(45)));

            // What the queue calls done: no chunk is owed a vector.
            let owed = s.chunks_needing_embedding(path, &edited).unwrap();

            // What the write path actually does with the stored vector, observed
            // rather than recomputed.
            let nid = s.note_upsert(path, "regras", Some("global"), &edited, None, &[], false, None).unwrap();
            let st = s
                .chunks_sync(nid, path, "regras", Some("global"), &edited, None, &[], &FreshVectors::new(), &ChunkSnapshot::new())
                .unwrap();
            // The two agree when the vector is kept (`owed` empty, `stale_reused` 1)
            // and disagree only in that the write path had to be told. What must hold
            // is that the queue never calls a chunk "done" the write path would
            // refuse: an empty `owed` implies the write path kept the vector.
            assert_eq!(
                owed.is_empty(),
                st.stale_reused == 1,
                "what the queue calls done must be exactly what the write path reuses, at \
                 min_similarity={}: owed={owed:?} stale_reused={}",
                policy.min_similarity,
                st.stale_reused
            );
        }
    }

    #[test]
    fn test_chunks_sync_preserves_a_vector_across_a_whitespace_only_edit() {
        // The reported case: reformatting the markdown (trailing space, re-wrapped
        // line, changed heading case) used to invalidate the vector, because
        // matching was byte-for-byte. Nothing about the text's meaning changed.
        let s = test_store();
        let path = "regras/global/reformat";
        let original = "## Alpha Beta Gamma\n\nthe quick brown fox jumps";
        let v = fixture_vec(21);
        store_with_chunk(&s, path, original, Some(&v));
        assert_eq!(embedded_count(&s), 1);

        let reformatted = "## alpha beta gamma   \n\nthe quick\nbrown fox\njumps\n";
        let nid = s.note_upsert(path, "regras", Some("global"), reformatted, None, &[], false, None).unwrap();
        let st = s.chunks_sync(nid, path, "regras", Some("global"), reformatted, None, &[], &FreshVectors::new(), &ChunkSnapshot::new()).unwrap();
        assert_eq!((st.embedded, st.nulls), (1, 0), "a reformatting must not cost the note its vector");
        assert_eq!(st.preserved, 1);
        assert_eq!(st.stale_reused, 0, "normalised-identical text is an exact match, not a stale reuse");
        assert_eq!(embedded_count(&s), 1);
    }

    #[test]
    fn test_chunks_sync_reuses_a_slightly_changed_chunk_only_under_an_explicit_opt_in() {
        // One sentence appended to a long chunk: 96% similar. At the default this
        // is now a rewrite — the vector is dropped and the chunk is owed a fresh
        // one. With `BRAIN_REUSE_SIMILARITY=0.90` the vector is kept and *counted*,
        // so the cost of that choice stays visible in the report rather than
        // assumed. Both halves are asserted because the default used to be the
        // lenient one.
        let mut original = String::from("## long section\n\n");
        for i in 0..40 { original.push_str(&format!("token{i} filler{i}\n")); }
        let mut edited = original.clone();
        edited.push_str("one more sentence appended here\n");

        let strict = test_store();
        let path = "regras/global/stale-strict";
        store_with_chunk(&strict, path, &original, Some(&fixture_vec(22)));
        let nid = strict.note_upsert(path, "regras", Some("global"), &edited, None, &[], false, None).unwrap();
        let st = strict.chunks_sync(nid, path, "regras", Some("global"), &edited, None, &[], &FreshVectors::new(), &ChunkSnapshot::new()).unwrap();
        assert_eq!((st.embedded, st.nulls, st.unmatched, st.stale_reused), (0, 1, 1, 0), "the default refuses a 96%-similar chunk and reports the loss");

        let lenient = Store::open_in_memory_with_reuse(ReusePolicy { min_similarity: 0.90 }).unwrap();
        let path = "regras/global/stale-lenient";
        store_with_chunk(&lenient, path, &original, Some(&fixture_vec(22)));
        let nid = lenient.note_upsert(path, "regras", Some("global"), &edited, None, &[], false, None).unwrap();
        let st = lenient.chunks_sync(nid, path, "regras", Some("global"), &edited, None, &[], &FreshVectors::new(), &ChunkSnapshot::new()).unwrap();
        assert_eq!((st.embedded, st.nulls, st.unmatched), (1, 0, 0), "a lightly edited chunk keeps its vector under the opt-in");
        assert_eq!(st.preserved, 1);
        assert_eq!(st.stale_reused, 1, "and the report must say the vector is now approximate");
        assert_eq!(embedded_count(&lenient), 1);
    }

    #[test]
    fn test_chunks_sync_nulls_a_vector_whose_text_was_rewritten() {
        // The other side of the threshold, and the property that keeps the
        // similarity fallback from becoming "reuse anything vaguely related".
        let s = test_store();
        let path = "regras/global/rewrite";
        let original = "## rust owns the sqlite connection and writes the wal file itself";
        let rewritten = "## postgres owns the connection pool and the migration files";
        let v = fixture_vec(23);
        store_with_chunk(&s, path, original, Some(&v));
        let nid = s.note_upsert(path, "regras", Some("global"), rewritten, None, &[], false, None).unwrap();
        let st = s.chunks_sync(nid, path, "regras", Some("global"), rewritten, None, &[], &FreshVectors::new(), &ChunkSnapshot::new()).unwrap();
        assert_eq!((st.embedded, st.nulls, st.unmatched), (0, 1, 1), "a rewritten chunk must not keep the old vector");
        assert_eq!(st.stale_reused, 0);
        assert_eq!(embedded_count(&s), 0, "NULL, not a vector for text that no longer exists");
        assert_eq!(s.embedding_coverage().unwrap().zero_vector, 0);
    }

    #[test]
    fn test_chunks_needing_embedding_is_a_diff_not_a_rebuild() {
        // The queue's work list. A session note grows by one section per tool
        // result; re-embedding the whole accumulated document each time is
        // quadratic work for vectors that already exist.
        let s = test_store();
        let path = "regras/global/diff";
        let first = "## first section\n\nbody one";
        let second = first.to_string() + "\n## second section\n\nbody two";
        store_with_chunk(&s, path, first, Some(&fixture_vec(31)));
        let note = s.note_get(path).unwrap().unwrap();
        assert_eq!(s.chunks_needing_embedding(path, &note.content).unwrap().len(), 0, "the only section already has a vector");
        s.note_upsert(path, "regras", Some("global"), &second, None, &[], false, None).unwrap();
        let note = s.note_get(path).unwrap().unwrap();
        let todo = s.chunks_needing_embedding(path, &note.content).unwrap();
        assert_eq!(todo.len(), 1, "only the new section is outstanding");
        assert_eq!(todo[0].0, 1, "and it is the new one");
    }

    #[test]
    fn test_chunks_needing_embedding_agrees_with_the_write_path_about_a_similar_chunk() {
        // W-02.3, the queue side. The old code reported a 96%-similar chunk as done
        // *and* re-used the stale vector, so the queue refused to fix the very thing
        // the write path had decided to keep: the error sustained itself until a
        // human ran `reindex --all`. The two must now agree at both settings.
        let mut original = String::from("## long section\n\n");
        for i in 0..40 { original.push_str(&format!("token{i} filler{i}\n")); }
        let edited = format!("{original}one more sentence\n");

        // Default: owed, because the write path would not have reused it.
        let s = test_store();
        let path = "regras/global/diff-strict";
        store_with_chunk(&s, path, &original, Some(&fixture_vec(32)));
        assert_eq!(s.chunks_needing_embedding(path, &edited).unwrap().len(), 1, "at the default a reworded chunk is owed a fresh vector");

        // Opt-in: not owed, because the write path keeps the approximate vector.
        let lenient = Store::open_in_memory_with_reuse(ReusePolicy { min_similarity: 0.90 }).unwrap();
        let path = "regras/global/diff-lenient";
        store_with_chunk(&lenient, path, &original, Some(&fixture_vec(32)));
        assert!(lenient.chunks_needing_embedding(path, &edited).unwrap().is_empty(), "under the opt-in a similar chunk is not re-queued");

        // Both settings: a rewritten chunk is always owed.
        let rewritten = "## postgres owns the pool and the migrations".to_string();
        assert_eq!(s.chunks_needing_embedding(path, &rewritten).unwrap().len(), 1, "a rewritten chunk is re-queued");
    }

    #[test]
    fn test_chunks_needing_embedding_requeues_a_chunk_whose_rule_was_inverted() {
        // The end-to-end consequence of the guard, on the path that actually
        // decides: a rule flipped from DEVE to NAO DEVE must come back as owed
        // work, not as "already has a vector". This is the test that fails if
        // `chunks_needing_embedding` is ever decoupled from the write path again.
        let s = test_store();
        let path = "regras/global/diff-negation";
        let before = "Todo serviço DEVE validar o token JWT";
        let after = "Todo serviço NAO DEVE validar o token JWT";
        store_with_chunk(&s, path, before, Some(&fixture_vec(46)));
        let todo = s.chunks_needing_embedding(path, after).unwrap();
        assert_eq!(todo.len(), 1, "an inverted rule must be re-embedded, not left with the old vector");
        assert_eq!(todo[0].0, 0);
        assert_eq!(normalize_for_match(&todo[0].1), normalize_for_match(after), "and the queue embeds the *new* text, not the old");
    }

    #[test]
    fn test_notes_needing_embedding_finds_work_a_restart_left_behind() {
        // W-04a. Boot recovery reads the database, not the in-memory queue, so this
        // is the only place the lost work can be discovered. Covers the note a
        // restart orphaned *and* the note that is already whole.
        let s = test_store();
        let owed = store_with_chunk(&s, "regras/global/owed", "## one\n\nbody", None);
        assert!(owed > 0);
        store_with_chunk(&s, "regras/global/done", "## two\n\nbody", Some(&fixture_vec(47)));
        let missing = s.notes_needing_embedding().unwrap();
        assert_eq!(missing.len(), 1, "only the NULL chunk is owed work: {missing:?}");
        assert_eq!(missing[0].0, "regras/global/owed");
        assert_eq!(missing[0].1, 1);
        // A reindex that hydrates everything must empty the list, or boot recovery
        // would re-queue the whole corpus on every restart.
        let mut embeds = std::collections::HashMap::new();
        embeds.insert("regras/global/owed".to_string(), NoteEmbed { chunks: vec!["## one\n\nbody".to_string()], vectors: vec![fixture_vec(48)] });
        s.reindex_all_with(&embeds).unwrap();
        assert!(s.notes_needing_embedding().unwrap().is_empty(), "a hydrated index owes nothing: {:?}", s.notes_needing_embedding().unwrap());
    }

    #[test]
    fn test_chunks_needing_embedding_ignores_a_legacy_zero_blob() {
        let s = test_store();
        let path = "regras/global/diff-zero";
        store_with_chunk(&s, path, "## body", Some(&fixture_vec(33)));
        // Overwrite the good vector with the legacy BLOB-of-zeros, the way the
        // pre-P0-ZV production index looked.
        s.conn.execute("UPDATE chunks SET embedding=zeroblob(3072) WHERE path='regras/global/diff-zero'", []).unwrap();
        assert_eq!(s.chunks_needing_embedding(path, "## body").unwrap().len(), 1, "a BLOB-of-zeros is not a vector; the chunk still needs embedding");
    }

    // ------------------------------------------------------------------
    // The cross-process embed lock: a reindex and the server's background queue
    // must not embed the same chunks twice.
    // ------------------------------------------------------------------

    #[test]
    fn test_embed_lock_is_exclusive_reentrant_and_released() {
        let s = test_store();
        let a = embed_lock_owner("queue");
        let b = embed_lock_owner("reindex");
        assert!(s.try_acquire_embed_lock(&a, 900).unwrap(), "the first caller wins");
        assert!(!s.try_acquire_embed_lock(&b, 900).unwrap(), "a second embed pass must be told to skip");
        assert_eq!(s.embed_lock_holder().unwrap().as_deref(), Some(a.as_str()));
        assert!(s.try_acquire_embed_lock(&a, 900).unwrap(), "re-entrant for the same owner");
        assert!(s.release_embed_lock(&a).unwrap());
        assert!(!s.release_embed_lock(&a).unwrap(), "releasing twice is not an error, just a no-op");
        assert!(s.try_acquire_embed_lock(&b, 900).unwrap(), "after release the lock is free");
    }

    #[test]
    fn test_embed_lock_expires_so_a_killed_process_cannot_wedge_embedding() {
        let s = test_store();
        let owner = embed_lock_owner("queue");
        assert!(s.try_acquire_embed_lock(&owner, 0).unwrap());
        // TTL 0 means "expired the moment it was taken", so a different owner may
        // take over immediately. Without this a crashed backfill would block
        // every future embed until the DB was deleted.
        assert!(s.try_acquire_embed_lock(&embed_lock_owner("reindex"), 900).unwrap());
        assert_eq!(s.embed_lock_holder().unwrap().as_deref(), Some(embed_lock_owner("reindex").as_str()));
    }

    #[test]
    fn test_embed_lock_reports_its_age_and_countdown() {
        // W-04.6. "The queue is standing down" and "a process died holding the
        // lock" look identical through the holder name alone. These two numbers
        // are what `brain status` needs to tell them apart, and they are only
        // derivable because the lock value carries `taken_at` — an expiry alone
        // does not say when the lock was taken, since the TTL is a caller argument.
        let s = test_store();
        assert_eq!(s.embed_lock_age_secs().unwrap(), None, "no lock, no age");
        assert_eq!(s.embed_lock_expires_in_secs().unwrap(), None);
        let owner = embed_lock_owner("queue");
        assert!(s.try_acquire_embed_lock(&owner, 900).unwrap());
        let age = s.embed_lock_age_secs().unwrap().expect("a held lock has an age");
        let left = s.embed_lock_expires_in_secs().unwrap().expect("a held lock has a countdown");
        assert!((0..=2).contains(&age), "just taken, got {age}");
        assert!((898..=900).contains(&left), "900s TTL, got {left}");
        assert!(s.release_embed_lock(&owner).unwrap());
        assert_eq!(s.embed_lock_age_secs().unwrap(), None, "released means no age again");
    }

    #[test]
    fn test_embed_lock_check_and_set_are_one_transaction() {
        // W-04.7. In autocommit the `SELECT` and the `INSERT OR REPLACE` were
        // separate statements, so two processes could both read "no lock" and both
        // conclude they hold it. `BEGIN IMMEDIATE` closes that window. The
        // observable proof that the transaction is real: the lock write is not
        // visible to a *second connection* until `COMMIT`, and it is visible after.
        let db = format!("/tmp/brain-store-locktx-{}.db", std::process::id());
        let _ = std::fs::remove_file(&db);
        {
            let a = Store::open(&db).unwrap();
            assert!(a.try_acquire_embed_lock(&embed_lock_owner("a"), 900).unwrap());
            // A second store on the same file must not see a half-done acquisition,
            // and must be told the lock is taken.
            let b = Store::open(&db).unwrap();
            assert_eq!(b.embed_lock_holder().unwrap().as_deref(), Some(embed_lock_owner("a").as_str()));
            assert!(!b.try_acquire_embed_lock(&embed_lock_owner("b"), 900).unwrap(), "the loser is told to stand down");
        }
        // After everyone closes, the lock is still held and still reclaimable only
        // by expiry: a crashed process leaves a lock that blocks for its TTL, which
        // is why the TTL exists and why `embed_lock_age_secs` is reported.
        let c = Store::open(&db).unwrap();
        assert_eq!(c.embed_lock_holder().unwrap().as_deref(), Some(embed_lock_owner("a").as_str()));
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn test_legacy_two_field_lock_value_does_not_report_a_wrong_age() {
        // A value written before `taken_at` existed has an expiry and no age. It
        // must report `None`, not `expires - now` presented as an age.
        let s = test_store();
        let now: i64 = s.conn.query_row("SELECT CAST(strftime('%s','now') AS INTEGER)", [], |r| r.get(0)).unwrap();
        let expires = now + 900;
        s.conn.execute("INSERT OR REPLACE INTO _meta(key, value) VALUES('embed_lock', ?1)", params![format!("brain:legacy:1|{expires}")]).unwrap();
        assert_eq!(s.embed_lock_holder().unwrap().as_deref(), Some("brain:legacy:1"));
        assert_eq!(s.embed_lock_age_secs().unwrap(), None, "no taken_at, no age — not a wrong one");
        assert!(s.embed_lock_expires_in_secs().unwrap().unwrap() > 0);
    }

    #[test]
    fn test_store_exposes_the_path_it_was_opened_from() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.path(), ":memory:");
        let db = format!("/tmp/brain-store-path-{}.db", std::process::id());
        let _ = std::fs::remove_file(&db);
        let s2 = Store::open(&db).unwrap();
        assert_eq!(s2.path(), db);
        let _ = std::fs::remove_file(&db);
    }

    // ------------------------------------------------------------------
    // X-01: a writer that has to wait must wait, not fail. This is the
    // `busy_timeout` and no-write-lock-on-open half of the hook fix, and they
    // are separate properties from atomicity: the append can be perfectly
    // serialised and still lose every event if a concurrent connection's write
    // lock is answered with an immediate `SQLITE_BUSY` instead of a wait.
    // ------------------------------------------------------------------

    fn temp_file_db(tag: &str) -> String {
        let db = format!("/tmp/brain-store-busy-{}-{tag}.db", std::process::id());
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{db}{suffix}"));
        }
        db
    }

    fn no_rotation(_part: u32, _why: &str) -> String {
        String::new()
    }

    /// Holds the database's write lock for ~400 ms, then releases it.
    ///
    /// Returns only once the lock is *taken*, so the caller's next statement
    /// provably contends. 400 ms is long enough that a connection with no busy
    /// handler cannot have been lucky, and well inside the 5 s timeout.
    fn hold_write_lock(db: &str) -> std::thread::JoinHandle<()> {
        use std::sync::mpsc;
        let (locked_tx, locked_rx) = mpsc::channel();
        let db = db.to_string();
        let handle = std::thread::spawn(move || {
            let s = Store::open(&db).unwrap();
            let tx = rusqlite::Transaction::new_unchecked(&s.conn, rusqlite::TransactionBehavior::Immediate).unwrap();
            locked_tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(400));
            tx.rollback().unwrap();
        });
        locked_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the holder must have taken the write lock");
        handle
    }

    /// The append waits for another connection's write lock instead of failing.
    ///
    /// The observable property: a writer that contends for ~400 ms still gets its
    /// section in, instead of an immediate `SQLITE_BUSY` that would drop the event.
    ///
    /// Stated precisely because it is easy to over-claim here: this passes because
    /// a busy handler is installed, and it fails if that handler is switched off
    /// (`busy_timeout(0)` was verified to fail it). It is *not* what makes the hook
    /// atomic, and it was not what fixed the original 6-of-12 failure — the
    /// `IMMEDIATE` transaction and the in-transaction read were. Its value is that
    /// it pins the wait, so a change to rusqlite's default cannot silently turn a
    /// contended write into a lost event.
    #[test]
    fn an_append_waits_for_a_concurrent_writer_instead_of_failing() {
        let db = temp_file_db("wait");
        // Create the schema first, so the contending transaction is the only
        // lock in play and the test measures the append, not the DDL.
        {
            let s = Store::open(&db).unwrap();
            s.note_upsert("sessoes/t/2026-01-01", "sessoes", None, "seed", None, &[], false, None).unwrap();
        }
        let holder = hold_write_lock(&db);

        let s = Store::open(&db).unwrap();
        // The section carries the marker the hook's dedup would key on, so the
        // assertion below checks the same text the hook writes.
        let result = s.note_append_section(
            "sessoes/t/2026-01-01",
            "sessoes",
            None,
            "## one id=one\n\nbody",
            "id=one\n",
            "# Test day\n",
            &no_rotation,
            None,
            &[],
        );

        holder.join().unwrap();
        let appended = result.expect("the append must wait for the lock, not fail with SQLITE_BUSY");
        assert!(appended.appended);
        let content = s.note_get("sessoes/t/2026-01-01").unwrap().unwrap().content;
        assert!(content.contains("id=one"), "the section must be in the note: {content}");
        let _ = std::fs::remove_file(&db);
    }

    /// The version marker is what decides whether the DDL has to run at all.
    ///
    /// `init_schema` re-ran the whole DDL batch on every `Store::open` — which is
    /// every MCP request and twice per hook event — so opening a database needed
    /// its write lock even to read a note. The fast path is a read of this marker,
    /// and it is the decision that removes the lock, so it is tested directly
    /// rather than through a wall-clock assertion (which would be the fragile
    /// kind of test this suite is trying not to grow).
    ///
    /// Note there is no "a file that does not exist yet" case to assert: opening
    /// *is* what creates the schema, so a `Store` can never observe that state. The
    /// observable equivalent is an older build's marker, which is also the
    /// migration path.
    #[test]
    fn the_schema_marker_decides_whether_the_ddl_runs() {
        let db = temp_file_db("marker");
        {
            let s = Store::open(&db).unwrap();
            assert!(s.schema_is_current(), "a database this build just created is current");
            // Pretend an older build wrote it.
            s.conn.execute("UPDATE _meta SET value='3' WHERE key='version'", []).unwrap();
            assert!(!s.schema_is_current(), "a version-3 database must not be mistaken for current");
        }
        {
            // Re-opening must run the DDL and bring the marker forward.
            let s = Store::open(&db).unwrap();
            assert!(s.schema_is_current(), "opening it must have migrated the marker forward");
        }
        let _ = std::fs::remove_file(&db);
    }

    /// Opening a database that is already at this version succeeds while another
    /// connection holds the write lock.
    ///
    /// **This does not, by itself, prove the fast path above.** With the DDL
    /// running on every open the test also passes, because the DDL transaction
    /// takes `IMMEDIATE` and simply waits out the holder. What the fast path buys
    /// is not *succeeding* but not *waiting* — a read no longer queues behind
    /// writers — and that is a latency claim, deliberately not asserted here
    /// because a timing assertion on a loaded CI machine is exactly the fragile
    /// test the review flagged. The decision itself is pinned by
    /// `the_schema_marker_decides_whether_the_ddl_runs`.
    #[test]
    fn opening_a_current_database_does_not_need_the_write_lock() {
        let db = temp_file_db("readonly");
        {
            let s = Store::open(&db).unwrap();
            s.note_upsert("regras/global/x", "regras", Some("global"), "## a", None, &[], false, None).unwrap();
        }
        let holder = hold_write_lock(&db);

        // A second open, plus a read, while the write lock is held elsewhere.
        let opened = Store::open(&db);
        holder.join().unwrap();
        let s = opened.expect("opening a current database must not require the write lock");
        assert_eq!(s.note_get("regras/global/x").unwrap().unwrap().content, "## a");
        let _ = std::fs::remove_file(&db);
    }

    // -------------------------------------------------------------- Y-04 --
    //
    // `chunks_sync` pruned rows past `chunks.len()` — the chunk count of the
    // **content the caller passed**, which is a snapshot read before the embed
    // pass and can be stale by the time the write lands. Two hooks appending to
    // the same session note interleave like this:
    //
    //   A: note_append_section -> 3 chunks,  A reads content_A
    //   B: note_append_section -> 4 chunks,  B reads content_B
    //   B: chunks_sync(4)  writes idx 0..3, prunes >= 4   (nothing to prune)
    //   A: chunks_sync(3)  writes idx 0..2, prunes >= 3   -> DELETES B's idx 3
    //
    // B has already returned. Nobody re-edits the note, so the semantic index is
    // silently short until the next event or the next boot's `recover()`.
    //
    // Not data loss — FTS comes from the `notes` trigger and stays consistent,
    // and `chunks_needing_embedding` is a diff of the note's text, so the row
    // comes back as `NULL` when the note is next touched. What is lost is
    // *coverage*: the note is no longer vector-searchable at index 3.

    /// A body with `n` `## ` sections, so `chunk_text` yields `n` chunks.
    fn y04_body(n: usize, salt: &str) -> String {
        let mut out = String::new();
        for i in 0..n {
            out.push_str(&format!("## section {i}\n\nprose {i} withtoken{i}{salt}\n\n"));
        }
        out
    }

    /// The chunk indices currently stored for `path`.
    fn y04_indices(s: &Store, path: &str) -> Vec<i32> {
        let mut v: Vec<i32> = s.chunk_snapshot(path).unwrap().keys().copied().collect();
        v.sort_unstable();
        v
    }

    /// Y-04: a stale writer that finishes last must not delete the row a newer
    /// writer added.
    ///
    /// The interleaving is written out in order rather than raced with threads,
    /// deliberately. The defect is not "two threads at once" — it is "a writer
    /// holding an older snapshot writes after a writer holding a newer one", and
    /// that ordering is exactly reproducible, so this test fails **every** run on
    /// the unfixed code instead of intermittently. A threaded version would only
    /// catch the interleaving when the scheduler happened to pick it.
    ///
    /// The guarantee asserted: **a chunk row whose index is below the note's
    /// current chunk count survives any `chunks_sync` of that note.**
    #[test]
    fn a_stale_chunks_sync_does_not_prune_a_chunk_the_note_still_has() {
        let s = Store::open_in_memory().unwrap();
        let path = "sessoes/shared/y04";
        let three = y04_body(3, "three");
        let four = y04_body(4, "four");

        // A writes its 3-chunk version, B then appends a 4th section.
        s.note_upsert(path, "sessoes", None, &three, None, &[], false, None).unwrap();
        let nid = s.note_upsert(path, "sessoes", None, &four, None, &[], false, None).unwrap();
        assert_eq!(nid, s.note_id(path).unwrap().unwrap());

        // A's pass, holding its 3-chunk snapshot.
        s.chunks_sync(nid, path, "sessoes", None, &three, None, &[], &FreshVectors::new(), &ChunkSnapshot::new())
            .unwrap();
        // B's pass, holding its 4-chunk snapshot. This writes index 3.
        s.chunks_sync(nid, path, "sessoes", None, &four, None, &[], &FreshVectors::new(), &ChunkSnapshot::new())
            .unwrap();
        assert_eq!(y04_indices(&s, path), vec![0, 1, 2, 3], "precondition: B's 4th chunk is stored");

        // A's write-back finally lands, still holding the 3-chunk snapshot.
        s.chunks_sync(nid, path, "sessoes", None, &three, None, &[], &FreshVectors::new(), &ChunkSnapshot::new())
            .unwrap();

        assert_eq!(
            y04_indices(&s, path),
            vec![0, 1, 2, 3],
            "the note has 4 chunks, so index 3 is still a chunk of this note and must survive a stale writer's \
             prune. Deleting it is what left the semantic index silently short: B had already returned, so no \
             further edit would bring the row back until the next event or the next boot's recover()."
        );
        // And the surviving row is *B's* text, not a blank or A's.
        let snap = s.chunk_snapshot(path).unwrap();
        assert!(
            snap[&3].0.contains("withtoken3four"),
            "index 3 must still hold the newer writer's text, got {:?}",
            snap[&3].0
        );
    }

    /// Y-04, other half: a note that genuinely shrank still has its stale tail
    /// pruned.
    ///
    /// Without this the bound could be "never prune", which would make the test
    /// above pass and quietly stop the note from ever shrinking — a worse bug,
    /// traded for a milder one.
    #[test]
    fn a_shrunken_note_is_still_pruned() {
        let s = Store::open_in_memory().unwrap();
        let path = "sessoes/shared/y04-shrink";
        let four = y04_body(4, "x");
        let two = y04_body(2, "x");
        let nid = s.note_upsert(path, "sessoes", None, &four, None, &[], false, None).unwrap();
        s.chunks_sync(nid, path, "sessoes", None, &four, None, &[], &FreshVectors::new(), &ChunkSnapshot::new())
            .unwrap();
        assert_eq!(y04_indices(&s, path), vec![0, 1, 2, 3], "precondition: 4 chunks stored");

        // The note itself now has 2 chunks, so there is nothing at index >= 2 for
        // the note to have, and the rows are dead weight.
        s.note_upsert(path, "sessoes", None, &two, None, &[], false, None).unwrap();
        s.chunks_sync(nid, path, "sessoes", None, &two, None, &[], &FreshVectors::new(), &ChunkSnapshot::new())
            .unwrap();
        assert_eq!(
            y04_indices(&s, path),
            vec![0, 1],
            "a note that shrank must still lose the chunks it no longer has, or the table grows forever"
        );
    }
}
