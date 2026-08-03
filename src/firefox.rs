use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::tab::{TabGroupSnapshot, TabSnapshot};

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
    #[serde(default)]
    groups: Vec<FirefoxGroup>,
}

#[derive(Deserialize)]
struct FirefoxTab {
    #[serde(default = "default_entry_index")]
    index: usize,
    #[serde(default)]
    entries: Vec<FirefoxEntry>,
    #[serde(default)]
    pinned: bool,
    #[serde(default, rename = "groupId")]
    group_id: Option<String>,
    image: Option<String>,
}

#[derive(Deserialize)]
struct FirefoxEntry {
    url: String,
}

#[derive(Deserialize)]
struct FirefoxGroup {
    id: String,
    name: String,
    #[serde(default)]
    collapsed: bool,
    color: Option<String>,
}

pub struct FirefoxImport {
    pub tabs: Vec<TabSnapshot>,
    pub groups: Vec<TabGroupSnapshot>,
}

fn default_entry_index() -> usize {
    1
}

pub fn current_tabs() -> Result<FirefoxImport, FirefoxImportError> {
    let session_path = find_session_file()?;
    let compressed = fs::read(session_path)?;
    let json = decompress_session(&compressed)?;
    parse_tabs(&json)
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

fn parse_tabs(json: &[u8]) -> Result<FirefoxImport, FirefoxImportError> {
    let session: FirefoxSession = serde_json::from_slice(json)
        .map_err(|error| FirefoxImportError::InvalidSession(error.to_string()))?;
    let mut tabs = Vec::new();
    let mut groups = Vec::new();
    let mut used_names = HashSet::new();

    for window in session.windows {
        let group_map: HashMap<_, _> = window
            .groups
            .into_iter()
            .map(|group| {
                let name = unique_group_name(&group.name, &mut used_names);
                (group.id, (name, group.collapsed, group.color))
            })
            .collect();
        let mut imported_groups = HashSet::new();

        for tab in window.tabs {
            let Some(entry) = tab.entries.into_iter().nth(tab.index.saturating_sub(1)) else {
                continue;
            };
            if !entry.url.starts_with("http://") && !entry.url.starts_with("https://") {
                continue;
            }

            let group = tab.group_id.as_ref().and_then(|id| group_map.get(id)).map(
                |(name, collapsed, color)| {
                    if imported_groups.insert(name.clone()) {
                        groups.push(TabGroupSnapshot {
                            name: name.clone(),
                            collapsed: *collapsed,
                            color: color.clone(),
                        });
                    }
                    name.clone()
                },
            );
            tabs.push(TabSnapshot {
                url: entry.url,
                pinned: tab.pinned,
                group,
                favicon: tab.image.filter(|image| image.starts_with("data:image/")),
            });
        }
    }

    Ok(FirefoxImport { tabs, groups })
}

fn unique_group_name(name: &str, used: &mut HashSet<String>) -> String {
    let base = if name.trim().is_empty() {
        "Firefox Group"
    } else {
        name.trim()
    };
    if used.insert(base.to_string()) {
        return base.to_string();
    }

    for suffix in 2.. {
        let candidate = format!("{} ({})", base, suffix);
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::parse_tabs;

    #[test]
    fn parses_active_http_entries_from_all_windows() {
        let json = br#"{
            "windows": [
                {"groups": [{"id": "g1", "name": "Work", "collapsed": true,
                    "color": "green"}],
                 "tabs": [
                    {"index": 2, "groupId": "g1", "entries": [
                        {"url": "https://old.example"},
                        {"url": "https://current.example"}
                    ]},
                    {"entries": [{"url": "about:newtab"}]}
                ]},
                {"groups": [{"id": "g2", "name": "Work"}],
                 "tabs": [
                    {"groupId": "g2", "entries": [{"url": "http://second.example"}]},
                    {"pinned": true, "image": "data:image/png;base64,AA==",
                     "entries": [{"url": "https://pinned.example"}]}
                 ]}
            ]
        }"#;

        let imported = parse_tabs(json).unwrap();
        assert_eq!(imported.tabs.len(), 3);
        assert_eq!(imported.tabs[0].group.as_deref(), Some("Work"));
        assert_eq!(imported.tabs[1].group.as_deref(), Some("Work (2)"));
        assert!(imported.tabs[2].pinned);
        assert_eq!(
            imported.tabs[2].favicon.as_deref(),
            Some("data:image/png;base64,AA==")
        );
        assert_eq!(imported.groups.len(), 2);
        assert!(imported.groups[0].collapsed);
        assert_eq!(imported.groups[0].color.as_deref(), Some("green"));
    }
}
