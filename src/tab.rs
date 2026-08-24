use gtk::prelude::*;
use gtk::{cairo, gdk_pixbuf};
use log::{error, info, warn};
use serde::{Deserialize, Deserializer, Serialize};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};
use webkit2gtk::{LoadEvent, SettingsExt, WebView, WebViewExt};

const TAB_WIDTH_CHARS: i32 = 20;
const FAVICON_SIZE: i32 = 16;
const LAZY_LOAD_DELAY: Duration = Duration::from_millis(75);
const PREWARM_INITIAL_DELAY_MS: u64 = 500;
const PREWARM_INTERVAL_MS: u64 = 150;
const TAB_LABEL_MIN_WIDTH: i32 = 80;
const TAB_LABEL_MAX_WIDTH: i32 = 220;
const TAB_LABEL_SPACING: i32 = 6;
const PINNED_TAB_LABEL_WIDTH: i32 = FAVICON_SIZE;
const PINNED_TAB_RESERVED_WIDTH: i32 = 28;
const TAB_BAR_END_RESERVED_WIDTH: i32 = 24;
const PENDING_URI_KEY: &str = "qust-pending-uri";
const TAB_META_KEY: &str = "qust-tab-meta";
const TAB_LABEL_KEY: &str = "qust-tab-label";
const TAB_STATUS_KEY: &str = "qust-tab-status";
const TAB_TITLE_KEY: &str = "qust-tab-title";
const TAB_ICON_KEY: &str = "qust-tab-icon";
const TAB_FAVICON_KEY: &str = "qust-tab-favicon";
const TAB_CACHED_TITLE_KEY: &str = "qust-tab-cached-title";
const TAB_PREWARMING_KEY: &str = "qust-tab-prewarming";
const TAB_PREWARMED_KEY: &str = "qust-tab-prewarmed";
const GROUPS_KEY: &str = "qust-tab-groups";
const TAB_ICON_CHILD: &str = "icon";
const TAB_LOADING_CHILD: &str = "loading";
const NOTEBOOK_STYLE_CLASS: &str = "qust-notebook";
const NOTEBOOK_CSS: &[u8] = br#"
.qust-notebook {
    -GtkNotebook-has-backward-stepper: false;
    -GtkNotebook-has-secondary-backward-stepper: false;
}
.qust-notebook tab {
    padding-left: 0;
    padding-right: 0;
}
.qust-tab-label {
    padding-left: 3px;
    padding-right: 3px;
    border-bottom: 3px solid transparent;
}
.qust-group-line-blue { border-bottom-color: #2f9bff; }
.qust-group-line-purple { border-bottom-color: #a970ff; }
.qust-group-line-cyan { border-bottom-color: #22c8dc; }
.qust-group-line-orange { border-bottom-color: #f28b3c; }
.qust-group-line-yellow { border-bottom-color: #e8c238; }
.qust-group-line-pink { border-bottom-color: #eb68ad; }
.qust-group-line-green { border-bottom-color: #45bd72; }
.qust-group-line-red { border-bottom-color: #e45b5b; }
.qust-group-line-gray { border-bottom-color: #9298a3; }
.qust-notebook > header > tabs > arrow:first-child {
    opacity: 0;
    min-width: 0;
    min-height: 0;
    margin: 0;
    padding: 0;
    border: 0;
    background: none;
    -gtk-icon-source: none;
}
.qust-group-badge {
    border-radius: 4px;
    padding: 1px 4px;
}
.qust-group-blue { background-color: rgba(0, 120, 212, 0.32); color: #8ecbff; }
.qust-group-purple { background-color: rgba(145, 90, 220, 0.32); color: #d0aeff; }
.qust-group-cyan { background-color: rgba(0, 155, 180, 0.32); color: #83e7f2; }
.qust-group-orange { background-color: rgba(220, 105, 25, 0.34); color: #ffbd82; }
.qust-group-yellow { background-color: rgba(205, 160, 0, 0.34); color: #ffe178; }
.qust-group-pink { background-color: rgba(210, 70, 145, 0.32); color: #ffabd5; }
.qust-group-green { background-color: rgba(35, 155, 85, 0.32); color: #8ce0a9; }
.qust-group-red { background-color: rgba(205, 65, 65, 0.34); color: #ff9c9c; }
.qust-group-gray { background-color: rgba(130, 135, 145, 0.32); color: #d0d3d8; }
"#;
const GROUP_COLORS: &[&str] = &[
    "blue", "purple", "cyan", "orange", "yellow", "pink", "green", "red", "gray",
];

pub struct Tab {
    pub webview: WebView,
    pub label: gtk::Box,
}

#[derive(Clone, Debug, Serialize)]
pub struct TabSnapshot {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favicon: Option<String>,
}

impl<'de> Deserialize<'de> for TabSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let compat = TabSnapshotCompat::deserialize(deserializer)?;
        Ok(match compat {
            TabSnapshotCompat::Url(url) => TabSnapshot {
                url,
                title: None,
                pinned: false,
                group: None,
                favicon: None,
            },
            TabSnapshotCompat::State {
                url,
                title,
                pinned,
                group,
                favicon,
            } => TabSnapshot {
                url,
                title,
                pinned,
                group: clean_group_name(group.as_deref()),
                favicon,
            },
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum TabSnapshotCompat {
    Url(String),
    State {
        url: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        pinned: bool,
        #[serde(default)]
        group: Option<String>,
        #[serde(default)]
        favicon: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabGroupSnapshot {
    pub name: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct TabMeta {
    pinned: bool,
    group: Option<String>,
}

#[derive(Clone, Debug)]
struct GroupState {
    name: String,
    collapsed: bool,
    color: String,
}

impl Tab {
    fn with_webview(url: &str, webview: WebView) -> Self {
        if std::env::var_os("QUST_WEBKIT_CONSOLE").is_some() {
            if let Some(settings) = WebViewExt::settings(&webview) {
                settings.set_enable_write_console_messages_to_stdout(true);
            }
        }
        if let Some(user_agent) = std::env::var_os("QUST_USER_AGENT") {
            if let (Some(settings), Some(user_agent)) =
                (WebViewExt::settings(&webview), user_agent.to_str())
            {
                settings.set_user_agent(Some(user_agent));
            }
        }
        unsafe {
            webview.set_data(PENDING_URI_KEY, url.to_string());
            webview.set_data(TAB_META_KEY, Rc::new(RefCell::new(TabMeta::default())));
        }
        info!("new tab created, pending load: {}", url);

        let label = gtk::Box::new(gtk::Orientation::Horizontal, TAB_LABEL_SPACING);
        label.style_context().add_class("qust-tab-label");
        label.set_size_request(TAB_LABEL_MAX_WIDTH, -1);

        let icon = gtk::Image::from_icon_name(Some("text-html-symbolic"), gtk::IconSize::Menu);
        icon.set_pixel_size(FAVICON_SIZE);
        icon.set_size_request(FAVICON_SIZE, FAVICON_SIZE);

        let spinner = gtk::Spinner::new();
        spinner.set_size_request(FAVICON_SIZE, FAVICON_SIZE);

        let icon_stack = gtk::Stack::new();
        icon_stack.set_size_request(FAVICON_SIZE, FAVICON_SIZE);
        icon_stack.add_named(&icon, TAB_ICON_CHILD);
        icon_stack.add_named(&spinner, TAB_LOADING_CHILD);
        icon_stack.set_visible_child_name(TAB_ICON_CHILD);

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.style_context().add_class("qust-group-badge");
        label.pack_start(&status, false, false, 0);
        label.pack_start(&icon_stack, false, false, 0);
        unsafe {
            webview.set_data(TAB_STATUS_KEY, status.clone());
            webview.set_data(TAB_ICON_KEY, icon.clone());
        }

        let title = gtk::Label::new(Some("New Tab"));
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_width_chars(1);
        title.set_max_width_chars(TAB_WIDTH_CHARS);
        title.set_xalign(0.0);
        label.pack_start(&title, true, true, 0);
        unsafe {
            webview.set_data(TAB_LABEL_KEY, label.clone());
            webview.set_data(TAB_TITLE_KEY, title.clone());
        }

        let title_label = title.clone();
        webview.connect_title_notify(move |wv| {
            if is_prewarming(wv) {
                return;
            }
            if let Some(title) = wv.title() {
                let title_str = title.to_string();
                info!("tab title changed: {}", title_str);
                title_label.set_text(&title_str);
                unsafe {
                    wv.set_data(TAB_CACHED_TITLE_KEY, title_str);
                }
            }
        });

        let icon_image = icon.clone();
        webview.connect_favicon_notify(move |wv| {
            if is_prewarming(wv) {
                return;
            }
            if let Some(favicon) = wv.favicon() {
                info!("tab favicon changed");
                set_favicon(&icon_image, &favicon);
                if let Some(data_uri) = favicon_data_uri(&favicon) {
                    unsafe {
                        wv.set_data(TAB_FAVICON_KEY, data_uri);
                    }
                }
                icon_image.show();
            } else if imported_favicon(wv).is_none() {
                icon_image.set_from_icon_name(Some("text-html-symbolic"), gtk::IconSize::Menu);
            }
        });

        let loading_stack = icon_stack.clone();
        let loading_spinner = spinner.clone();
        webview.connect_load_changed(move |webview, event| {
            info!("load event {:?}: {:?}", event, webview.uri());
            if is_prewarming(webview) {
                if event == LoadEvent::Finished {
                    set_prewarming(webview, false);
                    set_prewarmed(webview, true);
                }
                return;
            }
            match event {
                LoadEvent::Started | LoadEvent::Redirected | LoadEvent::Committed => {
                    loading_spinner.start();
                    loading_stack.set_visible_child_name(TAB_LOADING_CHILD);
                }
                LoadEvent::Finished => {
                    loading_spinner.stop();
                    loading_stack.set_visible_child_name(TAB_ICON_CHILD);
                }
                _ => {}
            }
        });

        let failed_stack = icon_stack.clone();
        let failed_spinner = spinner.clone();
        webview.connect_load_failed(move |_, event, uri, load_error| {
            error!("load failed during {:?} for {}: {}", event, uri, load_error);
            failed_spinner.stop();
            failed_stack.set_visible_child_name(TAB_ICON_CHILD);
            false
        });

        webview.connect_load_failed_with_tls_errors(move |_, uri, _, tls_errors| {
            error!("TLS load failed for {}: {:?}", uri, tls_errors);
            false
        });

        webview.connect_web_process_terminated(move |webview, reason| {
            warn!(
                "web process terminated for {:?}: {:?}",
                webview.uri(),
                reason
            );
        });

        label.show_all();

        Tab { webview, label }
    }
}

fn set_favicon(icon: &gtk::Image, surface: &cairo::Surface) {
    let Some(pixbuf) = favicon_pixbuf(surface) else {
        icon.set_from_surface(Some(surface));
        return;
    };

    icon.set_from_pixbuf(Some(&pixbuf));
}

fn favicon_pixbuf(surface: &cairo::Surface) -> Option<gdk_pixbuf::Pixbuf> {
    let image = cairo::ImageSurface::try_from(surface.clone()).ok()?;
    let pixbuf = gdk::pixbuf_get_from_surface(surface, 0, 0, image.width(), image.height())?;

    if pixbuf.width() == FAVICON_SIZE && pixbuf.height() == FAVICON_SIZE {
        return Some(pixbuf);
    }

    pixbuf.scale_simple(FAVICON_SIZE, FAVICON_SIZE, gdk_pixbuf::InterpType::Bilinear)
}

fn favicon_data_uri(surface: &cairo::Surface) -> Option<String> {
    let mut png = Vec::new();
    surface.write_to_png(&mut png).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        glib::base64_encode(&png)
    ))
}

fn set_imported_favicon(webview: &WebView, data_uri: &str) {
    let Some(pixbuf) = favicon_from_data_uri(data_uri) else {
        warn!("failed to decode imported favicon");
        return;
    };
    if let Some(icon) = tab_icon(webview) {
        icon.set_from_pixbuf(Some(&pixbuf));
    }
}

fn favicon_from_data_uri(data_uri: &str) -> Option<gdk_pixbuf::Pixbuf> {
    let bytes = favicon_bytes(data_uri)?;
    let loader = gdk_pixbuf::PixbufLoader::new();
    if loader.write(&bytes).is_err() || loader.close().is_err() {
        return None;
    }
    let pixbuf = loader.pixbuf()?;
    let pixbuf = if pixbuf.width() == FAVICON_SIZE && pixbuf.height() == FAVICON_SIZE {
        pixbuf
    } else {
        pixbuf.scale_simple(FAVICON_SIZE, FAVICON_SIZE, gdk_pixbuf::InterpType::Bilinear)?
    };
    Some(pixbuf)
}

fn favicon_bytes(data_uri: &str) -> Option<Vec<u8>> {
    let (_, encoded) = data_uri.split_once(";base64,")?;
    Some(glib::base64_decode(encoded))
}

pub fn create_notebook() -> gtk::Notebook {
    let notebook = gtk::Notebook::new();
    notebook.style_context().add_class(NOTEBOOK_STYLE_CLASS);
    if let Some(screen) = gdk::Screen::default() {
        let provider = gtk::CssProvider::new();
        provider
            .load_from_data(NOTEBOOK_CSS)
            .expect("valid notebook CSS");
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    notebook.set_scrollable(true);
    notebook.set_show_tabs(true);
    notebook.set_tab_pos(gtk::PositionType::Top);
    let width_notebook = notebook.clone();
    notebook.connect_size_allocate(move |_, allocation| {
        update_tab_widths(&width_notebook, allocation.width());
    });
    unsafe {
        notebook.set_data(GROUPS_KEY, Rc::new(RefCell::new(Vec::<GroupState>::new())));
    }
    info!("notebook created");
    notebook
}

pub fn connect_lazy_loading(notebook: &gtk::Notebook) {
    let nb = notebook.clone();
    notebook.connect_switch_page(move |_, _, page_num| {
        schedule_load_page(&nb, page_num);
    });
}

pub fn add_unloaded_tab(notebook: &gtk::Notebook, url: &str) -> Tab {
    let snapshot = TabSnapshot {
        url: url.to_string(),
        title: None,
        pinned: false,
        group: None,
        favicon: None,
    };
    add_unloaded_tab_snapshot(notebook, &snapshot)
}

pub fn add_unloaded_tab_snapshot(notebook: &gtk::Notebook, snapshot: &TabSnapshot) -> Tab {
    add_unloaded_tab_snapshot_with_webview(notebook, snapshot, WebView::new())
}

fn add_unloaded_tab_snapshot_with_webview(
    notebook: &gtk::Notebook,
    snapshot: &TabSnapshot,
    webview: WebView,
) -> Tab {
    if let Some(group) = snapshot.group.as_deref() {
        ensure_group(notebook, group);
    }

    let tab = Tab::with_webview(&snapshot.url, webview);
    if let Some(title) = snapshot.title.as_deref() {
        set_cached_title(&tab.webview, title);
    }
    if let Some(favicon) = snapshot.favicon.as_deref() {
        set_imported_favicon(&tab.webview, favicon);
    }
    unsafe {
        if let Some(favicon) = snapshot.favicon.clone() {
            tab.webview.set_data(TAB_FAVICON_KEY, favicon);
        }
    }
    set_meta(
        &tab.webview,
        TabMeta {
            pinned: snapshot.pinned,
            group: snapshot.group.clone(),
        },
    );
    let tab_event_box = gtk::EventBox::new();
    tab_event_box.set_visible_window(false);
    tab_event_box.add_events(gdk::EventMask::SCROLL_MASK | gdk::EventMask::SMOOTH_SCROLL_MASK);
    tab_event_box.add(&tab.label);
    connect_tab_scroll(&tab_event_box, notebook);
    let page_num = notebook.append_page(&tab.webview, Some(&tab_event_box));
    connect_new_window(&tab.webview, notebook);
    info!("unloaded tab added at page {}", page_num);
    update_layout(notebook);
    tab
}

fn connect_tab_scroll(tab_label: &gtk::EventBox, notebook: &gtk::Notebook) {
    let notebook = notebook.clone();
    let smooth_delta = Rc::new(Cell::new(0.0));
    tab_label.connect_scroll_event(move |_, event| {
        let step = match event.direction() {
            gdk::ScrollDirection::Up | gdk::ScrollDirection::Left => Some(-1),
            gdk::ScrollDirection::Down | gdk::ScrollDirection::Right => Some(1),
            gdk::ScrollDirection::Smooth => {
                let (dx, dy) = event.delta();
                let delta = if dx.abs() > dy.abs() { dx } else { dy };
                let accumulated = smooth_delta.get() + delta;
                if accumulated.abs() < 1.0 {
                    smooth_delta.set(accumulated);
                    None
                } else {
                    smooth_delta.set(0.0);
                    Some(if accumulated.is_sign_negative() {
                        -1
                    } else {
                        1
                    })
                }
            }
            _ => None,
        };

        match step {
            Some(-1) => prev_tab(&notebook),
            Some(1) => next_tab(&notebook),
            _ => {}
        }
        glib::Propagation::Stop
    });
}

fn connect_new_window(webview: &WebView, notebook: &gtk::Notebook) {
    let notebook = notebook.clone();
    webview.connect_create(move |opener, _| {
        info!("opening requested web window in a new tab");
        let snapshot = TabSnapshot {
            url: "about:blank".to_string(),
            title: None,
            pinned: false,
            group: None,
            favicon: None,
        };
        let related_webview = WebView::with_related_view(opener);
        let tab = add_unloaded_tab_snapshot_with_webview(&notebook, &snapshot, related_webview);
        take_pending_uri(&tab.webview);

        if let Some(page) = notebook.page_num(&tab.webview) {
            notebook.set_current_page(Some(page));
        }
        notebook.show_all();
        update_layout(&notebook);

        Some(tab.webview.upcast::<gtk::Widget>())
    });
}

pub fn add_tab(notebook: &gtk::Notebook, url: &str) -> Tab {
    let tab = add_unloaded_tab(notebook, url);
    let page_num = notebook.page_num(&tab.webview).unwrap_or(0);
    info!("tab added at page {}", page_num);
    notebook.set_current_page(Some(page_num));
    notebook.show_all();
    update_layout(notebook);
    schedule_load_page(notebook, page_num);
    tab
}

pub fn import_tabs(
    notebook: &gtk::Notebook,
    tabs: &[TabSnapshot],
    imported_groups: &[TabGroupSnapshot],
) -> (usize, usize) {
    if let Some(groups) = groups(notebook) {
        let mut state = groups.borrow_mut();
        for group in imported_groups {
            if let Some(existing) = state.iter_mut().find(|item| item.name == group.name) {
                existing.collapsed = group.collapsed;
                if let Some(color) = normalize_group_color(group.color.as_deref()) {
                    existing.color = color;
                }
            } else {
                let color = normalize_group_color(group.color.as_deref())
                    .unwrap_or_else(|| default_group_color(state.len()));
                state.push(GroupState {
                    name: group.name.clone(),
                    collapsed: group.collapsed,
                    color,
                });
            }
        }
    }
    let mut existing: Vec<(String, WebView, bool)> = (0..notebook.n_pages())
        .filter_map(|page| webview_at(notebook, page))
        .map(|webview| {
            let url = pending_uri(&webview)
                .or_else(|| webview.uri().map(|uri| uri.to_string()))
                .unwrap_or_default();
            (url, webview, false)
        })
        .collect();
    let mut added = 0;
    let mut updated = 0;
    let mut first_added = None;
    for snapshot in tabs {
        if let Some((_, webview, used)) = existing
            .iter_mut()
            .find(|(url, _, used)| !*used && url == &snapshot.url)
        {
            *used = true;
            set_meta(
                webview,
                TabMeta {
                    pinned: snapshot.pinned,
                    group: snapshot.group.clone(),
                },
            );
            if let Some(favicon) = snapshot.favicon.as_deref() {
                set_imported_favicon(webview, favicon);
                unsafe {
                    webview.set_data(TAB_FAVICON_KEY, favicon.to_string());
                }
            }
            if let Some(title) = snapshot.title.as_deref() {
                set_cached_title(webview, title);
            }
            updated += 1;
        } else {
            let tab = add_unloaded_tab_snapshot(notebook, snapshot);
            if first_added.is_none() {
                first_added = Some(tab.webview);
            }
            added += 1;
        }
    }
    if !tabs.is_empty() {
        notebook.show_all();
        update_layout(notebook);
        if let Some(webview) = first_added {
            if let Some(page) = notebook.page_num(&webview) {
                notebook.set_current_page(Some(page));
                schedule_load_page(notebook, page);
            }
        }
        prewarm_unloaded_tabs(notebook);
    }
    (added, updated)
}

pub fn load_current_tab(notebook: &gtk::Notebook) {
    if let Some(current) = notebook.current_page() {
        schedule_load_page(notebook, current);
    }
}

pub fn prewarm_unloaded_tabs(notebook: &gtk::Notebook) {
    let mut offset = 0_u64;
    for page in 0..notebook.n_pages() {
        let Some(webview) = webview_at(notebook, page) else {
            continue;
        };
        if pending_uri(&webview).is_none() || is_prewarmed(&webview) || is_prewarming(&webview) {
            continue;
        }
        let delay = Duration::from_millis(
            PREWARM_INITIAL_DELAY_MS + offset.saturating_mul(PREWARM_INTERVAL_MS),
        );
        offset += 1;
        glib::timeout_add_local_once(delay, move || {
            if pending_uri(&webview).is_none() || is_prewarmed(&webview) || is_prewarming(&webview)
            {
                return;
            }
            info!("prewarming unloaded tab {}", page);
            set_prewarming(&webview, true);
            webview.load_uri("about:blank");
        });
    }
}

fn schedule_load_page(notebook: &gtk::Notebook, page_num: u32) {
    let Some(widget) = notebook.nth_page(Some(page_num)) else {
        return;
    };
    let Ok(webview) = widget.downcast::<WebView>() else {
        return;
    };

    // Let GTK paint the selected tab before WebKit starts a web process.
    glib::timeout_add_local_once(LAZY_LOAD_DELAY, move || {
        load_pending_webview(&webview);
    });
}

fn load_pending_webview(webview: &WebView) {
    let Some(url) = take_pending_uri(webview) else {
        return;
    };

    set_prewarming(webview, false);
    info!("loading tab after layout: {}", url);
    let started = Instant::now();
    webview.load_uri(&url);
    info!("submitted tab load in {:?}: {}", started.elapsed(), url);
}

fn take_pending_uri(webview: &WebView) -> Option<String> {
    unsafe { webview.steal_data::<String>(PENDING_URI_KEY) }
}

fn pending_uri(webview: &WebView) -> Option<String> {
    unsafe {
        webview
            .data::<String>(PENDING_URI_KEY)
            .map(|uri| uri.as_ref().clone())
    }
}

fn set_prewarming(webview: &WebView, prewarming: bool) {
    unsafe {
        webview.set_data(TAB_PREWARMING_KEY, prewarming);
    }
}

fn is_prewarming(webview: &WebView) -> bool {
    unsafe {
        webview
            .data::<bool>(TAB_PREWARMING_KEY)
            .is_some_and(|value| *value.as_ref())
    }
}

fn set_prewarmed(webview: &WebView, prewarmed: bool) {
    unsafe {
        webview.set_data(TAB_PREWARMED_KEY, prewarmed);
    }
}

fn is_prewarmed(webview: &WebView) -> bool {
    unsafe {
        webview
            .data::<bool>(TAB_PREWARMED_KEY)
            .is_some_and(|value| *value.as_ref())
    }
}

pub fn close_current_tab(notebook: &gtk::Notebook) {
    let n_pages = notebook.n_pages();
    if n_pages <= 1 {
        info!("only one tab left, not closing");
        return;
    }
    if let Some(current) = notebook.current_page() {
        info!("closing tab {}", current);
        notebook.remove_page(Some(current));
        remove_empty_groups(notebook);
        update_layout(notebook);
        ensure_current_page_visible(notebook);
    }
}

pub fn next_tab(notebook: &gtk::Notebook) {
    let visible = visible_pages(notebook);
    if let Some(current) = notebook.current_page() {
        let Some(next) = adjacent_page(&visible, current, 1) else {
            return;
        };
        info!("switching to next tab: {} -> {}", current, next);
        notebook.set_current_page(Some(next));
    }
}

pub fn prev_tab(notebook: &gtk::Notebook) {
    let visible = visible_pages(notebook);
    if let Some(current) = notebook.current_page() {
        let Some(prev) = adjacent_page(&visible, current, -1) else {
            return;
        };
        info!("switching to prev tab: {} -> {}", current, prev);
        notebook.set_current_page(Some(prev));
    }
}

fn adjacent_page(visible: &[u32], current: u32, offset: isize) -> Option<u32> {
    let position = visible.iter().position(|page| *page == current)?;
    visible.get(position.checked_add_signed(offset)?).copied()
}

pub fn first_tab(notebook: &gtk::Notebook) {
    if let Some(page) = visible_pages(notebook).first() {
        info!("switching to first tab: {}", page);
        notebook.set_current_page(Some(*page));
    }
}

pub fn last_tab(notebook: &gtk::Notebook) {
    if let Some(page) = visible_pages(notebook).last() {
        info!("switching to last tab: {}", page);
        notebook.set_current_page(Some(*page));
    }
}

pub fn move_current_tab_left(notebook: &gtk::Notebook) {
    move_current_tab(notebook, -1);
}

pub fn move_current_tab_right(notebook: &gtk::Notebook) {
    move_current_tab(notebook, 1);
}

fn move_current_tab(notebook: &gtk::Notebook, offset: isize) {
    let Some(current) = notebook.current_page() else {
        return;
    };
    let visible = visible_pages(notebook);
    let Some(target) = move_target(&visible, current, offset) else {
        return;
    };
    let Some(current_webview) = webview_at(notebook, current) else {
        return;
    };
    notebook.reorder_child(&current_webview, Some(target));
    if let Some(page) = notebook.page_num(&current_webview) {
        notebook.set_current_page(Some(page));
    }
    info!("moved current tab from {} to {}", current, target);
}

fn move_target(visible: &[u32], current: u32, offset: isize) -> Option<u32> {
    let position = visible.iter().position(|page| *page == current)?;
    let target = position.checked_add_signed(offset)?;
    visible.get(target).copied()
}

pub fn current_webview(notebook: &gtk::Notebook) -> Option<WebView> {
    let page = notebook.current_page()?;
    let widget = notebook.nth_page(Some(page))?;
    widget.downcast::<WebView>().ok()
}

pub fn display_url(webview: &WebView) -> String {
    webview
        .uri()
        .map(|uri| uri.to_string())
        .or_else(|| pending_uri(webview))
        .unwrap_or_default()
}

pub fn tab_snapshots(notebook: &gtk::Notebook) -> Vec<TabSnapshot> {
    let n_pages = notebook.n_pages();
    let mut urls = Vec::with_capacity(n_pages as usize);

    for i in 0..n_pages {
        if let Some(widget) = notebook.nth_page(Some(i)) {
            if let Ok(webview) = widget.downcast::<WebView>() {
                let meta = meta(&webview);
                let url = pending_uri(&webview)
                    .or_else(|| webview.uri().map(|u| u.to_string()))
                    .unwrap_or_default();
                info!("tab {}: {}", i, url);
                urls.push(TabSnapshot {
                    url,
                    title: cached_title(&webview),
                    pinned: meta.pinned,
                    group: meta.group,
                    favicon: imported_favicon(&webview),
                });
            }
        }
    }

    info!("collected {} tab snapshots", urls.len());
    urls
}

pub fn group_snapshots(notebook: &gtk::Notebook) -> Vec<TabGroupSnapshot> {
    let Some(groups) = groups(notebook) else {
        return Vec::new();
    };

    let snapshots = groups
        .borrow()
        .iter()
        .map(|group| TabGroupSnapshot {
            name: group.name.clone(),
            collapsed: group.collapsed,
            color: Some(group.color.clone()),
        })
        .collect();
    snapshots
}

pub fn set_group_snapshots(notebook: &gtk::Notebook, snapshots: &[TabGroupSnapshot]) {
    let Some(groups) = groups(notebook) else {
        return;
    };

    let mut state = groups.borrow_mut();
    state.clear();
    for snapshot in snapshots {
        let Some(name) = clean_group_name(Some(&snapshot.name)) else {
            continue;
        };
        if state.iter().any(|group| group.name == name) {
            continue;
        }
        let color = normalize_group_color(snapshot.color.as_deref())
            .unwrap_or_else(|| default_group_color(state.len()));
        state.push(GroupState {
            name,
            collapsed: snapshot.collapsed,
            color,
        });
    }
}

pub fn create_group(notebook: &gtk::Notebook, name: &str) {
    let Some(name) = clean_group_name(Some(name)) else {
        log::warn!("group name is empty");
        return;
    };

    ensure_group(notebook, &name);
    add_current_to_group(notebook, &name);
}

pub fn add_current_to_group(notebook: &gtk::Notebook, name: &str) {
    let Some(name) = clean_group_name(Some(name)) else {
        log::warn!("group name is empty");
        return;
    };

    ensure_group(notebook, &name);
    let Some(webview) = current_webview(notebook) else {
        return;
    };

    if let Some(meta) = meta_cell(&webview) {
        let mut meta = meta.borrow_mut();
        meta.group = Some(name.clone());
        meta.pinned = false;
    }

    info!("added current tab to group: {}", name);
    update_layout(notebook);
    ensure_current_page_visible(notebook);
}

pub fn collapse_group(notebook: &gtk::Notebook, name: Option<&str>) {
    set_group_collapsed(notebook, name, true);
}

pub fn expand_group(notebook: &gtk::Notebook, name: Option<&str>) {
    set_group_collapsed(notebook, name, false);
}

pub fn toggle_group(notebook: &gtk::Notebook) {
    let Some(name) = resolve_group_name(notebook, None) else {
        log::warn!("no group selected");
        return;
    };
    set_group_collapsed(notebook, Some(&name), !group_collapsed(notebook, &name));
}

pub fn collapse_all_groups(notebook: &gtk::Notebook) {
    set_all_groups_collapsed(notebook, true);
}

pub fn expand_all_groups(notebook: &gtk::Notebook) {
    set_all_groups_collapsed(notebook, false);
}

pub fn toggle_current_pin(notebook: &gtk::Notebook) {
    let Some(webview) = current_webview(notebook) else {
        return;
    };

    if let Some(meta) = meta_cell(&webview) {
        let mut meta = meta.borrow_mut();
        meta.pinned = !meta.pinned;
        if meta.pinned {
            meta.group = None;
        }
        info!("current tab pinned: {}", meta.pinned);
    }

    remove_empty_groups(notebook);
    update_layout(notebook);
}

pub fn set_current_pin(notebook: &gtk::Notebook, pinned: bool) {
    let Some(webview) = current_webview(notebook) else {
        return;
    };

    if let Some(meta) = meta_cell(&webview) {
        let mut meta = meta.borrow_mut();
        meta.pinned = pinned;
        if meta.pinned {
            meta.group = None;
        }
        info!("current tab pinned: {}", meta.pinned);
    }

    remove_empty_groups(notebook);
    update_layout(notebook);
}

pub fn ensure_current_page_visible(notebook: &gtk::Notebook) {
    if let Some(current) = notebook.current_page() {
        if page_is_visible(notebook, current) {
            return;
        }
    }

    if let Some(first_visible) = visible_pages(notebook).first() {
        notebook.set_current_page(Some(*first_visible));
    }
}

fn set_group_collapsed(notebook: &gtk::Notebook, name: Option<&str>, collapsed: bool) {
    let Some(name) = resolve_group_name(notebook, name) else {
        log::warn!("no group selected");
        return;
    };
    let current_target = resolve_group_name(notebook, None)
        .filter(|current| current == &name)
        .and_then(|_| first_group_page(notebook, &name));
    let Some(groups) = groups(notebook) else {
        return;
    };

    let mut found = false;
    for group in groups.borrow_mut().iter_mut() {
        if group.name == name {
            group.collapsed = collapsed;
            found = true;
            break;
        }
    }

    if !found {
        log::warn!("unknown group: {}", name);
        return;
    }

    info!("group '{}' collapsed: {}", name, collapsed);
    update_layout(notebook);
    if collapsed {
        if let Some(page) = current_target {
            notebook.set_current_page(Some(page));
        }
    }
    ensure_current_page_visible(notebook);
}

fn set_all_groups_collapsed(notebook: &gtk::Notebook, collapsed: bool) {
    let current_target =
        resolve_group_name(notebook, None).and_then(|name| first_group_page(notebook, &name));
    let Some(groups) = groups(notebook) else {
        return;
    };
    for group in groups.borrow_mut().iter_mut() {
        group.collapsed = collapsed;
    }
    info!("all tab groups collapsed: {}", collapsed);
    update_layout(notebook);
    if collapsed {
        if let Some(page) = current_target {
            notebook.set_current_page(Some(page));
        }
    }
    ensure_current_page_visible(notebook);
}

fn update_layout(notebook: &gtk::Notebook) {
    reorder_tabs(notebook);
    apply_group_visibility(notebook);
    refresh_all_tab_labels(notebook);
    update_tab_widths(notebook, notebook.allocated_width());
}

fn update_tab_widths(notebook: &gtk::Notebook, total_width: i32) {
    if total_width <= 0 {
        return;
    }

    let visible: Vec<WebView> = visible_pages(notebook)
        .into_iter()
        .filter_map(|page| webview_at(notebook, page))
        .collect();
    let pinned_count = visible
        .iter()
        .filter(|webview| meta(webview).pinned)
        .count() as i32;
    let regular_count = visible.len() as i32 - pinned_count;
    if regular_count <= 0 {
        return;
    }

    let group_badge_width: i32 = visible
        .iter()
        .map(|webview| group_badge_extra_width(notebook, webview))
        .sum();
    let Some(tab_width) =
        regular_tab_width(total_width, pinned_count, regular_count, group_badge_width)
    else {
        return;
    };

    for webview in visible {
        if meta(&webview).pinned {
            continue;
        }
        if let Some(label) = tab_label(&webview) {
            let width = tab_width + group_badge_extra_width(notebook, &webview);
            label.set_size_request(width, -1);
        }
    }
}

fn group_badge_extra_width(notebook: &gtk::Notebook, webview: &WebView) -> i32 {
    let meta = meta(webview);
    if meta.pinned {
        return 0;
    }
    let Some(group) = meta.group else {
        return 0;
    };
    if notebook.page_num(webview) != first_group_page(notebook, &group) {
        return 0;
    }
    status_label(webview)
        .map(|status| status.preferred_width().1 + TAB_LABEL_SPACING)
        .unwrap_or(0)
}

fn regular_tab_width(
    total_width: i32,
    pinned_count: i32,
    regular_count: i32,
    group_badge_width: i32,
) -> Option<i32> {
    if total_width <= 0 || regular_count <= 0 {
        return None;
    }
    let available = total_width
        .saturating_sub(pinned_count.saturating_mul(PINNED_TAB_RESERVED_WIDTH))
        .saturating_sub(TAB_BAR_END_RESERVED_WIDTH)
        .saturating_sub(group_badge_width);
    Some((available / regular_count).clamp(TAB_LABEL_MIN_WIDTH, TAB_LABEL_MAX_WIDTH))
}

fn reorder_tabs(notebook: &gtk::Notebook) {
    let current_widget = notebook
        .current_page()
        .and_then(|page| notebook.nth_page(Some(page)));
    let order = ordered_widgets(notebook);

    for (position, widget) in order.iter().enumerate() {
        notebook.reorder_child(widget, Some(position as u32));
    }

    if let Some(current_widget) = current_widget {
        if let Some(page) = notebook.page_num(&current_widget) {
            notebook.set_current_page(Some(page));
        }
    }
}

fn ordered_widgets(notebook: &gtk::Notebook) -> Vec<gtk::Widget> {
    let mut widgets = Vec::with_capacity(notebook.n_pages() as usize);

    for page in 0..notebook.n_pages() {
        if let Some(widget) = notebook.nth_page(Some(page)) {
            widgets.push(widget);
        }
    }

    widgets.sort_by_key(|widget| {
        let Some(webview) = widget.clone().downcast::<WebView>().ok() else {
            return (2, usize::MAX);
        };
        let meta = meta(&webview);
        if meta.pinned {
            (0, 0)
        } else if let Some(group) = meta.group {
            (1, group_index(notebook, &group).unwrap_or(usize::MAX))
        } else {
            (2, usize::MAX)
        }
    });

    widgets
}

fn apply_group_visibility(notebook: &gtk::Notebook) {
    for page in 0..notebook.n_pages() {
        let Some(widget) = notebook.nth_page(Some(page)) else {
            continue;
        };
        let visible = should_show_page(notebook, page);
        widget.set_no_show_all(!visible);
        if visible {
            widget.show();
        } else {
            widget.hide();
        }
    }
}

fn should_show_page(notebook: &gtk::Notebook, page: u32) -> bool {
    let Some(webview) = webview_at(notebook, page) else {
        return true;
    };
    let meta = meta(&webview);
    let Some(group) = meta.group else {
        return true;
    };

    if !group_collapsed(notebook, &group) {
        return true;
    }

    first_group_page(notebook, &group) == Some(page)
}

fn refresh_all_tab_labels(notebook: &gtk::Notebook) {
    for page in 0..notebook.n_pages() {
        if let Some(webview) = webview_at(notebook, page) {
            refresh_tab_label(notebook, &webview);
        }
    }
}

fn refresh_tab_label(notebook: &gtk::Notebook, webview: &WebView) {
    let Some(label) = tab_label(webview) else {
        return;
    };
    let Some(status) = status_label(webview) else {
        return;
    };
    let Some(title) = title_label(webview) else {
        return;
    };
    let Some(page) = notebook.page_num(webview) else {
        return;
    };
    let meta = meta(webview);
    set_group_line_color(
        &label,
        meta.group
            .as_deref()
            .and_then(|group| group_color(notebook, group)),
    );

    if meta.pinned {
        label.set_size_request(PINNED_TAB_LABEL_WIDTH, -1);
        label.set_hexpand(false);
        label.set_halign(gtk::Align::Center);
        status.set_no_show_all(true);
        status.hide();
        title.set_no_show_all(true);
        title.hide();
        return;
    }

    label.set_size_request(TAB_LABEL_MAX_WIDTH, -1);
    label.set_hexpand(true);
    label.set_halign(gtk::Align::Fill);
    title.set_no_show_all(false);
    title.show();

    let Some(group) = meta.group else {
        status.set_text("");
        status.set_no_show_all(true);
        status.hide();
        return;
    };

    let is_first_group_tab = first_group_page(notebook, &group) == Some(page);
    if !is_first_group_tab {
        status.set_text("");
        status.set_no_show_all(true);
        status.hide();
        return;
    }

    status.set_no_show_all(false);
    set_group_badge_color(&status, group_color(notebook, &group).as_deref());
    if group_collapsed(notebook, &group) {
        status.set_text(&format!(
            "{} ({})",
            group,
            group_tab_count(notebook, &group)
        ));
    } else {
        status.set_text(&group);
    }
    status.show();
}

fn remove_empty_groups(notebook: &gtk::Notebook) {
    let Some(groups) = groups(notebook) else {
        return;
    };

    let active_groups: Vec<String> = (0..notebook.n_pages())
        .filter_map(|page| webview_at(notebook, page))
        .filter_map(|webview| meta(&webview).group)
        .collect();

    groups
        .borrow_mut()
        .retain(|group| active_groups.iter().any(|name| name == &group.name));
}

fn ensure_group(notebook: &gtk::Notebook, name: &str) {
    let Some(groups) = groups(notebook) else {
        return;
    };
    let Some(name) = clean_group_name(Some(name)) else {
        return;
    };

    if groups.borrow().iter().any(|group| group.name == name) {
        return;
    }

    info!("created tab group: {}", name);
    let color = default_group_color(groups.borrow().len());
    groups.borrow_mut().push(GroupState {
        name,
        collapsed: false,
        color,
    });
}

fn default_group_color(index: usize) -> String {
    GROUP_COLORS[index % GROUP_COLORS.len()].to_string()
}

fn normalize_group_color(color: Option<&str>) -> Option<String> {
    let color = color?.trim().to_ascii_lowercase();
    GROUP_COLORS.contains(&color.as_str()).then_some(color)
}

fn group_color(notebook: &gtk::Notebook, name: &str) -> Option<String> {
    groups(notebook)?
        .borrow()
        .iter()
        .find(|group| group.name == name)
        .map(|group| group.color.clone())
}

fn set_group_badge_color(status: &gtk::Label, color: Option<&str>) {
    let context = status.style_context();
    for candidate in GROUP_COLORS {
        context.remove_class(&format!("qust-group-{}", candidate));
    }
    if let Some(color) = normalize_group_color(color) {
        context.add_class(&format!("qust-group-{}", color));
    }
}

fn set_group_line_color(label: &gtk::Box, color: Option<String>) {
    let context = label.style_context();
    for candidate in GROUP_COLORS {
        context.remove_class(&format!("qust-group-line-{}", candidate));
    }
    if let Some(color) = normalize_group_color(color.as_deref()) {
        context.add_class(&format!("qust-group-line-{}", color));
    }
}

fn resolve_group_name(notebook: &gtk::Notebook, name: Option<&str>) -> Option<String> {
    if let Some(name) = clean_group_name(name) {
        return Some(name);
    }

    let webview = current_webview(notebook)?;
    meta(&webview).group
}

fn clean_group_name(name: Option<&str>) -> Option<String> {
    let name = name?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn groups(notebook: &gtk::Notebook) -> Option<Rc<RefCell<Vec<GroupState>>>> {
    unsafe {
        notebook
            .data::<Rc<RefCell<Vec<GroupState>>>>(GROUPS_KEY)
            .map(|groups| groups.as_ref().clone())
    }
}

fn meta_cell(webview: &WebView) -> Option<Rc<RefCell<TabMeta>>> {
    unsafe {
        webview
            .data::<Rc<RefCell<TabMeta>>>(TAB_META_KEY)
            .map(|meta| meta.as_ref().clone())
    }
}

fn meta(webview: &WebView) -> TabMeta {
    meta_cell(webview)
        .map(|meta| meta.borrow().clone())
        .unwrap_or_default()
}

fn set_meta(webview: &WebView, next: TabMeta) {
    if let Some(meta) = meta_cell(webview) {
        *meta.borrow_mut() = next;
    }
}

fn status_label(webview: &WebView) -> Option<gtk::Label> {
    unsafe {
        webview
            .data::<gtk::Label>(TAB_STATUS_KEY)
            .map(|label| label.as_ref().clone())
    }
}

fn tab_label(webview: &WebView) -> Option<gtk::Box> {
    unsafe {
        webview
            .data::<gtk::Box>(TAB_LABEL_KEY)
            .map(|label| label.as_ref().clone())
    }
}

fn title_label(webview: &WebView) -> Option<gtk::Label> {
    unsafe {
        webview
            .data::<gtk::Label>(TAB_TITLE_KEY)
            .map(|label| label.as_ref().clone())
    }
}

fn tab_icon(webview: &WebView) -> Option<gtk::Image> {
    unsafe {
        webview
            .data::<gtk::Image>(TAB_ICON_KEY)
            .map(|icon| icon.as_ref().clone())
    }
}

fn imported_favicon(webview: &WebView) -> Option<String> {
    unsafe {
        webview
            .data::<String>(TAB_FAVICON_KEY)
            .map(|favicon| favicon.as_ref().clone())
    }
}

fn set_cached_title(webview: &WebView, title: &str) {
    let title = title.trim();
    if title.is_empty() {
        return;
    }
    if let Some(label) = title_label(webview) {
        label.set_text(title);
    }
    unsafe {
        webview.set_data(TAB_CACHED_TITLE_KEY, title.to_string());
    }
}

fn cached_title(webview: &WebView) -> Option<String> {
    webview
        .title()
        .map(|title| title.to_string())
        .filter(|title| !title.trim().is_empty())
        .or_else(|| unsafe {
            webview
                .data::<String>(TAB_CACHED_TITLE_KEY)
                .map(|title| title.as_ref().clone())
        })
}

fn webview_at(notebook: &gtk::Notebook, page: u32) -> Option<WebView> {
    notebook.nth_page(Some(page))?.downcast::<WebView>().ok()
}

fn visible_pages(notebook: &gtk::Notebook) -> Vec<u32> {
    (0..notebook.n_pages())
        .filter(|page| page_is_visible(notebook, *page))
        .collect()
}

fn page_is_visible(notebook: &gtk::Notebook, page: u32) -> bool {
    notebook
        .nth_page(Some(page))
        .map(|widget| widget.is_visible())
        .unwrap_or(false)
}

fn group_index(notebook: &gtk::Notebook, name: &str) -> Option<usize> {
    groups(notebook)?
        .borrow()
        .iter()
        .position(|group| group.name == name)
}

fn group_collapsed(notebook: &gtk::Notebook, name: &str) -> bool {
    groups(notebook)
        .and_then(|groups| {
            groups
                .borrow()
                .iter()
                .find(|group| group.name == name)
                .map(|group| group.collapsed)
        })
        .unwrap_or(false)
}

fn first_group_page(notebook: &gtk::Notebook, name: &str) -> Option<u32> {
    (0..notebook.n_pages()).find(|page| {
        webview_at(notebook, *page)
            .map(|webview| meta(&webview).group.as_deref() == Some(name))
            .unwrap_or(false)
    })
}

fn group_tab_count(notebook: &gtk::Notebook, name: &str) -> usize {
    (0..notebook.n_pages())
        .filter(|page| {
            webview_at(notebook, *page)
                .map(|webview| meta(&webview).group.as_deref() == Some(name))
                .unwrap_or(false)
        })
        .count()
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::{adjacent_page, cairo, favicon_bytes, favicon_data_uri, move_target};
    use gtk::prelude::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};
    use webkit2gtk::{SettingsExt, WebViewExt};

    #[test]
    #[ignore = "requires a graphical display"]
    fn javascript_popup_opens_in_related_tab() {
        gtk::init().expect("GTK display");
        let notebook = super::create_notebook();
        let tab = super::add_unloaded_tab(&notebook, "about:blank");
        super::take_pending_uri(&tab.webview);
        WebViewExt::settings(&tab.webview)
            .expect("WebKit settings")
            .set_javascript_can_open_windows_automatically(true);

        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.add(&notebook);
        window.show_all();

        let completed = Rc::new(Cell::new(false));
        let completed_poll = completed.clone();
        let notebook_poll = notebook.clone();
        let main_loop = glib::MainLoop::new(None, false);
        let main_loop_poll = main_loop.clone();
        let deadline = Instant::now() + Duration::from_secs(5);
        glib::timeout_add_local(Duration::from_millis(20), move || {
            if notebook_poll.n_pages() == 2 {
                completed_poll.set(true);
                main_loop_poll.quit();
                return glib::ControlFlow::Break;
            }
            if Instant::now() >= deadline {
                main_loop_poll.quit();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });

        tab.webview.load_html(
            "<script>window.open('about:blank', '_blank', 'width=320,height=240')</script>",
            None,
        );
        main_loop.run();
        window.close();

        assert!(completed.get(), "popup tab was not created");
    }

    #[test]
    fn regular_tabs_shrink_between_maximum_and_minimum_widths() {
        assert_eq!(super::regular_tab_width(1200, 0, 3, 0), Some(220));
        assert_eq!(super::regular_tab_width(1000, 2, 6, 0), Some(153));
        assert_eq!(super::regular_tab_width(1000, 2, 6, 80), Some(140));
        assert_eq!(super::regular_tab_width(600, 4, 20, 100), Some(80));
        assert_eq!(super::regular_tab_width(600, 4, 0, 0), None);
    }

    #[test]
    fn adjacent_page_stops_at_tab_bar_edges() {
        let visible = [0, 2, 4];

        assert_eq!(adjacent_page(&visible, 0, -1), None);
        assert_eq!(adjacent_page(&visible, 4, 1), None);
        assert_eq!(adjacent_page(&visible, 2, -1), Some(0));
        assert_eq!(adjacent_page(&visible, 2, 1), Some(4));
    }

    #[test]
    fn move_target_uses_adjacent_visible_tabs() {
        let visible = [0, 2, 4];

        assert_eq!(move_target(&visible, 2, -1), Some(0));
        assert_eq!(move_target(&visible, 2, 1), Some(4));
    }

    #[test]
    fn move_target_stops_at_tab_bar_edges() {
        let visible = [0, 1, 2];

        assert_eq!(move_target(&visible, 0, -1), None);
        assert_eq!(move_target(&visible, 2, 1), None);
        assert_eq!(move_target(&visible, 3, -1), None);
    }

    #[test]
    fn imported_favicon_data_uri_is_decoded() {
        let bytes = favicon_bytes("data:image/png;base64,iVBORw0KGgo=").unwrap();

        assert_eq!(&bytes, b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn webkit_favicon_surface_is_serialized_as_png() {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 2, 2).unwrap();
        let data_uri = favicon_data_uri(surface.as_ref()).unwrap();
        let bytes = favicon_bytes(&data_uri).unwrap();

        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
}
