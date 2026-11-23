//! Scrobbling: Last.fm and ListenBrainz. Credentials come from the imported
//! settings; they are never printed or logged. Filled in step 5.

use serde_json::Value;

pub struct Scrobbler;

impl Scrobbler {
    pub fn new() -> Self {
        Scrobbler
    }

    /// Call when a track starts (now playing) and when the scrobble threshold
    /// is passed (submit).
    pub fn now_playing(&self, _track: &Value) {}

    pub fn submit(&self, _track: &Value) {}

    pub fn love(&self, _track: &Value, _love: bool) {}

    pub fn shutdown(&self) {}
}
