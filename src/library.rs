//! Library: the SQLite database, the scanner, the folder watcher and every
//! library.*, track.* and playlists.* command. The schema is ours; the
//! Dopamine import (migrate.rs) fills it from a copy of the old database.

use crate::logic;
use crate::settings::{app_dir, art_dir};
use crate::Events;
use rusqlite::{params, Connection, Row};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct Library {
    pub conn: Mutex<Connection>,
    pub events: Arc<Events>,
    db_path: PathBuf,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS folders (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  path TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS tracks (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  path TEXT NOT NULL UNIQUE,
  title TEXT NOT NULL DEFAULT '',
  artists TEXT NOT NULL DEFAULT '[]',
  album_artists TEXT NOT NULL DEFAULT '[]',
  album TEXT NOT NULL DEFAULT '',
  album_key TEXT NOT NULL DEFAULT '',
  genres TEXT NOT NULL DEFAULT '[]',
  year INTEGER,
  track_no INTEGER,
  disc_no INTEGER,
  duration REAL NOT NULL DEFAULT 0,
  bitrate INTEGER NOT NULL DEFAULT 0,
  sample_rate INTEGER NOT NULL DEFAULT 0,
  rating INTEGER NOT NULL DEFAULT 0,
  love INTEGER NOT NULL DEFAULT 0,
  play_count INTEGER NOT NULL DEFAULT 0,
  skip_count INTEGER NOT NULL DEFAULT 0,
  date_added INTEGER,
  last_played INTEGER,
  artwork_id TEXT,
  mtime INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album_key);
CREATE INDEX IF NOT EXISTS idx_tracks_title ON tracks(title COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artists);
CREATE TABLE IF NOT EXISTS playlists (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS kv (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
";

/// Extensions the scanner accepts.
const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "m4a", "aac", "ogg", "oga", "opus", "wav", "aiff", "aif", "wma", "ape", "mpc", "wv",
];

pub fn now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl Library {
    pub fn open(db_path: &Path, events: Arc<Events>) -> Result<Library, String> {
        if let Some(p) = db_path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "synchronous", "NORMAL").ok();
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Library { conn: Mutex::new(conn), events, db_path: db_path.to_path_buf() })
    }

    pub fn path(&self) -> &Path {
        &self.db_path
    }

    // ---- rows ----

    /// The same row as a rusqlite mapper.
    fn track_row(row: &Row) -> rusqlite::Result<Value> {
        Ok(Self::row_to_track(row))
    }

    pub fn row_to_track(row: &Row) -> Value {
        let artists: Value =
            serde_json::from_str(&row.get::<_, String>("artists").unwrap_or_default()).unwrap_or(json!([]));
        let album_artists: Value =
            serde_json::from_str(&row.get::<_, String>("album_artists").unwrap_or_default()).unwrap_or(json!([]));
        let genres: Value =
            serde_json::from_str(&row.get::<_, String>("genres").unwrap_or_default()).unwrap_or(json!([]));
        json!({
            "id": row.get::<_, i64>("id").unwrap_or(0),
            "path": row.get::<_, String>("path").unwrap_or_default(),
            "title": row.get::<_, String>("title").unwrap_or_default(),
            "artists": artists,
            "albumArtists": album_artists,
            "album": row.get::<_, String>("album").unwrap_or_default(),
            "albumKey": row.get::<_, String>("album_key").unwrap_or_default(),
            "genres": genres,
            "year": row.get::<_, Option<i64>>("year").unwrap_or(None),
            "trackNo": row.get::<_, Option<i64>>("track_no").unwrap_or(None),
            "discNo": row.get::<_, Option<i64>>("disc_no").unwrap_or(None),
            "duration": row.get::<_, f64>("duration").unwrap_or(0.0),
            "bitrate": row.get::<_, i64>("bitrate").unwrap_or(0),
            "sampleRate": row.get::<_, i64>("sample_rate").unwrap_or(0),
            "rating": row.get::<_, i64>("rating").unwrap_or(0),
            "love": row.get::<_, i64>("love").unwrap_or(0),
            "playCount": row.get::<_, i64>("play_count").unwrap_or(0),
            "skipCount": row.get::<_, i64>("skip_count").unwrap_or(0),
            "dateAdded": row.get::<_, Option<i64>>("date_added").unwrap_or(None),
            "lastPlayed": row.get::<_, Option<i64>>("last_played").unwrap_or(None),
            "artworkId": row.get::<_, Option<String>>("artwork_id").unwrap_or(None),
        })
    }

    const TRACK_COLS: &str = "id, path, title, artists, album_artists, album, album_key, genres, \
        year, track_no, disc_no, duration, bitrate, sample_rate, rating, love, play_count, \
        skip_count, date_added, last_played, artwork_id, mtime";

    fn query_tracks(&self, where_sql: &str, p: &[&dyn rusqlite::ToSql], order: &str, offset: i64, limit: i64) -> (i64, Vec<Value>) {
        let conn = self.conn.lock().unwrap();
        let total: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM tracks {where_sql}"),
                p,
                |r| r.get(0),
            )
            .unwrap_or(0);
        let mut st = match conn.prepare(&format!(
            "SELECT {} FROM tracks {where_sql} ORDER BY {order} LIMIT ?{} OFFSET ?{}",
            Self::TRACK_COLS,
            p.len() + 1,
            p.len() + 2
        )) {
            Ok(s) => s,
            Err(_) => return (total, Vec::new()),
        };
        let mut all: Vec<&dyn rusqlite::ToSql> = p.to_vec();
        all.push(&limit);
        all.push(&offset);
        let rows: Vec<Value> = match st.query_map(all.as_slice(), Self::track_row) {
            Ok(r) => r.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        };
        (total, rows)
    }

    pub fn track_by_id(&self, id: i64) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn
            .prepare(&format!("SELECT {} FROM tracks WHERE id = ?1", Self::TRACK_COLS))
            .ok()?;
        st.query_row(params![id], Self::track_row).ok()
    }

    pub fn tracks_by_ids(&self, ids: &[i64]) -> Vec<Value> {
        let conn = self.conn.lock().unwrap();
        let mut out = Vec::new();
        for id in ids {
            if let Ok(mut st) = conn.prepare(&format!("SELECT {} FROM tracks WHERE id = ?1", Self::TRACK_COLS)) {
                if let Ok(v) = st.query_row(params![id], Self::track_row) {
                    out.push(v);
                }
            }
        }
        out
    }

    fn ids_by_where(&self, where_sql: &str, p: &[&dyn rusqlite::ToSql], order: &str) -> Vec<i64> {
        let conn = self.conn.lock().unwrap();
        let mut st = match conn.prepare(&format!("SELECT id FROM tracks {where_sql} ORDER BY {order}")) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = st.query_map(p, |r| r.get::<_, i64>(0)).map(|it| it.collect::<Vec<_>>()).unwrap_or_default();
        rows.into_iter().filter_map(|r| r.ok()).collect()
    }

    // ---- overview / listing ----

    pub fn overview(&self) -> Value {
        // The counts take the connection lock one at a time; holding it
        // across the distinct counters deadlocks, because they lock too.
        let counts = {
            let conn = self.conn.lock().unwrap();
            let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap_or(0) };
            (
                count("SELECT COUNT(*) FROM tracks"),
                count("SELECT COUNT(DISTINCT album_key) FROM tracks WHERE album_key != ''"),
                count("SELECT COUNT(*) FROM folders"),
                self.playlist_files().len() as i64,
            )
        };
        let artists = self.count_distinct("artists");
        let genres = self.count_distinct("genres");
        json!({
            "tracks": counts.0,
            "albums": counts.1,
            "artists": artists,
            "genres": genres,
            "playlists": counts.3,
            "folders": counts.2,
        })
    }

    /// Distinct names in a JSON-array column.
    fn collect_distinct(&self, col: &str) -> HashMap<String, i64> {
        let conn = self.conn.lock().unwrap();
        let mut st = match conn.prepare(&format!("SELECT {col} FROM tracks")) {
            Ok(s) => s,
            Err(_) => return HashMap::new(),
        };
        let rows = st.query_map([], |r| r.get::<_, String>(0)).map(|it| it.collect::<Vec<_>>()).unwrap_or_default();
        let mut counts: HashMap<String, i64> = HashMap::new();
        for row in rows.into_iter().flatten() {
            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&row) {
                for v in arr {
                    if let Some(name) = v.as_str() {
                        let key = name.to_lowercase();
                        *counts.entry(key).or_insert(0) += 1;
                    }
                }
            }
        }
        counts
    }

    fn count_distinct(&self, col: &str) -> i64 {
        self.collect_distinct(col).len() as i64
    }

    /// The sort ORDER BY for the songs table. `sort` is a key, optionally with
    /// `:desc` on the end to turn the main column round (ties keep their order).
    fn track_order(sort: &str) -> String {
        let (key, dir) = sort.split_once(':').unwrap_or((sort, "asc"));
        let (main, main_desc, rest) = match key {
            "artist" => ("artists COLLATE NOCASE", false, "album COLLATE NOCASE, disc_no, track_no, title COLLATE NOCASE"),
            "album" => ("album COLLATE NOCASE", false, "disc_no, track_no, title COLLATE NOCASE"),
            "year" => ("year", false, "album COLLATE NOCASE, disc_no, track_no"),
            "dateAdded" => ("date_added", true, "id DESC"),
            "datePlayed" => ("last_played", true, "id DESC"),
            "rating" => ("rating", true, "title COLLATE NOCASE"),
            "playCount" => ("play_count", true, "title COLLATE NOCASE"),
            "duration" => ("duration", false, "title COLLATE NOCASE"),
            "path" => ("path COLLATE NOCASE", false, "id"),
            _ => ("title COLLATE NOCASE", false, "album COLLATE NOCASE, disc_no, track_no"),
        };
        let desc = main_desc != (dir == "desc");
        format!("{main}{}, {rest}", if desc { " DESC" } else { "" })
    }

    pub fn tracks(&self, sort: &str, filter: Option<&str>, offset: i64, limit: i64) -> Value {
        let (total, items) = match filter {
            Some(f) if !f.trim().is_empty() => {
                let like = format!("%{}%", f.trim().replace('%', "").replace('_', ""));
                self.query_tracks(
                    "WHERE title LIKE ?1 OR artists LIKE ?1 OR album LIKE ?1 OR genres LIKE ?1",
                    &[&like],
                    &Self::track_order(sort),
                    offset,
                    limit,
                )
            }
            _ => self.query_tracks("", &[], &Self::track_order(sort), offset, limit),
        };
        json!({ "total": total, "items": items })
    }

    /// Album rows grouped in SQL, shaped in Rust.
    fn album_rows(&self, artist: Option<&str>, genre: Option<&str>) -> Vec<Value> {
        let mut conds = Vec::new();
        let mut binds: Vec<String> = Vec::new();
        if let Some(a) = artist {
            conds.push("(album_artists LIKE ? OR artists LIKE ?)");
            let like = format!("%\"{}\"%", a.replace('\"', ""));
            binds.push(like.clone());
            binds.push(like);
        }
        if let Some(g) = genre {
            conds.push("genres LIKE ?");
            binds.push(format!("%\"{}\"%", g.replace('\"', "")));
        }
        let where_sql = if conds.is_empty() { String::new() } else { format!("WHERE {}", conds.join(" AND ")) };
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "SELECT album_key, album, MIN(year) AS year, MIN(artwork_id) AS artwork_id, \
             COUNT(*) AS track_count, SUM(duration) AS total, MIN(album_artists) AS album_artists, \
             MIN(date_added) AS date_added \
             FROM tracks WHERE album_key != '' {where_clause} GROUP BY album_key",
            where_clause = if where_sql.is_empty() { String::new() } else { format!("AND {where_sql}") }
        );
        let mut st = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b as &dyn rusqlite::ToSql).collect();
        let rows = st
            .query_map(refs.as_slice(), |r| {
                let key: String = r.get("album_key").unwrap_or_default();
                let album: String = r.get("album").unwrap_or_default();
                let year: Option<i64> = r.get("year").unwrap_or(None);
                let art: Option<String> = r.get("artwork_id").unwrap_or(None);
                let count: i64 = r.get("track_count").unwrap_or(0);
                let total: f64 = r.get("total").unwrap_or(0.0);
                let aa: String = r.get("album_artists").unwrap_or_default();
                let added: Option<i64> = r.get("date_added").unwrap_or(None);
                Ok(json!({
                    "albumKey": key,
                    "name": album,
                    "year": year,
                    "artworkId": art,
                    "trackCount": count,
                    "duration": total,
                    "artists": serde_json::from_str::<Value>(&aa).unwrap_or(json!([])),
                    "dateAdded": added,
                }))
            })
            .map(|it| it.collect::<Vec<_>>()).unwrap_or_default();
        let mut items: Vec<Value> = rows.into_iter().filter_map(|r| r.ok()).collect();
        items
    }

    fn sort_albums(items: &mut Vec<Value>, sort: &str) {
        let key = |v: &Value| -> (String, String, i64) {
            let artists = v["artists"].as_array().cloned().unwrap_or_default();
            let first_artist = artists.first().and_then(|a| a.as_str()).unwrap_or("").to_string();
            (
                logic::sortable_string_default(&first_artist),
                v["name"].as_str().unwrap_or("").to_lowercase(),
                v["year"].as_i64().unwrap_or(0),
            )
        };
        match sort {
            "album" => items.sort_by(|a, b| key(a).1.cmp(&key(b).1)),
            "year" => items.sort_by(|a, b| key(b).2.cmp(&key(a).2)),
            "artist" | _ => items.sort_by(|a, b| key(a).cmp(&key(b))),
        }
    }

    pub fn albums(&self, sort: &str, artist: Option<&str>, genre: Option<&str>) -> Value {
        let mut items = self.album_rows(artist, genre);
        Self::sort_albums(&mut items, sort);
        json!(items)
    }

    pub fn artists(&self, kind: &str) -> Value {
        let col = if kind == "album" { "album_artists" } else { "artists" };
        let counts = self.collect_distinct(col);
        // Album counts per artist need the album keys: one pass over the rows.
        let conn = self.conn.lock().unwrap();
        let mut st = match conn.prepare(&format!("SELECT {col}, album_key FROM tracks")) {
            Ok(s) => s,
            Err(_) => return json!([]),
        };
        let rows = st
            .query_map([], |r| {
                Ok((r.get::<_, String>(0).unwrap_or_default(), r.get::<_, String>(1).unwrap_or_default()))
            })
            .map(|it| it.collect::<Vec<_>>()).unwrap_or_default();
        let mut albums_per: HashMap<String, HashSet<String>> = HashMap::new();
        for row in rows.into_iter().flatten() {
            if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&row.0) {
                for v in arr {
                    if let Some(name) = v.as_str() {
                        albums_per
                            .entry(name.to_lowercase())
                            .or_default()
                            .insert(row.1.clone());
                    }
                }
            }
        }
        drop(st);
        drop(conn);
        let mut items: Vec<Value> = counts
            .into_iter()
            .map(|(name, track_count)| {
                json!({
                    "name": name,
                    "trackCount": track_count,
                    "albumCount": albums_per.get(&name).map(|s| s.len()).unwrap_or(0),
                })
            })
            .collect();
        items.sort_by(|a, b| {
            let an = logic::sortable_string_default(a["name"].as_str().unwrap_or(""));
            let bn = logic::sortable_string_default(b["name"].as_str().unwrap_or(""));
            an.cmp(&bn)
        });
        json!(items)
    }

    pub fn genres(&self) -> Value {
        let counts = self.collect_distinct("genres");
        let mut items: Vec<Value> = counts
            .into_iter()
            .map(|(name, track_count)| json!({ "name": name, "trackCount": track_count }))
            .collect();
        items.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
        json!(items)
    }

    pub fn album(&self, key: &str) -> Value {
        let ids = self.ids_by_where(
            "WHERE album_key = ?1",
            &[&key],
            "disc_no, track_no, title COLLATE NOCASE",
        );
        let tracks = self.tracks_by_ids(&ids);
        if tracks.is_empty() {
            return Value::Null;
        }
        let first = &tracks[0];
        json!({
            "album": {
                "albumKey": key,
                "name": first["album"],
                "year": first["year"],
                "artists": first["albumArtists"],
                "artworkId": first["artworkId"],
                "trackCount": tracks.len(),
                "duration": tracks.iter().filter_map(|t| t["duration"].as_f64()).sum::<f64>(),
            },
            "tracks": tracks,
        })
    }

    pub fn artist(&self, name: &str) -> Value {
        let like = format!("%\"{}\"%", name.replace('\"', ""));
        let track_ids = self.ids_by_where(
            "WHERE artists LIKE ?1 ORDER BY album COLLATE NOCASE, disc_no, track_no, title COLLATE NOCASE",
            &[&like],
            "",
        );
        let albums = self.album_rows(Some(name), None);
        let tracks = self.tracks_by_ids(&track_ids);
        json!({
            "artist": { "name": name, "trackCount": tracks.len() },
            "albums": albums,
            "tracks": tracks,
            "imageId": Value::Null,
        })
    }

    // ---- folders ----

    pub fn folders(&self) -> Value {
        let conn = self.conn.lock().unwrap();
        let mut st = match conn.prepare("SELECT id, path FROM folders ORDER BY path COLLATE NOCASE") {
            Ok(s) => s,
            Err(_) => return json!([]),
        };
        let rows = st
            .query_map([], |r| {
                Ok(json!({ "id": r.get::<_, i64>(0).unwrap_or(0), "path": r.get::<_, String>(1).unwrap_or_default() }))
            })
            .map(|it| it.collect::<Vec<_>>()).unwrap_or_default();
        let items: Vec<Value> = rows.into_iter().filter_map(|r| r.ok()).collect();
        json!(items)
    }

    pub fn folders_paths(&self) -> Vec<String> {
        self.folders()
            .as_array()
            .map(|a| a.iter().filter_map(|v| v["path"].as_str().map(String::from)).collect())
            .unwrap_or_default()
    }

    pub fn folder(&self, path: &str) -> Value {
        let prefix = format!("{}%", path.trim_end_matches('\\'));
        let deep_prefix = format!("{}\\%", path.trim_end_matches('\\'));
        let direct = self.query_tracks(
            "WHERE path LIKE ?1 AND path NOT LIKE ?2",
            &[&prefix, &deep_prefix],
            "title COLLATE NOCASE",
            0,
            100000,
        );
        // Subfolders: direct child directories of the path.
        let mut subfolders = Vec::new();
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let count_prefix = format!("{}%", p.to_string_lossy());
                    let count: i64 = {
                        let conn = self.conn.lock().unwrap();
                        conn.query_row("SELECT COUNT(*) FROM tracks WHERE path LIKE ?1", params![count_prefix], |r| r.get(0))
                            .unwrap_or(0)
                    };
                    subfolders.push(json!({ "name": name, "path": p.to_string_lossy(), "trackCount": count }));
                }
            }
        }
        subfolders.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
        json!({ "subfolders": subfolders, "tracks": direct.1 })
    }

    pub fn add_folder(&self, path: &str) -> Result<Value, String> {
        {
            let conn = self.conn.lock().unwrap();
            conn.execute("INSERT OR IGNORE INTO folders(path) VALUES (?1)", params![path])
                .map_err(|e| e.to_string())?;
        }
        self.events.emit("library.changed", json!({}));
        Ok(json!({ "ok": true }))
    }

    pub fn remove_folder(&self, id: i64) -> Result<Value, String> {
        let path: Option<String> = {
            let conn = self.conn.lock().unwrap();
            conn.query_row("SELECT path FROM folders WHERE id = ?1", params![id], |r| r.get(0)).ok()
        };
        {
            let conn = self.conn.lock().unwrap();
            conn.execute("DELETE FROM folders WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
            if let Some(p) = &path {
                let like = format!("{}%", p.trim_end_matches('\\'));
                let _ = conn.execute("DELETE FROM tracks WHERE path LIKE ?1", params![like]);
            }
        }
        self.events.emit("library.changed", json!({}));
        Ok(json!({ "ok": true }))
    }

    // ---- search ----

    pub fn search(&self, q: &str) -> Value {
        let like = format!("%{}%", q.trim().replace('%', "").replace('_', ""));
        let (_, tracks) = self.query_tracks(
            "WHERE title LIKE ?1 OR artists LIKE ?1 OR album LIKE ?1",
            &[&like],
            "title COLLATE NOCASE",
            0,
            50,
        );
        let albums = self.album_rows(None, None);
        let albums: Vec<Value> = albums
            .into_iter()
            .filter(|a| a["name"].as_str().unwrap_or("").to_lowercase().contains(&q.to_lowercase()))
            .take(50)
            .collect();
        let artists: Vec<Value> = self
            .collect_distinct("artists")
            .into_keys()
            .filter(|n| n.contains(&q.to_lowercase()))
            .take(50)
            .map(|n| json!({ "name": n }))
            .collect();
        json!({ "tracks": tracks, "albums": albums, "artists": artists })
    }

    // ---- track changes ----

    pub fn set_love(&self, id: i64, love: bool) -> Result<Value, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE tracks SET love = ?2 WHERE id = ?1", params![id, love as i64])
            .map_err(|e| e.to_string())?;
        Ok(json!({ "ok": true }))
    }

    pub fn set_rating(&self, id: i64, rating: i64) -> Result<Value, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE tracks SET rating = ?2 WHERE id = ?1", params![id, rating.clamp(0, 5)])
            .map_err(|e| e.to_string())?;
        Ok(json!({ "ok": true }))
    }

    /// The player calls these; playCount and lastPlayed update on a real
    /// play, skipCount when the track is left early.
    pub fn record_play(&self, id: i64) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE tracks SET play_count = play_count + 1, last_played = ?2 WHERE id = ?1",
            params![id, now_secs()],
        );
    }

    pub fn record_skip(&self, id: i64) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("UPDATE tracks SET skip_count = skip_count + 1 WHERE id = ?1", params![id]);
    }

    /// Delete to the Recycle Bin and drop the rows.
    pub fn delete_tracks(&self, ids: &[i64]) -> Result<Value, String> {
        let mut deleted = 0i64;
        let mut failed = 0i64;
        for id in ids {
            let path: Option<String> = {
                let conn = self.conn.lock().unwrap();
                conn.query_row("SELECT path FROM tracks WHERE id = ?1", params![id], |r| r.get(0)).ok()
            };
            if let Some(p) = path {
                let pb = PathBuf::from(&p);
                if pb.exists() {
                    match recycle_file(&pb) {
                        Ok(()) => deleted += 1,
                        Err(_) => failed += 1,
                    }
                } else {
                    deleted += 1;
                }
                if failed == 0 {
                    let conn = self.conn.lock().unwrap();
                    let _ = conn.execute("DELETE FROM tracks WHERE id = ?1", params![id]);
                }
            }
        }
        self.events.emit("library.changed", json!({}));
        Ok(json!({ "deleted": deleted, "failed": failed }))
    }

    // ---- playlists: m3u files in the app data Playlists folder ----

    pub fn playlists_dir(&self) -> PathBuf {
        app_dir().join("Playlists")
    }

    pub fn playlist_files(&self) -> Vec<PathBuf> {
        let dir = self.playlists_dir();
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                let is_m3u = p
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.eq_ignore_ascii_case("m3u") || s.eq_ignore_ascii_case("m3u8"))
                    .unwrap_or(false);
                if is_m3u {
                    out.push(p);
                }
            }
        }
        out.sort();
        out
    }

    fn playlist_file(&self, path: &str) -> Option<PathBuf> {
        // Accepts either a full file path or a playlist name.
        let p = PathBuf::from(path);
        if p.extension().is_some() && p.exists() {
            return Some(p);
        }
        let safe = sanitize_playlist_name(path);
        let file = self.playlists_dir().join(format!("{safe}.m3u"));
        file.exists().then_some(file)
    }

    pub fn playlists(&self) -> Value {
        let items: Vec<Value> = self
            .playlist_files()
            .iter()
            .map(|p| {
                let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
                let count = std::fs::read_to_string(p)
                    .map(|body| logic::read_m3u(&body, &dir).len())
                    .unwrap_or(0);
                json!({
                    "path": p.to_string_lossy(),
                    "name": name,
                    "trackCount": count,
                })
            })
            .collect();
        json!(items)
    }

    pub fn playlist_tracks(&self, path: &str) -> Value {
        let Some(file) = self.playlist_file(path) else {
            return json!({ "items": [], "missing": 0 });
        };
        let body = std::fs::read_to_string(&file).unwrap_or_default();
        let playlist_dir = file.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let entries = logic::read_m3u(&body, &playlist_dir);
        let mut items = Vec::new();
        let mut missing = 0i64;
        for e in entries {
            match self.track_by_path(&e.full_path) {
                Some(t) => items.push(t),
                None => missing += 1,
            }
        }
        json!({ "items": items, "missing": missing })
    }

    /// The playlist's entries as written in the file, so a rewrite keeps
    /// relative paths relative.
    fn playlist_lines(&self, file: &Path) -> Vec<String> {
        let dir = file.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        logic::read_m3u(&std::fs::read_to_string(file).unwrap_or_default(), &dir)
            .into_iter()
            .map(|e| e.raw_path)
            .collect()
    }

    pub fn album_track_ids(&self, album_key: &str) -> Vec<i64> {
        self.ids_by_where("WHERE album_key = ?1", &[&album_key], "disc_no, track_no, title COLLATE NOCASE")
    }

    pub fn artist_track_ids(&self, name: &str) -> Vec<i64> {
        let pat = format!("%{}%", serde_json::to_string(name).unwrap_or_default());
        self.ids_by_where(
            "WHERE artists LIKE ?1 OR album_artists LIKE ?1",
            &[&pat],
            "album COLLATE NOCASE, disc_no, track_no",
        )
    }

    pub fn playlist_track_ids(&self, path: &str) -> Vec<i64> {
        match self.playlist_tracks(path) {
            Value::Object(o) => o["items"]
                .as_array()
                .map(|a| a.iter().filter_map(|t| t["id"].as_i64()).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn track_by_path(&self, path: &str) -> Option<Value> {
        let conn = self.conn.lock().unwrap();
        let mut st = conn
            .prepare(&format!("SELECT {} FROM tracks WHERE path = ?1 COLLATE NOCASE", Self::TRACK_COLS))
            .ok()?;
        st.query_row(params![path], Self::track_row).ok()
    }

    pub fn playlist_create(&self, name: &str) -> Result<Value, String> {
        let dir = self.playlists_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join(format!("{}.m3u", sanitize_playlist_name(name)));
        if file.exists() {
            return Err("a playlist with that name already exists".into());
        }
        std::fs::write(&file, logic::write_m3u(&[])).map_err(|e| e.to_string())?;
        self.events.emit("library.changed", json!({}));
        Ok(json!({ "ok": true, "path": file.to_string_lossy() }))
    }

    pub fn playlist_rename(&self, path: &str, name: &str) -> Result<Value, String> {
        let Some(file) = self.playlist_file(path) else { return Err("playlist not found".into()) };
        let target = self.playlists_dir().join(format!("{}.m3u", sanitize_playlist_name(name)));
        if target.exists() {
            return Err("a playlist with that name already exists".into());
        }
        std::fs::rename(&file, &target).map_err(|e| e.to_string())?;
        self.events.emit("library.changed", json!({}));
        Ok(json!({ "ok": true, "path": target.to_string_lossy() }))
    }

    pub fn playlist_delete(&self, path: &str) -> Result<Value, String> {
        let Some(file) = self.playlist_file(path) else { return Err("playlist not found".into()) };
        std::fs::remove_file(&file).map_err(|e| e.to_string())?;
        self.events.emit("library.changed", json!({}));
        Ok(json!({ "ok": true }))
    }

    pub fn playlist_add(&self, path: &str, ids: &[i64]) -> Result<Value, String> {
        let Some(file) = self.playlist_file(path) else { return Err("playlist not found".into()) };
        let mut entries = self.playlist_lines(&file);
        for id in ids {
            if let Some(t) = self.track_by_id(*id) {
                entries.push(t["path"].as_str().unwrap_or_default().to_string());
            }
        }
        std::fs::write(&file, logic::write_m3u(&entries)).map_err(|e| e.to_string())?;
        self.events.emit("library.changed", json!({}));
        Ok(json!({ "ok": true }))
    }

    pub fn playlist_remove(&self, path: &str, indexes: &[usize]) -> Result<Value, String> {
        let Some(file) = self.playlist_file(path) else { return Err("playlist not found".into()) };
        let mut entries = self.playlist_lines(&file);
        let mut idx: Vec<usize> = indexes.to_vec();
        idx.sort_unstable();
        idx.dedup();
        for i in idx.into_iter().rev() {
            if i < entries.len() {
                entries.remove(i);
            }
        }
        std::fs::write(&file, logic::write_m3u(&entries)).map_err(|e| e.to_string())?;
        self.events.emit("library.changed", json!({}));
        Ok(json!({ "ok": true }))
    }

    pub fn playlist_reorder(&self, path: &str, order: &[usize]) -> Result<Value, String> {
        let Some(file) = self.playlist_file(path) else { return Err("playlist not found".into()) };
        let entries = self.playlist_lines(&file);
        if order.len() != entries.len() {
            return Err("reorder needs every index once".into());
        }
        let mut reordered = Vec::with_capacity(entries.len());
        for i in order {
            match entries.get(*i) {
                Some(e) => reordered.push(e.clone()),
                None => return Err("reorder index out of range".into()),
            }
        }
        std::fs::write(&file, logic::write_m3u(&reordered)).map_err(|e| e.to_string())?;
        self.events.emit("library.changed", json!({}));
        Ok(json!({ "ok": true }))
    }

    // ---- scanning ----

    /// Rescan every watched folder: add new files, update changed ones, drop
    /// gone ones. Emits scan.progress while running and library.changed after.
    pub fn rescan(&self, events: &Arc<Events>) {
        let mut files = Vec::new();
        for folder in self.folders_paths() {
            walk_audio(Path::new(&folder), 0, &mut files);
        }
        let total = files.len() as i64;
        events.emit("scan.progress", json!({ "done": 0, "total": total }));
        let existing: HashMap<String, (i64, i64)> = {
            let conn = self.conn.lock().unwrap();
            let mut st = match conn.prepare("SELECT path, mtime, id FROM tracks") {
                Ok(s) => s,
                Err(_) => return,
            };
            let rows = st
                .query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))
                })
                .map(|it| it.collect::<Vec<_>>()).unwrap_or_default();
            rows.into_iter().filter_map(|r| r.ok()).collect()
        };
        let mut done = 0i64;
        let mut seen: HashSet<String> = HashSet::new();
        for path in &files {
            let key = path.to_string_lossy().to_string();
            seen.insert(key.to_lowercase());
            let mtime = file_mtime(path);
            match existing.get(&key.to_lowercase()) {
                Some((m, id)) if *m == mtime => {}
                Some((_, id)) => {
                    self.upsert_track(path, mtime, Some(*id));
                }
                None => {
                    self.upsert_track(path, mtime, None);
                }
            }
            done += 1;
            if done % 25 == 0 || done == total {
                events.emit("scan.progress", json!({ "done": done, "total": total }));
            }
        }
        // Rows whose files are gone.
        let gone: Vec<i64> = existing
            .iter()
            .filter(|(p, _)| !seen.contains(p.as_str()))
            .map(|(_, (_, id))| *id)
            .collect();
        if !gone.is_empty() {
            let conn = self.conn.lock().unwrap();
            for id in gone {
                let _ = conn.execute("DELETE FROM tracks WHERE id = ?1", params![id]);
            }
        }
        events.emit("library.changed", json!({}));
    }

    /// Read the tags of one file and insert or update its row.
    fn upsert_track(&self, path: &Path, mtime: i64, id: Option<i64>) {
        let meta = read_tags(path);
        let conn = self.conn.lock().unwrap();
        if let Some(id) = id {
            let _ = conn.execute(
                "UPDATE tracks SET path=?2, title=?3, artists=?4, album_artists=?5, album=?6, album_key=?7, \
                 genres=?8, year=?9, track_no=?10, disc_no=?11, duration=?12, bitrate=?13, sample_rate=?14, \
                 artwork_id=?15, mtime=?16 WHERE id=?1",
                params![
                    id,
                    path.to_string_lossy(),
                    meta.title,
                    meta.artists_json,
                    meta.album_artists_json,
                    meta.album,
                    meta.album_key,
                    meta.genres_json,
                    meta.year,
                    meta.track_no,
                    meta.disc_no,
                    meta.duration,
                    meta.bitrate,
                    meta.sample_rate,
                    meta.artwork_id,
                    mtime,
                ],
            );
        } else {
            let added = file_date_added(path);
            let _ = conn.execute(
                "INSERT OR REPLACE INTO tracks (path, title, artists, album_artists, album, album_key, genres, \
                 year, track_no, disc_no, duration, bitrate, sample_rate, rating, love, play_count, skip_count, \
                 date_added, last_played, artwork_id, mtime) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,0,0,0,0,?14,NULL,?15,?16)",
                params![
                    path.to_string_lossy(),
                    meta.title,
                    meta.artists_json,
                    meta.album_artists_json,
                    meta.album,
                    meta.album_key,
                    meta.genres_json,
                    meta.year,
                    meta.track_no,
                    meta.disc_no,
                    meta.duration,
                    meta.bitrate,
                    meta.sample_rate,
                    added,
                    meta.artwork_id,
                    mtime,
                ],
            );
        }
    }

    /// A debounced watcher over every watched folder. Runs until the process
    /// exits; folder changes arrive on the command channel.
    pub fn start_watcher(lib: Arc<Library>) -> std::sync::mpsc::Sender<WatcherCmd> {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<WatcherCmd>();
        std::thread::Builder::new()
            .name("watcher".into())
            .spawn(move || {
                let (ev_tx, ev_rx) = std::sync::mpsc::channel();
                let mut debouncer =
                    match notify_debouncer_full::new_debouncer(Duration::from_millis(800), None, ev_tx) {
                        Ok(d) => d,
                        Err(_) => return,
                    };
                {
                    for folder in lib.folders_paths() {
                        let _ = debouncer.watch(Path::new(&folder), notify::RecursiveMode::Recursive);
                    }
                    loop {
                        match ev_rx.recv_timeout(Duration::from_millis(200)) {
                            Ok(_events) => {
                                // Any change: rescan. The scan diffs by path and
                                // mtime, so it only touches what actually moved.
                                let lib2 = Arc::clone(&lib);
                                let ev = Arc::clone(&lib.events);
                                std::thread::Builder::new()
                                    .name("rescan".into())
                                    .spawn(move || lib2.rescan(&ev))
                                    .ok();
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                        while let Ok(cmd) = cmd_rx.try_recv() {
                            match cmd {
                                WatcherCmd::Add(p) => {
                                    let _ = debouncer.watch(Path::new(&p), notify::RecursiveMode::Recursive);
                                }
                                WatcherCmd::Remove(p) => {
                                    let _ = debouncer.unwatch(Path::new(&p));
                                }
                            }
                        }
                    }
                }
            })
            .ok();
        cmd_tx
    }
}

pub enum WatcherCmd {
    Add(String),
    Remove(String),
}

#[derive(Debug, Clone, PartialEq)]
struct FileMeta {
    title: String,
    artists_json: String,
    album_artists_json: String,
    album: String,
    album_key: String,
    genres_json: String,
    year: Option<i64>,
    track_no: Option<i64>,
    disc_no: Option<i64>,
    duration: f64,
    bitrate: i64,
    sample_rate: i64,
    artwork_id: Option<String>,
}

impl Default for FileMeta {
    fn default() -> Self {
        FileMeta {
            title: String::new(),
            artists_json: "[]".into(),
            album_artists_json: "[]".into(),
            album: String::new(),
            album_key: String::new(),
            genres_json: "[]".into(),
            year: None,
            track_no: None,
            disc_no: None,
            duration: 0.0,
            bitrate: 0,
            sample_rate: 0,
            artwork_id: None,
        }
    }
}

/// Read tags with lofty and produce the row values.
pub fn read_tags(path: &Path) -> FileMeta {
    use lofty::prelude::*;
    let mut meta = FileMeta::default();
    let name_title = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    meta.title = name_title.clone();
    let Ok(tagged) = lofty::probe::Probe::open(path).and_then(|p| p.read()) else {
        return meta;
    };
    let props = tagged.properties();
    meta.duration = props.duration().as_secs_f64();
    meta.bitrate = props.audio_bitrate().unwrap_or(0) as i64;
    meta.sample_rate = props.sample_rate().unwrap_or(0) as i64;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    if let Some(tag) = tag {
        if let Some(t) = tag.title() {
            if !t.is_empty() {
                meta.title = t.to_string();
            }
        }
        let split = |s: &str, genre: bool| -> String {
            let parts = logic::split_tag_values(s, genre);
            serde_json::to_string(&parts).unwrap_or_else(|_| "[]".into())
        };
        if let Some(a) = tag.artist() {
            meta.artists_json = split(&a, false);
        }
        if let Some(a) = tag.get_string(&lofty::tag::ItemKey::AlbumArtist) {
            meta.album_artists_json = split(a, false);
        }
        if let Some(a) = tag.album() {
            meta.album = a.to_string();
        }
        if let Some(g) = tag.genre() {
            meta.genres_json = split(&g, true);
        }
        meta.year = tag.year().map(|y| y as i64);
        meta.track_no = tag.track().and_then(|t| t.try_into().ok()).map(|t: u32| t as i64);
        meta.disc_no = tag.disk().and_then(|t| t.try_into().ok()).map(|t: u32| t as i64);
        if let Some(pic) = tag.pictures().first() {
            meta.artwork_id = store_artwork(pic.data());
        }
    }
    let album_artists: Vec<String> =
        serde_json::from_str::<Vec<String>>(&meta.album_artists_json).unwrap_or_default();
    meta.album_key = logic::album_key(&meta.album, &album_artists);
    meta
}

fn walk_audio(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth > 12 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().map(|n| n.to_string_lossy().to_lowercase()) else { continue };
        if p.is_dir() {
            if name.starts_with('.') || name == "$recycle.bin" || name == "system volume information" {
                continue;
            }
            walk_audio(&p, depth + 1, out);
        } else if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
            if AUDIO_EXTS.contains(&ext.to_ascii_lowercase().as_str()) {
                out.push(p);
            }
        }
    }
}

fn file_mtime(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn file_date_added(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.created().or_else(|_| m.modified()))
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or_else(now_secs)
}

fn sanitize_playlist_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string()
}


/// Move a file to the Recycle Bin.
pub fn recycle_file(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::{
        SHFileOperationW, FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_SILENT,
        SHFILEOPSTRUCTW,
    };
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    wide.push(0);
    let mut op = SHFILEOPSTRUCTW {
        hwnd: HWND::default(),
        wFunc: FO_DELETE,
        pFrom: windows::core::PCWSTR(wide.as_ptr()),
        pTo: windows::core::PCWSTR::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT).0 as u16,
        ..Default::default()
    };
    let code = unsafe { SHFileOperationW(&mut op) };
    if code == 0 {
        Ok(())
    } else {
        Err(format!("recycle failed with code {code}"))
    }
}

/// Store artwork bytes under a content hash id; returns the id.
pub fn store_artwork(bytes: &[u8]) -> Option<String> {
    use sha1::{Digest, Sha1};
    if bytes.is_empty() {
        return None;
    }
    let mut h = Sha1::new();
    h.update(bytes);
    let id = format!("{:x}", h.finalize());
    let dir = art_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let file = dir.join(format!("{id}.jpg"));
    if !file.exists() {
        let mut f = std::fs::File::create(&file).ok()?;
        f.write_all(bytes).ok()?;
    }
    Some(id)
}

/// The artwork cache folder, served at smp://art/<id>.
pub fn artwork_path(id: &str) -> PathBuf {
    art_dir().join(format!("{id}.jpg"))
}

pub fn ensure_app_dirs() {
    let _ = std::fs::create_dir_all(app_dir());
    let _ = std::fs::create_dir_all(art_dir());
}

