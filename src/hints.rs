use gtk::prelude::*;
use javascriptcore::ValueExt;
use log::info;
use webkit2gtk::WebViewExt;

use crate::modes::{self, HintBuffer, Mode, ModeState};

const HINT_CHARS: &str = "asdfghjkl";

const INJECT_HINTS_JS: &str = r#"
(function() {
    document.querySelectorAll('.qust-hint').forEach(el => el.remove());

    const CHARS = 'asdfghjkl';
    const elements = document.querySelectorAll('a, button, input, select, textarea, [onclick], [role="button"], [role="link"]');
    const visible = [];

    for (const el of elements) {
        const rect = el.getBoundingClientRect();
        if (rect.width > 0 && rect.height > 0 &&
            rect.top >= 0 && rect.top < window.innerHeight &&
            rect.left >= 0 && rect.left < window.innerWidth) {
            visible.push({ el, rect });
        }
    }

    let labelWidth = 1;
    let capacity = CHARS.length;
    while (capacity < visible.length) {
        labelWidth++;
        capacity *= CHARS.length;
    }

    function generateLabel(index) {
        let label = '';
        for (let position = 0; position < labelWidth; position++) {
            label = CHARS[index % CHARS.length] + label;
            index = Math.floor(index / CHARS.length);
        }
        return label;
    }

    const hints = [];
    for (let i = 0; i < visible.length; i++) {
        const { el, rect } = visible[i];
        const label = generateLabel(i);

        const hint = document.createElement('div');
        hint.className = 'qust-hint';
        hint.dataset.label = label;
        hint.textContent = label;
        hint.style.cssText = `
            position: fixed;
            left: ${rect.left}px;
            top: ${rect.top}px;
            z-index: 2147483647;
            background: #f1c40f;
            color: #000;
            font-size: 12px;
            font-weight: bold;
            font-family: monospace;
            padding: 1px 4px;
            border-radius: 3px;
            border: 1px solid #000;
            pointer-events: none;
        `;
        document.body.appendChild(hint);
        hints.push({ label, el });
    }

    window.__qust_hints = hints;
    return visible.length.toString();
})()
"#;

const REMOVE_HINTS_JS: &str = r#"
(function() {
    document.querySelectorAll('.qust-hint').forEach(el => el.remove());
    delete window.__qust_hints;
})()
"#;

fn build_filter_js(typed: &str) -> String {
    format!(
        r#"
(function() {{
    const typed = '{}';
    const hints = document.querySelectorAll('.qust-hint');
    let matchCount = 0;
    let exactMatch = null;

    for (const hint of hints) {{
        const label = hint.dataset.label;
        if (label === typed) {{
            exactMatch = label;
            matchCount++;
            hint.style.background = '#2ecc71';
        }} else if (label.startsWith(typed)) {{
            matchCount++;
            hint.style.background = '#f1c40f';
        }} else {{
            hint.style.display = 'none';
        }}
    }}

    if (exactMatch && matchCount === 1) {{
        const hintData = window.__qust_hints;
        if (hintData) {{
            for (const h of hintData) {{
                if (h.label === exactMatch) {{
                    h.el.click();
                    break;
                }}
            }}
        }}
        document.querySelectorAll('.qust-hint').forEach(el => el.remove());
        delete window.__qust_hints;
        return 'clicked';
    }}

    if (matchCount === 0) {{
        document.querySelectorAll('.qust-hint').forEach(el => el.remove());
        delete window.__qust_hints;
        return 'none';
    }}

    return 'filtering';
}})()
"#,
        typed
    )
}

pub fn inject_hints(webview: &webkit2gtk::WebView) {
    info!("injecting hint labels into page");
    run_js(webview, INJECT_HINTS_JS);
}

pub fn remove_hints(webview: &webkit2gtk::WebView) {
    info!("removing hint labels from page");
    run_js(webview, REMOVE_HINTS_JS);
}

pub fn filter_hints(
    webview: &webkit2gtk::WebView,
    typed: &str,
    mode_state: &ModeState,
    hint_buffer: &HintBuffer,
    mode_label: &gtk::Label,
) {
    info!("filtering hints with typed chars: '{}'", typed);
    let js = build_filter_js(typed);

    let ms = mode_state.clone();
    let hb = hint_buffer.clone();
    let ml = mode_label.clone();

    webview.evaluate_javascript(
        &js,
        None,
        None,
        None::<&gtk::gio::Cancellable>,
        move |result| match result {
            Ok(value) => {
                let result_str = value.to_str().to_string();
                info!("hint filter result: {}", result_str);
                if result_str == "clicked" || result_str == "none" {
                    info!("hints done ({}), returning to Normal", result_str);
                    modes::set_mode(&ms, Mode::Normal);
                    hb.borrow_mut().clear();
                    ml.set_text(&Mode::Normal.to_string());
                }
            }
            Err(e) => {
                log::error!("failed to filter hints: {}", e);
                modes::set_mode(&ms, Mode::Normal);
                hb.borrow_mut().clear();
                ml.set_text(&Mode::Normal.to_string());
            }
        },
    );
}

pub fn label_for_keyval(keyval: gdk::keys::Key) -> Option<char> {
    let c = keyval_to_char(keyval)?;
    if HINT_CHARS.contains(c) {
        Some(c)
    } else {
        None
    }
}

fn keyval_to_char(keyval: gdk::keys::Key) -> Option<char> {
    let unicode = unsafe { gdk::ffi::gdk_keyval_to_unicode(*keyval) };
    if unicode == 0 {
        return None;
    }
    char::from_u32(unicode)
}

fn run_js(webview: &webkit2gtk::WebView, script: &str) {
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
