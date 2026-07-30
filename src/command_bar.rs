use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use log::info;
use webkit2gtk::WebViewExt;

use crate::commands;
use crate::modes::{self, Mode, ModeState, NewTabFlag};
use crate::password_manager::PasswordManager;
use crate::tab;

const MAX_COMPLETIONS: usize = 8;

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
    selected_suggestion: Rc<Cell<usize>>,
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

        let url_label = gtk::Label::new(None);
        url_label.set_xalign(0.0);
        url_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        url_label.set_selectable(true);

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
            selected_suggestion: Rc::new(Cell::new(0)),
        };
        command_bar.connect_completion_updates();
        command_bar
    }

    pub fn update_mode_label(&self, mode: Mode) {
        self.mode_label.set_text(&mode.to_string());
        if matches!(mode, Mode::Insert | Mode::Command | Mode::Terminal) {
            self.input_stack.set_visible_child_name("entry");
        } else {
            self.input_stack.set_visible_child_name("url");
        }
    }

    pub fn update_zoom_label(&self, zoom: f64) {
        self.zoom_label.set_text(&format!("{:.0}%", zoom * 100.0));
    }

    pub fn focus_with_url(&self, url: &str) {
        self.input_stack.set_visible_child_name("entry");
        self.entry.set_text(url);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
        info!("command bar focused with URL: {}", url);
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
                url_label.set_text(&tab::display_url(&webview));
            }
        });

        if let Some(webview) = tab::current_webview(notebook) {
            self.url_label.set_text(&tab::display_url(&webview));
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
                _ => {
                    let url = normalize_url(&text);
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

    fn visible_suggestion_count(&self) -> usize {
        self.suggestions.borrow().len().min(MAX_COMPLETIONS)
    }

    fn render_completions(&self) {
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

fn connect_webview_url(
    webview: &webkit2gtk::WebView,
    notebook: &gtk::Notebook,
    label: &gtk::Label,
) {
    let notebook = notebook.clone();
    let label = label.clone();
    webview.connect_uri_notify(move |webview| {
        if notebook.page_num(webview) == notebook.current_page() {
            label.set_text(&tab::display_url(webview));
        }
    });
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

fn normalize_url(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    if trimmed.contains('.') && !trimmed.contains(' ') {
        return format!("https://{}", trimmed);
    }
    format!("https://duckduckgo.com/?q={}", trimmed)
}

#[cfg(test)]
mod tests {
    use super::{format_terminal_output, terminal_command, TerminalOutput};

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
}
