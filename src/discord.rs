//! Discord rich presence: title, artist, album, artwork, progress.
//! Filled in step 5.

use serde_json::Value;

pub struct Discord {
    pub enabled: bool,
}

impl Discord {
    pub fn new(enabled: bool) -> Self {
        Discord { enabled }
    }

    pub fn update(&mut self, _state: &Value) {}

    pub fn clear(&mut self) {}

    pub fn shutdown(&mut self) {}
}
