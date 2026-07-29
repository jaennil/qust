use gtk::prelude::*;
use gtk::{cairo, gdk_pixbuf};
use log::{error, info, warn};
use serde::{Deserialize, Deserializer, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use webkit2gtk::{LoadEvent, SettingsExt, WebView, WebViewExt};

const TAB_WIDTH_CHARS: i32 = 20;
const FAVICON_SIZE: i32 = 16;
const TAB_LABEL_WIDTH: i32 = 220;
const PINNED_TAB_LABEL_WIDTH: i32 = 36;
const PENDING_URI_KEY: &str = "qust-pending-uri";
const TAB_META_KEY: &str = "qust-tab-meta";
const TAB_LABEL_KEY: &str = "qust-tab-label";
const TAB_STATUS_KEY: &str = "qust-tab-status";
const TAB_TITLE_KEY: &str = "qust-tab-title";
const GROUPS_KEY: &str = "qust-tab-groups";
const TAB_ICON_CHILD: &str = "icon";
const TAB_LOADING_CHILD: &str = "loading";

pub struct Tab {
    pub webview: WebView,
    pub label: gtk::Box,
}

#[derive(Clone, Debug, Serialize)]
pub struct TabSnapshot {
    pub url: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
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
                pinned: false,
                group: None,
            },
            TabSnapshotCompat::State { url, pinned, group } => TabSnapshot {
                url,
                pinned,
                group: clean_group_name(group.as_deref()),
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
        pinned: bool,
        #[serde(default)]
        group: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabGroupSnapshot {
    pub name: String,
    #[serde(default)]
    pub collapsed: bool,
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
}

impl Tab {
    pub fn new(url: &str) -> Self {
        let webview = WebView::new();
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

        let label = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        label.set_size_request(TAB_LABEL_WIDTH, -1);

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
        label.pack_start(&icon_stack, false, false, 0);

        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        label.pack_start(&status, false, false, 0);
        unsafe {
            webview.set_data(TAB_STATUS_KEY, status.clone());
        }

        let title = gtk::Label::new(Some("New Tab"));
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_width_chars(TAB_WIDTH_CHARS);
        title.set_xalign(0.0);
        label.pack_start(&title, true, true, 0);
        unsafe {
            webview.set_data(TAB_LABEL_KEY, label.clone());
            webview.set_data(TAB_TITLE_KEY, title.clone());
        }

        let title_label = title.clone();
        webview.connect_title_notify(move |wv| {
            if let Some(title) = wv.title() {
                let title_str = title.to_string();
                info!("tab title changed: {}", title_str);
                title_label.set_text(&title_str);
            }
        });

        let icon_image = icon.clone();
        webview.connect_favicon_notify(move |wv| {
            if let Some(favicon) = wv.favicon() {
                info!("tab favicon changed");
                set_favicon(&icon_image, &favicon);
                icon_image.show();
            } else {
                icon_image.set_from_icon_name(Some("text-html-symbolic"), gtk::IconSize::Menu);
            }
        });

        let loading_stack = icon_stack.clone();
        let loading_spinner = spinner.clone();
        webview.connect_load_changed(move |webview, event| {
            info!("load event {:?}: {:?}", event, webview.uri());
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

pub fn create_notebook() -> gtk::Notebook {
    let notebook = gtk::Notebook::new();
    notebook.set_scrollable(true);
    notebook.set_show_tabs(true);
    notebook.set_tab_pos(gtk::PositionType::Top);
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
        pinned: false,
        group: None,
    };
    add_unloaded_tab_snapshot(notebook, &snapshot)
}

pub fn add_unloaded_tab_snapshot(notebook: &gtk::Notebook, snapshot: &TabSnapshot) -> Tab {
    if let Some(group) = snapshot.group.as_deref() {
        ensure_group(notebook, group);
    }

    let tab = Tab::new(&snapshot.url);
    set_meta(
        &tab.webview,
        TabMeta {
            pinned: snapshot.pinned,
            group: snapshot.group.clone(),
        },
    );
    let page_num = notebook.append_page(&tab.webview, Some(&tab.label));
    connect_new_window(&tab.webview, notebook);
    info!("unloaded tab added at page {}", page_num);
    update_layout(notebook);
    tab
}

fn connect_new_window(webview: &WebView, notebook: &gtk::Notebook) {
    let notebook = notebook.clone();
    webview.connect_create(move |_, _| {
        info!("opening requested web window in a new tab");
        let tab = add_unloaded_tab(&notebook, "about:blank");
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

pub fn load_current_tab(notebook: &gtk::Notebook) {
    if let Some(current) = notebook.current_page() {
        schedule_load_page(notebook, current);
    }
}

fn schedule_load_page(notebook: &gtk::Notebook, page_num: u32) {
    let Some(widget) = notebook.nth_page(Some(page_num)) else {
        return;
    };
    let Ok(webview) = widget.downcast::<WebView>() else {
        return;
    };

    glib::idle_add_local_once(move || {
        load_pending_webview(&webview);
    });
}

fn load_pending_webview(webview: &WebView) {
    let Some(url) = take_pending_uri(webview) else {
        return;
    };

    info!("loading tab after layout: {}", url);
    webview.load_uri(&url);
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
    if visible.is_empty() {
        return;
    }
    if let Some(current) = notebook.current_page() {
        let current_pos = visible
            .iter()
            .position(|page| *page == current)
            .unwrap_or(0);
        let next = visible[(current_pos + 1) % visible.len()];
        info!("switching to next tab: {} -> {}", current, next);
        notebook.set_current_page(Some(next));
    }
}

pub fn prev_tab(notebook: &gtk::Notebook) {
    let visible = visible_pages(notebook);
    if visible.is_empty() {
        return;
    }
    if let Some(current) = notebook.current_page() {
        let current_pos = visible
            .iter()
            .position(|page| *page == current)
            .unwrap_or(0);
        let prev = if current_pos == 0 {
            visible[visible.len() - 1]
        } else {
            visible[current_pos - 1]
        };
        info!("switching to prev tab: {} -> {}", current, prev);
        notebook.set_current_page(Some(prev));
    }
}

pub fn current_webview(notebook: &gtk::Notebook) -> Option<WebView> {
    let page = notebook.current_page()?;
    let widget = notebook.nth_page(Some(page))?;
    widget.downcast::<WebView>().ok()
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
                    pinned: meta.pinned,
                    group: meta.group,
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
        state.push(GroupState {
            name,
            collapsed: snapshot.collapsed,
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
    ensure_current_page_visible(notebook);
}

fn update_layout(notebook: &gtk::Notebook) {
    reorder_tabs(notebook);
    apply_group_visibility(notebook);
    refresh_all_tab_labels(notebook);
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

    if meta.pinned {
        label.set_size_request(PINNED_TAB_LABEL_WIDTH, -1);
        status.set_no_show_all(true);
        status.hide();
        title.set_no_show_all(true);
        title.hide();
        return;
    }

    label.set_size_request(TAB_LABEL_WIDTH, -1);
    title.set_no_show_all(false);
    title.show();

    let Some(group) = meta.group else {
        status.set_text("");
        status.set_no_show_all(true);
        status.hide();
        return;
    };

    status.set_no_show_all(false);
    if group_collapsed(notebook, &group) && first_group_page(notebook, &group) == Some(page) {
        status.set_text(&format!(
            "{} ({})",
            group,
            group_tab_count(notebook, &group)
        ));
    } else {
        status.set_text(&format!("[{}]", group));
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
    groups.borrow_mut().push(GroupState {
        name,
        collapsed: false,
    });
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
