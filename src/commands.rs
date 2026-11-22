//! The subcommand table: one entry per subcommand feeds the help text, the dispatch and the
//! completions, so an alias added here works and completes everywhere at once.

/// What a subcommand does; `paku::run` matches on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Upgrade,
    Maintain,
    Check,
    Update,
    Search,
    Install,
    Remove,
    Info,
    Owns,
    Files,
    List,
    Aur,
    AurAudit,
    Clean,
    Autoremove,
    Completions,
    Helper,
}

/// What the shells complete after the subcommand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Complete {
    /// Nothing (the subcommand takes no arguments).
    Nothing,
    /// Packages in the sync repos (`pacman -Ssq`).
    Repo,
    /// Installed packages (`pacman -Qq`).
    Installed,
    /// Both: `info` also works on installed AUR packages.
    RepoAndInstalled,
    /// Paths on the filesystem.
    Paths,
    /// The flags `maintain` passes on to cachy-update / arch-update.
    MaintainFlags,
    /// The flags of `upgrade` (`--no-flatpak`).
    UpgradeFlags,
    /// The shells `completions` knows.
    Shells,
}

pub struct Command {
    pub kind: Kind,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    /// The left column of the help: the name and its arguments.
    pub usage: &'static str,
    /// The right column of the help, after the `#`.
    pub help: &'static str,
    /// The line printed (exit 1) when the subcommand needs an argument and got none.
    pub needs_args: Option<&'static str>,
    /// The description the shells show next to the subcommand.
    pub description: &'static str,
    pub complete: Complete,
}

impl Command {
    /// The name, then the aliases.
    pub fn words(&self) -> impl Iterator<Item = &'static str> {
        std::iter::once(self.name).chain(self.aliases.iter().copied())
    }
}

pub const COMMANDS: &[Command] = &[
    Command {
        kind: Kind::Upgrade,
        name: "upgrade",
        aliases: &["up"],
        usage: "upgrade",
        help: "paru -Syu (and flatpak upgrade if available, unless --no-flatpak)",
        needs_args: None,
        description: "Upgrade system (paru -Syu and flatpak upgrade if available)",
        complete: Complete::UpgradeFlags,
    },
    Command {
        kind: Kind::Maintain,
        name: "maintain",
        aliases: &["cu", "full", "full-upgrade"],
        usage: "maintain [args]",
        help: "full Cachy-Update maintenance run (cachy-update)",
        needs_args: None,
        description: "Full Cachy-Update maintenance run (cachy-update)",
        complete: Complete::MaintainFlags,
    },
    Command {
        kind: Kind::Check,
        name: "check",
        aliases: &["co"],
        usage: "check",
        help: "list pending updates without applying them",
        needs_args: None,
        description: "List pending updates without applying them",
        complete: Complete::Nothing,
    },
    Command {
        kind: Kind::Update,
        name: "update",
        aliases: &["refresh"],
        usage: "update",
        help: "paru -Sy (refresh package DB only)",
        needs_args: None,
        description: "Refresh package DB (paru -Sy)",
        complete: Complete::Nothing,
    },
    Command {
        kind: Kind::Search,
        name: "search",
        aliases: &["s"],
        usage: "search <pkg>",
        help: "paru -Ss <pkg>",
        needs_args: Some("paku search <package>"),
        description: "Search packages (paru -Ss)",
        complete: Complete::Repo,
    },
    Command {
        kind: Kind::Install,
        name: "install",
        aliases: &["add", "i"],
        usage: "install <pkg>...",
        help: "paru -S <pkg>",
        needs_args: Some("paku install <package>..."),
        description: "Install packages (paru -S)",
        complete: Complete::Repo,
    },
    Command {
        kind: Kind::Remove,
        name: "remove",
        aliases: &["rm", "uninstall"],
        usage: "remove <pkg>...",
        help: "paru -Rns <pkg>",
        needs_args: Some("paku remove <package>..."),
        description: "Remove packages (paru -Rns)",
        complete: Complete::Installed,
    },
    Command {
        kind: Kind::Info,
        name: "info",
        aliases: &[],
        usage: "info <pkg>",
        help: "paru -Si <pkg>",
        needs_args: Some("paku info <package>"),
        description: "Package info (paru -Si)",
        complete: Complete::RepoAndInstalled,
    },
    Command {
        kind: Kind::Owns,
        name: "owns",
        aliases: &[],
        usage: "owns <file>",
        help: "pacman -Qo <file>",
        needs_args: Some("paku owns <file>..."),
        description: "Find package owning a file (pacman -Qo)",
        complete: Complete::Paths,
    },
    Command {
        kind: Kind::Files,
        name: "files",
        aliases: &[],
        usage: "files <pkg>",
        help: "pacman -Ql <pkg>",
        needs_args: Some("paku files <package>..."),
        description: "List files in a package (pacman -Ql)",
        complete: Complete::Installed,
    },
    Command {
        kind: Kind::List,
        name: "list",
        aliases: &["explicit"],
        usage: "list",
        help: "explicitly installed packages (pacman -Qe)",
        needs_args: None,
        description: "List explicitly installed packages (pacman -Qe)",
        complete: Complete::Nothing,
    },
    Command {
        kind: Kind::Aur,
        name: "aur",
        aliases: &["aur-list"],
        usage: "aur",
        help: "AUR/foreign packages (pacman -Qm)",
        needs_args: None,
        description: "List AUR/foreign packages (pacman -Qm)",
        complete: Complete::Nothing,
    },
    Command {
        kind: Kind::AurAudit,
        name: "aur-audit",
        aliases: &["aur-sec", "aur-security"],
        usage: "aur-audit",
        help: "check installed AUR packages against the compromised list",
        needs_args: None,
        description: "Check installed AUR packages against the compromised list",
        complete: Complete::Nothing,
    },
    Command {
        kind: Kind::Clean,
        name: "clean",
        aliases: &[],
        usage: "clean",
        help: "paru -Sc",
        needs_args: None,
        description: "Clean cache (paru -Sc)",
        complete: Complete::Nothing,
    },
    Command {
        kind: Kind::Autoremove,
        name: "autoremove",
        aliases: &["orphans"],
        usage: "autoremove",
        help: "remove orphaned packages",
        needs_args: None,
        description: "Remove orphaned packages (pacman -Qdtq | paru -Rns)",
        complete: Complete::Nothing,
    },
    Command {
        kind: Kind::Completions,
        name: "completions",
        aliases: &[],
        usage: "completions <shell>",
        help: "print the bash, zsh or fish completions",
        needs_args: Some("paku completions <bash|zsh|fish>"),
        description: "Print the shell completions (bash, zsh or fish)",
        complete: Complete::Shells,
    },
    Command {
        kind: Kind::Helper,
        name: "helper",
        aliases: &[],
        usage: "helper",
        help: "print the tool paku uses: paru, yay, pikaur or pacman",
        needs_args: None,
        description: "Print the package tool paku uses (paru, yay, pikaur or pacman)",
        complete: Complete::Nothing,
    },
];

/// The flags of `paku upgrade`, with their descriptions.
pub const UPGRADE_FLAGS: &[(&str, &str)] = &[("--no-flatpak", "Only the system upgrade, without flatpak upgrade")];

/// What `paku upgrade` prints (exit 1) for an argument it doesn't take.
pub const UPGRADE_USAGE: &str = "paku upgrade [--no-flatpak]";

/// The flags of cachy-update / arch-update that `maintain` completes, with their descriptions.
pub const MAINTAIN_FLAGS: &[(&str, &str)] = &[
    ("--check", "Check for available updates"),
    ("--list", "Display the list of pending updates"),
    ("--devel", "Include AUR development packages updates"),
    ("--news", "Display latest Arch news (optionally pass a count)"),
    ("--services", "Check for services requiring a post upgrade restart"),
    ("--debug", "Display debug traces"),
];

/// The shells `paku completions` prints a script for.
pub const SHELLS: &[&str] = &["bash", "zsh", "fish"];

/// The subcommand a word names (its name or one of its aliases).
pub fn find(word: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.words().any(|w| w == word))
}

/// The help `paku` prints with no arguments.
pub fn help() -> String {
    let mut text = String::from("Usage:\n");
    for c in COMMANDS {
        text.push_str(&format!("  paku {:<20}# {}\n", c.usage, c.help));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_word_is_unique_and_resolves_to_its_command() {
        let mut seen = std::collections::HashSet::new();
        for c in COMMANDS {
            for w in c.words() {
                assert!(seen.insert(w), "{w} is used twice");
                assert_eq!(find(w).map(|f| f.kind), Some(c.kind));
            }
        }
        assert!(find("nope").is_none());
        // The hidden helper isn't a subcommand
        assert!(find("__complete").is_none());
    }

    #[test]
    fn the_help_is_the_one_of_the_fish_function() {
        // The help of the fish function pak (functions/pak.fish), with the new name, the completions and helper
        // lines and the --no-flatpak note
        let fish = "Usage:
  paku upgrade             # paru -Syu (and flatpak upgrade if available, unless --no-flatpak)
  paku maintain [args]     # full Cachy-Update maintenance run (cachy-update)
  paku check               # list pending updates without applying them
  paku update              # paru -Sy (refresh package DB only)
  paku search <pkg>        # paru -Ss <pkg>
  paku install <pkg>...    # paru -S <pkg>
  paku remove <pkg>...     # paru -Rns <pkg>
  paku info <pkg>          # paru -Si <pkg>
  paku owns <file>         # pacman -Qo <file>
  paku files <pkg>         # pacman -Ql <pkg>
  paku list                # explicitly installed packages (pacman -Qe)
  paku aur                 # AUR/foreign packages (pacman -Qm)
  paku aur-audit           # check installed AUR packages against the compromised list
  paku clean               # paru -Sc
  paku autoremove          # remove orphaned packages
  paku completions <shell> # print the bash, zsh or fish completions
  paku helper              # print the tool paku uses: paru, yay, pikaur or pacman
";
        assert_eq!(help(), fish);
    }

    #[test]
    fn the_commands_that_need_an_argument_complete_one() {
        for c in COMMANDS {
            assert_eq!(
                c.needs_args.is_some(),
                !matches!(c.complete, Complete::Nothing | Complete::MaintainFlags | Complete::UpgradeFlags),
                "{}",
                c.name
            );
        }
    }
}
