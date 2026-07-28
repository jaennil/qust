use glib::Propagation;
use gtk::prelude::*;
use log::info;
use webkit2gtk::WebViewExt;

use crate::command_bar::CommandBar;
use crate::hints;
use crate::modes::{self, HintBuffer, Mode, ModeState, NewTabFlag};
use crate::tab;

const SCROLL_STEP: i32 = 60;
const SCROLL_PAGE: i32 = 600;

pub fn handle_key_press(
    event: &gdk::EventKey,
    mode_state: &ModeState,
    hint_buffer: &HintBuffer,
    new_tab_flag: &NewTabFlag,
    notebook: &gtk::Notebook,
    command_bar: &CommandBar,
) -> Propagation {
    let keyval = event.keyval();
    let current = modes::current_mode(mode_state);

    match current {
        Mode::Normal => handle_normal_mode(
            keyval,
            mode_state,
            hint_buffer,
            new_tab_flag,
            notebook,
            command_bar,
        ),
        Mode::Insert => handle_insert_mode(event, mode_state, notebook, command_bar),
        Mode::Command => handle_command_mode(event, mode_state, notebook, command_bar),
        Mode::Hint => handle_hint_mode(keyval, mode_state, hint_buffer, notebook, command_bar),
    }
}

fn handle_normal_mode(
    keyval: gdk::keys::Key,
    mode_state: &ModeState,
    hint_buffer: &HintBuffer,
    new_tab_flag: &NewTabFlag,
    notebook: &gtk::Notebook,
    command_bar: &CommandBar,
) -> Propagation {
    if keyval == gdk::keys::constants::question {
        info!("'?' pressed: showing keyboard shortcuts");
        show_shortcuts(notebook);
        return Propagation::Stop;
    }

    let webview = match tab::current_webview(notebook) {
        Some(wv) => wv,
        None => return Propagation::Proceed,
    };

    if keyval == gdk::keys::constants::j {
        info!("'j' pressed: scrolling down");
        scroll_webview(&webview, 0, SCROLL_STEP);
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::k {
        info!("'k' pressed: scrolling up");
        scroll_webview(&webview, 0, -SCROLL_STEP);
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::h {
        info!("'h' pressed: scrolling left");
        scroll_webview(&webview, -SCROLL_STEP, 0);
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::l {
        info!("'l' pressed: scrolling right");
        scroll_webview(&webview, SCROLL_STEP, 0);
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::d {
        info!("'d' pressed: scrolling half page down");
        scroll_webview(&webview, 0, SCROLL_PAGE);
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::u {
        info!("'u' pressed: scrolling half page up");
        scroll_webview(&webview, 0, -SCROLL_PAGE);
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::g {
        info!("'g' pressed: scrolling to top");
        run_js(
            &webview,
            "window.scrollTo({left: 0, top: 0, behavior: 'smooth'})",
        );
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::G {
        info!("'G' pressed: scrolling to bottom");
        run_js(
            &webview,
            "window.scrollTo({left: 0, top: document.body.scrollHeight, behavior: 'smooth'})",
        );
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::i {
        info!("'i' pressed: entering Insert mode");
        modes::set_mode(mode_state, Mode::Insert);
        command_bar.update_mode_label(Mode::Insert);
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::o {
        info!("'o' pressed: opening command bar with current URL");
        *new_tab_flag.borrow_mut() = false;
        modes::set_mode(mode_state, Mode::Insert);
        command_bar.update_mode_label(Mode::Insert);
        let uri = webview.uri().map(|u| u.to_string()).unwrap_or_default();
        command_bar.focus_with_url(&uri);
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::O {
        info!("'O' pressed: opening new tab with command bar");
        *new_tab_flag.borrow_mut() = true;
        modes::set_mode(mode_state, Mode::Insert);
        command_bar.update_mode_label(Mode::Insert);
        command_bar.focus_with_url("");
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::colon {
        info!("':' pressed: entering Command mode");
        modes::set_mode(mode_state, Mode::Command);
        command_bar.update_mode_label(Mode::Command);
        command_bar.focus_for_command();
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::f {
        info!("'f' pressed: entering Hint mode");
        modes::set_mode(mode_state, Mode::Hint);
        command_bar.update_mode_label(Mode::Hint);
        hint_buffer.borrow_mut().clear();
        hints::inject_hints(&webview);
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::r {
        info!("'r' pressed: reloading page");
        webview.reload();
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::H {
        info!("'H' pressed: going back");
        webview.go_back();
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::L {
        info!("'L' pressed: going forward");
        webview.go_forward();
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::J {
        info!("'J' pressed: previous tab");
        tab::prev_tab(notebook);
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::K {
        info!("'K' pressed: next tab");
        tab::next_tab(notebook);
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::x {
        info!("'x' pressed: closing current tab");
        tab::close_current_tab(notebook);
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::p {
        info!("'p' pressed: toggling tab pin");
        tab::toggle_current_pin(notebook);
        return Propagation::Stop;
    }
    if keyval == gdk::keys::constants::Escape {
        info!("Escape pressed in Normal mode");
        webview.grab_focus();
        return Propagation::Stop;
    }
    Propagation::Proceed
}

fn show_shortcuts(notebook: &gtk::Notebook) {
    let parent = notebook
        .toplevel()
        .and_then(|widget| widget.downcast::<gtk::Window>().ok());
    let dialog = gtk::Dialog::with_buttons(
        Some("Keyboard Shortcuts"),
        parent.as_ref(),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        &[("Close", gtk::ResponseType::Close)],
    );
    dialog.set_default_size(760, 620);

    let scrolled = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scrolled.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scrolled.set_can_focus(true);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);

    add_shortcut_section(
        &content,
        "Navigation",
        &[
            ("h", "Scroll left"),
            ("j", "Scroll down"),
            ("k", "Scroll up"),
            ("l", "Scroll right"),
            ("d", "Scroll down further"),
            ("u", "Scroll up further"),
            ("g", "Jump to top"),
            ("G", "Jump to bottom"),
        ],
    );
    add_shortcut_section(
        &content,
        "Page",
        &[
            ("H", "Go back"),
            ("L", "Go forward"),
            ("r", "Reload page"),
            ("f", "Show link hints"),
        ],
    );
    add_shortcut_section(
        &content,
        "Tabs",
        &[
            ("J", "Select previous tab"),
            ("K", "Select next tab"),
            ("O", "Open in new tab"),
            ("x", "Close current tab"),
            ("p", "Pin or unpin current tab"),
        ],
    );
    add_shortcut_section(
        &content,
        "Modes and Input",
        &[
            ("o", "Edit current URL"),
            ("i", "Enter Insert mode"),
            (":", "Enter Command mode"),
            ("Enter", "Submit input"),
            ("Esc", "Return to Normal mode"),
            ("?", "Show shortcuts"),
        ],
    );
    add_shortcut_section(
        &content,
        "Hint Mode",
        &[
            ("a s d f g h j k l", "Filter and activate hints"),
            ("Esc", "Cancel link hints"),
        ],
    );
    add_shortcut_section(
        &content,
        "Commands",
        &[
            (":open URL, :o URL", "Open URL"),
            (":tabopen URL, :tabnew URL, :to URL", "Open tab"),
            (":close, :c", "Close tab"),
            (":group NAME", "Create group with current tab"),
            (":groupadd NAME", "Add current tab to group"),
            (":groupcollapse [NAME]", "Collapse current or named group"),
            (":groupexpand [NAME]", "Expand current or named group"),
            (":pin [on|off]", "Pin or unpin current tab"),
            (":bw status", "Show Bitwarden or Vaultwarden status"),
            (":bw server URL", "Configure a Vaultwarden server"),
            (":bw unlock", "Unlock the vault"),
            (":bw fill", "Fill login fields"),
            (":reload, :r", "Reload"),
            (":back", "Back"),
            (":forward", "Forward"),
            (":quit, :q", "Quit"),
        ],
    );

    scrolled.add(&content);
    let vadjustment = scrolled.vadjustment();
    connect_shortcuts_dialog_keys(&dialog, &scrolled, &vadjustment);
    dialog.content_area().pack_start(&scrolled, true, true, 0);
    dialog.connect_response(|dialog, _| dialog.close());
    glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
        dialog.show_all();
        scrolled.grab_focus();
        glib::idle_add_local_once(move || {
            vadjustment.set_value(vadjustment.lower());
        });
    });
}

fn add_shortcut_section(container: &gtk::Box, title: &str, rows: &[(&str, &str)]) {
    let frame = gtk::Frame::new(None);
    let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
    section.set_margin_top(12);
    section.set_margin_bottom(12);
    section.set_margin_start(12);
    section.set_margin_end(12);

    let heading = gtk::Label::new(None);
    heading.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(title)));
    heading.set_xalign(0.0);
    section.pack_start(&heading, false, false, 0);

    let grid = gtk::Grid::new();
    grid.set_column_spacing(8);
    grid.set_row_spacing(6);

    for (index, (keys, action)) in rows.iter().enumerate() {
        let key_label = gtk::Label::new(None);
        key_label.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(keys)));
        key_label.set_xalign(0.0);
        key_label.set_can_focus(false);
        key_label.set_width_chars(6);

        let action_label = gtk::Label::new(None);
        action_label.set_text(action);
        action_label.set_xalign(0.0);
        action_label.set_can_focus(false);

        grid.attach(&key_label, 0, index as i32, 1, 1);
        grid.attach(&action_label, 1, index as i32, 1, 1);
    }

    section.pack_start(&grid, false, false, 0);
    frame.add(&section);
    container.pack_start(&frame, false, false, 0);
}

fn connect_shortcuts_dialog_keys(
    dialog: &gtk::Dialog,
    scrolled: &gtk::ScrolledWindow,
    vadjustment: &gtk::Adjustment,
) {
    let scroll = scrolled.clone();
    let adjustment = vadjustment.clone();
    dialog.connect_key_press_event(move |dialog, event| {
        let keyval = event.keyval();

        if keyval == gdk::keys::constants::Escape {
            dialog.close();
            return Propagation::Stop;
        }

        let step = 48.0;
        let page = scroll.allocation().height().max(1) as f64 * 0.85;

        let target = if keyval == gdk::keys::constants::j || keyval == gdk::keys::constants::Down {
            Some(adjustment.value() + step)
        } else if keyval == gdk::keys::constants::k || keyval == gdk::keys::constants::Up {
            Some(adjustment.value() - step)
        } else if keyval == gdk::keys::constants::d || keyval == gdk::keys::constants::Page_Down {
            Some(adjustment.value() + page)
        } else if keyval == gdk::keys::constants::u || keyval == gdk::keys::constants::Page_Up {
            Some(adjustment.value() - page)
        } else if keyval == gdk::keys::constants::g || keyval == gdk::keys::constants::Home {
            Some(adjustment.lower())
        } else if keyval == gdk::keys::constants::G || keyval == gdk::keys::constants::End {
            Some(adjustment.upper())
        } else {
            None
        };

        if let Some(value) = target {
            let upper = adjustment.upper() - adjustment.page_size();
            adjustment.set_value(value.clamp(adjustment.lower(), upper));
            return Propagation::Stop;
        }

        Propagation::Proceed
    });
}

fn handle_insert_mode(
    event: &gdk::EventKey,
    mode_state: &ModeState,
    notebook: &gtk::Notebook,
    command_bar: &CommandBar,
) -> Propagation {
    let keyval = event.keyval();

    if command_bar.entry.has_focus()
        && keyval == gdk::keys::constants::w
        && event.state().contains(gdk::ModifierType::CONTROL_MASK)
    {
        info!("Ctrl+w pressed: deleting previous word in command bar");
        delete_previous_entry_word(&command_bar.entry);
        return Propagation::Stop;
    }

    if keyval == gdk::keys::constants::Escape {
        info!("Escape pressed: returning to Normal mode from Insert");
        modes::set_mode(mode_state, Mode::Normal);
        command_bar.update_mode_label(Mode::Normal);
        command_bar.clear_and_unfocus();
        if let Some(wv) = tab::current_webview(notebook) {
            wv.grab_focus();
        }
        return Propagation::Stop;
    }
    Propagation::Proceed
}

fn delete_previous_entry_word(entry: &gtk::Entry) {
    if let Some((selection_start, selection_end)) = entry.selection_bounds() {
        let start = selection_start.min(selection_end);
        let end = selection_start.max(selection_end);
        entry.delete_text(start, end);
        entry.set_position(start);
        return;
    }

    let text = entry.text().to_string();
    let cursor = entry.position().max(0) as usize;
    let start = previous_word_start(&text, cursor);

    if start < cursor {
        entry.delete_text(start as i32, cursor as i32);
        entry.set_position(start as i32);
    }
}

fn previous_word_start(text: &str, cursor: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor.min(chars.len());
    let mut start = cursor;

    while start > 0 && chars[start - 1].is_whitespace() {
        start -= 1;
    }

    if start == 0 {
        return 0;
    }

    if is_word_char(chars[start - 1]) {
        while start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
        }
    } else {
        while start > 0 && !chars[start - 1].is_whitespace() && !is_word_char(chars[start - 1]) {
            start -= 1;
        }
    }

    start
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn handle_command_mode(
    event: &gdk::EventKey,
    mode_state: &ModeState,
    notebook: &gtk::Notebook,
    command_bar: &CommandBar,
) -> Propagation {
    let keyval = event.keyval();

    if keyval == gdk::keys::constants::Escape {
        info!("Escape pressed: returning to Normal mode from Command");
        modes::set_mode(mode_state, Mode::Normal);
        command_bar.update_mode_label(Mode::Normal);
        command_bar.clear_and_unfocus();
        if let Some(wv) = tab::current_webview(notebook) {
            wv.grab_focus();
        }
        return Propagation::Stop;
    }

    if command_bar.entry.has_focus() {
        if keyval == gdk::keys::constants::Tab && command_bar.apply_selected_completion() {
            info!("Tab pressed: applying command completion");
            return Propagation::Stop;
        }

        if keyval == gdk::keys::constants::Down && command_bar.select_next_completion() {
            info!("Down pressed: selecting next command completion");
            return Propagation::Stop;
        }

        if keyval == gdk::keys::constants::Up && command_bar.select_previous_completion() {
            info!("Up pressed: selecting previous command completion");
            return Propagation::Stop;
        }
    }

    Propagation::Proceed
}

fn handle_hint_mode(
    keyval: gdk::keys::Key,
    mode_state: &ModeState,
    hint_buffer: &HintBuffer,
    notebook: &gtk::Notebook,
    command_bar: &CommandBar,
) -> Propagation {
    if keyval == gdk::keys::constants::Escape {
        info!("Escape pressed: canceling Hint mode");
        modes::set_mode(mode_state, Mode::Normal);
        command_bar.update_mode_label(Mode::Normal);
        hint_buffer.borrow_mut().clear();
        if let Some(wv) = tab::current_webview(notebook) {
            hints::remove_hints(&wv);
        }
        return Propagation::Stop;
    }

    if let Some(c) = hints::label_for_keyval(keyval) {
        hint_buffer.borrow_mut().push(c);
        let typed = hint_buffer.borrow().clone();
        info!("hint char typed: '{}', buffer: '{}'", c, typed);

        if let Some(wv) = tab::current_webview(notebook) {
            hints::filter_hints(
                &wv,
                &typed,
                mode_state,
                hint_buffer,
                &command_bar.mode_label,
            );
        }

        return Propagation::Stop;
    }

    // Unknown key in hint mode, ignore
    info!("ignoring non-hint key in Hint mode");
    Propagation::Stop
}

fn scroll_webview(webview: &webkit2gtk::WebView, x: i32, y: i32) {
    let js = format!(
        "window.scrollBy({{left: {}, top: {}, behavior: 'smooth'}})",
        x, y
    );
    run_js(webview, &js);
}

fn run_js(webview: &webkit2gtk::WebView, script: &str) {
    info!("executing JS: {}", script);
    webview.evaluate_javascript(
        script,
        None,
        None,
        None::<&gtk::gio::Cancellable>,
        |result| {
            if let Err(e) = result {
                log::error!("JS execution error: {}", e);
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::previous_word_start;

    #[test]
    fn previous_word_start_skips_trailing_space() {
        assert_eq!(previous_word_start("hello world   ", 14), 6);
    }

    #[test]
    fn previous_word_start_stops_at_url_punctuation() {
        assert_eq!(previous_word_start("https://example.com/path", 24), 20);
    }

    #[test]
    fn previous_word_start_deletes_punctuation_run() {
        assert_eq!(previous_word_start("https://", 8), 5);
    }
}
