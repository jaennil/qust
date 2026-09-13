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
const WINDOW_STYLE_CLASS: &str = "qust-window";
const KEY_CONTROLLER_KEY: &str = "qust-key-controller";
const WINDOW_NOTEBOOK_KEY: &str = "qust-window-notebook";
const WINDOW_CSS: &[u8] = br#"
.qust-window {
    font-family: "Adwaita Sans", "Noto Sans", sans-serif;
    letter-spacing: 0;
}
"#;

pub fn create_window(app: &gtk::Application) -> gtk::ApplicationWindow {
    let mode_state = modes::new_mode_state();
    let hint_buffer = modes::new_hint_buffer();
    let new_tab_flag = modes::new_tab_flag();
    let normal_prefix = modes::new_normal_prefix();
    let password_manager = PasswordManager::new();

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("qust")
        .default_width(DEFAULT_WIDTH)
        .default_height(DEFAULT_HEIGHT)
        .build();
    window.style_context().add_class(WINDOW_STYLE_CLASS);
    if let Some(screen) = gtk::gdk::Screen::default() {
        let provider = gtk::CssProvider::new();
        provider
            .load_from_data(WINDOW_CSS)
            .expect("valid window CSS");
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

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
    tab::prewarm_unloaded_tabs(&notebook);

    if let Some(tab_bar) = tab::tab_bar(&notebook) {
        vbox.pack_start(&tab_bar, false, false, 0);
    }
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
        &normal_prefix,
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
    unsafe {
        window.set_data(WINDOW_NOTEBOOK_KEY, notebook);
    }
    window
}

pub fn notebook(window: &gtk::ApplicationWindow) -> Option<gtk::Notebook> {
    unsafe {
        window
            .data::<gtk::Notebook>(WINDOW_NOTEBOOK_KEY)
            .map(|notebook| notebook.as_ref().clone())
    }
}

pub fn open_urls(window: &gtk::ApplicationWindow, urls: &[String]) {
    let Some(notebook) = notebook(window) else {
        log::warn!("cannot open urls: window has no notebook");
        return;
    };

    for url in urls {
        info!("opening url in a new tab: {}", url);
        tab::add_tab(&notebook, url);
    }

    if !urls.is_empty() {
        tab::ensure_current_page_visible(&notebook);
    }
    window.present();
}

fn setup_key_handler(
    window: &gtk::ApplicationWindow,
    notebook: &gtk::Notebook,
    command_bar: &CommandBar,
    mode_state: &ModeState,
    hint_buffer: &modes::HintBuffer,
    new_tab_flag: &modes::NewTabFlag,
    normal_prefix: &modes::NormalPrefixState,
) -> gtk::EventControllerKey {
    let ms = mode_state.clone();
    let hb = hint_buffer.clone();
    let ntf = new_tab_flag.clone();
    let nb = notebook.clone();
    let cb = command_bar.clone();
    let np = normal_prefix.clone();

    let controller = gtk::EventControllerKey::new(window);
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    controller.connect_key_pressed(move |_, keyval, _, state| {
        matches!(
            keybindings::handle_key_press((keyval.into(), state), &ms, &hb, &ntf, &np, &nb, &cb,),
            glib::Propagation::Stop
        )
    });
    unsafe {
        window.set_data(KEY_CONTROLLER_KEY, controller.clone());
    }
    controller
}

#[cfg(test)]
mod tests {
    use super::setup_key_handler;
    use crate::{command_bar::CommandBar, modes, tab};
    use gtk::prelude::*;

    #[test]
    #[ignore = "requires a graphical display"]
    fn normal_keys_are_captured_before_webview() {
        gtk::init().expect("GTK display");
        let window = gtk::ApplicationWindow::builder().build();
        let notebook = tab::create_notebook();
        tab::add_unloaded_tab(&notebook, "about:blank");
        let command_bar = CommandBar::new();
        let mode_state = modes::new_mode_state();
        let hint_buffer = modes::new_hint_buffer();
        let new_tab_flag = modes::new_tab_flag();
        let normal_prefix = modes::new_normal_prefix();
        let controller = setup_key_handler(
            &window,
            &notebook,
            &command_bar,
            &mode_state,
            &hint_buffer,
            &new_tab_flag,
            &normal_prefix,
        );
        assert_eq!(
            controller.propagation_phase(),
            gtk::PropagationPhase::Capture
        );

        let handled: bool = controller.emit_by_name(
            "key-pressed",
            &[
                &*gdk::keys::constants::j,
                &0u32,
                &gdk::ModifierType::empty(),
            ],
        );
        assert!(handled);

        let handled: bool = controller.emit_by_name(
            "key-pressed",
            &[
                &*gdk::keys::constants::q,
                &0u32,
                &gdk::ModifierType::empty(),
            ],
        );
        assert!(!handled);
        window.close();
    }
}
