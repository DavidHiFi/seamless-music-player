//! SMTC: Windows System Media Transport Controls. Pushes now-playing
//! metadata so the keyboard media keys and the Windows flyout work.
//! Filled in step 5.

use serde_json::Value;

pub struct Smtc;

impl Smtc {
    pub fn new() -> Self {
        Smtc
    }

    pub fn update(&self, _state: &Value) {}

    pub fn shutdown(&self) {}
}
