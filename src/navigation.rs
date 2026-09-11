use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use log::warn;
use serde::{Deserialize, Serialize};

const APP_DIR: &str = "qust";
const CONFIG_FILE: &str = "config.json";
pub const DEFAULT_SEARCH_TEMPLATE: &str = "https://duckduckgo.com/?q={query}";
pub const DEFAULT_HINT_SIZE: u16 = 12;
pub const MIN_HINT_SIZE: u16 = 8;
pub const MAX_HINT_SIZE: u16 = 32;
pub const DEFAULT_HINT_OPACITY: u8 = 100;
pub const MIN_HINT_OPACITY: u8 = 20;
pub const MAX_HINT_OPACITY: u8 = 100;

pub fn search_preset(name: &str) -> Option<&'static str> {
    match name {
        "google" => Some("https://www.google.com/search?q={query}"),
        "yandex" => Some("https://yandex.ru/search/?text={query}"),
        "duckduckgo" | "ddg" => Some(DEFAULT_SEARCH_TEMPLATE),
        "bing" => Some("https://www.bing.com/search?q={query}"),
        "brave" => Some("https://search.brave.com/search?q={query}"),
        _ => None,
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    search_engine: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hint_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hint_opacity: Option<u8>,
}

pub fn normalize_url(input: &str) -> String {
    normalize_url_with_template(input, &search_template())
}

pub fn search_template() -> String {
    let Some(path) = config_path() else {
        return DEFAULT_SEARCH_TEMPLATE.to_string();
    };
    load_config(&path)
        .search_engine
        .filter(|template| validate_search_template(template).is_ok())
        .unwrap_or_else(|| DEFAULT_SEARCH_TEMPLATE.to_string())
}

pub fn set_search_template(template: &str) -> io::Result<()> {
    validate_search_template(template)?;
    update_config(|config| config.search_engine = Some(template.to_string()))
}

pub fn reset_search_template() -> io::Result<()> {
    update_config(|config| config.search_engine = None)
}

pub fn hint_size() -> u16 {
    let Some(path) = config_path() else {
        return DEFAULT_HINT_SIZE;
    };
    load_config(&path)
        .hint_size
        .filter(|size| (MIN_HINT_SIZE..=MAX_HINT_SIZE).contains(size))
        .unwrap_or(DEFAULT_HINT_SIZE)
}

pub fn set_hint_size(size: u16) -> io::Result<()> {
    if !(MIN_HINT_SIZE..=MAX_HINT_SIZE).contains(&size) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("hint size must be between {MIN_HINT_SIZE} and {MAX_HINT_SIZE} pixels"),
        ));
    }
    update_config(|config| config.hint_size = Some(size))
}

pub fn reset_hint_size() -> io::Result<()> {
    update_config(|config| config.hint_size = None)
}

pub fn hint_opacity() -> u8 {
    let Some(path) = config_path() else {
        return DEFAULT_HINT_OPACITY;
    };
    load_config(&path)
        .hint_opacity
        .filter(|opacity| (MIN_HINT_OPACITY..=MAX_HINT_OPACITY).contains(opacity))
        .unwrap_or(DEFAULT_HINT_OPACITY)
}

pub fn set_hint_opacity(opacity: u8) -> io::Result<()> {
    if !(MIN_HINT_OPACITY..=MAX_HINT_OPACITY).contains(&opacity) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "hint opacity must be between {MIN_HINT_OPACITY} and {MAX_HINT_OPACITY} percent"
            ),
        ));
    }
    update_config(|config| config.hint_opacity = Some(opacity))
}

pub fn reset_hint_opacity() -> io::Result<()> {
    update_config(|config| config.hint_opacity = None)
}

fn normalize_url_with_template(input: &str, template: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    if trimmed.contains('.') && !trimmed.contains(' ') {
        return format!("https://{}", trimmed);
    }

    let query = glib::Uri::escape_string(trimmed, None, false);
    template.replace("{query}", &query)
}

fn validate_search_template(template: &str) -> io::Result<()> {
    if !template.starts_with("http://") && !template.starts_with("https://") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "search engine template must start with http:// or https://",
        ));
    }
    if !template.contains("{query}") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "search engine template must contain {query}",
        ));
    }
    Ok(())
}

fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(APP_DIR).join(CONFIG_FILE))
}

fn load_config(path: &Path) -> Config {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Config::default(),
        Err(error) => {
            warn!("failed to read config {:?}: {}", path, error);
            return Config::default();
        }
    };

    serde_json::from_str(&content).unwrap_or_else(|error| {
        warn!("failed to parse config {:?}: {}", path, error);
        Config::default()
    })
}

fn update_config(update: impl FnOnce(&mut Config)) -> io::Result<()> {
    let path = config_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "failed to determine config directory",
        )
    })?;
    let mut config = load_config(&path);
    update(&mut config);
    save_config_to(&path, config)
}

fn save_config_to(path: &Path, config: Config) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "invalid config path"))?;
    fs::create_dir_all(parent)?;
    let json = serde_json::to_string_pretty(&config).map_err(io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, json)?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::{
        load_config, normalize_url_with_template, save_config_to, search_preset,
        validate_search_template, Config, DEFAULT_SEARCH_TEMPLATE,
    };

    #[test]
    fn common_search_engine_presets_are_available() {
        assert_eq!(
            search_preset("google"),
            Some("https://www.google.com/search?q={query}")
        );
        assert_eq!(
            search_preset("yandex"),
            Some("https://yandex.ru/search/?text={query}")
        );
        assert_eq!(search_preset("ddg"), Some(DEFAULT_SEARCH_TEMPLATE));
        assert_eq!(search_preset("unknown"), None);
    }

    #[test]
    fn search_template_receives_encoded_query() {
        assert_eq!(
            normalize_url_with_template(
                "rust gtk & webkit",
                "https://search.example/?term={query}&source=qust"
            ),
            "https://search.example/?term=rust%20gtk%20%26%20webkit&source=qust"
        );
    }

    #[test]
    fn explicit_urls_do_not_use_search_template() {
        assert_eq!(
            normalize_url_with_template("example.com/path", "https://search/?q={query}"),
            "https://example.com/path"
        );
        assert_eq!(
            normalize_url_with_template("http://localhost:3000", "https://search/?q={query}"),
            "http://localhost:3000"
        );
    }

    #[test]
    fn search_template_requires_http_url_and_placeholder() {
        assert!(validate_search_template("https://search.example/?q={query}").is_ok());
        assert!(validate_search_template("https://search.example/").is_err());
        assert!(validate_search_template("search.example/?q={query}").is_err());
    }

    #[test]
    fn search_template_is_persisted() {
        let directory =
            std::env::temp_dir().join(format!("qust-config-test-{}", std::process::id()));
        let path = directory.join("config.json");
        let template = "https://search.example/?q={query}";

        save_config_to(
            &path,
            Config {
                search_engine: Some(template.to_string()),
                hint_size: None,
                hint_opacity: None,
            },
        )
        .expect("save config");

        assert_eq!(load_config(&path).search_engine.as_deref(), Some(template));
        std::fs::remove_dir_all(directory).expect("remove config test directory");
    }
}
