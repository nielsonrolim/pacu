//! What each subcommand runs. Everything that touches the system goes through `Sys`, so the tests can
//! check the command lines without running them.
use std::ffi::{OsStr, OsString};

use crate::audit;
use crate::commands::{self, Kind};
use crate::completions;
use crate::helper::{self, Choice};
use crate::maintain;

/// Where the "Arch Linux AUR Compromised" list is published (HedgeDoc's raw markdown download).
pub const AUR_AUDIT_URL: &str = "https://md.archlinux.org/s/SxbqukK6IA/download";

/// The codes of a child killed by SIGINT or SIGQUIT (128 + the signal).
pub const INTERRUPTED: [i32; 2] = [130, 131];

/// The system, as paku sees it.
pub trait Sys {
    /// Runs a command on the terminal and gives its exit code.
    fn run(&mut self, argv: &[OsString]) -> i32;
    /// Runs a command with stdout captured and stderr discarded; None if it couldn't start.
    fn capture(&mut self, argv: &[OsString]) -> Option<(i32, Vec<u8>)>;
    /// Whether a program is on the PATH.
    fn has(&self, program: &str) -> bool;
    /// Whether paku runs as root (then pacman needs no sudo).
    fn is_root(&self) -> bool;
    /// The helper the user picked and where (`PAKU_HELPER`, then the `helper` of the config file); None
    /// for auto.
    fn helper_setting(&self) -> Option<(String, String)>;
    /// Whether the terminal sent paku a Ctrl-C or Ctrl-\ (paru, unlike pacman, exits 1 on it, so the
    /// child's code doesn't tell).
    fn interrupted(&self) -> bool;
    fn out(&mut self, text: &str);
    fn err(&mut self, text: &str);
    /// Whether stdin and stdout are a terminal (`maintain` only asks and changes the system there).
    fn tty(&self) -> bool;
    /// Asks a yes/no question on the terminal; `default` is the answer to a plain Enter. A Ctrl-C there stops
    /// paku.
    fn ask(&mut self, question: &str, default: bool) -> bool;
    /// Takes `maintain`'s lock for the rest of the run; false when another run holds it.
    fn lock(&mut self) -> bool;
    /// The id of the newest Arch news item `maintain` showed last, and saving it.
    fn last_news(&self) -> Option<String>;
    fn save_last_news(&mut self, id: &str);
    /// Whether a path exists.
    fn exists(&self, path: &str) -> bool;
}

/// A fixed command followed by the user's arguments.
pub(crate) fn with_args(words: &[&str], args: &[OsString]) -> Vec<OsString> {
    let mut line = argv(words);
    line.extend(args.iter().cloned());
    line
}

pub(crate) fn argv<I, S>(words: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    words.into_iter().map(|w| w.as_ref().to_os_string()).collect()
}

/// The package tool: an AUR helper (repos and the AUR; see `helper`), otherwise pacman (the official
/// repos only, with sudo where pacman needs root).
pub(crate) struct Backend {
    pub(crate) choice: Choice,
    pub(crate) root: bool,
}

impl Backend {
    /// The tool for this run; a setting that can't be followed is said on stderr.
    pub(crate) fn of(sys: &mut dyn Sys) -> Backend {
        let setting = sys.helper_setting();
        let (choice, warning) = helper::choose(setting.as_ref().map(|(v, s)| (v.as_str(), s.as_str())), |p| sys.has(p));
        if let Some(warning) = warning {
            sys.err(&warning);
        }
        Backend { choice, root: sys.is_root() }
    }

    /// An operation that changes the system: `<helper> <op>`, else `sudo pacman <op>` (`pacman` as root).
    pub(crate) fn writes(&self, op: &str, args: &[OsString]) -> Vec<OsString> {
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

/// Whether the last command was stopped by Ctrl-C or Ctrl-\: by its code, or by what paku got.
pub(crate) fn stopped(sys: &dyn Sys, code: i32) -> bool {
    INTERRUPTED.contains(&code) || sys.interrupted()
}

/// Without a helper, a search, install or info that found nothing may be an AUR package (not one stopped).
fn note_no_aur(sys: &mut dyn Sys, backend: &Backend, code: i32) {
    if backend.choice.helper.is_none() && code != 0 && !stopped(sys, code) {
        sys.err(if backend.choice.pacman_asked {
            "paku: the helper is set to pacman, so only the official repos were used (no AUR)"
        } else {
            "paku: no AUR helper (paru, yay or pikaur) is installed, so only the official repos were used (no AUR)"
        });
    }
}

/// Lines of a command's output, without the empty ones.
pub(crate) fn lines(data: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(data).lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
}

/// paku with its arguments (without the program name); gives the exit code.
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
            sys.out(&format!("paku {}\n", env!("CARGO_PKG_VERSION")));
            return 0;
        }
        "__complete" => return complete(rest, sys),
        _ => {}
    }
    let Some(command) = commands::find(&word) else {
        sys.out(&format!("Unknown command: {word}\nRun: paku (no args) for help\n"));
        return 1;
    };
    if let Some(usage) = command.needs_args {
        if rest.is_empty() {
            sys.out(&format!("{usage}\n"));
            return 1;
        }
    }
    // Only the subcommands that run the package tool look for it: a setting that can't be followed doesn't
    // warn on `paku completions` (run by the shell at every start) or `paku owns`
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
            let flatpak = match rest.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().as_slice() {
                [] => true,
                [only] if only == "--no-flatpak" => false,
                _ => {
                    sys.out(&format!("{}\n", commands::UPGRADE_USAGE));
                    return 1;
                }
            };
            // flatpak only after the system upgrade went through. A failure includes a Ctrl-C, which paku
            // often can't see: sudo (use_pty, the default since 1.9.14) passes it to pacman in its own pty, and
            // paru then exits 1
            let code = sys.run(&backend.writes("-Syu", &[]));
            if code != 0 || !flatpak || !sys.has("flatpak") {
                return code;
            }
            sys.run(&argv(["flatpak", "upgrade"]))
        }
        Kind::Maintain => {
            // cachy-update when installed (with the arguments as given), otherwise paku's own flow
            if sys.has("cachy-update") {
                return sys.run(&with_args(&["cachy-update"], rest));
            }
            let backend = Backend::of(sys);
            maintain::run(rest, sys, &backend)
        }
        Kind::Check => {
            if sys.has("cachy-update") {
                return sys.run(&argv(["cachy-update", "--list"]));
            }
            if let Some(helper) = backend.choice.helper {
                sys.run(&argv([helper, "-Qu"]))
            } else if sys.has("checkupdates") && sys.has("fakeroot") {
                // pacman-contrib: checks against a fresh copy of the databases, without root (fakeroot is only
                // an optional dependency of pacman-contrib, and checkupdates fails without it)
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
        Kind::Clean => {
            match clean_download_dirs(sys, backend.root) {
                Cleaned::Stopped => return 130,
                Cleaned::Removed(0) | Cleaned::Nothing => {}
                Cleaned::Removed(n) => sys.out(&format!("{}\n", removed_line(n))),
                Cleaned::Failed => sys.err(DOWNLOAD_DIRS_FAILED),
            }
            sys.run(&backend.writes("-Sc", &[]))
        }
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
        Kind::Helper => {
            sys.out(&format!("{}\n", backend.choice.helper.unwrap_or("pacman")));
            0
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

/// A pacman-conf value (one per line, like CacheDir), or `default` without pacman-conf or an answer.
fn pacman_conf(sys: &mut dyn Sys, key: &str, default: &str) -> Vec<String> {
    let found = if sys.has("pacman-conf") {
        match sys.capture(&argv(["pacman-conf", key])) {
            Some((0, data)) => lines(&data),
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };
    if found.is_empty() {
        vec![default.to_string()]
    } else {
        found
    }
}

/// pacman 7 downloads as the `alpm` user (DownloadUser) into a `download-XXXXXX` dir it makes in the cache dir,
/// and a `pacman -U` of a local file (how paru and yay install an AUR package) leaves that dir behind, empty and
/// readable only by alpm; `pacman -Sc` then stops on it ("could not open file …/download-XXXXXX: Error reading
/// fd 8"). The `download-*` dirs right in each cache dir, by name (as the user, no sudo: the dirs can't be read,
/// but the cache dir can). None while pacman holds its lock: a download may be running in one of them.
pub(crate) fn download_dirs(sys: &mut dyn Sys) -> Vec<(String, usize)> {
    if pacman_busy(sys) {
        return Vec::new();
    }
    let mut found = Vec::new();
    for dir in pacman_conf(sys, "CacheDir", "/var/cache/pacman/pkg/") {
        if !dir.starts_with('/') {
            continue;
        }
        let line = argv(["find", &dir, "-mindepth", "1", "-maxdepth", "1", "-type", "d", "-name", "download-*"]);
        let count = match sys.capture(&line) {
            Some((_, data)) => lines(&data).iter().filter(|l| is_download_dir(l, &dir)).count(),
            None => 0,
        };
        if count > 0 {
            found.push((dir, count));
        }
    }
    found
}

/// Whether pacman holds its lock (`<DBPath>/db.lck`): a download may be under way.
fn pacman_busy(sys: &mut dyn Sys) -> bool {
    let db = pacman_conf(sys, "DBPath", "/var/lib/pacman/").remove(0);
    sys.exists(&format!("{}/db.lck", db.trim_end_matches('/')))
}

/// A `download-*` entry right in `dir`, as find prints it.
fn is_download_dir(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir.trim_end_matches('/'))
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|name| name.starts_with("download-") && !name.contains('/'))
}

pub(crate) enum Cleaned {
    Nothing,
    Removed(usize),
    /// sudo refused (or find failed): the dirs stay.
    Failed,
    /// Ctrl-C at sudo's password prompt.
    Stopped,
}

pub(crate) const DOWNLOAD_DIRS_FAILED: &str =
    "paku: couldn't remove the empty download dirs pacman left in the cache (pacman -Sc may stop on them)";

pub(crate) fn removed_line(n: usize) -> String {
    format!("Removed {n} empty download dir(s) pacman left in the package cache.")
}

/// Removes the empty `download-*` dirs of `download_dirs`, as root (find -empty -delete: a dir with a file in it,
/// a download under way, is kept). sudo -v first, on the terminal, so the sudo -n of the captured find can't hide
/// a password prompt.
pub(crate) fn clean_download_dirs(sys: &mut dyn Sys, root: bool) -> Cleaned {
    let found = download_dirs(sys);
    if found.is_empty() {
        return Cleaned::Nothing;
    }
    if !root {
        let code = sys.run(&argv(["sudo", "-v"]));
        if stopped(sys, code) {
            return Cleaned::Stopped;
        }
    }
    let mut removed = 0;
    let mut failed = false;
    for (dir, _) in found {
        // Again right before: a pacman may have started while sudo waited for the password, and its new
        // download dir is still empty
        if pacman_busy(sys) {
            break;
        }
        let mut line = if root { Vec::new() } else { argv(["sudo", "-n"]) };
        line.extend(argv([
            "find",
            &dir,
            "-mindepth",
            "1",
            "-maxdepth",
            "1",
            "-type",
            "d",
            "-name",
            "download-*",
            "-empty",
            "-print",
            "-delete",
        ]));
        match sys.capture(&line) {
            Some((code, data)) => {
                removed += lines(&data).iter().filter(|l| is_download_dir(l, &dir)).count();
                failed |= code != 0;
            }
            None => failed = true,
        }
    }
    if failed && removed == 0 {
        Cleaned::Failed
    } else {
        Cleaned::Removed(removed)
    }
}

/// `paku __complete repo|installed [PREFIX]`: the package names the completions offer, only the ones
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
        sys.out("curl not found: it fetches the compromised package list (paku depends on curl; install it: sudo pacman -S curl)\n");
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
        sys.err("paku: aur-audit interrupted");
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
    text.push_str("Review these packages and consider removing them with: paku remove <package>\n");
    sys.out(&text);
    1
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A system that records the command lines and answers from a script.
    #[derive(Default)]
    pub(crate) struct Fake {
        pub(crate) programs: Vec<&'static str>,
        pub(crate) root: bool,
        /// PAKU_HELPER or the config's helper, with where it came from.
        pub(crate) setting: Option<(&'static str, &'static str)>,
        /// Exit codes by command line ("sudo pacman -S foo"); 0 when missing.
        pub(crate) codes: HashMap<String, i32>,
        /// Captured outputs by command line; a missing one can't start.
        pub(crate) outputs: HashMap<String, (i32, String)>,
        pub(crate) ran: Vec<String>,
        /// A command line (or a question) during which the user presses Ctrl-C.
        pub(crate) interrupt_on: Option<&'static str>,
        pub(crate) interrupted: bool,
        pub(crate) out: String,
        pub(crate) err: String,
        pub(crate) tty: bool,
        /// The answers to the questions, in order; a question with none left fails the test.
        pub(crate) answers: Vec<bool>,
        pub(crate) asked: Vec<String>,
        /// The answer each question gives to a plain Enter.
        pub(crate) defaults: Vec<bool>,
        pub(crate) lock_held: bool,
        pub(crate) last_news: Option<String>,
        pub(crate) saved_news: Option<String>,
        pub(crate) files: Vec<String>,
        /// A file that appears when a command line runs (a pacman starting meanwhile, for example).
        pub(crate) creates: Option<(&'static str, &'static str)>,
    }

    pub(crate) fn line(argv: &[OsString]) -> String {
        argv.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ")
    }

    impl Sys for Fake {
        fn run(&mut self, argv: &[OsString]) -> i32 {
            let l = line(argv);
            self.ran.push(l.clone());
            if self.interrupt_on == Some(l.as_str()) {
                self.interrupted = true;
            }
            if let Some((_, file)) = self.creates.filter(|(when, _)| *when == l) {
                self.files.push(file.to_string());
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
        fn tty(&self) -> bool {
            self.tty
        }
        fn ask(&mut self, question: &str, default: bool) -> bool {
            self.asked.push(question.to_string());
            self.defaults.push(default);
            if self.interrupt_on.is_some_and(|q| question.contains(q)) {
                self.interrupted = true;
                return false;
            }
            assert!(!self.answers.is_empty(), "unexpected question: {question}");
            self.answers.remove(0)
        }
        fn lock(&mut self) -> bool {
            !self.lock_held
        }
        fn last_news(&self) -> Option<String> {
            self.last_news.clone()
        }
        fn save_last_news(&mut self, id: &str) {
            self.saved_news = Some(id.to_string());
        }
        fn exists(&self, path: &str) -> bool {
            self.files.iter().any(|f| f == path)
        }
    }

    pub(crate) fn paku(fake: &mut Fake, args: &[&str]) -> i32 {
        run(&argv(args), fake)
    }

    pub(crate) fn with(programs: &[&'static str]) -> Fake {
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
            (&["owns", "/usr/bin/pacman"], "pacman -Qo /usr/bin/pacman"),
            (&["files", "pacman"], "pacman -Ql pacman"),
            (&["list"], "pacman -Qe"),
            (&["aur"], "pacman -Qm"),
            (&["check"], "H -Qu"),
        ];
        for helper in helper::HELPERS {
            for (args, expected) in cases {
                let mut f = with(&[helper]);
                assert_eq!(paku(&mut f, args), 0, "{helper} {args:?}");
                assert_eq!(f.ran, [expected.replace("H ", &format!("{helper} "))], "{helper} {args:?}");
                assert_eq!(f.err, "", "{helper} {args:?}");
            }
        }
    }

    const PKG: &str = "/var/cache/pacman/pkg/";
    pub(crate) const LIST: &str = "find /var/cache/pacman/pkg/ -mindepth 1 -maxdepth 1 -type d -name download-*";
    pub(crate) const DELETE: &str =
        "find /var/cache/pacman/pkg/ -mindepth 1 -maxdepth 1 -type d -name download-* -empty -print -delete";

    /// pacman-conf with two cache dirs; the first has two download dirs left, and `removed` of them go.
    fn leftovers(programs: &[&'static str], removed: &str) -> Fake {
        let mut f = with(programs);
        add_leftovers(&mut f, removed);
        f
    }

    pub(crate) fn add_leftovers(f: &mut Fake, removed: &str) {
        f.programs.push("pacman-conf");
        f.outputs.insert("pacman-conf DBPath".into(), (0, "/var/lib/pacman/\n".into()));
        f.outputs.insert("pacman-conf CacheDir".into(), (0, format!("{PKG}\n/srv/cache/\n")));
        let both = "/var/cache/pacman/pkg/download-aB3xYz\n/var/cache/pacman/pkg/download-Q1w2E3\n";
        f.outputs.insert(LIST.into(), (0, both.into()));
        f.outputs.insert(format!("sudo -n {DELETE}"), (0, removed.into()));
        f.outputs.insert(DELETE.into(), (0, removed.into()));
    }

    #[test]
    fn clean_removes_the_empty_download_dirs_pacman_left_first() {
        // Both flavours: sudo -v on the terminal, then the captured sudo -n find, then the cache cleaning
        let gone = "/var/cache/pacman/pkg/download-aB3xYz\n/var/cache/pacman/pkg/download-Q1w2E3\n";
        for (programs, sc) in [(&["paru"][..], "paru -Sc"), (&[], "sudo pacman -Sc")] {
            let mut f = leftovers(programs, gone);
            assert_eq!(paku(&mut f, &["clean"]), 0);
            let tail: Vec<&str> = f.ran.iter().map(String::as_str).filter(|l| !l.starts_with("pacman-conf")).collect();
            assert_eq!(
                tail,
                [
                    LIST,
                    "find /srv/cache/ -mindepth 1 -maxdepth 1 -type d -name download-*",
                    "sudo -v",
                    &format!("sudo -n {DELETE}"),
                    sc
                ]
            );
            assert_eq!(f.out, "Removed 2 empty download dir(s) pacman left in the package cache.\n");
            assert_eq!(f.err, "");
        }
        // As root, no sudo
        let mut f = Fake { root: true, ..leftovers(&[], gone) };
        paku(&mut f, &["clean"]);
        assert!(f.ran.contains(&DELETE.to_string()) && !f.ran.iter().any(|l| l.starts_with("sudo")), "{:?}", f.ran);
    }

    #[test]
    fn clean_keeps_a_download_dir_in_use_and_says_nothing() {
        // find -empty leaves a dir with a file in it: nothing printed, nothing said
        let mut f = leftovers(&[], "");
        paku(&mut f, &["clean"]);
        assert!(f.ran.contains(&format!("sudo -n {DELETE}")));
        assert_eq!((f.out.as_str(), f.err.as_str()), ("", ""));
        // A line find printed that isn't a download dir right in the cache dir isn't counted
        let mut f =
            leftovers(&[], "/etc/passwd\n/var/cache/pacman/pkg/download-x/y\n/var/cache/pacman/pkg/download-ok\n");
        paku(&mut f, &["clean"]);
        assert_eq!(f.out, "Removed 1 empty download dir(s) pacman left in the package cache.\n");
    }

    #[test]
    fn clean_without_leftovers_asks_no_password() {
        let mut f = leftovers(&[], "");
        f.outputs.insert(LIST.into(), (0, String::new()));
        paku(&mut f, &["clean"]);
        assert!(!f.ran.iter().any(|l| l.starts_with("sudo -") || l.contains("-delete")), "{:?}", f.ran);
        assert_eq!(f.ran.last().unwrap(), "sudo pacman -Sc");
        // Without pacman-conf: the default cache dir
        let mut f = with(&[]);
        paku(&mut f, &["clean"]);
        assert_eq!(f.ran, [LIST, "sudo pacman -Sc"]);
    }

    #[test]
    fn clean_leaves_the_download_dirs_while_pacman_holds_its_lock() {
        let mut f = leftovers(&[], "/var/cache/pacman/pkg/download-aB3xYz\n");
        f.files.push("/var/lib/pacman/db.lck".into());
        paku(&mut f, &["clean"]);
        assert!(!f.ran.iter().any(|l| l.starts_with("find") || l.starts_with("sudo -")), "{:?}", f.ran);
        assert_eq!(f.ran.last().unwrap(), "sudo pacman -Sc");
    }

    #[test]
    fn a_pacman_started_while_sudo_asked_keeps_its_download_dir() {
        let mut f = leftovers(&[], "/var/cache/pacman/pkg/download-aB3xYz\n");
        f.creates = Some(("sudo -v", "/var/lib/pacman/db.lck"));
        paku(&mut f, &["clean"]);
        assert!(f.ran.contains(&"sudo -v".to_string()));
        assert!(!f.ran.iter().any(|l| l.contains("-delete")), "{:?}", f.ran);
        assert_eq!((f.out.as_str(), f.err.as_str()), ("", ""));
    }

    #[test]
    fn clean_goes_on_when_sudo_refuses_and_stops_on_ctrl_c() {
        let mut f = leftovers(&[], "");
        f.outputs.insert(format!("sudo -n {DELETE}"), (1, String::new()));
        assert_eq!(paku(&mut f, &["clean"]), 0);
        assert_eq!(f.err, format!("{DOWNLOAD_DIRS_FAILED}\n"));
        assert_eq!(f.ran.last().unwrap(), "sudo pacman -Sc");
        let mut f = Fake { interrupt_on: Some("sudo -v"), ..leftovers(&[], "") };
        assert_eq!(paku(&mut f, &["clean"]), 130);
        assert!(!f.ran.iter().any(|l| l.ends_with("-Sc")), "{:?}", f.ran);
    }

    #[test]
    fn the_helper_order_and_the_setting() {
        // auto: paru, then yay, then pikaur
        let mut f = with(&["pikaur", "yay"]);
        paku(&mut f, &["install", "htop"]);
        assert_eq!(f.ran, ["yay -S htop"]);
        // The setting wins over the order
        let mut f = Fake { setting: Some(("pikaur", "PAKU_HELPER")), ..with(&["paru", "pikaur"]) };
        paku(&mut f, &["install", "htop"]);
        assert_eq!((f.ran.as_slice(), f.err.as_str()), (&["pikaur -S htop".to_string()][..], ""));
        // pacman even with a helper installed; a failure then says why there was no AUR
        let mut f = Fake { setting: Some(("pacman", "~/.config/paku/config")), ..with(&["paru"]) };
        f.codes.insert("sudo pacman -S yay".into(), 1);
        assert_eq!(paku(&mut f, &["install", "yay"]), 1);
        assert_eq!(f.ran, ["sudo pacman -S yay"]);
        assert!(f.err.contains("the helper is set to pacman"), "{}", f.err);
        // A chosen helper that isn't installed: a warning, then auto
        let mut f = Fake { setting: Some(("yay", "PAKU_HELPER")), ..with(&["paru"]) };
        paku(&mut f, &["search", "htop"]);
        assert_eq!(f.ran, ["paru -Ss htop"]);
        assert_eq!(f.err, "paku: yay (set in PAKU_HELPER) isn't installed; using paru\n");
        // The subcommands that don't run the package tool don't look for it (and don't warn)
        let mut f = Fake { setting: Some(("aura", "PAKU_HELPER")), ..with(&[]) };
        for args in [&["--help"][..], &["--version"], &["completions", "bash"], &["owns", "/x"], &["list"], &["aur"]] {
            paku(&mut f, args);
        }
        assert_eq!(f.err, "");
        paku(&mut f, &["check"]);
        assert!(f.err.starts_with("paku: unknown helper \"aura\""), "{}", f.err);
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
            (&["check"], "pacman -Qu"),
        ];
        for (args, expected) in cases {
            let mut f = with(&[]);
            assert_eq!(paku(&mut f, args), 0, "{args:?}");
            assert_eq!(f.ran, [*expected], "{args:?}");
        }
        // As root, no sudo
        let mut f = Fake { root: true, ..Fake::default() };
        paku(&mut f, &["install", "htop"]);
        assert_eq!(f.ran, ["pacman -S htop"]);
        // pacman-contrib's checkupdates, when there with fakeroot
        let mut f = with(&["checkupdates", "fakeroot"]);
        paku(&mut f, &["co"]);
        assert_eq!(f.ran, ["checkupdates"]);
        let mut f = with(&["checkupdates"]);
        paku(&mut f, &["co"]);
        assert_eq!(f.ran, ["pacman -Qu"]);
    }

    #[test]
    fn without_a_helper_a_package_not_found_mentions_the_aur() {
        let mut f = with(&[]);
        f.codes.insert("sudo pacman -S yay".into(), 1);
        assert_eq!(paku(&mut f, &["install", "yay"]), 1);
        assert!(f.err.contains("no AUR helper (paru, yay or pikaur) is installed"), "{}", f.err);
        // Found: no note
        let mut f = with(&[]);
        paku(&mut f, &["search", "htop"]);
        assert_eq!(f.err, "");
        // info: the repos, then the installed packages
        let mut f = with(&[]);
        f.codes.insert("pacman -Si yay".into(), 1);
        assert_eq!(paku(&mut f, &["info", "yay"]), 0);
        assert_eq!(f.ran, ["pacman -Si yay", "pacman -Qi yay"]);
        assert_eq!(f.err, "");
        // Ctrl-C in the first: no second, and no AUR note
        let mut h = with(&[]);
        h.codes.insert("sudo pacman -S yay".into(), 130);
        assert_eq!(paku(&mut h, &["install", "yay"]), 130);
        assert_eq!(h.err, "");
        let mut g = with(&[]);
        g.codes.insert("pacman -Si yay".into(), 130);
        assert_eq!(paku(&mut g, &["info", "yay"]), 130);
        assert_eq!(g.ran, ["pacman -Si yay"]);
        f.codes.insert("pacman -Qi yay".into(), 1);
        f.ran.clear();
        assert_eq!(paku(&mut f, &["info", "yay"]), 1);
        assert!(f.err.contains("no AUR"));
        // With paru the failure is just paru's
        let mut f = with(&["paru"]);
        f.codes.insert("paru -S nope".into(), 1);
        assert_eq!(paku(&mut f, &["install", "nope"]), 1);
        assert_eq!(f.err, "");
    }

    #[test]
    fn exit_codes_and_messages_like_the_fish_function() {
        let mut f = with(&["paru"]);
        assert_eq!(paku(&mut f, &[]), 1);
        assert!(f.out.starts_with("Usage:\n  paku upgrade"));
        let mut f = with(&["paru"]);
        assert_eq!(paku(&mut f, &["frobnicate"]), 1);
        assert_eq!(f.out, "Unknown command: frobnicate\nRun: paku (no args) for help\n");
        for (cmd, usage) in [
            ("search", "paku search <package>"),
            ("i", "paku install <package>..."),
            ("rm", "paku remove <package>..."),
            ("info", "paku info <package>"),
            ("owns", "paku owns <file>..."),
            ("files", "paku files <package>..."),
        ] {
            let mut f = with(&["paru"]);
            assert_eq!(paku(&mut f, &[cmd]), 1);
            assert_eq!(f.out, format!("{usage}\n"));
            assert!(f.ran.is_empty());
        }
        // The child's code is paku's
        let mut f = with(&["paru"]);
        f.codes.insert("paru -Rns x".into(), 1);
        assert_eq!(paku(&mut f, &["remove", "x"]), 1);
        let mut f = with(&["paru"]);
        assert_eq!(paku(&mut f, &["--help"]), 0);
        assert_eq!(paku(&mut f, &["--version"]), 0);
        assert!(f.out.ends_with(&format!("paku {}\n", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn upgrade_also_upgrades_flatpak_only_after_the_system() {
        let mut f = with(&["paru", "flatpak"]);
        assert_eq!(paku(&mut f, &["upgrade"]), 0);
        assert_eq!(f.ran, ["paru -Syu", "flatpak upgrade"]);
        // paru failed, or exited 1 on a Ctrl-C paku didn't get (sudo's pty): no flatpak
        let mut f = with(&["paru", "flatpak"]);
        f.codes.insert("paru -Syu".into(), 1);
        assert_eq!(paku(&mut f, &["upgrade"]), 1);
        assert_eq!(f.ran, ["paru -Syu"]);
        let mut f = with(&["paru", "flatpak"]);
        f.codes.insert("paru -Syu".into(), 130);
        assert_eq!(paku(&mut f, &["upgrade"]), 130);
        assert_eq!(f.ran, ["paru -Syu"]);
        // paru exits 1 on Ctrl-C: what paku got says it was stopped
        let mut f = with(&["paru", "flatpak"]);
        f.codes.insert("paru -Syu".into(), 1);
        f.interrupt_on = Some("paru -Syu");
        assert_eq!(paku(&mut f, &["upgrade"]), 1);
        assert_eq!(f.ran, ["paru -Syu"]);
        let mut f = with(&["paru", "flatpak"]);
        f.codes.insert("flatpak upgrade".into(), 3);
        assert_eq!(paku(&mut f, &["upgrade"]), 3);
    }

    #[test]
    fn upgrade_without_flatpak() {
        let mut f = with(&["paru", "flatpak"]);
        assert_eq!(paku(&mut f, &["upgrade", "--no-flatpak"]), 0);
        assert_eq!(f.ran, ["paru -Syu"]);
        let mut f = with(&["flatpak"]);
        f.codes.insert("sudo pacman -Syu".into(), 2);
        assert_eq!(paku(&mut f, &["up", "--no-flatpak"]), 2);
        assert_eq!(f.ran, ["sudo pacman -Syu"]);
        // Anything else is a usage error, before anything runs
        for args in [&["upgrade", "--noflatpak"][..], &["upgrade", "--no-flatpak", "x"], &["upgrade", "htop"]] {
            let mut f = with(&["paru", "flatpak"]);
            assert_eq!(paku(&mut f, args), 1, "{args:?}");
            assert_eq!(f.out, "paku upgrade [--no-flatpak]\n");
            assert!(f.ran.is_empty());
        }
    }

    #[test]
    fn helper_prints_the_tool() {
        for (programs, setting, tool) in [
            (&["paru", "yay"][..], None, "paru"),
            (&["yay", "pikaur"][..], None, "yay"),
            (&["pikaur"][..], None, "pikaur"),
            (&[][..], None, "pacman"),
            (&["paru"][..], Some(("pacman", "PAKU_HELPER")), "pacman"),
            (&["paru", "pikaur"][..], Some(("pikaur", "PAKU_HELPER")), "pikaur"),
        ] {
            let mut f = Fake { setting, ..with(programs) };
            assert_eq!(paku(&mut f, &["helper"]), 0);
            assert_eq!((f.out.as_str(), f.err.as_str(), f.ran.len()), (format!("{tool}\n").as_str(), "", 0));
        }
        // A setting that can't be followed: the warning on stderr, what runs on stdout
        let mut f = Fake { setting: Some(("yay", "PAKU_HELPER")), ..with(&["paru"]) };
        assert_eq!(paku(&mut f, &["helper"]), 0);
        assert_eq!(
            (f.out.as_str(), f.err.as_str()),
            ("paru\n", "paku: yay (set in PAKU_HELPER) isn't installed; using paru\n")
        );
    }

    #[test]
    fn maintain_and_check_go_to_cachy_update_only() {
        // cachy-update installed: it runs, with the arguments as given
        let mut f = with(&["paru", "cachy-update", "arch-update"]);
        paku(&mut f, &["full", "--news", "10"]);
        paku(&mut f, &["check"]);
        assert_eq!(f.ran, ["cachy-update --news 10", "cachy-update --list"]);
        // arch-update alone: paku's own flow and list (not a terminal here, so the flow only lists)
        let mut f = with(&["arch-update", "checkupdates", "fakeroot"]);
        f.outputs.insert("checkupdates".into(), (2, String::new()));
        assert_eq!(paku(&mut f, &["maintain"]), 0);
        assert!(f.ran.iter().all(|l| !l.starts_with("arch-update")), "{:?}", f.ran);
        assert!(f.out.contains("paku maintain: no terminal, listing only"));
        f.ran.clear();
        paku(&mut f, &["check"]);
        assert_eq!(f.ran, ["checkupdates"]);
        // neither: the same
        let mut f = with(&["paru"]);
        assert_eq!(paku(&mut f, &["cu"]), 0);
        assert!(f.ran.contains(&"paru -Qua".to_string()), "{:?}", f.ran);
    }

    #[test]
    fn autoremove_removes_the_orphans_or_says_there_are_none() {
        let mut f = with(&[]);
        f.outputs.insert("pacman -Qdtq".into(), (0, "a\nb\n".into()));
        assert_eq!(paku(&mut f, &["orphans"]), 0);
        assert_eq!(f.ran, ["pacman -Qdtq", "sudo pacman -Rns a b"]);
        // pacman -Qdtq exits 1 with no orphans
        let mut f = with(&["paru"]);
        f.outputs.insert("pacman -Qdtq".into(), (1, String::new()));
        assert_eq!(paku(&mut f, &["autoremove"]), 0);
        assert_eq!(f.out, "No orphaned packages found.\n");
        assert_eq!(f.ran, ["pacman -Qdtq"]);
    }

    const CURL: &str =
        "curl -fsSL --proto =https --proto-redir =https --max-time 20 https://md.archlinux.org/s/SxbqukK6IA/download";

    #[test]
    fn aur_audit_codes() {
        // No curl
        let mut f = with(&[]);
        assert_eq!(paku(&mut f, &["aur-audit"]), 1);
        assert!(f.out.starts_with("curl not found"));
        // No network
        let mut f = with(&["curl"]);
        f.outputs.insert(CURL.into(), (6, String::new()));
        assert_eq!(paku(&mut f, &["aur-sec"]), 1);
        assert!(f.out.starts_with("Could not fetch the compromised package list from:\n  https://md.archlinux.org/"));
        // Ctrl-C during the download
        let mut f = with(&["curl"]);
        f.outputs.insert(CURL.into(), (6, String::new()));
        f.interrupted = true;
        assert_eq!(paku(&mut f, &["aur-audit"]), 130);
        assert_eq!((f.out.as_str(), f.err.as_str()), ("", "paku: aur-audit interrupted\n"));
        // Nothing from the AUR
        let mut f = with(&["curl"]);
        f.outputs.insert(CURL.into(), (0, "```\nbad-bin\n```\n".into()));
        f.outputs.insert("pacman -Qmq".into(), (1, String::new()));
        assert_eq!(paku(&mut f, &["aur-audit"]), 0);
        assert_eq!(f.out, "No AUR (foreign) packages installed — nothing to check.\n");
        // Clean
        let mut f = with(&["curl"]);
        f.outputs.insert(CURL.into(), (0, "```\nbad-bin\nworse-git\n```\n".into()));
        f.outputs.insert("pacman -Qmq".into(), (0, "good\n".into()));
        assert_eq!(paku(&mut f, &["aur-audit"]), 0);
        assert_eq!(
            f.out,
            "Checked 1 installed AUR package(s) against 2 known-compromised entries.\nNo installed AUR packages are on the compromised list.\n"
        );
        // A hit
        let mut f = with(&["curl"]);
        f.outputs.insert(CURL.into(), (0, "```\nbad-bin\nworse-git\n```\n".into()));
        f.outputs.insert("pacman -Qmq".into(), (0, "good\nworse-git\n".into()));
        assert_eq!(paku(&mut f, &["aur-security"]), 1);
        assert!(f.out.contains(
            "WARNING: the following installed AUR packages are on the compromised list:\n  worse-git\nReview"
        ));
    }

    #[test]
    fn the_hidden_completer_lists_packages() {
        let mut f = with(&[]);
        f.outputs.insert("pacman -Ssq".into(), (0, "a\nb\n".into()));
        f.outputs.insert("pacman -Qq".into(), (0, "c\n".into()));
        assert_eq!(paku(&mut f, &["__complete", "repo"]), 0);
        assert_eq!(paku(&mut f, &["__complete", "installed"]), 0);
        assert_eq!(f.out, "a\nb\nc\n");
        assert_eq!(paku(&mut f, &["__complete", "other"]), 1);
        assert_eq!(paku(&mut f, &["__complete"]), 1);
    }

    #[test]
    fn completions_print_a_script_per_shell() {
        for shell in commands::SHELLS {
            let mut f = with(&[]);
            assert_eq!(paku(&mut f, &["completions", shell]), 0);
            assert!(f.out.contains("aur-audit"), "{shell}");
        }
        let mut f = with(&[]);
        assert_eq!(paku(&mut f, &["completions", "tcsh"]), 1);
        assert_eq!(f.out, "paku completions <bash|zsh|fish>\n");
    }
}
