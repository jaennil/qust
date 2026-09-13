use gtk::prelude::*;
use javascriptcore::ValueExt;
use log::info;
use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void};
use webkit2gtk::WebViewExt;

use crate::modes::{self, HintBuffer, Mode, ModeState};
use crate::navigation;

const HINT_CHARS: &str = "asdfghjkl";

const INJECT_HINTS_JS: &str = r#"
(function() {
    document.querySelectorAll('.qust-hint').forEach(el => el.remove());

    const CHARS = 'asdfghjkl';
    const topLayer = (function() {
        if (document.fullscreenElement) {
            return document.fullscreenElement;
        }
        const dialogs = document.querySelectorAll('dialog[open]');
        for (let i = dialogs.length - 1; i >= 0; i--) {
            try {
                if (dialogs[i].matches(':modal')) {
                    return dialogs[i];
                }
            } catch (error) {
                break;
            }
        }
        return null;
    })();
    const root = topLayer || document;
    const container = topLayer || document.body;
    const elements = root.querySelectorAll('a, button, input, select, textarea, iframe, [onclick], [role="button"], [role="link"], [role="menuitem"], [role="option"]');
    const visible = [];

    for (const candidate of elements) {
        let el = candidate;
        let rect = el.getBoundingClientRect();
        if ((rect.width <= 0 || rect.height <= 0) &&
            el instanceof HTMLInputElement && el.labels && el.labels.length > 0) {
            el = el.labels[0];
            rect = el.getBoundingClientRect();
        }
        if (rect.width > 0 && rect.height > 0 &&
            rect.top >= 0 && rect.top < window.innerHeight &&
            rect.left >= 0 && rect.left < window.innerWidth) {
            const frameDescription = el.tagName === 'IFRAME'
                ? `${el.src || ''} ${el.title || ''} ${el.name || ''}`
                : '';
            const isChallengeFrame = /challenges\.cloudflare\.com|turnstile|captcha/i.test(frameDescription);
            const clickX = isChallengeFrame
                ? rect.left + Math.min(28, rect.width / 2)
                : rect.left + rect.width / 2;
            const clickY = rect.top + rect.height / 2;
            const useNativeClick = isChallengeFrame || el.matches('input, select, textarea, iframe');
            visible.push({ el, rect, clickX, clickY, useNativeClick });
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
        const { el, rect, clickX, clickY, useNativeClick } = visible[i];
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
            background: rgba(241, 196, 15, __QUST_HINT_ALPHA__);
            color: #000;
            font-size: __QUST_HINT_SIZE__px;
            font-weight: bold;
            font-family: monospace;
            padding: 1px 4px;
            border-radius: 3px;
            border: 1px solid #000;
            pointer-events: none;
        `;
        container.appendChild(hint);
        hints.push({ label, el, clickX, clickY, useNativeClick });
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

fn build_filter_js(typed: &str, opacity: u8) -> String {
    let alpha = opacity_alpha(opacity);
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
            hint.style.background = 'rgba(46, 204, 113, {alpha})';
        }} else if (label.startsWith(typed)) {{
            matchCount++;
            hint.style.background = 'rgba(241, 196, 15, {alpha})';
        }} else {{
            hint.style.display = 'none';
        }}
    }}

    if (exactMatch && matchCount === 1) {{
        const hintData = window.__qust_hints;
        if (hintData) {{
            for (const h of hintData) {{
                if (h.label === exactMatch) {{
                    const x = Math.round(h.clickX);
                    const y = Math.round(h.clickY);
                    document.querySelectorAll('.qust-hint').forEach(el => el.remove());
                    delete window.__qust_hints;
                    if (!h.useNativeClick && h.el && h.el.isConnected) {{
                        h.el.click();
                        return 'clicked';
                    }}
                    return 'native:' + x + ':' + y;
                }}
            }}
        }}
        document.querySelectorAll('.qust-hint').forEach(el => el.remove());
        delete window.__qust_hints;
        return 'none';
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
    run_js(
        webview,
        &build_inject_hints_js(navigation::hint_size(), navigation::hint_opacity()),
    );
}

fn build_inject_hints_js(size: u16, opacity: u8) -> String {
    INJECT_HINTS_JS
        .replace("__QUST_HINT_SIZE__", &size.to_string())
        .replace("__QUST_HINT_ALPHA__", &opacity_alpha(opacity))
}

fn opacity_alpha(opacity: u8) -> String {
    format!("{:.2}", f64::from(opacity) / 100.0)
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
    let js = build_filter_js(typed, navigation::hint_opacity());

    let ms = mode_state.clone();
    let hb = hint_buffer.clone();
    let ml = mode_label.clone();
    let click_webview = webview.clone();

    webview.evaluate_javascript(
        &js,
        None,
        None,
        None::<&gtk::gio::Cancellable>,
        move |result| match result {
            Ok(value) => {
                let result_str = value.to_str().to_string();
                info!("hint filter result: {}", result_str);
                if let Some((x, y)) = parse_native_click(&result_str) {
                    click_webview_at(&click_webview, x, y);
                }
                if result_str.starts_with("native:")
                    || result_str == "clicked"
                    || result_str == "none"
                {
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

fn parse_native_click(result: &str) -> Option<(i32, i32)> {
    let mut parts = result.split(':');
    if parts.next()? != "native" {
        return None;
    }
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((x, y))
}

fn click_webview_at(webview: &webkit2gtk::WebView, css_x: i32, css_y: i32) {
    let Some(window) = webview.window() else {
        log::warn!("cannot click hint: WebView has no GDK window");
        return;
    };
    let zoom = webview.zoom_level();
    let x = (f64::from(css_x) * zoom).round() as i32;
    let y = (f64::from(css_y) * zoom).round() as i32;
    let (_, origin_x, origin_y) = window.origin();
    if xtest_click(origin_x + x, origin_y + y) {
        return;
    }

    let pressed = gdk::test_simulate_button(
        &window,
        x,
        y,
        1,
        gdk::ModifierType::empty(),
        gdk::EventType::ButtonPress,
    );
    let released = gdk::test_simulate_button(
        &window,
        x,
        y,
        1,
        gdk::ModifierType::empty(),
        gdk::EventType::ButtonRelease,
    );
    if !pressed {
        log::warn!("failed to press native hint click at {x},{y}");
    }
    if !released {
        log::warn!("failed to release native hint click at {x},{y}");
    }
}

fn xtest_click(root_x: i32, root_y: i32) -> bool {
    unsafe {
        let display = XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return false;
        }
        let moved = XTestFakeMotionEvent(display, 0, root_x, root_y, 0) != 0;
        let pressed = XTestFakeButtonEvent(display, 1, 1, 0) != 0;
        let released = XTestFakeButtonEvent(display, 1, 0, 10) != 0;
        XFlush(display);
        XCloseDisplay(display);
        moved && pressed && released
    }
}

#[link(name = "X11")]
extern "C" {
    fn XOpenDisplay(display_name: *const c_char) -> *mut c_void;
    fn XCloseDisplay(display: *mut c_void) -> c_int;
    fn XFlush(display: *mut c_void) -> c_int;
}

#[link(name = "Xtst")]
extern "C" {
    fn XTestFakeMotionEvent(
        display: *mut c_void,
        screen_number: c_int,
        x: c_int,
        y: c_int,
        delay: c_ulong,
    ) -> c_int;
    fn XTestFakeButtonEvent(
        display: *mut c_void,
        button: c_uint,
        is_press: c_int,
        delay: c_ulong,
    ) -> c_int;
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

#[cfg(test)]
mod tests {
    use super::{build_filter_js, build_inject_hints_js, click_webview_at, parse_native_click};
    use gtk::prelude::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};
    use webkit2gtk::{LoadEvent, WebViewExt};

    #[test]
    fn native_click_result_requires_two_integer_coordinates() {
        assert_eq!(parse_native_click("native:12:34"), Some((12, 34)));
        assert_eq!(parse_native_click("native:12"), None);
        assert_eq!(parse_native_click("native:x:34"), None);
        assert_eq!(parse_native_click("clicked"), None);
    }

    #[test]
    fn hint_size_is_injected_into_styles() {
        let script = build_inject_hints_js(18, 75);
        assert!(script.contains("font-size: 18px"));
        assert!(script.contains("rgba(241, 196, 15, 0.75)"));
    }

    #[test]
    #[ignore = "requires a graphical display"]
    fn hints_render_inside_an_open_modal_dialog() {
        gtk::init().expect("GTK display");
        let webview = webkit2gtk::WebView::new();
        webview.connect_load_changed(|webview, event| {
            if event == LoadEvent::Finished {
                let script = format!(
                    "document.querySelector('dialog').showModal();\n{};\n(function() {{\n    const hints = Array.from(document.querySelectorAll('.qust-hint'));\n    const parents = new Set(hints.map(hint => hint.parentElement.tagName));\n    document.title = `hints=${{hints.length}} parents=${{Array.from(parents).join(',')}}`;\n}})()",
                    build_inject_hints_js(12, 100)
                );
                webview.evaluate_javascript(
                    &script,
                    None,
                    None,
                    None::<&gtk::gio::Cancellable>,
                    |result| {
                        if let Err(error) = result {
                            log::error!("modal hint injection failed: {error}");
                        }
                    },
                );
            }
        });

        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_default_size(400, 300);
        window.add(&webview);
        window.show_all();

        let title = Rc::new(std::cell::RefCell::new(String::new()));
        let title_poll = title.clone();
        let webview_poll = webview.clone();
        let main_loop = glib::MainLoop::new(None, false);
        let main_loop_poll = main_loop.clone();
        let deadline = Instant::now() + Duration::from_secs(5);
        glib::timeout_add_local(Duration::from_millis(20), move || {
            if let Some(current) = webview_poll.title() {
                if current.starts_with("hints=") {
                    *title_poll.borrow_mut() = current.to_string();
                    main_loop_poll.quit();
                    return glib::ControlFlow::Break;
                }
            }
            if Instant::now() >= deadline {
                main_loop_poll.quit();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });

        webview.load_html(
            r#"<button id="background">background</button><dialog><input id="title"><button id="create">Create</button></dialog>"#,
            None,
        );
        main_loop.run();
        window.close();

        assert_eq!(
            title.borrow().as_str(),
            "hints=2 parents=DIALOG",
            "modal dialog controls must be hinted inside the dialog"
        );
    }

    #[test]
    #[ignore = "requires a graphical display"]
    fn dom_hint_activates_hidden_labeled_control() {
        gtk::init().expect("GTK display");
        let webview = webkit2gtk::WebView::new();
        webview.connect_load_changed(|webview, event| {
            if event == LoadEvent::Finished {
                let script = format!(
                    "{};\n{}",
                    build_inject_hints_js(12, 100),
                    build_filter_js("a", 100)
                );
                webview.evaluate_javascript(
                    &script,
                    None,
                    None,
                    None::<&gtk::gio::Cancellable>,
                    |result| {
                        if let Err(error) = result {
                            log::error!("DOM hint activation failed: {error}");
                        }
                    },
                );
            }
        });

        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_default_size(400, 300);
        window.add(&webview);
        window.show_all();

        let clicked = Rc::new(Cell::new(false));
        let clicked_poll = clicked.clone();
        let webview_poll = webview.clone();
        let main_loop = glib::MainLoop::new(None, false);
        let main_loop_poll = main_loop.clone();
        let deadline = Instant::now() + Duration::from_secs(5);
        glib::timeout_add_local(Duration::from_millis(20), move || {
            if webview_poll.title().as_deref() == Some("clicked") {
                clicked_poll.set(true);
                main_loop_poll.quit();
                return glib::ControlFlow::Break;
            }
            if Instant::now() >= deadline {
                main_loop_poll.quit();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });

        webview.load_html(
            r#"<input id="range" data-role="item" type="checkbox" style="width:0;opacity:0" onchange="document.title='clicked'"><label for="range">Last 5 minutes</label>"#,
            None,
        );
        main_loop.run();
        window.close();

        assert!(clicked.get(), "DOM hint did not activate labeled control");
    }

    #[test]
    #[ignore = "requires a graphical display"]
    fn native_hint_click_activates_web_content() {
        gtk::init().expect("GTK display");
        let webview = webkit2gtk::WebView::new();
        webview.connect_load_changed(|webview, event| {
            if event == LoadEvent::Finished {
                click_webview_at(webview, 70, 45);
            }
        });

        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_default_size(400, 300);
        window.add(&webview);
        window.show_all();

        let clicked = Rc::new(Cell::new(false));
        let clicked_poll = clicked.clone();
        let webview_poll = webview.clone();
        let main_loop = glib::MainLoop::new(None, false);
        let main_loop_poll = main_loop.clone();
        let deadline = Instant::now() + Duration::from_secs(5);
        glib::timeout_add_local(Duration::from_millis(20), move || {
            if webview_poll.title().as_deref() == Some("clicked") {
                clicked_poll.set(true);
                main_loop_poll.quit();
                return glib::ControlFlow::Break;
            }
            if Instant::now() >= deadline {
                main_loop_poll.quit();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });

        webview.load_html(
            r#"<button style="position:fixed;left:20px;top:20px;width:100px;height:50px" onclick="document.title='clicked'">Click</button>"#,
            None,
        );
        main_loop.run();
        window.close();

        assert!(clicked.get(), "native click did not reach web content");
    }
}
