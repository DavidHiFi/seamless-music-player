//! One-time import from Dopamine Enhanced: a copy of its SQLite database and
//! config, artwork from its cache, m3u playlists, and the ratings backup.
//! Filled in step 3; the report lands in reports/import-report.md.

use crate::library::Library;
use serde_json::{json, Value};
use std::path::Path;

pub struct ImportCounts {
    pub tracks_in: i64,
    pub tracks_out: i64,
    pub loved_in: i64,
    pub loved_out: i64,
    pub plays_in: i64,
    pub plays_out: i64,
}

pub fn run_import(_db_copy: &Path, _config: Option<&Path>, _lib: &Library) -> Result<Value, String> {
    Ok(json!({ "skipped": true, "reason": "import lands in step 3" }))
}

/// The one-time import, on first start with an empty library. Runs in a
/// worker thread; the real logic lands in step 3.
pub fn maybe_import(app: &crate::App) {
    let db = dirs::data_dir()
        .map(|d| d.join("Dopamine Enhanced").join("Dopamine.db"))
        .unwrap_or_default();
    if !db.exists() {
        return;
    }
    let lib = &app.lib;
    let already: i64 = {
        let conn = lib.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0)).unwrap_or(0)
    };
    let _ = already; // the real check arrives with the import itself
}
