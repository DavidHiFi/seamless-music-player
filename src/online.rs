//! Online: slskd REST search + streams and hidden yt-dlp downloads, with the
//! same behaviour as Dopamine's music-search.service.ts. Filled in step 5.

use serde_json::{json, Value};

pub struct Online {
    pub jobs: std::sync::Mutex<Vec<Value>>,
}

impl Online {
    pub fn new() -> Self {
        Online { jobs: std::sync::Mutex::new(Vec::new()) }
    }

    pub fn search(&self, _q: &str) {}

    pub fn stream(&self, _result_id: &str) -> Value {
        json!({ "ok": false })
    }

    pub fn download(&self, _result_id: &str) -> Value {
        json!({ "ok": false })
    }

    pub fn cancel(&self, _job_id: &str) {}
}
