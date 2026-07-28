use gtk::prelude::*;
use log::info;
use serde::Deserialize;
use std::cell::RefCell;
use std::io::Write;
use std::process::{Command, Stdio};
use std::rc::Rc;
use webkit2gtk::WebViewExt;

const DEFAULT_BW_COMMAND: &str = "bw";
const FILL_SCRIPT_TEMPLATE: &str = r#"
(function() {
    const username = __USERNAME__;
    const password = __PASSWORD__;

    function visible(el) {
        const rect = el.getBoundingClientRect();
        const style = window.getComputedStyle(el);
        return rect.width > 0 && rect.height > 0 &&
            style.visibility !== 'hidden' &&
            style.display !== 'none' &&
            !el.disabled && !el.readOnly;
    }

    function setValue(el, value) {
        el.focus();
        el.value = value;
        el.dispatchEvent(new Event('input', { bubbles: true }));
        el.dispatchEvent(new Event('change', { bubbles: true }));
    }

    const inputs = Array.from(document.querySelectorAll('input')).filter(visible);
    const passwordField = inputs.find(el => el.type === 'password');
    const usernameField = inputs.find(el => {
        const type = (el.type || '').toLowerCase();
        const name = `${el.name || ''} ${el.id || ''} ${el.autocomplete || ''}`.toLowerCase();
        return el !== passwordField &&
            ['text', 'email', 'tel', 'url', 'search', ''].includes(type) &&
            (name.includes('user') || name.includes('email') || name.includes('login') ||
                name.includes('account') || name.includes('identifier'));
    }) || inputs.find(el => {
        const type = (el.type || '').toLowerCase();
        return el !== passwordField && ['text', 'email', 'tel', 'url', 'search', ''].includes(type);
    });

    if (usernameField) {
        setValue(usernameField, username);
    }
    if (passwordField) {
        setValue(passwordField, password);
    }

    return passwordField ? 'filled' : 'no-password-field';
})()
"#;

#[derive(Clone)]
pub struct PasswordManager {
    backend: Rc<BwCli>,
}

impl PasswordManager {
    pub fn new() -> Self {
        Self {
            backend: Rc::new(BwCli::new(DEFAULT_BW_COMMAND)),
        }
    }

    pub fn status(&self) -> Result<BwStatus, PasswordError> {
        self.backend.status()
    }

    pub fn configure_server(&self, server_url: &str) -> Result<(), PasswordError> {
        self.backend.configure_server(server_url)
    }

    pub fn unlock(&self, password: &str) -> Result<(), PasswordError> {
        self.backend.unlock(password)
    }

    pub fn lock(&self) -> Result<(), PasswordError> {
        self.backend.lock()
    }

    pub fn fill_current_page(
        &self,
        webview: &webkit2gtk::WebView,
        parent: &gtk::ApplicationWindow,
    ) -> Result<(), PasswordError> {
        let uri = webview
            .uri()
            .map(|uri| uri.to_string())
            .ok_or(PasswordError::NoCurrentUrl)?;
        let credentials = self.backend.credentials_for_url(&uri)?;
        let Some(credential) = choose_credential(parent, &credentials) else {
            return Ok(());
        };

        fill_webview(webview, &credential);
        Ok(())
    }

    pub fn copy_username(
        &self,
        webview: &webkit2gtk::WebView,
        parent: &gtk::ApplicationWindow,
    ) -> Result<(), PasswordError> {
        let credential = self.credential_for_current_page(webview, parent)?;
        copy_to_clipboard(&credential.username);
        Ok(())
    }

    pub fn copy_password(
        &self,
        webview: &webkit2gtk::WebView,
        parent: &gtk::ApplicationWindow,
    ) -> Result<(), PasswordError> {
        let credential = self.credential_for_current_page(webview, parent)?;
        copy_to_clipboard(&credential.password);
        Ok(())
    }

    fn credential_for_current_page(
        &self,
        webview: &webkit2gtk::WebView,
        parent: &gtk::ApplicationWindow,
    ) -> Result<Credential, PasswordError> {
        let uri = webview
            .uri()
            .map(|uri| uri.to_string())
            .ok_or(PasswordError::NoCurrentUrl)?;
        let credentials = self.backend.credentials_for_url(&uri)?;
        choose_credential(parent, &credentials).ok_or(PasswordError::NoCredentialSelected)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwStatus {
    pub server_url: Option<String>,
    pub user_email: Option<String>,
    pub status: VaultStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultStatus {
    Unauthenticated,
    Locked,
    Unlocked,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub name: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug)]
pub enum PasswordError {
    CommandFailed(String),
    InvalidJson(String),
    MissingPassword,
    NoCredentials(String),
    NoCredentialSelected,
    NoCurrentUrl,
}

impl std::fmt::Display for PasswordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasswordError::CommandFailed(message) => write!(f, "Bitwarden CLI failed: {}", message),
            PasswordError::InvalidJson(message) => {
                write!(f, "invalid Bitwarden CLI output: {}", message)
            }
            PasswordError::MissingPassword => write!(f, "unlock requires a password"),
            PasswordError::NoCredentials(url) => write!(f, "no credentials found for {}", url),
            PasswordError::NoCredentialSelected => write!(f, "no credential selected"),
            PasswordError::NoCurrentUrl => write!(f, "current tab has no URL"),
        }
    }
}

impl std::error::Error for PasswordError {}

struct BwCli {
    command: String,
    session: RefCell<Option<String>>,
}

impl BwCli {
    fn new(command: &str) -> Self {
        Self {
            command: command.to_string(),
            session: RefCell::new(None),
        }
    }

    fn status(&self) -> Result<BwStatus, PasswordError> {
        let output = self.run(["status"])?;
        parse_status(&output)
    }

    fn configure_server(&self, server_url: &str) -> Result<(), PasswordError> {
        self.run(["config", "server", server_url])?;
        Ok(())
    }

    fn unlock(&self, password: &str) -> Result<(), PasswordError> {
        if password.is_empty() {
            return Err(PasswordError::MissingPassword);
        }

        let mut child = Command::new(&self.command)
            .arg("unlock")
            .arg("--raw")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| PasswordError::CommandFailed(e.to_string()))?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(format!("{}\n", password).as_bytes())
                .map_err(|e| PasswordError::CommandFailed(e.to_string()))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|e| PasswordError::CommandFailed(e.to_string()))?;
        if !output.status.success() {
            return Err(PasswordError::CommandFailed(stderr_text(&output.stderr)));
        }

        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if token.is_empty() {
            return Err(PasswordError::CommandFailed(
                "unlock did not return a session token".to_string(),
            ));
        }

        *self.session.borrow_mut() = Some(token);
        Ok(())
    }

    fn lock(&self) -> Result<(), PasswordError> {
        self.run(["lock"])?;
        *self.session.borrow_mut() = None;
        Ok(())
    }

    fn credentials_for_url(&self, url: &str) -> Result<Vec<Credential>, PasswordError> {
        let Some(session) = self.session.borrow().clone() else {
            return Err(PasswordError::CommandFailed(
                "vault is locked; run :bw unlock".to_string(),
            ));
        };

        let output = Command::new(&self.command)
            .arg("list")
            .arg("items")
            .arg("--url")
            .arg(url)
            .env("BW_SESSION", session)
            .output()
            .map_err(|e| PasswordError::CommandFailed(e.to_string()))?;
        if !output.status.success() {
            return Err(PasswordError::CommandFailed(stderr_text(&output.stderr)));
        }

        let items = parse_items(&String::from_utf8_lossy(&output.stdout))?;
        let credentials: Vec<Credential> = items
            .into_iter()
            .filter(|item| item.matches_url(url))
            .filter_map(BwItem::into_credential)
            .collect();

        if credentials.is_empty() {
            return Err(PasswordError::NoCredentials(url.to_string()));
        }

        Ok(credentials)
    }

    fn run<const N: usize>(&self, args: [&str; N]) -> Result<String, PasswordError> {
        let output = Command::new(&self.command)
            .args(args)
            .output()
            .map_err(|e| PasswordError::CommandFailed(e.to_string()))?;
        if !output.status.success() {
            return Err(PasswordError::CommandFailed(stderr_text(&output.stderr)));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

fn stderr_text(stderr: &[u8]) -> String {
    let message = String::from_utf8_lossy(stderr).trim().to_string();
    if message.is_empty() {
        "unknown error".to_string()
    } else {
        message
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStatus {
    server_url: Option<String>,
    user_email: Option<String>,
    status: String,
}

fn parse_status(json: &str) -> Result<BwStatus, PasswordError> {
    let status: RawStatus =
        serde_json::from_str(json).map_err(|e| PasswordError::InvalidJson(e.to_string()))?;
    Ok(BwStatus {
        server_url: status.server_url,
        user_email: status.user_email,
        status: match status.status.as_str() {
            "unauthenticated" => VaultStatus::Unauthenticated,
            "locked" => VaultStatus::Locked,
            "unlocked" => VaultStatus::Unlocked,
            other => VaultStatus::Unknown(other.to_string()),
        },
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BwItem {
    name: String,
    login: Option<BwLogin>,
}

impl BwItem {
    fn matches_url(&self, url: &str) -> bool {
        self.login
            .as_ref()
            .map(|login| login.matches_url(url))
            .unwrap_or(false)
    }

    fn into_credential(self) -> Option<Credential> {
        let login = self.login?;
        Some(Credential {
            name: self.name,
            username: login.username.unwrap_or_default(),
            password: login.password?,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BwLogin {
    username: Option<String>,
    password: Option<String>,
    #[serde(default)]
    uris: Vec<BwUri>,
}

impl BwLogin {
    fn matches_url(&self, url: &str) -> bool {
        self.uris.iter().any(|uri| uri.matches_url(url))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BwUri {
    uri: Option<String>,
}

impl BwUri {
    fn matches_url(&self, url: &str) -> bool {
        let Some(stored) = self.uri.as_deref() else {
            return false;
        };
        urls_match(stored, url)
    }
}

fn parse_items(json: &str) -> Result<Vec<BwItem>, PasswordError> {
    serde_json::from_str(json).map_err(|e| PasswordError::InvalidJson(e.to_string()))
}

fn urls_match(stored: &str, current: &str) -> bool {
    if stored == current {
        return true;
    }

    match (host(stored), host(current)) {
        (Some(stored_host), Some(current_host)) => {
            current_host == stored_host || current_host.ends_with(&format!(".{}", stored_host))
        }
        _ => false,
    }
}

fn host(url: &str) -> Option<String> {
    let mut rest = url.trim();
    if let Some((_, after_scheme)) = rest.split_once("://") {
        rest = after_scheme;
    }

    rest = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .rsplit('@')
        .next()
        .unwrap_or(rest)
        .trim()
        .trim_end_matches('.');

    let host = if rest.starts_with('[') {
        rest.split_once(']')
            .map(|(host, _)| format!("{}]", host))
            .unwrap_or_else(|| rest.to_string())
    } else {
        rest.split_once(':')
            .map(|(host, _)| host.to_string())
            .unwrap_or_else(|| rest.to_string())
    }
    .trim()
    .to_ascii_lowercase();

    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn choose_credential(
    parent: &gtk::ApplicationWindow,
    credentials: &[Credential],
) -> Option<Credential> {
    if credentials.len() == 1 {
        return credentials.first().cloned();
    }

    let dialog = gtk::Dialog::with_buttons(
        Some("Choose Credential"),
        Some(parent),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Use", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_default_size(420, 240);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    for credential in credentials {
        let label = gtk::Label::new(Some(&format!(
            "{} ({})",
            credential.name, credential.username
        )));
        label.set_xalign(0.0);
        label.set_margin_top(8);
        label.set_margin_bottom(8);
        label.set_margin_start(8);
        label.set_margin_end(8);
        list.add(&label);
    }
    list.select_row(list.row_at_index(0).as_ref());
    dialog.content_area().pack_start(&list, true, true, 0);
    dialog.show_all();

    let response = dialog.run();
    let selected = list
        .selected_row()
        .map(|row| row.index() as usize)
        .and_then(|index| credentials.get(index))
        .cloned();
    dialog.close();

    if response == gtk::ResponseType::Accept {
        selected
    } else {
        None
    }
}

fn fill_webview(webview: &webkit2gtk::WebView, credential: &Credential) {
    let js = build_fill_js(&credential.username, &credential.password);
    info!(
        "filling credentials with Bitwarden item: {}",
        credential.name
    );
    webview.evaluate_javascript(&js, None, None, None::<&gtk::gio::Cancellable>, |result| {
        if let Err(e) = result {
            log::error!("credential fill failed: {}", e);
        }
    });
}

fn copy_to_clipboard(text: &str) {
    let Some(display) = gdk::Display::default() else {
        log::warn!("cannot copy credential: no display");
        return;
    };
    let Some(clipboard) = gtk::Clipboard::default(&display) else {
        log::warn!("cannot copy credential: no clipboard");
        return;
    };
    clipboard.set_text(text);
}

fn build_fill_js(username: &str, password: &str) -> String {
    FILL_SCRIPT_TEMPLATE
        .replace("__USERNAME__", &json_string(username))
        .replace("__PASSWORD__", &json_string(password))
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_maps_known_states() {
        let status = parse_status(
            r#"{"serverUrl":"https://vault.example.com","userEmail":"me@example.com","status":"unlocked"}"#,
        )
        .unwrap();

        assert_eq!(
            status.server_url,
            Some("https://vault.example.com".to_string())
        );
        assert_eq!(status.user_email, Some("me@example.com".to_string()));
        assert_eq!(status.status, VaultStatus::Unlocked);
    }

    #[test]
    fn parse_items_extracts_matching_credentials() {
        let items = parse_items(
            r#"[
                {
                    "name": "Example",
                    "login": {
                        "username": "alice",
                        "password": "secret",
                        "uris": [{"uri": "https://example.com/login"}]
                    }
                }
            ]"#,
        )
        .unwrap();

        let credentials: Vec<Credential> = items
            .into_iter()
            .filter(|item| item.matches_url("https://example.com/account"))
            .filter_map(BwItem::into_credential)
            .collect();

        assert_eq!(
            credentials,
            vec![Credential {
                name: "Example".to_string(),
                username: "alice".to_string(),
                password: "secret".to_string(),
            }]
        );
    }

    #[test]
    fn url_matching_allows_subdomains_but_not_suffix_tricks() {
        assert!(urls_match(
            "https://example.com/login",
            "https://app.example.com/session"
        ));
        assert!(!urls_match(
            "https://example.com/login",
            "https://badexample.com/session"
        ));
    }

    #[test]
    fn fill_js_uses_json_escaping_for_secret_values() {
        let js = build_fill_js("ali'ce", "p\"ass\nword");

        assert!(js.contains(r#""ali'ce""#));
        assert!(js.contains(r#""p\"ass\nword""#));
        assert!(!js.contains("const password = p\"ass"));
    }
}
