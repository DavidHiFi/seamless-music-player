//! Shared bits for the online work: the slskd REST client (login read from
//! slskd.yml at run time, never copied anywhere) and the hidden yt-dlp child
//! (CREATE_NO_WINDOW on every spawn). Ported from Dopamine's
//! music-search.service.ts so the behaviour matches line for line.

use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

// ---- slskd REST ----

pub const SLSKD_CONFIG_PATH: &str = "C:\\ProgramData\\slskd\\data\\slskd.yml";
pub const API_HOST: &str = "127.0.0.1";
pub const API_PORT: u16 = 5030;

pub struct Slskd {
    token: std::sync::Mutex<String>,
}

impl Slskd {
    pub fn new() -> Self {
        Slskd { token: std::sync::Mutex::new(String::new()) }
    }

    /// The login, straight from slskd.yml. Nothing is written anywhere.
    pub fn read_login() -> (String, String) {
        let Ok(body) = std::fs::read_to_string(SLSKD_CONFIG_PATH) else {
            return (String::new(), String::new());
        };
        let lines: Vec<&str> = body.lines().collect();
        let start = lines.iter().position(|l| l.trim_start().starts_with("authentication:"));
        let value = |name: &str| -> String {
            let from = start.map(|s| s + 1).unwrap_or(0);
            for line in lines.iter().skip(from).take(6) {
                if line.trim_start().starts_with(&format!("{name}:")) {
                    return yaml_value(line);
                }
            }
            String::new()
        };
        (value("username"), value("password"))
    }

    /// The download and incomplete folders from slskd.yml, with Dopamine's
    /// fallbacks so a fresh setup behaves the same way.
    pub fn read_folders() -> (String, String) {
        let Ok(body) = std::fs::read_to_string(SLSKD_CONFIG_PATH) else {
            return (
                "G:\\Music\\Downloads".into(),
                "G:\\Music Downloads\\Incomplete".into(),
            );
        };
        let value = |name: &str, fallback: &str| -> String {
            for line in body.lines() {
                if line.trim_start().starts_with(&format!("{name}:")) {
                    let v = yaml_value(line);
                    return if v.is_empty() { fallback.into() } else { v };
                }
            }
            fallback.into()
        };
        (
            value("downloads", "G:\\Music\\Downloads"),
            value("incomplete", "G:\\Music Downloads\\Incomplete"),
        )
    }

    fn sign_in(&self) -> Result<String, String> {
        let (username, password) = Self::read_login();
        let body = json!({ "username": username, "password": password });
        let v = request("POST", "/session", Some(&body), "")?;
        let token = v["token"].as_str().unwrap_or("").to_string();
        if token.is_empty() {
            return Err("signin".into());
        }
        Ok(token)
    }

    fn token(&self) -> Result<String, String> {
        let mut t = self.token.lock().unwrap();
        if t.is_empty() {
            *t = self.sign_in()?;
        }
        Ok(t.clone())
    }

    fn drop_token(&self) {
        self.token.lock().unwrap().clear();
    }

    /// One REST call, retrying once after a 401 with a fresh sign-in.
    pub fn api(&self, method: &str, route: &str, body: Option<&Value>) -> Result<Value, String> {
        let token = self.token()?;
        match request(method, route, body, &token) {
            Ok(v) => Ok(v),
            Err(e) if e == "401" => {
                self.drop_token();
                let fresh = self.token()?;
                request(method, route, body, &fresh)
            }
            Err(e) => Err(e),
        }
    }
}

impl Default for Slskd {
    fn default() -> Self {
        Self::new()
    }
}

/// One HTTP call to the local slskd API. Non-2xx comes back as the status
/// number as the error string, like the TS version's reject({ status }).
fn request(method: &str, route: &str, body: Option<&Value>, token: &str) -> Result<Value, String> {
    let url = format!("http://{API_HOST}:{API_PORT}/api/v0{route}");
    let agent = agent();
    let mut req = match method {
        "GET" => agent.get(&url),
        "POST" => agent.post(&url),
        "DELETE" => agent.delete(&url),
        other => return Err(format!("unknown method {other}")),
    };
    req = req.set("Accept", "application/json");
    if !token.is_empty() {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    let mut resp = match body {
        Some(b) => req.send_json(b.clone()),
        None => req.call(),
    }
    .map_err(|e| {
        match e {
            ureq::Error::Status(code, _) => code.to_string(),
            other => other.to_string(),
        }
    })?;
    let text = resp.into_string().unwrap_or_default();
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    Ok(serde_json::from_str(&text).unwrap_or(json!({})))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()
}

fn yaml_value(line: &str) -> String {
    let v = line.split_once(':').map(|(_, v)| v.trim()).unwrap_or("");
    let quoted = v.len() >= 2
        && ((v.starts_with('\'') && v.ends_with('\'')) || (v.starts_with('"') && v.ends_with('"')));
    if quoted {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

// ---- yt-dlp ----

/// yt-dlp: the WinGet link first, then PATH.
pub fn yt_dlp_path() -> String {
    let links = std::env::var("LOCALAPPDATA")
        .map(|l| PathBuf::from(l).join("Microsoft").join("WinGet").join("Links").join("yt-dlp.exe"))
        .unwrap_or_default();
    if links.exists() {
        links.to_string_lossy().to_string()
    } else {
        "yt-dlp.exe".into()
    }
}

/// Spawn any child program hidden. CREATE_NO_WINDOW so a console never
/// flashes on the owner's screen.
pub fn spawn_hidden(program: &str, args: &[String]) -> std::io::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(program)
        .args(args)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
}

/// yt-dlp reads "@value" as plain text and copies it into the tag; % and :
/// are escaped because yt-dlp treats them as a field reference and the field
/// separator. Same as the TS tagArgs().
pub fn tag_args(tags: &Value) -> Vec<String> {
    let mut args = Vec::new();
    for field in ["artist", "title", "album"] {
        let text = tags[field]
            .as_str()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join(" ");
        if !text.is_empty() {
            args.push("--parse-metadata".into());
            args.push(format!(
                "@{}:@(?P<meta_{field}>.+)",
                text.replace('%', "%%").replace(':', "\\:")
            ));
        }
    }
    args
}

// ---- name parsing, ported from music-search.service.ts ----

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "flac", "mp3", "m4a", "aac", "ogg", "opus", "wav", "wma", "ape", "aiff", "aif", "alac", "wv",
];
pub const LOSSLESS_EXTENSIONS: &[&str] = &["flac", "wav", "ape", "aiff", "aif", "alac", "wv"];

pub fn is_audio_file(filename: &str) -> bool {
    ext_of(filename).is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.as_str()))
}

pub fn is_lossless(filename: &str) -> bool {
    ext_of(filename).is_some_and(|e| LOSSLESS_EXTENSIONS.contains(&e.as_str()))
}

fn ext_of(filename: &str) -> Option<String> {
    let name = filename.rsplit('\\').next().unwrap_or(filename);
    let e = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    Some(e)
}

/// normalize(): lowercase, strip accents, non a-z0-9 to spaces. With forKey,
/// bracketed bits (tags like [FLAC]) drop out first.
pub fn normalize(text: &str, for_key: bool) -> String {
    let mut s = text.to_lowercase();
    if for_key {
        let bracketed = regex::Regex::new(r"[\[\(][^\]\)]*[\]\)]").unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
        s = bracketed.replace_all(&s, " ").to_string();
    }
    let decomposed = unicode_normalization::UnicodeNormalization::nfd(s.chars());
    let folded: String = decomposed.filter(|c| !(*c as u32 >= 0x0300 && *c as u32 <= 0x036f)).collect();
    folded
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

/// parseName(): "artist - title" from the file name, album from the folder.
pub fn parse_name(filename: &str) -> (String, String, String) {
    let mut parts: Vec<&str> = filename.split('\\').filter(|p| !p.is_empty()).collect();
    let file = parts.pop().unwrap_or("");
    let base_path: Vec<&str> = parts.clone();
    let (f_artist, f_album) = parse_folder(&base_path);
    let stem = Path::new(file.rsplit('\\').next().unwrap_or(file))
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut base = stem.replace('_', " ");
    // Drop a leading track number, optionally with a CD marker.
    let prefix = regex::Regex::new(r"(?i)^\s*(cd\s*\d+\s*[-.]?\s*)?\d{1,3}\s*[-.)\]]?\s*")
        .unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
    base = prefix.replace(&base, "").trim().to_string();

    let pieces: Vec<String> = base
        .split(" - ")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty() && !p.chars().all(|c| c.is_ascii_digit()))
        .collect();
    if pieces.len() >= 2 {
        return (pieces[0].clone(), pieces[pieces.len() - 1].clone(), f_album);
    }
    (f_artist, pieces.first().cloned().unwrap_or(base), f_album)
}

/// parseFolder(): artist and album out of the peer folder path.
pub fn parse_folder(parts: &[&str]) -> (String, String) {
    let clean = |s: &str| -> String {
        let re = regex::Regex::new(
            r"(?i)[\[\(][^\]\)]*(flac|mp3|320|v0|web|cd|24|16|bit|khz)[^\]\)]*[\]\)]",
        )
        .unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
        re.replace_all(s, "").replace('_', " ").trim().to_string()
    };
    let mut work: Vec<&str> = parts.to_vec();
    let mut album_folder = clean(work.last().copied().unwrap_or(""));
    let cd = regex::Regex::new(r"(?i)^(cd|disc|disk)\s*\d+$")
        .unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
    if cd.is_match(&album_folder) && work.len() > 1 {
        album_folder = clean(work[work.len() - 2]);
        work.pop();
    }
    let year = regex::Regex::new(r"^\(?\d{4}\)?\s*-?\s*")
        .unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
    let pieces: Vec<&str> = album_folder.split(" - ").filter(|p| !p.is_empty()).collect();
    if pieces.len() >= 2 {
        let album = year
            .replace(&pieces[1..].join(" - "), "")
            .trim()
            .to_string();
        let album = if album.is_empty() { pieces[1].to_string() } else { album };
        return (pieces[0].to_string(), album);
    }
    let artist_folder = if work.len() > 1 { clean(work[work.len() - 2]) } else { String::new() };
    let generic =
        regex::Regex::new(r"(?i)^(music|mp3|flac|albums?|downloads?|shared?|complete|soulseek|[a-z]:)$")
            .unwrap_or_else(|_| regex::Regex::new(r"$^").unwrap());
    let artist = if generic.is_match(&artist_folder) { String::new() } else { artist_folder };
    let album = year.replace(&album_folder, "").trim().to_string();
    (artist, album)
}

/// qualityText(): "FLAC 16/44" or "MP3 320" or just the format name.
pub fn quality_text(extension: &str, raw: &Value) -> String {
    let name = extension.to_uppercase();
    let bit_depth = raw["bitDepth"].as_i64();
    let sample_rate = raw["sampleRate"].as_i64();
    if let (Some(d), Some(r)) = (bit_depth, sample_rate) {
        return format!("{name} {d}/{}", (r as f64 / 1000.0).round() as i64);
    }
    if let Some(br) = raw["bitRate"].as_i64() {
        return format!("{name} {br}");
    }
    name
}

pub fn duration_text(seconds: Option<f64>) -> String {
    let Some(s) = seconds.filter(|s| *s > 0.0) else { return String::new() };
    let s = s.round() as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// mostCommon() for album qualities.
pub fn most_common(values: &[String]) -> String {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for v in values {
        *counts.entry(v.as_str()).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(v, _)| v.to_string())
        .unwrap_or_default()
}
