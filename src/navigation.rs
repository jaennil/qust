use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use log::warn;
use serde::{Deserialize, Serialize};

const APP_DIR: &str = "qust";
const CONFIG_FILE: &str = "config.json";
pub const DEFAULT_SEARCH_TEMPLATE: &str = "https://duckduckgo.com/?q={query}";

#[derive(Debug, Default, Deserialize, Serialize)]
struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    search_engine: Option<String>,
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
    save_config(Config {
        search_engine: Some(template.to_string()),
    })
}

pub fn reset_search_template() -> io::Result<()> {
    save_config(Config::default())
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

fn save_config(config: Config) -> io::Result<()> {
    let path = config_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "failed to determine config directory",
        )
    })?;
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
        load_config, normalize_url_with_template, save_config_to, validate_search_template, Config,
    };

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
            },
        )
        .expect("save config");

        assert_eq!(load_config(&path).search_engine.as_deref(), Some(template));
        std::fs::remove_dir_all(directory).expect("remove config test directory");
    }
}
