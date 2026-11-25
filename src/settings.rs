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
        // The settings page names keys by group ("behaviour.gapless"). The ones
        // the backend acts on land on their field; the rest nest in extra.
        let value = match key {
            "appearance.fontWeight" => match &value {
                Value::Number(n) => Value::String(n.to_string()),
                _ => value,
            },
            "online.slskdAddress" => match value.as_str() {
                Some(a) => Value::String(slskd_url_from_address(a)),
                None => value,
            },
            _ => value,
        };
        if key == "behaviour.scrobble" {
            let on = value.as_bool().unwrap_or(false);
            self.scrobble_lastfm = on;
            self.scrobble_listenbrainz = on;
            return true;
        }
        if let Some(field) = page_alias(key) {
            return self.set_key(field, value);
        }
        if let Some((group, name)) = key.split_once('.') {
            if PAGE_GROUPS.contains(&group) && !name.is_empty() && !name.contains('.') {
                let g = self
                    .extra
                    .entry(group.to_string())
                    .or_insert_with(|| Value::Object(Map::new()));
                if !g.is_object() {
                    *g = Value::Object(Map::new());
                }
                g.as_object_mut().unwrap().insert(name.to_string(), value);
                return true;
            }
        }
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

    /// What the page sees: the stored fields plus the grouped view the
    /// settings screen reads, with every secret left out.
    pub fn to_json(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        let Some(o) = v.as_object_mut() else { return v };
        for k in SECRET_KEYS {
            o.remove(*k);
        }
        let group = |name: &str| -> Map<String, Value> {
            self.extra.get(name).and_then(Value::as_object).cloned().unwrap_or_default()
        };
        let mut appearance = group("appearance");
        appearance.insert("fontFamily".into(), self.font_family.clone().into());
        let weight = self
            .font_weight
            .parse::<f64>()
            .map(Value::from)
            .unwrap_or_else(|_| self.font_weight.clone().into());
        appearance.insert("fontWeight".into(), weight);
        appearance.insert("blur".into(), self.blur_amount.into());
        let mut behaviour = group("behaviour");
        behaviour.insert("gapless".into(), self.gapless.into());
        behaviour.insert("crossfade".into(), self.crossfade_seconds.into());
        behaviour.insert("scrobble".into(), (self.scrobble_lastfm || self.scrobble_listenbrainz).into());
        let mut eq = group("eq");
        eq.insert("enabled".into(), self.eq_enabled.into());
        eq.insert("preset".into(), self.eq_preset.clone().into());
        eq.insert("gains".into(), serde_json::to_value(&self.eq_gains).unwrap_or_default());
        let mut online = group("online");
        if let Some(d) = &self.downloads_folder {
            online.insert("downloadsFolder".into(), d.clone().into());
        }
        online.insert("slskdAddress".into(), slskd_address_from_url(&self.slskd_url).into());
        let mut advanced = group("advanced");
        if let Some(d) = &self.device_id {
            advanced.insert("outputDevice".into(), d.clone().into());
        }
        o.insert("appearance".into(), Value::Object(appearance));
        o.insert("behaviour".into(), Value::Object(behaviour));
        o.insert("eq".into(), Value::Object(eq));
        o.insert("online".into(), Value::Object(online));
        o.insert("advanced".into(), Value::Object(advanced));
        o.insert("motion".into(), Value::Object(group("motion")));
        v
    }

    /// A true/false switch the page keeps under a group, like
    /// ("behaviour", "rememberPlayback"). Missing means `default`.
    pub fn page_flag(&self, group: &str, name: &str, default: bool) -> bool {
        self.extra
            .get(group)
            .and_then(|g| g.get(name))
            .and_then(Value::as_bool)
            .unwrap_or(default)
    }

    /// Earlier builds stored page keys flat ("motion.enabled" in extra).
    /// Move them onto their field or into their group.
    fn migrate_flat_keys(&mut self) {
        let flat: Vec<String> = self.extra.keys().filter(|k| k.contains('.')).cloned().collect();
        for k in flat {
            if let Some(v) = self.extra.remove(&k) {
                if !self.set_key(&k, v) {
                    self.extra.remove(&k);
                }
            }
        }
    }
}

/// Groups the settings screen keeps its own switches under.
const PAGE_GROUPS: &[&str] = &["appearance", "behaviour", "eq", "online", "advanced", "motion"];

/// Never sent to the page: tokens, sessions and the Last.fm app key pair.
const SECRET_KEYS: &[&str] = &[
    "listenbrainz_token",
    "lastfm_key",
    "lastfm_session",
    "lastfm_api_key",
    "lastfm_api_secret",
];

/// Page keys that are really a backend field.
fn page_alias(key: &str) -> Option<&'static str> {
    Some(match key {
        "behaviour.gapless" => "gapless",
        "behaviour.crossfade" => "crossfade_seconds",
        "appearance.fontFamily" => "font_family",
        "appearance.fontWeight" => "font_weight",
        "appearance.blur" => "blur_amount",
        "eq.enabled" => "eq_enabled",
        "eq.preset" => "eq_preset",
        "eq.gains" => "eq_gains",
        "online.downloadsFolder" => "downloads_folder",
        "online.slskdAddress" => "slskd_url",
        "advanced.outputDevice" => "device_id",
        _ => return None,
    })
}

/// "127.0.0.1:5030" to the slskd API base the backend calls.
fn slskd_url_from_address(addr: &str) -> String {
    let a = addr.trim().trim_end_matches('/');
    if a.starts_with("http://") || a.starts_with("https://") {
        if a.ends_with("/api/v0") {
            a.to_string()
        } else {
            format!("{a}/api/v0")
        }
    } else {
        format!("http://{a}/api/v0")
    }
}

/// The API base back to the short address the settings screen shows.
fn slskd_address_from_url(url: &str) -> String {
    url.trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .trim_end_matches("/api/v0")
        .to_string()
}

pub struct SettingsFile {
    path: PathBuf,
    pub settings: Mutex<Settings>,
}

impl SettingsFile {
    pub fn load(path: PathBuf) -> Self {
        let mut settings: Settings = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        settings.migrate_flat_keys();
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn page_keys_reach_their_fields() {
        let mut s = Settings::default();
        assert!(s.set_key("behaviour.crossfade", json!(5)));
        assert!(s.set_key("appearance.fontWeight", json!(600)));
        assert!(s.set_key("online.slskdAddress", json!("127.0.0.1:5031")));
        assert!(s.set_key("behaviour.scrobble", json!(true)));
        assert_eq!(s.crossfade_seconds, 5.0);
        assert_eq!(s.font_weight, "600");
        assert_eq!(s.slskd_url, "http://127.0.0.1:5031/api/v0");
        assert!(s.scrobble_lastfm && s.scrobble_listenbrainz);
        let v = s.to_json();
        assert_eq!(v["behaviour"]["crossfade"], json!(5.0));
        assert_eq!(v["online"]["slskdAddress"], json!("127.0.0.1:5031"));
        assert_eq!(v["appearance"]["fontWeight"], json!(600.0));
    }

    #[test]
    fn other_page_keys_nest_and_read_back() {
        let mut s = Settings::default();
        assert!(s.set_key("behaviour.rememberPlayback", json!(false)));
        assert!(s.set_key("motion.surfaces", json!({"toasts": false})));
        assert!(!s.page_flag("behaviour", "rememberPlayback", true));
        assert!(s.page_flag("behaviour", "showArtwork", true));
        assert_eq!(s.to_json()["motion"]["surfaces"]["toasts"], json!(false));
    }

    #[test]
    fn secrets_never_reach_the_page() {
        let mut s = Settings::default();
        s.listenbrainz_token = "tok-1".into();
        s.extra.insert("lastfm_session".into(), json!("sess-2"));
        s.extra.insert("lastfm_api_secret".into(), json!("sec-3"));
        let text = s.to_json().to_string();
        assert!(!text.contains("tok-1") && !text.contains("sess-2") && !text.contains("sec-3"));
    }

    #[test]
    fn old_flat_keys_move_into_groups() {
        let mut s = Settings::default();
        s.extra.insert("motion.enabled".into(), json!(false));
        s.extra.insert("behaviour.gapless".into(), json!(false));
        s.migrate_flat_keys();
        assert!(!s.extra.contains_key("motion.enabled"));
        assert!(!s.page_flag("motion", "enabled", true));
        assert!(!s.gapless);
    }
}
