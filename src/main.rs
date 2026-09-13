mod command_bar;
mod commands;
mod firefox;
mod hints;
mod keybindings;
mod modes;
mod navigation;
mod password_manager;
mod session;
mod tab;
mod window;

use gtk::gio::ApplicationFlags;
use gtk::prelude::*;
use log::{error, info};
use std::ffi::OsString;
use webkit2gtk::{CookieManagerExt, CookiePersistentStorage, WebContext, WebContextExt};

const APP_ID: &str = "com.github.qust";
const APP_DIR: &str = "qust";
const FAVICON_DIR: &str = "favicons";
const COOKIE_FILE: &str = "cookies.sqlite";

fn main() {
    env_logger::init();
    info!("starting qust browser");

    gtk::init().expect("failed to initialize GTK");
    info!("GTK initialized");

    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    app.connect_activate(|app| {
        info!("activating application");
        if let Some(window) = main_window(app) {
            window.present();
            return;
        }
        configure_web_context();
        let win = window::create_window(app);
        win.show_all();
    });

    app.connect_command_line(|app, command_line| {
        let urls = urls_from_arguments(&command_line.arguments());
        info!("command line with {} url(s)", urls.len());
        app.activate();

        if let Some(window) = main_window(app) {
            window::open_urls(&window, &urls);
        }
        0
    });

    info!("running GTK application");
    app.run();
}

fn main_window(app: &gtk::Application) -> Option<gtk::ApplicationWindow> {
    app.windows()
        .into_iter()
        .find_map(|window| window.downcast::<gtk::ApplicationWindow>().ok())
}

fn urls_from_arguments(arguments: &[OsString]) -> Vec<String> {
    arguments
        .iter()
        .skip(1)
        .filter_map(|argument| argument.to_str())
        .filter(|argument| !argument.is_empty() && !argument.starts_with('-'))
        .map(navigation::normalize_url)
        .collect()
}

fn configure_web_context() {
    let data_dir = glib::user_data_dir().join(APP_DIR);
    let favicon_dir = data_dir.join(FAVICON_DIR);

    if let Err(e) = std::fs::create_dir_all(&favicon_dir) {
        error!(
            "failed to create favicon directory {:?}: {}",
            favicon_dir, e
        );
        return;
    }

    let Some(path) = favicon_dir.to_str() else {
        error!("favicon directory is not valid UTF-8: {:?}", favicon_dir);
        return;
    };

    let Some(context) = WebContext::default() else {
        error!("failed to get default WebKit context");
        return;
    };

    context.set_favicon_database_directory(Some(path));
    info!("favicon database directory set to {:?}", favicon_dir);

    let cookie_path = data_dir.join(COOKIE_FILE);
    let Some(cookie_path) = cookie_path.to_str() else {
        error!("cookie database path is not valid UTF-8");
        return;
    };
    let Some(cookie_manager) = context.cookie_manager() else {
        error!("failed to get WebKit cookie manager");
        return;
    };

    cookie_manager.set_persistent_storage(cookie_path, CookiePersistentStorage::Sqlite);
    info!("persistent cookie storage set to {}", cookie_path);
}

#[cfg(test)]
mod tests {
    use super::urls_from_arguments;
    use std::ffi::OsString;

    #[test]
    fn command_line_urls_skip_the_program_name_and_flags() {
        let arguments: Vec<OsString> =
            ["qust", "--verbose", "https://example.com", "http://a.test"]
                .iter()
                .map(OsString::from)
                .collect();

        assert_eq!(
            urls_from_arguments(&arguments),
            vec![
                "https://example.com".to_string(),
                "http://a.test".to_string()
            ]
        );
    }

    #[test]
    fn command_line_without_urls_opens_nothing() {
        let arguments: Vec<OsString> = ["qust"].iter().map(OsString::from).collect();
        assert!(urls_from_arguments(&arguments).is_empty());
    }
}
