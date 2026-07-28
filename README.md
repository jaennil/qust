# Qust

Qust is a keyboard-driven GTK/WebKit browser with Vim-style modes, tab groups,
command completion, and optional Bitwarden/Vaultwarden integration.

## Build

The project requires Rust and the GTK 3/WebKitGTK development libraries.

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
