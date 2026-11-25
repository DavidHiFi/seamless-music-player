//! Scrobbling: Last.fm and ListenBrainz. Credentials come from the imported
//! settings and are read at send time, so the switch in Settings applies at
//! once. Nothing here prints or logs a key, a session or a token: failures
//! log the service name and the HTTP status only.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use md5::{Digest, Md5};
use serde_json::{json, Value};

use crate::settings::SettingsFile;

const LASTFM_URL: &str = "https://ws.audioscrobbler.com/2.0/";
const LISTENBRAINZ_URL: &str = "https://api.listenbrainz.org/1/submit-listens";
const CLIENT: &str = "Seamless Music Player";

enum Job {
    NowPlaying(Meta),
    Scrobble(Meta, u64),
    Love(Meta, bool),
}

#[derive(Clone)]
struct Meta {
    artist: String,
    title: String,
    album: String,
    duration: f64,
}

impl Meta {
    fn from_track(t: &Value) -> Option<Meta> {
        let artist = t["artists"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|a| a.as_str())
            .or_else(|| t["artist"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let title = t["title"].as_str().unwrap_or("").trim().to_string();
        if artist.is_empty() || title.is_empty() {
            return None;
        }
        Some(Meta {
            artist,
            title,
            album: t["album"].as_str().unwrap_or("").trim().to_string(),
            duration: t["duration"].as_f64().unwrap_or(0.0),
        })
    }
}

static SENDER: OnceLock<Sender<Job>> = OnceLock::new();

fn send(job: Job) {
    if let Some(tx) = SENDER.get() {
        let _ = tx.send(job);
    }
}

/// A track started playing.
pub fn now_playing(track: &Value) {
    if let Some(m) = Meta::from_track(track) {
        send(Job::NowPlaying(m));
    }
}

/// A track passed the play mark (half of it or four minutes). `elapsed` is how
/// long it played, used to work out when it started.
pub fn played(track: &Value, elapsed: f64) {
    let Some(m) = Meta::from_track(track) else { return };
    // Both services ignore tracks shorter than 30 seconds.
    if m.duration > 0.0 && m.duration < 30.0 {
        return;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let played_for = if m.duration > 0.0 { elapsed.min(m.duration) } else if elapsed < 86_400.0 { elapsed } else { 0.0 };
    let started = now.saturating_sub(played_for.max(0.0) as u64);
    send(Job::Scrobble(m, started));
}

pub struct Scrobbler;

impl Scrobbler {
    /// Starts the one network thread. Calls before this are dropped.
    pub fn new(settings: Arc<SettingsFile>) -> Self {
        let (tx, rx) = channel::<Job>();
        if SENDER.set(tx).is_ok() {
            let _ = std::thread::Builder::new().name("scrobble".into()).spawn(move || {
                let agent = ureq::AgentBuilder::new()
                    .timeout(Duration::from_secs(15))
                    .user_agent(concat!("SeamlessMusicPlayer/", env!("CARGO_PKG_VERSION")))
                    .build();
                while let Ok(job) = rx.recv() {
                    let creds = Creds::read(&settings);
                    run(&agent, &creds, job);
                }
            });
        }
        Scrobbler
    }

    pub fn love(&self, track: &Value, love: bool) {
        if let Some(m) = Meta::from_track(track) {
            send(Job::Love(m, love));
        }
    }
}

struct Creds {
    lastfm_on: bool,
    lastfm_key: String,
    lastfm_secret: String,
    lastfm_session: String,
    lb_on: bool,
    lb_token: String,
}

impl Creds {
    fn read(settings: &SettingsFile) -> Creds {
        let s = settings.settings.lock().unwrap();
        let extra = |k: &str| s.extra.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mut session = extra("lastfm_session");
        if session.is_empty() {
            session = s.lastfm_key.clone();
        }
        Creds {
            lastfm_on: s.scrobble_lastfm,
            lastfm_key: extra("lastfm_api_key"),
            lastfm_secret: extra("lastfm_api_secret"),
            lastfm_session: session,
            lb_on: s.scrobble_listenbrainz,
            lb_token: s.listenbrainz_token.clone(),
        }
    }

    fn lastfm_ready(&self) -> bool {
        self.lastfm_on
            && !self.lastfm_key.is_empty()
            && !self.lastfm_secret.is_empty()
            && !self.lastfm_session.is_empty()
    }

    fn lb_ready(&self) -> bool {
        self.lb_on && !self.lb_token.is_empty()
    }
}

fn run(agent: &ureq::Agent, c: &Creds, job: Job) {
    match job {
        Job::NowPlaying(m) => {
            if c.lastfm_ready() {
                let mut p = base_params("track.updateNowPlaying", &m);
                lastfm_post(agent, c, &mut p, "now playing");
            }
            if c.lb_ready() {
                lb_post(agent, c, "playing_now", &m, None);
            }
        }
        Job::Scrobble(m, started) => {
            if c.lastfm_ready() {
                let mut p = base_params("track.scrobble", &m);
                p.push(("timestamp".into(), started.to_string()));
                lastfm_post(agent, c, &mut p, "scrobble");
            }
            if c.lb_ready() {
                lb_post(agent, c, "single", &m, Some(started));
            }
        }
        Job::Love(m, love) => {
            // ListenBrainz loves need a MusicBrainz id, which local tags rarely
            // carry, so loves go to Last.fm only (same as Dopamine).
            if c.lastfm_ready() {
                let method = if love { "track.love" } else { "track.unlove" };
                let mut p = vec![
                    ("method".to_string(), method.to_string()),
                    ("artist".to_string(), m.artist.clone()),
                    ("track".to_string(), m.title.clone()),
                ];
                lastfm_post(agent, c, &mut p, "love");
            }
        }
    }
}

fn base_params(method: &str, m: &Meta) -> Vec<(String, String)> {
    let mut p = vec![
        ("method".to_string(), method.to_string()),
        ("artist".to_string(), m.artist.clone()),
        ("track".to_string(), m.title.clone()),
    ];
    if !m.album.is_empty() {
        p.push(("album".into(), m.album.clone()));
    }
    if m.duration > 0.0 {
        p.push(("duration".into(), (m.duration.round() as u64).to_string()));
    }
    p
}

/// Last.fm signature: every parameter except `format`, sorted by name, written
/// as name then value, the shared secret on the end, md5 in lower-case hex.
fn lastfm_sig(params: &[(String, String)], secret: &str) -> String {
    let mut sorted: Vec<&(String, String)> = params.iter().filter(|(k, _)| k != "format").collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = Md5::new();
    for (k, v) in sorted {
        h.update(k.as_bytes());
        h.update(v.as_bytes());
    }
    h.update(secret.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn lastfm_post(agent: &ureq::Agent, c: &Creds, p: &mut Vec<(String, String)>, what: &str) {
    p.push(("api_key".into(), c.lastfm_key.clone()));
    p.push(("sk".into(), c.lastfm_session.clone()));
    let sig = lastfm_sig(p, &c.lastfm_secret);
    p.push(("api_sig".into(), sig));
    p.push(("format".into(), "json".into()));
    let form: Vec<(&str, &str)> = p.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    match agent.post(LASTFM_URL).send_form(&form) {
        Ok(r) => {
            // Last.fm can answer 200 with an error body.
            if let Ok(v) = r.into_json::<Value>() {
                if let Some(code) = v["error"].as_i64() {
                    eprintln!("last.fm {what}: refused, error {code}");
                }
            }
        }
        Err(ureq::Error::Status(code, _)) => eprintln!("last.fm {what}: http {code}"),
        Err(_) => eprintln!("last.fm {what}: no connection"),
    }
}

fn lb_post(agent: &ureq::Agent, c: &Creds, kind: &str, m: &Meta, started: Option<u64>) {
    let mut meta = json!({
        "artist_name": m.artist,
        "track_name": m.title,
        "additional_info": {
            "submission_client": CLIENT,
            "submission_client_version": env!("CARGO_PKG_VERSION"),
        }
    });
    if !m.album.is_empty() {
        meta["release_name"] = json!(m.album);
    }
    if m.duration > 0.0 {
        meta["additional_info"]["duration_ms"] = json!((m.duration * 1000.0).round() as u64);
    }
    let mut listen = json!({ "track_metadata": meta });
    if let Some(ts) = started {
        listen["listened_at"] = json!(ts);
    }
    let body = json!({ "listen_type": kind, "payload": [listen] });
    let auth = format!("Token {}", c.lb_token);
    match agent.post(LISTENBRAINZ_URL).set("Authorization", &auth).send_json(body) {
        Ok(_) => {}
        Err(ureq::Error::Status(code, _)) => eprintln!("listenbrainz {kind}: http {code}"),
        Err(_) => eprintln!("listenbrainz {kind}: no connection"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_sorts_and_skips_format() {
        let p = vec![
            ("track".to_string(), "b".to_string()),
            ("artist".to_string(), "a".to_string()),
            ("format".to_string(), "json".to_string()),
        ];
        // md5("artistatrackbs")
        let mut h = Md5::new();
        h.update(b"artistatrackbs");
        let want: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(lastfm_sig(&p, "s"), want);
    }

    #[test]
    fn meta_needs_artist_and_title() {
        assert!(Meta::from_track(&json!({"title": "x"})).is_none());
        let m = Meta::from_track(&json!({"title": "x", "artists": ["y", "z"], "duration": 200.0})).unwrap();
        assert_eq!(m.artist, "y");
        assert_eq!(m.duration, 200.0);
    }
}
