use gtk::prelude::*;
use log::info;
use webkit2gtk::WebViewExt;

use crate::command_bar::CommandBar;
use crate::keybindings;
use crate::modes::{self, ModeState};
use crate::password_manager::PasswordManager;
use crate::session;
use crate::tab;

const DEFAULT_URL: &str = "https://start.duckduckgo.com";
const DEFAULT_WIDTH: i32 = 1024;
const DEFAULT_HEIGHT: i32 = 768;

pub fn create_window(app: &gtk::Application) -> gtk::ApplicationWindow {
    let mode_state = modes::new_mode_state();
    let hint_buffer = modes::new_hint_buffer();
    let new_tab_flag = modes::new_tab_flag();
    let g_prefix = modes::new_g_prefix();
    let password_manager = PasswordManager::new();

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("qust")
        .default_width(DEFAULT_WIDTH)
        .default_height(DEFAULT_HEIGHT)
        .build();

    info!(
        "created application window ({}x{})",
        DEFAULT_WIDTH, DEFAULT_HEIGHT
    );

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let notebook = tab::create_notebook();

    match session::load() {
        Some(session) => {
            info!("restoring session with {} tabs", session.tabs.len());
            tab::set_group_snapshots(&notebook, &session.groups);
            for tab_snapshot in &session.tabs {
                tab::add_unloaded_tab_snapshot(&notebook, tab_snapshot);
            }
            let active = session.active.min(notebook.n_pages().saturating_sub(1));
            notebook.set_current_page(Some(active));
            tab::ensure_current_page_visible(&notebook);
            info!("restored active tab: {}", active);
        }
        None => {
            info!("no session to restore, opening default URL");
            tab::add_tab(&notebook, DEFAULT_URL);
        }
    }

    tab::connect_lazy_loading(&notebook);
    tab::load_current_tab(&notebook);

    vbox.pack_start(&notebook, true, true, 0);

    let command_bar = CommandBar::new();
    command_bar.connect_url_updates(&notebook);
    if let Some(webview) = tab::current_webview(&notebook) {
        command_bar.update_zoom_label(webview.zoom_level());
    }
    let zoom_bar = command_bar.clone();
    notebook.connect_switch_page(move |_, page, _| {
        if let Ok(webview) = page.clone().downcast::<webkit2gtk::WebView>() {
            zoom_bar.update_zoom_label(webview.zoom_level());
        }
    });
    command_bar.connect_activate(
        mode_state.clone(),
        new_tab_flag.clone(),
        notebook.clone(),
        window.clone(),
        password_manager.clone(),
    );
    vbox.pack_start(&command_bar.container, false, false, 0);

    setup_key_handler(
        &window,
        &notebook,
        &command_bar,
        &mode_state,
        &hint_buffer,
        &new_tab_flag,
        &g_prefix,
    );

    let nb_for_close = notebook.clone();
    window.connect_delete_event(move |_, _| {
        info!("window closing, saving session");
        let tabs = tab::tab_snapshots(&nb_for_close);
        let groups = tab::group_snapshots(&nb_for_close);
        let active = nb_for_close.current_page().unwrap_or(0);
        session::save(tabs, groups, active);
        glib::Propagation::Proceed
    });

    window.add(&vbox);
    window
}

fn setup_key_handler(
    window: &gtk::ApplicationWindow,
    notebook: &gtk::Notebook,
    command_bar: &CommandBar,
    mode_state: &ModeState,
    hint_buffer: &modes::HintBuffer,
    new_tab_flag: &modes::NewTabFlag,
    g_prefix: &modes::GPrefix,
) {
    let ms = mode_state.clone();
    let hb = hint_buffer.clone();
    let ntf = new_tab_flag.clone();
    let nb = notebook.clone();
    let cb = command_bar.clone();
    let gp = g_prefix.clone();

    window.connect_key_press_event(move |_, event| {
        keybindings::handle_key_press(event, &ms, &hb, &ntf, &gp, &nb, &cb)
    });
}
