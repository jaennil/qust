# Qust

Qust is a keyboard-driven GTK/WebKit browser with Vim-style modes, tab groups,
command completion, and optional Bitwarden/Vaultwarden integration.

## Build

The project requires Rust, the GTK 3/WebKitGTK development libraries, and the
GStreamer good plugins used by WebKit for audio output. On Arch Linux:

```sh
sudo pacman -S --needed gst-plugins-good
```

```sh
cargo build --release
```

## Install

Install an existing release binary for the current user:

```sh
make install
```

The binary is installed to `~/.local/bin/qust`. Make sure `~/.local/bin` is in
your `PATH`. Override `PREFIX` or `BINDIR` when a different destination is
needed. `make install` also installs `qust.desktop` into
`~/.local/share/applications` so that desktop environments can list Qust as a
browser.

## Default browser

Make Qust the system browser so links from other applications open in it:

```sh
make default-browser
```

That installs the current build, registers `qust.desktop` with
`xdg-settings set default-web-browser`, and claims `text/html`,
`x-scheme-handler/http`, and `x-scheme-handler/https`. Restart Qust afterwards
so the running instance accepts URLs.

Opening a URL while Qust runs adds it as a new tab in the existing window
instead of starting a second browser:

```sh
qust https://example.com
```

Arguments go through the same normalization as `:open`, so `qust example.com`
and `qust rust ownership` work as well; the latter uses the configured search
engine. To restore another browser, run
`xdg-settings set default-web-browser <other>.desktop`.

## Commands

Press `:` to open the command bar. Suggestions include command usage and a
short description. Commands with subcommands, such as `:bw` and `:pin`, show
nested suggestions after a space.

Common commands include `:open`, `:tabopen`, `:tabclose`, `:group`, `:pin`, and
`:bw`. Use Tab or the arrow keys to select a suggestion and Enter to run it.

Use `:firefox-import` to import the active HTTP(S) URL from every open Firefox
tab. Qust reads the newest recovery session from standard Firefox profile
locations, preserves pinned tabs and named tab groups, and creates unloaded tabs
so the whole session is not loaded at once.

Press `/` in Normal mode to search open tabs by cached title or URL. Use
`Ctrl+j` and `Ctrl+k` (or the arrow keys) to select a result, Enter to focus it,
and Escape to cancel.

Set the default search engine using a built-in preset:

```text
:search-engine google
:search-engine yandex
```

Available presets are `google`, `yandex`, `duckduckgo` (`ddg`), `bing`, and
`brave`. A custom URL template containing `{query}` is also supported. Use
`:search-engine` to show the current template and `:search-engine reset` to
restore DuckDuckGo. The setting is stored in `~/.config/qust/config.json`.

Set the hint label font size in pixels with `:hint-size 16`. Supported sizes are
8 through 32 pixels. Use `:hint-size` to show the current size and
`:hint-size reset` to restore the default 12px size.

Set the hint label background opacity with `:hint-opacity 80`. Supported values
are 20 through 100 percent; label text and borders stay fully opaque. Use
`:hint-opacity` to show the current value and `:hint-opacity reset` to restore
the default 100% opacity.
