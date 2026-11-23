//! Settings: one JSON file in the app data folder, every key with a default.
//! Keys the backend does not model are kept in `extra` so they survive a
//! save, and the Dopamine import can drop any key in there for the UI.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The app data folder. The SMP_DATA_DIR env var overrides it, which the
/// --selftest flag and cargo tests use to keep the owner's data untouched.
pub fn app_dir() -> PathBuf {
    if let Ok(d) = std::env::var("SMP_DATA_DIR") {
        return PathBuf::from(d);
    }
    dirs::data_dir()
        .unwrap_or_else(|| std::env::var("APPDATA").map(PathBuf::from).unwrap_or_default())
        .join("Seamless Music Player")
}

pub fn db_path() -> PathBuf {
    app_dir().join("library.db")
}

pub fn settings_path() -> PathBuf {
    app_dir().join("settings.json")
}

pub fn art_dir() -> PathBuf {
    app_dir().join("art")
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub maximized: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub window: Option<WindowRect>,
    // Playback
    pub volume: f64,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub gapless: bool,
    pub crossfade_seconds: f64,
    pub replaygain: String,
    pub preamp_db: f64,
    pub eq_enabled: bool,
    pub eq_preset: String,
    pub eq_gains: Vec<f64>,
    pub speed: f64,
    // Queue state, remembered across restarts
    pub shuffle: bool,
    pub loop_mode: String,
    pub last_queue: Vec<i64>,
    pub last_index: usize,
    pub last_track: Option<i64>,
    pub last_position: f64,
    pub was_playing: bool,
    // Sleep timer, minutes; null when off
    pub sleep_timer_minutes: Option<f64>,
    // Library
    pub folders: Vec<String>,
    pub downloads_folder: Option<String>,
    // Window behaviour
    pub close_to_tray: bool,
    pub minimize_to_tray: bool,
    pub start_hidden: bool,
    // Appearance
    pub font_family: String,
    pub font_weight: String,
    pub blur_amount: f64,
    pub visualizer: bool,
    // Online
    pub slskd_url: String,
    pub yt_dlp_path: Option<String>,
    // Scrobbling
    pub scrobble_lastfm: bool,
    pub scrobble_listenbrainz: bool,
    pub lastfm_user: String,
    #[serde(skip_serializing)]
    pub lastfm_key: String,
    pub listenbrainz_token: String,
    // Discord
    pub discord_enabled: bool,
    // Keys we do not model: kept for the UI and the import.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            window: None,
            volume: 0.5,
            device_id: None,
            device_name: None,
            gapless: true,
            crossfade_seconds: 0.0,
            replaygain: "off".into(),
            preamp_db: 0.0,
            eq_enabled: false,
            eq_preset: "Flat".into(),
            eq_gains: vec![0.0; 10],
            speed: 1.0,
            shuffle: false,
            loop_mode: "off".into(),
            last_queue: Vec::new(),
            last_index: 0,
            last_track: None,
            last_position: 0.0,
            was_playing: false,
            sleep_timer_minutes: None,
            folders: Vec::new(),
            downloads_folder: None,
            close_to_tray: false,
            minimize_to_tray: false,
            start_hidden: false,
            font_family: "FiraCode Nerd Font".into(),
            font_weight: "500".into(),
            blur_amount: 30.0,
            visualizer: true,
            slskd_url: "http://127.0.0.1:5030/api/v0".into(),
            yt_dlp_path: None,
            scrobble_lastfm: false,
            scrobble_listenbrainz: false,
            lastfm_user: String::new(),
            lastfm_key: String::new(),
            listenbrainz_token: String::new(),
            discord_enabled: false,
            extra: Map::new(),
        }
    }
}

impl Settings {
    /// Set one key by name. Known keys map onto fields; anything else lands
    /// in `extra`. Returns false when the key is unknown, so the caller can
    /// say so instead of silently storing it.
    pub fn set_key(&mut self, key: &str, value: Value) -> bool {
        match key {
            "volume" => {
                if let Some(v) = value.as_f64() { self.volume = v.clamp(0.0, 1.0) }
            }
            "device_id" => self.device_id = value.as_str().map(String::from),
            "device_name" => self.device_name = value.as_str().map(String::from),
            "gapless" => self.gapless = value.as_bool().unwrap_or(true),
            "crossfade_seconds" => {
                if let Some(v) = value.as_f64() { self.crossfade_seconds = v.clamp(0.0, 12.0) }
            }
            "replaygain" => {
                if let Some(v) = value.as_str() { self.replaygain = v.to_string() }
            }
            "preamp_db" => {
                if let Some(v) = value.as_f64() { self.preamp_db = v.clamp(-15.0, 15.0) }
            }
            "eq_enabled" => self.eq_enabled = value.as_bool().unwrap_or(false),
            "eq_preset" => {
                if let Some(v) = value.as_str() { self.eq_preset = v.to_string() }
            }
            "eq_gains" => {
                if let Value::Array(a) = value {
                    let g: Vec<f64> = a.iter().filter_map(|v| v.as_f64()).collect();
                    if g.len() == 10 {
                        self.eq_gains = g;
                    }
                }
            }
            "speed" => {
                if let Some(v) = value.as_f64() { self.speed = v.clamp(0.5, 2.0) }
            }
            "shuffle" => self.shuffle = value.as_bool().unwrap_or(false),
            "loop_mode" => {
                if let Some(v) = value.as_str() {
                    if matches!(v, "off" | "all" | "one") { self.loop_mode = v.to_string() }
                }
            }
            "sleep_timer_minutes" => self.sleep_timer_minutes = value.as_f64(),
            "folders" => {
                if let Value::Array(a) = value {
                    self.folders = a.iter().filter_map(|v| v.as_str().map(String::from)).collect();
                }
            }
            "downloads_folder" => self.downloads_folder = value.as_str().map(String::from),
            "close_to_tray" => self.close_to_tray = value.as_bool().unwrap_or(true),
            "minimize_to_tray" => self.minimize_to_tray = value.as_bool().unwrap_or(false),
            "start_hidden" => self.start_hidden = value.as_bool().unwrap_or(false),
            "font_family" => {
                if let Some(v) = value.as_str() { self.font_family = v.to_string() }
            }
            "font_weight" => {
                if let Some(v) = value.as_str() { self.font_weight = v.to_string() }
            }
            "blur_amount" => {
                if let Some(v) = value.as_f64() { self.blur_amount = v.clamp(0.0, 60.0) }
            }
            "visualizer" => self.visualizer = value.as_bool().unwrap_or(true),
            "slskd_url" => {
                if let Some(v) = value.as_str() { self.slskd_url = v.to_string() }
            }
            "yt_dlp_path" => self.yt_dlp_path = value.as_str().map(String::from),
            "scrobble_lastfm" => self.scrobble_lastfm = value.as_bool().unwrap_or(false),
            "scrobble_listenbrainz" => self.scrobble_listenbrainz = value.as_bool().unwrap_or(false),
            "lastfm_user" => {
                if let Some(v) = value.as_str() { self.lastfm_user = v.to_string() }
            }
            "lastfm_key" => {
                if let Some(v) = value.as_str() { self.lastfm_key = v.to_string() }
            }
            "listenbrainz_token" => {
                if let Some(v) = value.as_str() { self.listenbrainz_token = v.to_string() }
            }
            "discord_enabled" => self.discord_enabled = value.as_bool().unwrap_or(false),
            "window" => {
                self.window = serde_json::from_value(value).ok();
            }
            _ => {
                self.extra.insert(key.to_string(), value);
                return false;
            }
        }
        true
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

pub struct SettingsFile {
    path: PathBuf,
    pub settings: Mutex<Settings>,
}

impl SettingsFile {
    pub fn load(path: PathBuf) -> Self {
        let settings = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        SettingsFile { path, settings: Mutex::new(settings) }
    }

    pub fn save(&self, s: &Settings) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(b) = serde_json::to_vec_pretty(s) {
            let tmp = self.path.with_extension("json.tmp");
            if fs::write(&tmp, &b).is_ok() {
                let _ = fs::rename(&tmp, &self.path);
            }
        }
    }

    pub fn update<F: FnOnce(&mut Settings)>(&self, f: F) -> Settings {
        let mut g = self.settings.lock().unwrap();
        f(&mut g);
        let clone = g.clone();
        self.save(&clone);
        clone
    }
}

/// Copy a file if the source exists, creating the target folder. Used by the
/// import for artwork and playlists.
pub fn copy_if_exists(from: &Path, to: &Path) -> Option<u64> {
    fs::copy(from, to).ok()
}
