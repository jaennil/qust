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
        input_row.pack_start(&entry, true, true, 0);

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
    }

    pub fn focus_with_url(&self, url: &str) {
        self.entry.set_text(url);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
        info!("command bar focused with URL: {}", url);
    }

    pub fn focus_for_command(&self) {
        self.entry.set_text(":");
        self.entry.grab_focus();
        self.entry.set_position(-1);
        info!("command bar focused for command input");
    }

    pub fn clear_and_unfocus(&self) {
        self.entry.set_text("");
        self.hide_completions();
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

        self.entry.connect_activate(move |e| {
            let text = e.text().to_string();
            let current = modes::current_mode(&ms);
            info!("command bar activated (mode={}, text={})", current, text);

            match current {
                Mode::Command => {
                    commands::execute(&text, &nb, &win, &pm);
                }
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
