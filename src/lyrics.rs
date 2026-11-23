//! Lyrics: embedded tags first, then LRCLIB online (synced when available).
//! Filled in step 5.

use serde_json::{json, Value};

pub fn get(_path: &str, title: &str, artist: &str, album: &str, duration: f64) -> Value {
    let _ = (title, artist, album, duration);
    json!({ "synced": null, "plain": null, "source": "none" })
}
