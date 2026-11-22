//! What each subcommand runs. Everything that touches the system goes through `Sys`, so the tests can
//! check the command lines without running them.
use std::ffi::{OsStr, OsString};

use crate::audit;
use crate::commands::{self, Kind};
use crate::completions;
use crate::helper::{self, Choice};

/// Where the "Arch Linux AUR Compromised" list is published (HedgeDoc's raw markdown download).
pub const AUR_AUDIT_URL: &str = "https://md.archlinux.org/s/SxbqukK6IA/download";

/// The codes of a child killed by SIGINT or SIGQUIT (128 + the signal).
pub const INTERRUPTED: [i32; 2] = [130, 131];

/// The system, as pacu sees it.
pub trait Sys {
    /// Runs a command on the terminal and gives its exit code.
    fn run(&mut self, argv: &[OsString]) -> i32;
    /// Runs a command with stdout captured and stderr discarded; None if it couldn't start.
    fn capture(&mut self, argv: &[OsString]) -> Option<(i32, Vec<u8>)>;
    /// Whether a program is on the PATH.
    fn has(&self, program: &str) -> bool;
    /// Whether pacu runs as root (then pacman needs no sudo).
    fn is_root(&self) -> bool;
    /// The helper the user picked and where (`PACU_HELPER`, then the `helper` of the config file); None
    /// for auto.
    fn helper_setting(&self) -> Option<(String, String)>;
    /// Whether the terminal sent pacu a Ctrl-C or Ctrl-\ (paru, unlike pacman, exits 1 on it, so the
    /// child's code doesn't tell).
    fn interrupted(&self) -> bool;
    fn out(&mut self, text: &str);
    fn err(&mut self, text: &str);
}

/// A fixed command followed by the user's arguments.
fn with_args(words: &[&str], args: &[OsString]) -> Vec<OsString> {
    let mut line = argv(words);
    line.extend(args.iter().cloned());
    line
}

fn argv<I, S>(words: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    words.into_iter().map(|w| w.as_ref().to_os_string()).collect()
}

/// The package tool: an AUR helper (repos and the AUR; see `helper`), otherwise pacman (the official
/// repos only, with sudo where pacman needs root).
struct Backend {
    choice: Choice,
    root: bool,
}

impl Backend {
    /// The tool for this run; a setting that can't be followed is said on stderr.
    fn of(sys: &mut dyn Sys) -> Backend {
        let setting = sys.helper_setting();
        let (choice, warning) = helper::choose(setting.as_ref().map(|(v, s)| (v.as_str(), s.as_str())), |p| sys.has(p));
        if let Some(warning) = warning {
            sys.err(&warning);
        }
        Backend { choice, root: sys.is_root() }
    }

    /// An operation that changes the system: `<helper> <op>`, else `sudo pacman <op>` (`pacman` as root).
    fn writes(&self, op: &str, args: &[OsString]) -> Vec<OsString> {
        let tool: &[&str] = match self.choice.helper {
            Some(helper) => &[helper, op],
            None if self.root => &["pacman", op],
            None => &["sudo", "pacman", op],
        };
        with_args(tool, args)
    }

    /// A query: `<helper> <op>`, else `pacman <op>`.
    fn reads(&self, op: &str, args: &[OsString]) -> Vec<OsString> {
        with_args(&[self.choice.helper.unwrap_or("pacman"), op], args)
    }
}

/// Whether the last command was stopped by Ctrl-C or Ctrl-\: by its code, or by what pacu got.
fn stopped(sys: &dyn Sys, code: i32) -> bool {
    INTERRUPTED.contains(&code) || sys.interrupted()
}

/// Without a helper, a search, install or info that found nothing may be an AUR package (not one stopped).
fn note_no_aur(sys: &mut dyn Sys, backend: &Backend, code: i32) {
    if backend.choice.helper.is_none() && code != 0 && !stopped(sys, code) {
        sys.err(if backend.choice.pacman_asked {
            "pacu: the helper is set to pacman, so only the official repos were used (no AUR)"
        } else {
            "pacu: no AUR helper (paru, yay or pikaur) is installed, so only the official repos were used (no AUR)"
        });
    }
}

/// Lines of a command's output, without the empty ones.
fn lines(data: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(data).lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
}

/// pacu with its arguments (without the program name); gives the exit code.
pub fn run(args: &[OsString], sys: &mut dyn Sys) -> i32 {
    let Some(first) = args.first() else {
        sys.out(&commands::help());
        return 1;
    };
    let rest = &args[1..];
    let word = first.to_string_lossy();
    match word.as_ref() {
        "-h" | "--help" | "help" => {
            sys.out(&commands::help());
            return 0;
        }
        "-V" | "--version" => {
            sys.out(&format!("pacu {}\n", env!("CARGO_PKG_VERSION")));
            return 0;
        }
        "__complete" => return complete(rest, sys),
        _ => {}
    }
    let Some(command) = commands::find(&word) else {
        sys.out(&format!("Unknown command: {word}\nRun: pacu (no args) for help\n"));
        return 1;
    };
    if let Some(usage) = command.needs_args {
        if rest.is_empty() {
            sys.out(&format!("{usage}\n"));
            return 1;
        }
    }
    // Only the subcommands that run the package tool look for it: a setting that can't be followed doesn't
    // warn on `pacu completions` (run by the shell at every start) or `pacu owns`
    let backend = if matches!(
        command.kind,
        Kind::Maintain | Kind::Owns | Kind::Files | Kind::List | Kind::Aur | Kind::AurAudit | Kind::Completions
    ) {
        Backend { choice: Choice { helper: None, pacman_asked: false }, root: false }
    } else {
        Backend::of(sys)
    };
    match command.kind {
        Kind::Upgrade => {
            // flatpak only after the system upgrade went through. A failure includes a Ctrl-C, which pacu
            // often can't see: sudo (use_pty, the default since 1.9.14) passes it to pacman in its own pty, and
            // paru then exits 1
            let code = sys.run(&backend.writes("-Syu", &[]));
            if code != 0 || !sys.has("flatpak") {
                return code;
            }
            sys.run(&argv(["flatpak", "upgrade"]))
        }
        Kind::Maintain => {
            for tool in ["cachy-update", "arch-update"] {
                if sys.has(tool) {
                    return sys.run(&with_args(&[tool], rest));
                }
            }
            sys.out("Cachy-Update is not installed. Install it with:\n  pacu install cachy-update\n");
            1
        }
        Kind::Check => {
            for tool in ["cachy-update", "arch-update"] {
                if sys.has(tool) {
                    return sys.run(&argv([tool, "--list"]));
                }
            }
            if let Some(helper) = backend.choice.helper {
                sys.run(&argv([helper, "-Qu"]))
            } else if sys.has("checkupdates") {
                // pacman-contrib: checks against a fresh copy of the databases, without root
                sys.run(&argv(["checkupdates"]))
            } else {
                sys.run(&argv(["pacman", "-Qu"]))
            }
        }
        Kind::Update => sys.run(&backend.writes("-Sy", &[])),
        Kind::Search => {
            let code = sys.run(&backend.reads("-Ss", rest));
            note_no_aur(sys, &backend, code);
            code
        }
        Kind::Install => {
            let code = sys.run(&backend.writes("-S", rest));
            note_no_aur(sys, &backend, code);
            code
        }
        Kind::Remove => sys.run(&backend.writes("-Rns", rest)),
        Kind::Info => {
            let mut code = sys.run(&backend.reads("-Si", rest));
            if code != 0 && backend.choice.helper.is_none() && !stopped(sys, code) {
                // Not in the repos: maybe installed from elsewhere
                code = sys.run(&with_args(&["pacman", "-Qi"], rest));
            }
            note_no_aur(sys, &backend, code);
            code
        }
        Kind::Owns => sys.run(&with_args(&["pacman", "-Qo"], rest)),
        Kind::Files => sys.run(&with_args(&["pacman", "-Ql"], rest)),
        Kind::List => sys.run(&argv(["pacman", "-Qe"])),
        Kind::Aur => sys.run(&argv(["pacman", "-Qm"])),
        Kind::AurAudit => aur_audit(sys),
        Kind::Clean => sys.run(&backend.writes("-Sc", &[])),
        Kind::Autoremove => {
            let orphans = match sys.capture(&argv(["pacman", "-Qdtq"])) {
                Some((_, data)) => lines(&data),
                None => return 1,
            };
            if orphans.is_empty() {
                sys.out("No orphaned packages found.\n");
                return 0;
            }
            let orphans: Vec<OsString> = orphans.into_iter().map(OsString::from).collect();
            sys.run(&backend.writes("-Rns", &orphans))
        }
        Kind::Completions => match completions::script(&rest[0].to_string_lossy()) {
            Some(script) => {
                sys.out(&script);
                0
            }
            None => {
                sys.out(&format!("{}\n", command.needs_args.unwrap_or_default()));
                1
            }
        },
    }
}

/// `pacu __complete repo|installed [PREFIX]`: the package names the completions offer, only the ones
/// starting with PREFIX when given (bash then takes them as they are, with no expansion).
fn complete(args: &[OsString], sys: &mut dyn Sys) -> i32 {
    let op = match args.first().map(|a| a.to_string_lossy()) {
        Some(ref w) if w == "repo" => "-Ssq",
        Some(ref w) if w == "installed" => "-Qq",
        _ => return 1,
    };
    let prefix = args.get(1).map(|a| a.to_string_lossy().into_owned()).unwrap_or_default();
    match sys.capture(&argv(["pacman", op])) {
        Some((code, data)) => {
            let text = String::from_utf8_lossy(&data);
            let mut names = String::new();
            for name in text.lines().filter(|l| l.starts_with(&prefix)) {
                names.push_str(name);
                names.push('\n');
            }
            sys.out(&names);
            code
        }
        None => 1,
    }
}

/// Checks the installed AUR (foreign) packages against the published compromised list.
fn aur_audit(sys: &mut dyn Sys) -> i32 {
    if !sys.has("curl") {
        sys.out("curl is required to fetch the compromised package list. Install it with:\n  pacu install curl\n");
        return 1;
    }
    let list = sys
        .capture(&argv([
            "curl",
            "-fsSL",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            "20",
            AUR_AUDIT_URL,
        ]))
        .filter(|(code, _)| *code == 0)
        .map(|(_, data)| audit::parse_list(&String::from_utf8_lossy(&data)))
        .unwrap_or_default();
    // Ctrl-C during the download isn't a network problem
    if sys.interrupted() {
        sys.err("pacu: aur-audit interrupted");
        return 130;
    }
    if list.is_empty() {
        sys.out(&format!(
            "Could not fetch the compromised package list from:\n  {AUR_AUDIT_URL}\nCheck your network connection and try again.\n"
        ));
        return 1;
    }
    let installed = sys.capture(&argv(["pacman", "-Qmq"])).map(|(_, data)| lines(&data)).unwrap_or_default();
    if installed.is_empty() {
        sys.out("No AUR (foreign) packages installed — nothing to check.\n");
        return 0;
    }
    let hits = audit::hits(&installed, &list);
    sys.out(&format!(
        "Checked {} installed AUR package(s) against {} known-compromised entries.\n",
        installed.len(),
        list.len()
    ));
    if hits.is_empty() {
        sys.out("No installed AUR packages are on the compromised list.\n");
        return 0;
    }
    let mut text = String::from("WARNING: the following installed AUR packages are on the compromised list:\n");
    for pkg in &hits {
        text.push_str(&format!("  {pkg}\n"));
    }
    text.push_str("Review these packages and consider removing them with: pacu remove <package>\n");
    sys.out(&text);
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A system that records the command lines and answers from a script.
    #[derive(Default)]
    struct Fake {
        programs: Vec<&'static str>,
        root: bool,
        /// PACU_HELPER or the config's helper, with where it came from.
        setting: Option<(&'static str, &'static str)>,
        /// Exit codes by command line ("sudo pacman -S foo"); 0 when missing.
        codes: HashMap<String, i32>,
        /// Captured outputs by command line; a missing one can't start.
        outputs: HashMap<String, (i32, String)>,
        ran: Vec<String>,
        /// A command line during which the user presses Ctrl-C.
        interrupt_on: Option<&'static str>,
        interrupted: bool,
        out: String,
        err: String,
    }

    fn line(argv: &[OsString]) -> String {
        argv.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ")
    }

    impl Sys for Fake {
        fn run(&mut self, argv: &[OsString]) -> i32 {
            let l = line(argv);
            self.ran.push(l.clone());
            if self.interrupt_on == Some(l.as_str()) {
                self.interrupted = true;
            }
            self.codes.get(&l).copied().unwrap_or(0)
        }
        fn capture(&mut self, argv: &[OsString]) -> Option<(i32, Vec<u8>)> {
            let l = line(argv);
            self.ran.push(l.clone());
            self.outputs.get(&l).map(|(c, o)| (*c, o.clone().into_bytes()))
        }
        fn has(&self, program: &str) -> bool {
            self.programs.contains(&program)
        }
        fn is_root(&self) -> bool {
            self.root
        }
        fn helper_setting(&self) -> Option<(String, String)> {
            self.setting.map(|(v, s)| (v.to_string(), s.to_string()))
        }
        fn interrupted(&self) -> bool {
            self.interrupted
        }
        fn out(&mut self, text: &str) {
            self.out.push_str(text);
        }
        fn err(&mut self, text: &str) {
            self.err.push_str(text);
            self.err.push('\n');
        }
    }

    fn pacu(fake: &mut Fake, args: &[&str]) -> i32 {
        run(&argv(args), fake)
    }

    fn with(programs: &[&'static str]) -> Fake {
        Fake { programs: programs.to_vec(), ..Fake::default() }
    }

    #[test]
    fn with_a_helper_everything_goes_through_it() {
        let cases: &[(&[&str], &str)] = &[
            (&["upgrade"], "H -Syu"),
            (&["update"], "H -Sy"),
            (&["search", "fire", "fox"], "H -Ss fire fox"),
            (&["i", "htop", "--needed"], "H -S htop --needed"),
            (&["rm", "htop"], "H -Rns htop"),
            (&["info", "htop"], "H -Si htop"),
            (&["clean"], "H -Sc"),
            (&["owns", "/usr/bin/pacman"], "pacman -Qo /usr/bin/pacman"),
            (&["files", "pacman"], "pacman -Ql pacman"),
            (&["list"], "pacman -Qe"),
            (&["aur"], "pacman -Qm"),
            (&["check"], "H -Qu"),
        ];
        for helper in helper::HELPERS {
            for (args, expected) in cases {
                let mut f = with(&[helper]);
                assert_eq!(pacu(&mut f, args), 0, "{helper} {args:?}");
                assert_eq!(f.ran, [expected.replace("H ", &format!("{helper} "))], "{helper} {args:?}");
                assert_eq!(f.err, "", "{helper} {args:?}");
            }
        }
    }

    #[test]
    fn the_helper_order_and_the_setting() {
        // auto: paru, then yay, then pikaur
        let mut f = with(&["pikaur", "yay"]);
        pacu(&mut f, &["install", "htop"]);
        assert_eq!(f.ran, ["yay -S htop"]);
        // The setting wins over the order
        let mut f = Fake { setting: Some(("pikaur", "PACU_HELPER")), ..with(&["paru", "pikaur"]) };
        pacu(&mut f, &["install", "htop"]);
        assert_eq!((f.ran.as_slice(), f.err.as_str()), (&["pikaur -S htop".to_string()][..], ""));
        // pacman even with a helper installed; a failure then says why there was no AUR
        let mut f = Fake { setting: Some(("pacman", "~/.config/pacu/config")), ..with(&["paru"]) };
        f.codes.insert("sudo pacman -S yay".into(), 1);
        assert_eq!(pacu(&mut f, &["install", "yay"]), 1);
        assert_eq!(f.ran, ["sudo pacman -S yay"]);
        assert!(f.err.contains("the helper is set to pacman"), "{}", f.err);
        // A chosen helper that isn't installed: a warning, then auto
        let mut f = Fake { setting: Some(("yay", "PACU_HELPER")), ..with(&["paru"]) };
        pacu(&mut f, &["search", "htop"]);
        assert_eq!(f.ran, ["paru -Ss htop"]);
        assert_eq!(f.err, "pacu: yay (set in PACU_HELPER) isn't installed; using paru\n");
        // The subcommands that don't run the package tool don't look for it (and don't warn)
        let mut f = Fake { setting: Some(("aura", "PACU_HELPER")), ..with(&[]) };
        for args in [&["--help"][..], &["--version"], &["completions", "bash"], &["owns", "/x"], &["list"], &["aur"]] {
            pacu(&mut f, args);
        }
        assert_eq!(f.err, "");
        pacu(&mut f, &["check"]);
        assert!(f.err.starts_with("pacu: unknown helper \"aura\""), "{}", f.err);
    }

    #[test]
    fn without_a_helper_pacman_writes_through_sudo_and_reads_without() {
        let cases: &[(&[&str], &str)] = &[
            (&["up"], "sudo pacman -Syu"),
            (&["refresh"], "sudo pacman -Sy"),
            (&["s", "firefox"], "pacman -Ss firefox"),
            (&["add", "htop"], "sudo pacman -S htop"),
            (&["uninstall", "htop"], "sudo pacman -Rns htop"),
            (&["info", "htop"], "pacman -Si htop"),
            (&["clean"], "sudo pacman -Sc"),
            (&["check"], "pacman -Qu"),
        ];
        for (args, expected) in cases {
            let mut f = with(&[]);
            assert_eq!(pacu(&mut f, args), 0, "{args:?}");
            assert_eq!(f.ran, [*expected], "{args:?}");
        }
        // As root, no sudo
        let mut f = Fake { root: true, ..Fake::default() };
        pacu(&mut f, &["install", "htop"]);
        assert_eq!(f.ran, ["pacman -S htop"]);
        // pacman-contrib's checkupdates, when there
        let mut f = with(&["checkupdates"]);
        pacu(&mut f, &["co"]);
        assert_eq!(f.ran, ["checkupdates"]);
    }

    #[test]
    fn without_a_helper_a_package_not_found_mentions_the_aur() {
        let mut f = with(&[]);
        f.codes.insert("sudo pacman -S yay".into(), 1);
        assert_eq!(pacu(&mut f, &["install", "yay"]), 1);
        assert!(f.err.contains("no AUR helper (paru, yay or pikaur) is installed"), "{}", f.err);
        // Found: no note
        let mut f = with(&[]);
        pacu(&mut f, &["search", "htop"]);
        assert_eq!(f.err, "");
        // info: the repos, then the installed packages
        let mut f = with(&[]);
        f.codes.insert("pacman -Si yay".into(), 1);
        assert_eq!(pacu(&mut f, &["info", "yay"]), 0);
        assert_eq!(f.ran, ["pacman -Si yay", "pacman -Qi yay"]);
        assert_eq!(f.err, "");
        // Ctrl-C in the first: no second, and no AUR note
        let mut h = with(&[]);
        h.codes.insert("sudo pacman -S yay".into(), 130);
        assert_eq!(pacu(&mut h, &["install", "yay"]), 130);
        assert_eq!(h.err, "");
        let mut g = with(&[]);
        g.codes.insert("pacman -Si yay".into(), 130);
        assert_eq!(pacu(&mut g, &["info", "yay"]), 130);
        assert_eq!(g.ran, ["pacman -Si yay"]);
        f.codes.insert("pacman -Qi yay".into(), 1);
        f.ran.clear();
        assert_eq!(pacu(&mut f, &["info", "yay"]), 1);
        assert!(f.err.contains("no AUR"));
        // With paru the failure is just paru's
        let mut f = with(&["paru"]);
        f.codes.insert("paru -S nope".into(), 1);
        assert_eq!(pacu(&mut f, &["install", "nope"]), 1);
        assert_eq!(f.err, "");
    }

    #[test]
    fn exit_codes_and_messages_like_the_fish_function() {
        let mut f = with(&["paru"]);
        assert_eq!(pacu(&mut f, &[]), 1);
        assert!(f.out.starts_with("Usage:\n  pacu upgrade"));
        let mut f = with(&["paru"]);
        assert_eq!(pacu(&mut f, &["frobnicate"]), 1);
        assert_eq!(f.out, "Unknown command: frobnicate\nRun: pacu (no args) for help\n");
        for (cmd, usage) in [
            ("search", "pacu search <package>"),
            ("i", "pacu install <package>..."),
            ("rm", "pacu remove <package>..."),
            ("info", "pacu info <package>"),
            ("owns", "pacu owns <file>..."),
            ("files", "pacu files <package>..."),
        ] {
            let mut f = with(&["paru"]);
            assert_eq!(pacu(&mut f, &[cmd]), 1);
            assert_eq!(f.out, format!("{usage}\n"));
            assert!(f.ran.is_empty());
        }
        // The child's code is pacu's
        let mut f = with(&["paru"]);
        f.codes.insert("paru -Rns x".into(), 1);
        assert_eq!(pacu(&mut f, &["remove", "x"]), 1);
        let mut f = with(&["paru"]);
        assert_eq!(pacu(&mut f, &["--help"]), 0);
        assert_eq!(pacu(&mut f, &["--version"]), 0);
        assert!(f.out.ends_with(&format!("pacu {}\n", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn upgrade_also_upgrades_flatpak_only_after_the_system() {
        let mut f = with(&["paru", "flatpak"]);
        assert_eq!(pacu(&mut f, &["upgrade"]), 0);
        assert_eq!(f.ran, ["paru -Syu", "flatpak upgrade"]);
        // paru failed, or exited 1 on a Ctrl-C pacu didn't get (sudo's pty): no flatpak
        let mut f = with(&["paru", "flatpak"]);
        f.codes.insert("paru -Syu".into(), 1);
        assert_eq!(pacu(&mut f, &["upgrade"]), 1);
        assert_eq!(f.ran, ["paru -Syu"]);
        let mut f = with(&["paru", "flatpak"]);
        f.codes.insert("paru -Syu".into(), 130);
        assert_eq!(pacu(&mut f, &["upgrade"]), 130);
        assert_eq!(f.ran, ["paru -Syu"]);
        // paru exits 1 on Ctrl-C: what pacu got says it was stopped
        let mut f = with(&["paru", "flatpak"]);
        f.codes.insert("paru -Syu".into(), 1);
        f.interrupt_on = Some("paru -Syu");
        assert_eq!(pacu(&mut f, &["upgrade"]), 1);
        assert_eq!(f.ran, ["paru -Syu"]);
        let mut f = with(&["paru", "flatpak"]);
        f.codes.insert("flatpak upgrade".into(), 3);
        assert_eq!(pacu(&mut f, &["upgrade"]), 3);
    }

    #[test]
    fn maintain_and_check_prefer_cachy_update_then_arch_update() {
        let mut f = with(&["paru", "cachy-update", "arch-update"]);
        pacu(&mut f, &["full", "--news", "10"]);
        pacu(&mut f, &["check"]);
        assert_eq!(f.ran, ["cachy-update --news 10", "cachy-update --list"]);
        let mut f = with(&["arch-update"]);
        pacu(&mut f, &["maintain"]);
        pacu(&mut f, &["check"]);
        assert_eq!(f.ran, ["arch-update", "arch-update --list"]);
        let mut f = with(&["paru"]);
        assert_eq!(pacu(&mut f, &["cu"]), 1);
        assert_eq!(f.out, "Cachy-Update is not installed. Install it with:\n  pacu install cachy-update\n");
        assert!(f.ran.is_empty());
    }

    #[test]
    fn autoremove_removes_the_orphans_or_says_there_are_none() {
        let mut f = with(&[]);
        f.outputs.insert("pacman -Qdtq".into(), (0, "a\nb\n".into()));
        assert_eq!(pacu(&mut f, &["orphans"]), 0);
        assert_eq!(f.ran, ["pacman -Qdtq", "sudo pacman -Rns a b"]);
        // pacman -Qdtq exits 1 with no orphans
        let mut f = with(&["paru"]);
        f.outputs.insert("pacman -Qdtq".into(), (1, String::new()));
        assert_eq!(pacu(&mut f, &["autoremove"]), 0);
        assert_eq!(f.out, "No orphaned packages found.\n");
        assert_eq!(f.ran, ["pacman -Qdtq"]);
    }

    const CURL: &str =
        "curl -fsSL --proto =https --proto-redir =https --max-time 20 https://md.archlinux.org/s/SxbqukK6IA/download";

    #[test]
    fn aur_audit_codes() {
        // No curl
        let mut f = with(&[]);
        assert_eq!(pacu(&mut f, &["aur-audit"]), 1);
        assert!(f.out.starts_with("curl is required"));
        // No network
        let mut f = with(&["curl"]);
        f.outputs.insert(CURL.into(), (6, String::new()));
        assert_eq!(pacu(&mut f, &["aur-sec"]), 1);
        assert!(f.out.starts_with("Could not fetch the compromised package list from:\n  https://md.archlinux.org/"));
        // Ctrl-C during the download
        let mut f = with(&["curl"]);
        f.outputs.insert(CURL.into(), (6, String::new()));
        f.interrupted = true;
        assert_eq!(pacu(&mut f, &["aur-audit"]), 130);
        assert_eq!((f.out.as_str(), f.err.as_str()), ("", "pacu: aur-audit interrupted\n"));
        // Nothing from the AUR
        let mut f = with(&["curl"]);
        f.outputs.insert(CURL.into(), (0, "```\nbad-bin\n```\n".into()));
        f.outputs.insert("pacman -Qmq".into(), (1, String::new()));
        assert_eq!(pacu(&mut f, &["aur-audit"]), 0);
        assert_eq!(f.out, "No AUR (foreign) packages installed — nothing to check.\n");
        // Clean
        let mut f = with(&["curl"]);
        f.outputs.insert(CURL.into(), (0, "```\nbad-bin\nworse-git\n```\n".into()));
        f.outputs.insert("pacman -Qmq".into(), (0, "good\n".into()));
        assert_eq!(pacu(&mut f, &["aur-audit"]), 0);
        assert_eq!(
            f.out,
            "Checked 1 installed AUR package(s) against 2 known-compromised entries.\nNo installed AUR packages are on the compromised list.\n"
        );
        // A hit
        let mut f = with(&["curl"]);
        f.outputs.insert(CURL.into(), (0, "```\nbad-bin\nworse-git\n```\n".into()));
        f.outputs.insert("pacman -Qmq".into(), (0, "good\nworse-git\n".into()));
        assert_eq!(pacu(&mut f, &["aur-security"]), 1);
        assert!(f.out.contains(
            "WARNING: the following installed AUR packages are on the compromised list:\n  worse-git\nReview"
        ));
    }

    #[test]
    fn the_hidden_completer_lists_packages() {
        let mut f = with(&[]);
        f.outputs.insert("pacman -Ssq".into(), (0, "a\nb\n".into()));
        f.outputs.insert("pacman -Qq".into(), (0, "c\n".into()));
        assert_eq!(pacu(&mut f, &["__complete", "repo"]), 0);
        assert_eq!(pacu(&mut f, &["__complete", "installed"]), 0);
        assert_eq!(f.out, "a\nb\nc\n");
        assert_eq!(pacu(&mut f, &["__complete", "other"]), 1);
        assert_eq!(pacu(&mut f, &["__complete"]), 1);
    }

    #[test]
    fn completions_print_a_script_per_shell() {
        for shell in commands::SHELLS {
            let mut f = with(&[]);
            assert_eq!(pacu(&mut f, &["completions", shell]), 0);
            assert!(f.out.contains("aur-audit"), "{shell}");
        }
        let mut f = with(&[]);
        assert_eq!(pacu(&mut f, &["completions", "tcsh"]), 1);
        assert_eq!(f.out, "pacu completions <bash|zsh|fish>\n");
    }
}
