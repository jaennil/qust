use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use log::{error, info, warn};
use serde::{Deserialize, Serialize};

use crate::tab::{TabGroupSnapshot, TabSnapshot};

const SESSION_FILE: &str = "session.json";
const APP_DIR: &str = "qust";
static SESSION_SAVE_ENABLED: AtomicBool = AtomicBool::new(true);

#[derive(Serialize, Deserialize, Debug)]
pub struct Session {
    pub tabs: Vec<TabSnapshot>,
    #[serde(default)]
    pub groups: Vec<TabGroupSnapshot>,
    pub active: u32,
}

fn session_path() -> Option<PathBuf> {
    let data_dir = dirs::data_dir()?.join(APP_DIR);
    Some(data_dir.join(SESSION_FILE))
}

pub fn save(tabs: Vec<TabSnapshot>, groups: Vec<TabGroupSnapshot>, active: u32) {
    if !SESSION_SAVE_ENABLED.load(Ordering::Relaxed) {
        info!("session save skipped after session clear");
        return;
    }

    let path = match session_path() {
        Some(p) => p,
        None => {
            error!("failed to determine session file path");
            return;
        }
    };

    let session = Session {
        tabs,
        groups,
        active,
    };

    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            error!("failed to create session directory {:?}: {}", parent, e);
            return;
        }
    }

    match serde_json::to_string_pretty(&session) {
        Ok(json) => match fs::write(&path, json) {
            Ok(()) => info!(
                "session saved to {:?} ({} tabs, {} groups, active: {})",
                path,
                session.tabs.len(),
                session.groups.len(),
                session.active
            ),
            Err(e) => error!("failed to write session file {:?}: {}", path, e),
        },
        Err(e) => error!("failed to serialize session: {}", e),
    }
}

pub fn clear() -> io::Result<()> {
    let Some(path) = session_path() else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "failed to determine session file path",
        ));
    };
    SESSION_SAVE_ENABLED.store(false, Ordering::Relaxed);

    match fs::remove_file(&path) {
        Ok(()) => info!("session cleared at {:?}", path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            info!("session was already clear at {:?}", path);
        }
        Err(error) => {
            SESSION_SAVE_ENABLED.store(true, Ordering::Relaxed);
            return Err(error);
        }
    }
    Ok(())
}

pub fn load() -> Option<Session> {
    let path = session_path()?;

    if !path.exists() {
        info!("no session file found at {:?}", path);
        return None;
    }

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            warn!("failed to read session file {:?}: {}", path, e);
            return None;
        }
    };

    match serde_json::from_str::<Session>(&content) {
        Ok(session) => {
            info!(
                "session loaded from {:?} ({} tabs, {} groups, active: {})",
                path,
                session.tabs.len(),
                session.groups.len(),
                session.active
            );
            if session.tabs.is_empty() {
                info!("session has no tabs, ignoring");
                return None;
            }
            Some(session)
        }
        Err(e) => {
            warn!("failed to parse session file {:?}: {}", path, e);
            None
        }
    }
}
