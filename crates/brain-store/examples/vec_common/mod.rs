//! Shared helpers for the C3-01 `sqlite-vec` spike examples.
//!
//! Included with `#[path = "vec_common.rs"] mod common;` from each example
//! crate root. Kept separate so the `unsafe` auto-extension registration lives
//! in exactly one place — it is the only genuinely unsafe step in the whole
//! spike, and it is the thing C3-02 would have to get right.

/// Registers the sqlite-vec entry point as a process-global SQLite
/// auto-extension.
///
/// MUST be called before the first `Connection::open()`: SQLite applies
/// auto-extensions to connections created *after* the call. `Once` makes it
/// idempotent, which is what brain's per-request `Store::open()` model needs.
///
/// This does NOT require `loadable_extension`, a shared library on disk, or a
/// custom `libsqlite3-sys` build — `sqlite-vec`'s `build.rs` statically links
/// the C amalgamation, so rusqlite's `bundled` strategy is untouched.
pub fn register_sqlite_vec() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        // SAFETY: `sqlite3_vec_init` is the extension entry point emitted by
        // sqlite-vec's build.rs. `sqlite3_auto_extension` only stores the
        // pointer in SQLite's global extension list; it never dereferences or
        // frees it, so widening the fn item pointer to a plain fn pointer is
        // sound and needs no lifetime tie.
        let entry: unsafe extern "C" fn(
            *mut rusqlite::ffi::sqlite3,
            *mut *mut i8,
            *const rusqlite::ffi::sqlite3_api_routines,
        ) -> i32 = std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
        rusqlite::ffi::sqlite3_auto_extension(Some(entry));
    });
}
