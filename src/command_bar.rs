use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use log::info;
use webkit2gtk::WebViewExt;

use crate::commands;
use crate::modes::{self, Mode, ModeState, NewTabFlag};
use crate::navigation;
use crate::password_manager::PasswordManager;
use crate::tab;

const MAX_COMPLETIONS: usize = 8;
const URL_BLOCK_CURSOR_CLASS: &str = "url-block-cursor";
const URL_BLOCK_CURSOR_CSS: &[u8] = br#"
.url-block-cursor selection {
    background-color: #d8dee9;
    color: #20242b;
}
"#;

#[derive(Clone)]
struct CompletionRow {
    row: gtk::Box,
    command_label: gtk::Label,
    description_label: gtk::Label,
}

#[derive(Clone)]
pub struct CommandBar {
    pub container: gtk::Box,
    pub mode_label: gtk::Label,
    pub entry: gtk::Entry,
    input_stack: gtk::Stack,
    url_label: gtk::Label,
    zoom_label: gtk::Label,
    completion_frame: gtk::Frame,
    completion_box: gtk::Box,
    completion_rows: Vec<CompletionRow>,
    suggestions: Rc<RefCell<Vec<commands::CommandSuggestion>>>,
    tab_suggestions: Rc<RefCell<Vec<tab::TabSearchResult>>>,
    selected_suggestion: Rc<Cell<usize>>,
    url_cursor: Rc<Cell<i32>>,
}

impl CommandBar {
    pub fn new() -> Self {
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let input_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);

        let mode_label = gtk::Label::new(Some("NORMAL"));
        mode_label.set_width_chars(10);
        input_row.pack_start(&mode_label, false, false, 4);

        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some("URL or :command"));
        let cursor_provider = gtk::CssProvider::new();
        cursor_provider
            .load_from_data(URL_BLOCK_CURSOR_CSS)
            .expect("valid URL block cursor CSS");
        entry
            .style_context()
            .add_provider(&cursor_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);

        let url_label = gtk::Label::new(None);
        url_label.set_xalign(0.0);
        url_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        url_label.set_selectable(true);
        url_label.set_margin_start(6);
        url_label.set_margin_end(6);

        let input_stack = gtk::Stack::new();
        input_stack.set_hexpand(true);
        input_stack.add_named(&url_label, "url");
        input_stack.add_named(&entry, "entry");
        input_stack.set_visible_child_name("url");
        input_row.pack_start(&input_stack, true, true, 0);

        let zoom_label = gtk::Label::new(Some("100%"));
        zoom_label.set_width_chars(5);
        zoom_label.set_xalign(1.0);
        zoom_label.set_margin_end(8);
        input_row.pack_start(&zoom_label, false, false, 0);

        let completion_frame = gtk::Frame::new(None);
        completion_frame.set_no_show_all(true);
        completion_frame.set_shadow_type(gtk::ShadowType::In);
        completion_frame.set_margin_start(14);
        completion_frame.set_margin_end(8);
        completion_frame.set_margin_top(4);
        completion_frame.set_margin_bottom(4);

        let completion_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        completion_box.set_margin_top(6);
        completion_box.set_margin_bottom(6);
        completion_box.set_margin_start(8);
        completion_box.set_margin_end(8);

        let completion_rows = build_completion_rows(&completion_box);
        completion_frame.add(&completion_box);

        container.pack_start(&completion_frame, false, false, 0);
        container.pack_start(&input_row, false, false, 0);

        info!("command bar created");

        let command_bar = CommandBar {
            container,
            mode_label,
            entry,
            input_stack,
            url_label,
            zoom_label,
            completion_frame,
            completion_box,
            completion_rows,
            suggestions: Rc::new(RefCell::new(Vec::new())),
            tab_suggestions: Rc::new(RefCell::new(Vec::new())),
            selected_suggestion: Rc::new(Cell::new(0)),
            url_cursor: Rc::new(Cell::new(0)),
        };
        command_bar.connect_completion_updates();
        command_bar
    }

    pub fn update_mode_label(&self, mode: Mode) {
        self.mode_label.set_text(&mode.to_string());
        let entry_style = self.entry.style_context();
        if mode == Mode::UrlNormal {
            entry_style.add_class(URL_BLOCK_CURSOR_CLASS);
        } else {
            entry_style.remove_class(URL_BLOCK_CURSOR_CLASS);
        }
        if matches!(
            mode,
            Mode::UrlNormal | Mode::UrlInsert | Mode::Command | Mode::Terminal | Mode::Search
        ) {
            self.input_stack.set_visible_child_name("entry");
        } else {
            self.input_stack.set_visible_child_name("url");
        }
    }

    pub fn update_zoom_label(&self, zoom: f64) {
        self.zoom_label.set_text(&format!("{:.0}%", zoom * 100.0));
    }

    pub fn focus_url_editor(&self, url: &str, normal_mode: bool) {
        self.input_stack.set_visible_child_name("entry");
        self.entry.set_text(url);
        self.entry.grab_focus();
        if normal_mode {
            let end = self.entry.text_length().saturating_sub(1) as i32;
            self.set_url_normal_cursor(end);
        } else {
            self.entry.set_position(-1);
        }
        info!("URL editor focused");
    }

    pub fn url_cursor(&self) -> i32 {
        self.url_cursor.get()
    }

    pub fn set_url_normal_cursor(&self, requested: i32) {
        let len = self.entry.text_length() as i32;
        let cursor = if len == 0 {
            0
        } else {
            requested.clamp(0, len - 1)
        };
        self.url_cursor.set(cursor);
        if len == 0 {
            self.entry.select_region(0, 0);
        } else {
            self.entry.select_region(cursor, cursor + 1);
        }
    }

    pub fn enter_url_insert_at(&self, requested: i32) {
        let len = self.entry.text_length() as i32;
        let cursor = requested.clamp(0, len);
        self.url_cursor.set(cursor);
        self.entry.select_region(cursor, cursor);
        self.entry.set_position(cursor);
    }

    pub fn restore_url_normal_cursor(&self) {
        self.set_url_normal_cursor(self.entry.position());
    }

    pub fn focus_for_command(&self) {
        self.input_stack.set_visible_child_name("entry");
        self.entry.set_text(":");
        self.entry.grab_focus();
        self.entry.set_position(-1);
        info!("command bar focused for command input");
    }

    pub fn focus_for_terminal(&self) {
        self.input_stack.set_visible_child_name("entry");
        self.entry.set_text("!");
        self.entry.grab_focus();
        self.entry.set_position(-1);
        info!("command bar focused for terminal input");
    }

    pub fn focus_for_tab_search(&self) {
        self.input_stack.set_visible_child_name("entry");
        self.entry.set_text("/");
        self.entry.grab_focus();
        self.entry.set_position(-1);
        info!("command bar focused for tab search");
    }

    pub fn clear_and_unfocus(&self) {
        self.entry.set_text("");
        self.hide_completions();
        self.input_stack.set_visible_child_name("url");
    }

    pub fn connect_url_updates(&self, notebook: &gtk::Notebook) {
        for page in 0..notebook.n_pages() {
            if let Some(widget) = notebook.nth_page(Some(page)) {
                if let Ok(webview) = widget.downcast::<webkit2gtk::WebView>() {
                    connect_webview_url(&webview, notebook, &self.url_label);
                }
            }
        }

        let notebook_for_add = notebook.clone();
        let label_for_add = self.url_label.clone();
        notebook.connect_page_added(move |_, child, _| {
            if let Ok(webview) = child.clone().downcast::<webkit2gtk::WebView>() {
                connect_webview_url(&webview, &notebook_for_add, &label_for_add);
            }
        });

        let url_label = self.url_label.clone();
        notebook.connect_switch_page(move |_, page, _| {
            if let Ok(webview) = page.clone().downcast::<webkit2gtk::WebView>() {
                set_url_label(&url_label, &tab::display_url(&webview));
            }
        });

        if let Some(webview) = tab::current_webview(notebook) {
            set_url_label(&self.url_label, &tab::display_url(&webview));
        }
    }

    pub fn select_next_completion(&self) -> bool {
        let len = self.visible_suggestion_count();
        if len == 0 {
            return false;
        }

        let next = (self.selected_suggestion.get() + 1) % len;
        self.selected_suggestion.set(next);
        self.render_completions();
        true
    }

    pub fn select_previous_completion(&self) -> bool {
        let len = self.visible_suggestion_count();
        if len == 0 {
            return false;
        }

        let selected = self.selected_suggestion.get();
        let previous = if selected == 0 { len - 1 } else { selected - 1 };
        self.selected_suggestion.set(previous);
        self.render_completions();
        true
    }

    pub fn apply_selected_completion(&self) -> bool {
        let completion = {
            let suggestions = self.suggestions.borrow();
            let Some(suggestion) = suggestions.get(
                self.selected_suggestion
                    .get()
                    .min(suggestions.len().saturating_sub(1)),
            ) else {
                return false;
            };
            let suffix = if suggestion.accepts_args { " " } else { "" };
            format!("{}{}", suggestion.completion, suffix)
        };

        self.entry.set_text(&completion);
        self.entry.set_position(-1);
        true
    }

    pub fn connect_activate(
        &self,
        mode_state: ModeState,
        new_tab_flag: NewTabFlag,
        notebook: gtk::Notebook,
        window: gtk::ApplicationWindow,
        password_manager: PasswordManager,
    ) {
        let ms = mode_state.clone();
        let ntf = new_tab_flag.clone();
        let nb = notebook.clone();
        let win = window.clone();
        let pm = password_manager.clone();
        let ml = self.mode_label.clone();
        let entry = self.entry.clone();
        let input_stack = self.input_stack.clone();
        let tab_suggestions = self.tab_suggestions.clone();
        let selected_suggestion = self.selected_suggestion.clone();

        self.connect_tab_search_updates(&mode_state, &notebook);

        self.entry.connect_activate(move |e| {
            let text = e.text().to_string();
            let current = modes::current_mode(&ms);
            if current == Mode::Terminal {
                info!("command bar activated in Terminal mode");
            } else {
                info!("command bar activated (mode={}, text={})", current, text);
            }

            match current {
                Mode::Command => {
                    commands::execute(&text, &nb, &win, &pm);
                }
                Mode::Terminal => run_terminal_command(&win, &text),
                Mode::Search => {
                    let selected = selected_suggestion
                        .get()
                        .min(tab_suggestions.borrow().len().saturating_sub(1));
                    if let Some(result) = tab_suggestions.borrow().get(selected) {
                        info!("focusing tab {} from search", result.page);
                        tab::focus_page(&nb, result.page);
                    }
                }
                _ => {
                    let url = navigation::normalize_url(&text);
                    let open_new = *ntf.borrow();
                    if open_new {
                        info!("opening in new tab: {}", url);
                        tab::add_tab(&nb, &url);
                    } else {
                        info!("navigating current tab to: {}", url);
                        if let Some(wv) = tab::current_webview(&nb) {
                            wv.load_uri(&url);
                            wv.grab_focus();
                        }
                    }
                    *ntf.borrow_mut() = false;
                }
            }
            modes::set_mode(&ms, Mode::Normal);
            ml.set_text(&Mode::Normal.to_string());
            entry.set_text("");
            input_stack.set_visible_child_name("url");
        });
    }

    fn connect_completion_updates(&self) {
        let completion_frame = self.completion_frame.clone();
        let completion_box = self.completion_box.clone();
        let completion_rows = self.completion_rows.clone();
        let suggestions = self.suggestions.clone();
        let selected_suggestion = self.selected_suggestion.clone();

        self.entry.connect_changed(move |entry| {
            let text = entry.text().to_string();
            if !text.trim_start().starts_with(':') {
                suggestions.borrow_mut().clear();
                selected_suggestion.set(0);
                hide_completion_rows(&completion_frame, &completion_rows);
                return;
            }

            let matches = commands::command_suggestions(&text);
            log::debug!(
                "command completion update: text='{}', matches={}",
                text,
                matches.len()
            );
            *suggestions.borrow_mut() = matches;
            selected_suggestion.set(0);
            render_completion_rows(
                &completion_frame,
                &completion_box,
                &completion_rows,
                &suggestions.borrow(),
                selected_suggestion.get(),
            );
        });
    }

    fn connect_tab_search_updates(&self, mode_state: &ModeState, notebook: &gtk::Notebook) {
        let completion_frame = self.completion_frame.clone();
        let completion_box = self.completion_box.clone();
        let completion_rows = self.completion_rows.clone();
        let tab_suggestions = self.tab_suggestions.clone();
        let selected_suggestion = self.selected_suggestion.clone();
        let mode_state = mode_state.clone();
        let notebook = notebook.clone();
        let mode_label = self.mode_label.clone();
        let input_stack = self.input_stack.clone();

        self.entry.connect_changed(move |entry| {
            if modes::current_mode(&mode_state) != Mode::Search {
                tab_suggestions.borrow_mut().clear();
                return;
            }
            let text = entry.text();
            let query = text.strip_prefix('/').unwrap_or(&text);
            let matches = tab::search_open_tabs(&notebook, query);
            if should_open_unique_tab(query, matches.len()) {
                let page = matches[0].page;
                modes::set_mode(&mode_state, Mode::Normal);
                mode_label.set_text(&Mode::Normal.to_string());
                tab_suggestions.borrow_mut().clear();
                hide_completion_rows(&completion_frame, &completion_rows);
                entry.set_text("");
                input_stack.set_visible_child_name("url");
                tab::focus_page(&notebook, page);
                return;
            }
            *tab_suggestions.borrow_mut() = matches;
            selected_suggestion.set(0);
            render_tab_search_rows(
                &completion_frame,
                &completion_box,
                &completion_rows,
                &tab_suggestions.borrow(),
                0,
            );
        });
    }

    fn visible_suggestion_count(&self) -> usize {
        if self.entry.text().starts_with('/') {
            return self.tab_suggestions.borrow().len().min(MAX_COMPLETIONS);
        }
        self.suggestions.borrow().len().min(MAX_COMPLETIONS)
    }

    fn render_completions(&self) {
        if self.entry.text().starts_with('/') {
            render_tab_search_rows(
                &self.completion_frame,
                &self.completion_box,
                &self.completion_rows,
                &self.tab_suggestions.borrow(),
                self.selected_suggestion.get(),
            );
            return;
        }
        render_completion_rows(
            &self.completion_frame,
            &self.completion_box,
            &self.completion_rows,
            &self.suggestions.borrow(),
            self.selected_suggestion.get(),
        );
    }

    fn hide_completions(&self) {
        hide_completion_rows(&self.completion_frame, &self.completion_rows);
    }
}

fn should_open_unique_tab(query: &str, match_count: usize) -> bool {
    !query.trim().is_empty() && match_count == 1
}

fn render_tab_search_rows(
    completion_frame: &gtk::Frame,
    completion_box: &gtk::Box,
    rows: &[CompletionRow],
    suggestions: &[tab::TabSearchResult],
    selected: usize,
) {
    if suggestions.is_empty() {
        hide_completion_rows(completion_frame, rows);
        return;
    }

    for (index, row) in rows.iter().enumerate() {
        let Some(suggestion) = suggestions.get(index) else {
            row.row.hide();
            continue;
        };
        let marker = if index == selected { "▶" } else { " " };
        row.command_label
            .set_text(&format!("{} {}", marker, suggestion.title));
        row.description_label.set_text(&suggestion.url);
        row.command_label.show();
        row.description_label.show();
        row.row.show();
    }
    completion_box.show();
    let visible_rows = suggestions.len().min(rows.len()) as i32;
    completion_frame.set_size_request(-1, 12 + visible_rows * 22);
    completion_frame.show();
    completion_frame.queue_resize();
}

fn connect_webview_url(
    webview: &webkit2gtk::WebView,
    notebook: &gtk::Notebook,
    label: &gtk::Label,
) {
    let notebook = notebook.clone();
    let label = label.clone();
    webview.connect_uri_notify(move |webview| {
        if notebook.page_num(webview) == notebook.current_page() {
            set_url_label(&label, &tab::display_url(webview));
        }
    });
}

fn set_url_label(label: &gtk::Label, url: &str) {
    label.set_markup(&format_url_markup(url));
    label.set_tooltip_text((!url.is_empty()).then_some(url));
}

fn format_url_markup(url: &str) -> String {
    if url.is_empty() {
        return String::new();
    }

    let (scheme, remainder) = if let Some(rest) = url.strip_prefix("https://") {
        ("", rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        ("http://", rest)
    } else {
        return glib::markup_escape_text(&readable_percent_encoding(url)).to_string();
    };
    let origin_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let origin = format!("{}{}", scheme, &remainder[..origin_end]);
    let detail = readable_percent_encoding(&remainder[origin_end..]);

    format!(
        "<span weight=\"semibold\">{}</span><span alpha=\"65%\">{}</span>",
        glib::markup_escape_text(&origin),
        glib::markup_escape_text(&detail)
    )
}

fn readable_percent_encoding(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'%' || index + 2 >= bytes.len() {
            let character = input[index..].chars().next().unwrap();
            output.push(character);
            index += character.len_utf8();
            continue;
        }

        let start = index;
        let mut decoded = Vec::new();
        while index + 2 < bytes.len() && bytes[index] == b'%' {
            let Some(value) = hex_byte(bytes[index + 1], bytes[index + 2]) else {
                break;
            };
            decoded.push(value);
            index += 3;
        }
        if let Ok(text) = std::str::from_utf8(&decoded) {
            if !text.is_ascii() {
                output.push_str(text);
                continue;
            }
        }
        if index == start {
            output.push('%');
            index += 1;
        } else {
            output.push_str(&input[start..index]);
        }
    }

    output
}

fn hex_byte(high: u8, low: u8) -> Option<u8> {
    Some(hex_digit(high)? * 16 + hex_digit(low)?)
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn run_terminal_command(window: &gtk::ApplicationWindow, input: &str) {
    let command = terminal_command(input);
    if command.is_empty() {
        return;
    }

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let flags = gtk::gio::SubprocessFlags::STDOUT_PIPE | gtk::gio::SubprocessFlags::STDERR_PIPE;
    let process = match gtk::gio::Subprocess::newv(
        [shell.as_ref(), "-lc".as_ref(), command.as_ref()].as_slice(),
        flags,
    ) {
        Ok(process) => process,
        Err(error) => {
            show_terminal_output(window, Err(error.to_string()));
            return;
        }
    };

    let window = window.clone();
    let command = command.to_string();
    let finished_process = process.clone();
    process.communicate_utf8_async(None, None::<&gtk::gio::Cancellable>, move |result| {
        let result = result
            .map(|(stdout, stderr)| TerminalOutput {
                command,
                status: finished_process
                    .has_exited()
                    .then(|| finished_process.exit_status()),
                stdout: stdout.map(|text| text.to_string()).unwrap_or_default(),
                stderr: stderr.map(|text| text.to_string()).unwrap_or_default(),
            })
            .map_err(|error| error.to_string());
        show_terminal_output(&window, result);
    });
}

fn terminal_command(input: &str) -> &str {
    input.trim().strip_prefix('!').unwrap_or(input).trim()
}

struct TerminalOutput {
    command: String,
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

fn show_terminal_output(window: &gtk::ApplicationWindow, result: Result<TerminalOutput, String>) {
    let dialog = gtk::Dialog::with_buttons(
        Some("Terminal Output"),
        Some(window),
        gtk::DialogFlags::DESTROY_WITH_PARENT,
        &[("Close", gtk::ResponseType::Close)],
    );
    dialog.set_default_size(760, 480);

    let text = match result {
        Ok(output) => format_terminal_output(&output),
        Err(error) => format!("Failed to start command:\n{}", error),
    };
    let view = gtk::TextView::new();
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_monospace(true);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    if let Some(buffer) = view.buffer() {
        buffer.set_text(&text);
    }

    let scrolled = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scrolled.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scrolled.add(&view);
    dialog.content_area().pack_start(&scrolled, true, true, 0);
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.show_all();
}

fn format_terminal_output(output: &TerminalOutput) -> String {
    let status = output
        .status
        .map(|code| code.to_string())
        .unwrap_or_else(|| "signal".to_string());
    let mut text = format!("$ {}\n\nexit: {}", output.command, status);
    if !output.stdout.is_empty() {
        text.push_str("\n\nstdout:\n");
        text.push_str(&output.stdout);
    }
    if !output.stderr.is_empty() {
        text.push_str("\n\nstderr:\n");
        text.push_str(&output.stderr);
    }
    text
}

fn build_completion_rows(completion_box: &gtk::Box) -> Vec<CompletionRow> {
    (0..MAX_COMPLETIONS)
        .map(|_| {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_no_show_all(true);

            let command_label = gtk::Label::new(None);
            command_label.set_xalign(0.0);
            command_label.set_width_chars(28);
            command_label.set_use_markup(true);

            let description_label = gtk::Label::new(None);
            description_label.set_xalign(0.0);
            description_label.set_ellipsize(gtk::pango::EllipsizeMode::End);

            row.pack_start(&command_label, false, false, 0);
            row.pack_start(&description_label, true, true, 0);
            completion_box.pack_start(&row, false, false, 0);

            CompletionRow {
                row,
                command_label,
                description_label,
            }
        })
        .collect()
}

fn render_completion_rows(
    completion_frame: &gtk::Frame,
    completion_box: &gtk::Box,
    rows: &[CompletionRow],
    suggestions: &[commands::CommandSuggestion],
    selected: usize,
) {
    if suggestions.is_empty() {
        hide_completion_rows(completion_frame, rows);
        return;
    }

    for (index, row) in rows.iter().enumerate() {
        let Some(suggestion) = suggestions.get(index) else {
            row.row.hide();
            continue;
        };

        let marker = if index == selected { ">" } else { " " };
        let aliases = if suggestion.aliases.is_empty() {
            String::new()
        } else {
            format!(" [{}]", suggestion.aliases.join(", "))
        };
        let command = format!("{} :{}{}", marker, suggestion.name, aliases);
        let description = format!("{} - {}", suggestion.usage, suggestion.description);

        row.command_label
            .set_markup(&format!("<b>{}</b>", glib::markup_escape_text(&command)));
        row.description_label.set_text(&description);
        row.command_label.show();
        row.description_label.show();
        row.row.show();
    }

    completion_box.show();
    let visible_rows = suggestions.len().min(rows.len()) as i32;
    completion_frame.set_size_request(-1, 12 + visible_rows * 22);
    completion_frame.show();
    completion_frame.queue_resize();
}

fn hide_completion_rows(completion_frame: &gtk::Frame, rows: &[CompletionRow]) {
    for row in rows {
        row.row.hide();
    }
    completion_frame.hide();
}

#[cfg(test)]
mod tests {
    use super::{
        build_completion_rows, format_terminal_output, format_url_markup,
        readable_percent_encoding, render_tab_search_rows, should_open_unique_tab,
        terminal_command, TerminalOutput,
    };
    use gtk::prelude::*;

    #[test]
    fn unique_tab_opens_only_after_query_input() {
        assert!(!should_open_unique_tab("", 1));
        assert!(!should_open_unique_tab("vik", 2));
        assert!(should_open_unique_tab("v", 1));
    }

    #[test]
    #[ignore = "requires a graphical display"]
    fn tab_search_result_rows_are_visible() {
        gtk::init().expect("GTK display");
        let frame = gtk::Frame::new(None);
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let rows = build_completion_rows(&container);
        frame.add(&container);
        let suggestions = vec![crate::tab::TabSearchResult {
            page: 0,
            title: "Vikunja".to_string(),
            url: "https://vikunja.example".to_string(),
        }];

        render_tab_search_rows(&frame, &container, &rows, &suggestions, 0);

        assert!(frame.is_visible());
        assert!(rows[0].row.is_visible());
        assert!(rows[0].command_label.is_visible());
        assert!(!rows[1].row.is_visible());
    }

    #[test]
    fn terminal_command_removes_prompt_and_whitespace() {
        assert_eq!(terminal_command("  ! printf hello  "), "printf hello");
        assert_eq!(terminal_command("pwd"), "pwd");
    }

    #[test]
    fn terminal_output_contains_status_and_both_streams() {
        let output = TerminalOutput {
            command: "test command".to_string(),
            status: Some(2),
            stdout: "output\n".to_string(),
            stderr: "error\n".to_string(),
        };

        let formatted = format_terminal_output(&output);
        assert!(formatted.contains("$ test command"));
        assert!(formatted.contains("exit: 2"));
        assert!(formatted.contains("stdout:\noutput"));
        assert!(formatted.contains("stderr:\nerror"));
    }

    #[test]
    fn url_markup_emphasizes_origin_and_hides_https() {
        let markup = format_url_markup("https://docs.example.com/path?q=one&x=two");

        assert!(markup.contains(">docs.example.com</span>"));
        assert!(markup.contains("alpha=\"65%\">/path?q=one&amp;x=two"));
        assert!(!markup.contains("https://"));
    }

    #[test]
    fn url_display_decodes_unicode_but_keeps_ascii_escapes() {
        assert_eq!(
            readable_percent_encoding("?q=%D1%84%D0%B8%D1%87%D0%B0%20one"),
            "?q=фича one"
        );
        assert_eq!(readable_percent_encoding("/one%2Ftwo"), "/one%2Ftwo");
    }
}
