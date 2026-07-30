use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

const MOZLZ4_HEADER: &[u8; 8] = b"mozLz40\0";

#[derive(Debug)]
pub enum FirefoxImportError {
    ProfileNotFound,
    SessionNotFound,
    InvalidSession(String),
    Io(std::io::Error),
}

impl std::fmt::Display for FirefoxImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProfileNotFound => write!(f, "Firefox profile not found"),
            Self::SessionNotFound => write!(f, "Firefox recovery session not found"),
            Self::InvalidSession(message) => write!(f, "invalid Firefox session: {}", message),
            Self::Io(error) => write!(f, "failed to read Firefox session: {}", error),
        }
    }
}

impl std::error::Error for FirefoxImportError {}

impl From<std::io::Error> for FirefoxImportError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Deserialize)]
struct FirefoxSession {
    #[serde(default)]
    windows: Vec<FirefoxWindow>,
}

#[derive(Deserialize)]
struct FirefoxWindow {
    #[serde(default)]
    tabs: Vec<FirefoxTab>,
}

#[derive(Deserialize)]
struct FirefoxTab {
    #[serde(default = "default_entry_index")]
    index: usize,
    #[serde(default)]
    entries: Vec<FirefoxEntry>,
}

#[derive(Deserialize)]
struct FirefoxEntry {
    url: String,
}

fn default_entry_index() -> usize {
    1
}

pub fn current_tab_urls() -> Result<Vec<String>, FirefoxImportError> {
    let session_path = find_session_file()?;
    let compressed = fs::read(session_path)?;
    let json = decompress_session(&compressed)?;
    parse_tab_urls(&json)
}

fn find_session_file() -> Result<PathBuf, FirefoxImportError> {
    let home = dirs::home_dir().ok_or(FirefoxImportError::ProfileNotFound)?;
    let roots = [
        home.join(".config/mozilla/firefox"),
        home.join(".mozilla/firefox"),
        home.join(".var/app/org.mozilla.firefox/.mozilla/firefox"),
    ];
    let mut candidates = Vec::new();

    for root in roots.iter().filter(|root| root.is_dir()) {
        for entry in fs::read_dir(root)? {
            let profile = entry?.path();
            if !profile.is_dir() {
                continue;
            }
            for name in ["recovery.jsonlz4", "previous.jsonlz4"] {
                let path = profile.join("sessionstore-backups").join(name);
                if path.is_file() {
                    candidates.push(path);
                }
            }
        }
    }

    candidates
        .into_iter()
        .max_by_key(|path| modified(path))
        .ok_or(FirefoxImportError::SessionNotFound)
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    path.metadata().ok()?.modified().ok()
}

fn decompress_session(data: &[u8]) -> Result<Vec<u8>, FirefoxImportError> {
    if data.len() < MOZLZ4_HEADER.len() || &data[..MOZLZ4_HEADER.len()] != MOZLZ4_HEADER {
        return Err(FirefoxImportError::InvalidSession(
            "missing mozLz40 header".to_string(),
        ));
    }

    lz4_flex::block::decompress_size_prepended(&data[MOZLZ4_HEADER.len()..])
        .map_err(|error| FirefoxImportError::InvalidSession(error.to_string()))
}

fn parse_tab_urls(json: &[u8]) -> Result<Vec<String>, FirefoxImportError> {
    let session: FirefoxSession = serde_json::from_slice(json)
        .map_err(|error| FirefoxImportError::InvalidSession(error.to_string()))?;
    let urls = session
        .windows
        .into_iter()
        .flat_map(|window| window.tabs)
        .filter_map(|tab| tab.entries.into_iter().nth(tab.index.saturating_sub(1)))
        .map(|entry| entry.url)
        .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
        .collect();
    Ok(urls)
}

#[cfg(test)]
mod tests {
    use super::parse_tab_urls;

    #[test]
    fn parses_active_http_entries_from_all_windows() {
        let json = br#"{
            "windows": [
                {"tabs": [
                    {"index": 2, "entries": [
                        {"url": "https://old.example"},
                        {"url": "https://current.example"}
                    ]},
                    {"entries": [{"url": "about:newtab"}]}
                ]},
                {"tabs": [{"entries": [{"url": "http://second.example"}]}]}
            ]
        }"#;

        assert_eq!(
            parse_tab_urls(json).unwrap(),
            ["https://current.example", "http://second.example"]
        );
    }
}
