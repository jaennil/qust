mod command_bar;
mod commands;
mod hints;
mod keybindings;
mod modes;
mod password_manager;
mod session;
mod tab;
mod window;

use gtk::prelude::*;
use log::{error, info};
use webkit2gtk::{WebContext, WebContextExt};

const APP_ID: &str = "com.github.qust";
const APP_DIR: &str = "qust";
const FAVICON_DIR: &str = "favicons";

fn main() {
    env_logger::init();
    info!("starting qust browser");

    gtk::init().expect("failed to initialize GTK");
    info!("GTK initialized");

    let app = gtk::Application::builder().application_id(APP_ID).build();

    app.connect_activate(|app| {
        info!("activating application");
        configure_favicon_database();
        let win = window::create_window(app);
        win.show_all();
    });

    info!("running GTK application");
    app.run();
}

fn configure_favicon_database() {
    let favicon_dir = glib::user_data_dir().join(APP_DIR).join(FAVICON_DIR);

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
}
