use gtk::prelude::*;
use log::info;
use webkit2gtk::WebViewExt;

use crate::password_manager::{BwStatus, PasswordError, PasswordManager, VaultStatus};
use crate::tab;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
    pub description: &'static str,
    pub accepts_args: bool,
    pub subcommands: &'static [SubcommandSpec],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubcommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
    pub description: &'static str,
    pub accepts_args: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSuggestion {
    pub name: String,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
    pub description: &'static str,
    pub completion: String,
    pub accepts_args: bool,
}

const NO_SUBCOMMANDS: &[SubcommandSpec] = &[];

const PIN_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "on",
        aliases: &[],
        usage: ":pin on",
        description: "Pin the current tab",
        accepts_args: false,
    },
    SubcommandSpec {
        name: "off",
        aliases: &[],
        usage: ":pin off",
        description: "Unpin the current tab",
        accepts_args: false,
    },
];

const BW_SUBCOMMANDS: &[SubcommandSpec] = &[
    SubcommandSpec {
        name: "status",
        aliases: &[],
        usage: ":bw status",
        description: "Show vault, server, and account status",
        accepts_args: false,
    },
    SubcommandSpec {
        name: "server",
        aliases: &[],
        usage: ":bw server URL",
        description: "Configure the Bitwarden or Vaultwarden server",
        accepts_args: true,
    },
    SubcommandSpec {
        name: "unlock",
        aliases: &[],
        usage: ":bw unlock",
        description: "Unlock the vault",
        accepts_args: false,
    },
    SubcommandSpec {
        name: "lock",
        aliases: &[],
        usage: ":bw lock",
        description: "Lock the vault",
        accepts_args: false,
    },
    SubcommandSpec {
        name: "fill",
        aliases: &[],
        usage: ":bw fill",
        description: "Fill login fields on the current page",
        accepts_args: false,
    },
    SubcommandSpec {
        name: "copy-user",
        aliases: &["copy-username"],
        usage: ":bw copy-user",
        description: "Copy the matching username",
        accepts_args: false,
    },
    SubcommandSpec {
        name: "copy-pass",
        aliases: &["copy-password"],
        usage: ":bw copy-pass",
        description: "Copy the matching password",
        accepts_args: false,
    },
];

const COMMAND_SPECS: &[CommandSpec] = &[
    CommandSpec {
        name: "open",
        aliases: &["o"],
        usage: ":open URL",
        description: "Open URL or search in the current tab",
        accepts_args: true,
        subcommands: NO_SUBCOMMANDS,
    },
    CommandSpec {
        name: "tabopen",
        aliases: &["tabnew", "to"],
        usage: ":tabopen [URL]",
        description: "Open a new tab",
        accepts_args: true,
        subcommands: NO_SUBCOMMANDS,
    },
    CommandSpec {
        name: "close",
        aliases: &["c"],
        usage: ":close",
        description: "Close the current tab",
        accepts_args: false,
        subcommands: NO_SUBCOMMANDS,
    },
    CommandSpec {
        name: "quit",
        aliases: &["q"],
        usage: ":quit",
        description: "Close the browser window",
        accepts_args: false,
        subcommands: NO_SUBCOMMANDS,
    },
    CommandSpec {
        name: "reload",
        aliases: &["r"],
        usage: ":reload",
        description: "Reload the current page",
        accepts_args: false,
        subcommands: NO_SUBCOMMANDS,
    },
    CommandSpec {
        name: "back",
        aliases: &[],
        usage: ":back",
        description: "Go back in the current tab",
        accepts_args: false,
        subcommands: NO_SUBCOMMANDS,
    },
    CommandSpec {
        name: "forward",
        aliases: &[],
        usage: ":forward",
        description: "Go forward in the current tab",
        accepts_args: false,
        subcommands: NO_SUBCOMMANDS,
    },
    CommandSpec {
        name: "group",
        aliases: &["groupnew"],
        usage: ":group NAME",
        description: "Create a group with the current tab",
        accepts_args: true,
        subcommands: NO_SUBCOMMANDS,
    },
    CommandSpec {
        name: "groupadd",
        aliases: &[],
        usage: ":groupadd NAME",
        description: "Add the current tab to a group",
        accepts_args: true,
        subcommands: NO_SUBCOMMANDS,
    },
    CommandSpec {
        name: "groupcollapse",
        aliases: &["gcollapse"],
        usage: ":groupcollapse [NAME]",
        description: "Collapse the current or named group",
        accepts_args: true,
        subcommands: NO_SUBCOMMANDS,
    },
    CommandSpec {
        name: "groupexpand",
        aliases: &["gexpand"],
        usage: ":groupexpand [NAME]",
        description: "Expand the current or named group",
        accepts_args: true,
        subcommands: NO_SUBCOMMANDS,
    },
    CommandSpec {
        name: "pin",
        aliases: &[],
        usage: ":pin [on|off]",
        description: "Pin or unpin the current tab",
        accepts_args: true,
        subcommands: PIN_SUBCOMMANDS,
    },
    CommandSpec {
        name: "bw",
        aliases: &["bitwarden", "vaultwarden"],
        usage: ":bw SUBCOMMAND",
        description: "Manage Bitwarden or Vaultwarden integration",
        accepts_args: true,
        subcommands: BW_SUBCOMMANDS,
    },
];

pub fn command_specs() -> &'static [CommandSpec] {
    COMMAND_SPECS
}

pub fn command_suggestions(input: &str) -> Vec<CommandSuggestion> {
    let command_input = command_input(input);

    if command_input.is_empty() {
        return command_specs().iter().map(top_level_suggestion).collect();
    }

    let (command_name, args) = split_token(command_input);
    let Some(command) = command_specs()
        .iter()
        .find(|spec| matching_command_name(spec, command_name, true).is_some())
    else {
        return command_specs()
            .iter()
            .filter(|spec| matching_command_name(spec, command_name, false).is_some())
            .map(top_level_suggestion)
            .collect();
    };

    let Some(args) = args else {
        return vec![top_level_suggestion(command)];
    };

    if command.subcommands.is_empty() {
        return vec![top_level_suggestion(command)];
    }

    let args = args.trim_start();
    let (subcommand_name, subcommand_args) = split_token(args);
    let exact = subcommand_args.is_some();

    command
        .subcommands
        .iter()
        .filter(|subcommand| {
            matching_name(subcommand.name, subcommand.aliases, subcommand_name, exact).is_some()
        })
        .map(|subcommand| subcommand_suggestion(command, subcommand))
        .collect()
}

fn top_level_suggestion(spec: &'static CommandSpec) -> CommandSuggestion {
    CommandSuggestion {
        name: spec.name.to_string(),
        aliases: spec.aliases,
        usage: spec.usage,
        description: spec.description,
        completion: format!(":{}", spec.name),
        accepts_args: spec.accepts_args,
    }
}

fn subcommand_suggestion(
    command: &'static CommandSpec,
    subcommand: &'static SubcommandSpec,
) -> CommandSuggestion {
    CommandSuggestion {
        name: format!("{} {}", command.name, subcommand.name),
        aliases: subcommand.aliases,
        usage: subcommand.usage,
        description: subcommand.description,
        completion: format!(":{} {}", command.name, subcommand.name),
        accepts_args: subcommand.accepts_args,
    }
}

fn split_token(input: &str) -> (&str, Option<&str>) {
    input
        .find(char::is_whitespace)
        .map(|index| (&input[..index], Some(&input[index..])))
        .unwrap_or((input, None))
}

fn command_input(input: &str) -> &str {
    input
        .trim_start()
        .strip_prefix(':')
        .unwrap_or(input)
        .trim_start()
}

fn matching_command_name(
    spec: &'static CommandSpec,
    command_name: &str,
    exact: bool,
) -> Option<&'static str> {
    matching_name(spec.name, spec.aliases, command_name, exact)
}

fn matching_name(
    name: &'static str,
    aliases: &'static [&'static str],
    input: &str,
    exact: bool,
) -> Option<&'static str> {
    if matches_command_name(name, input, exact) {
        return Some(name);
    }

    aliases
        .iter()
        .copied()
        .find(|alias| matches_command_name(alias, input, exact))
}

fn matches_command_name(candidate: &str, command_name: &str, exact: bool) -> bool {
    if exact {
        candidate == command_name
    } else {
        candidate.starts_with(command_name)
    }
}

pub fn execute(
    input: &str,
    notebook: &gtk::Notebook,
    window: &gtk::ApplicationWindow,
    password_manager: &PasswordManager,
) {
    let input = input.trim_start_matches(':').trim();
    info!("executing command: '{}'", input);

    let (cmd, args) = match input.split_once(' ') {
        Some((c, a)) => (c.trim(), a.trim()),
        None => (input, ""),
    };

    match cmd {
        "open" | "o" => cmd_open(args, notebook),
        "tabopen" | "tabnew" | "to" => cmd_tabopen(args, notebook),
        "close" | "c" => cmd_close(notebook),
        "quit" | "q" => cmd_quit(window),
        "reload" | "r" => cmd_reload(notebook),
        "back" => cmd_back(notebook),
        "forward" => cmd_forward(notebook),
        "group" | "groupnew" => cmd_group(args, notebook),
        "groupadd" => cmd_groupadd(args, notebook),
        "groupcollapse" | "gcollapse" => cmd_groupcollapse(args, notebook),
        "groupexpand" | "gexpand" => cmd_groupexpand(args, notebook),
        "pin" => cmd_pin(args, notebook),
        "bw" | "bitwarden" | "vaultwarden" => cmd_bw(args, notebook, window, password_manager),
        _ => {
            log::warn!("unknown command: '{}'", cmd);
        }
    }
}

fn cmd_open(args: &str, notebook: &gtk::Notebook) {
    if args.is_empty() {
        log::warn!(":open requires a URL argument");
        return;
    }
    let url = normalize_url(args);
    info!(":open navigating to: {}", url);
    if let Some(wv) = tab::current_webview(notebook) {
        wv.load_uri(&url);
    }
}

fn cmd_tabopen(args: &str, notebook: &gtk::Notebook) {
    if args.is_empty() {
        info!(":tabopen opening blank tab");
        tab::add_tab(notebook, "about:blank");
        return;
    }
    let url = normalize_url(args);
    info!(":tabopen opening new tab: {}", url);
    tab::add_tab(notebook, &url);
}

fn cmd_close(notebook: &gtk::Notebook) {
    info!(":close closing current tab");
    tab::close_current_tab(notebook);
}

fn cmd_quit(window: &gtk::ApplicationWindow) {
    info!(":quit closing window");
    window.close();
}

fn cmd_reload(notebook: &gtk::Notebook) {
    info!(":reload reloading current page");
    if let Some(wv) = tab::current_webview(notebook) {
        wv.reload();
    }
}

fn cmd_back(notebook: &gtk::Notebook) {
    info!(":back going back");
    if let Some(wv) = tab::current_webview(notebook) {
        wv.go_back();
    }
}

fn cmd_forward(notebook: &gtk::Notebook) {
    info!(":forward going forward");
    if let Some(wv) = tab::current_webview(notebook) {
        wv.go_forward();
    }
}

fn cmd_group(args: &str, notebook: &gtk::Notebook) {
    if args.is_empty() {
        log::warn!(":group requires a name");
        return;
    }

    info!(":group creating group and adding current tab: {}", args);
    tab::create_group(notebook, args);
}

fn cmd_groupadd(args: &str, notebook: &gtk::Notebook) {
    if args.is_empty() {
        log::warn!(":groupadd requires a name");
        return;
    }

    info!(":groupadd adding current tab to group: {}", args);
    tab::add_current_to_group(notebook, args);
}

fn cmd_groupcollapse(args: &str, notebook: &gtk::Notebook) {
    let name = if args.is_empty() { None } else { Some(args) };
    info!(":groupcollapse collapsing group: {:?}", name);
    tab::collapse_group(notebook, name);
}

fn cmd_groupexpand(args: &str, notebook: &gtk::Notebook) {
    let name = if args.is_empty() { None } else { Some(args) };
    info!(":groupexpand expanding group: {:?}", name);
    tab::expand_group(notebook, name);
}

fn cmd_pin(args: &str, notebook: &gtk::Notebook) {
    match args {
        "" => tab::toggle_current_pin(notebook),
        "on" => tab::set_current_pin(notebook, true),
        "off" => tab::set_current_pin(notebook, false),
        _ => log::warn!(":pin accepts no argument, 'on', or 'off'"),
    }
}

fn cmd_bw(
    args: &str,
    notebook: &gtk::Notebook,
    window: &gtk::ApplicationWindow,
    password_manager: &PasswordManager,
) {
    let (subcommand, rest) = match args.split_once(' ') {
        Some((cmd, rest)) => (cmd.trim(), rest.trim()),
        None => (args.trim(), ""),
    };

    match subcommand {
        "" | "status" => match password_manager.status() {
            Ok(status) => show_info(window, "Bitwarden Status", &format_status(&status)),
            Err(error) => show_error(window, &error),
        },
        "server" => {
            if rest.is_empty() {
                show_info(window, "Bitwarden Server", ":bw server requires a server URL");
                return;
            }
            match password_manager.configure_server(rest) {
                Ok(()) => show_info(window, "Bitwarden Server", "Server URL configured"),
                Err(error) => show_error(window, &error),
            }
        }
        "unlock" => {
            if let Some(password) = prompt_password(window) {
                match password_manager.unlock(&password) {
                    Ok(()) => show_info(window, "Bitwarden Unlock", "Vault unlocked"),
                    Err(error) => show_error(window, &error),
                }
            }
        }
        "lock" => match password_manager.lock() {
            Ok(()) => show_info(window, "Bitwarden Lock", "Vault locked"),
            Err(error) => show_error(window, &error),
        },
        "fill" => {
            let Some(webview) = tab::current_webview(notebook) else {
                show_info(window, "Bitwarden Fill", "No current tab");
                return;
            };
            match password_manager.fill_current_page(&webview, window) {
                Ok(()) => {}
                Err(error) => show_error(window, &error),
            }
        }
        "copy-user" | "copy-username" => {
            let Some(webview) = tab::current_webview(notebook) else {
                show_info(window, "Bitwarden Copy", "No current tab");
                return;
            };
            match password_manager.copy_username(&webview, window) {
                Ok(()) => show_info(window, "Bitwarden Copy", "Username copied"),
                Err(error) => show_error(window, &error),
            }
        }
        "copy-pass" | "copy-password" => {
            let Some(webview) = tab::current_webview(notebook) else {
                show_info(window, "Bitwarden Copy", "No current tab");
                return;
            };
            match password_manager.copy_password(&webview, window) {
                Ok(()) => show_info(window, "Bitwarden Copy", "Password copied"),
                Err(error) => show_error(window, &error),
            }
        }
        _ => show_info(
            window,
            "Bitwarden Commands",
            ":bw status, :bw server URL, :bw unlock, :bw lock, :bw fill, :bw copy-user, :bw copy-pass",
        ),
    }
}

fn format_status(status: &BwStatus) -> String {
    let vault_status = match &status.status {
        VaultStatus::Unauthenticated => "unauthenticated",
        VaultStatus::Locked => "locked",
        VaultStatus::Unlocked => "unlocked",
        VaultStatus::Unknown(value) => value,
    };
    let server = status.server_url.as_deref().unwrap_or("not configured");
    let user = status.user_email.as_deref().unwrap_or("not signed in");

    format!(
        "Status: {}\nServer: {}\nUser: {}",
        vault_status, server, user
    )
}

fn prompt_password(window: &gtk::ApplicationWindow) -> Option<String> {
    let dialog = gtk::Dialog::with_buttons(
        Some("Unlock Bitwarden"),
        Some(window),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Unlock", gtk::ResponseType::Accept),
        ],
    );

    let entry = gtk::Entry::new();
    entry.set_visibility(false);
    entry.set_activates_default(true);
    entry.set_placeholder_text(Some("Master password"));
    entry.set_margin_top(12);
    entry.set_margin_bottom(12);
    entry.set_margin_start(12);
    entry.set_margin_end(12);
    dialog.set_default_response(gtk::ResponseType::Accept);
    dialog.content_area().pack_start(&entry, false, false, 0);
    dialog.show_all();
    entry.grab_focus();

    let response = dialog.run();
    let password = if response == gtk::ResponseType::Accept {
        Some(entry.text().to_string())
    } else {
        None
    };
    entry.set_text("");
    dialog.close();
    password
}

fn show_info(window: &gtk::ApplicationWindow, title: &str, message: &str) {
    show_message(window, gtk::MessageType::Info, title, message);
}

fn show_error(window: &gtk::ApplicationWindow, error: &PasswordError) {
    show_message(
        window,
        gtk::MessageType::Error,
        "Bitwarden Error",
        &error.to_string(),
    );
}

fn show_message(
    window: &gtk::ApplicationWindow,
    message_type: gtk::MessageType,
    title: &str,
    message: &str,
) {
    let dialog = gtk::MessageDialog::new(
        Some(window),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        message_type,
        gtk::ButtonsType::Ok,
        message,
    );
    dialog.set_title(title);
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.show_all();
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
    use super::{command_specs, command_suggestions};

    #[test]
    fn command_suggestions_match_primary_name_prefix() {
        let suggestions = command_suggestions(":gr");
        let names: Vec<&str> = suggestions
            .iter()
            .map(|suggestion| suggestion.name.as_str())
            .collect();

        assert_eq!(
            names,
            vec!["group", "groupadd", "groupcollapse", "groupexpand"]
        );
    }

    #[test]
    fn command_suggestions_match_alias_prefix() {
        let suggestions = command_suggestions(":to");
        let names: Vec<&str> = suggestions
            .iter()
            .map(|suggestion| suggestion.name.as_str())
            .collect();

        assert_eq!(names, vec!["tabopen"]);
        assert_eq!(suggestions[0].completion, ":tabopen");
    }

    #[test]
    fn command_suggestions_with_args_show_exact_command() {
        let suggestions = command_suggestions(":open example.com");
        let names: Vec<&str> = suggestions
            .iter()
            .map(|suggestion| suggestion.name.as_str())
            .collect();

        assert_eq!(names, vec!["open"]);
    }

    #[test]
    fn command_suggestions_list_bitwarden_subcommands() {
        let suggestions = command_suggestions(":bw ");
        let names: Vec<&str> = suggestions
            .iter()
            .map(|suggestion| suggestion.name.as_str())
            .collect();

        assert_eq!(
            names,
            vec![
                "bw status",
                "bw server",
                "bw unlock",
                "bw lock",
                "bw fill",
                "bw copy-user",
                "bw copy-pass",
            ]
        );
    }

    #[test]
    fn command_suggestions_filter_bitwarden_subcommands() {
        let suggestions = command_suggestions(":bw s");
        let names: Vec<&str> = suggestions
            .iter()
            .map(|suggestion| suggestion.name.as_str())
            .collect();

        assert_eq!(names, vec!["bw status", "bw server"]);
        assert_eq!(suggestions[0].completion, ":bw status");
    }

    #[test]
    fn command_suggestions_match_bitwarden_subcommand_alias() {
        let suggestions = command_suggestions(":bw copy-password");

        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].name, "bw copy-pass");
        assert_eq!(suggestions[0].completion, ":bw copy-pass");
    }

    #[test]
    fn command_specs_include_descriptions() {
        assert!(command_specs()
            .iter()
            .all(|spec| !spec.usage.is_empty() && !spec.description.is_empty()));
    }
}
