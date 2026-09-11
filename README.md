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
needed.

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

Set the default search engine using a built-in preset:

```text
:search-engine google
:search-engine yandex
```

Available presets are `google`, `yandex`, `duckduckgo` (`ddg`), `bing`, and
`brave`. A custom URL template containing `{query}` is also supported. Use
`:search-engine` to show the current template and `:search-engine reset` to
restore DuckDuckGo. The setting is stored in `~/.config/qust/config.json`.
