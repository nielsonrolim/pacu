//! `paku maintain` when cachy-update isn't installed: the upgrade, then the maintenance steps. The flow follows
//! the one of arch-update 4.4.7 (https://github.com/Antiz96/arch-update), reimplemented here (no code of it is
//! used). It differs in the defaults (every step that removes or restarts something answers no to a plain
//! Enter) and in two steps that only inform: the failed units and the journal size.
//!
//! Without a terminal (or with `--dry-run`) every step only lists what it would do: no question, no change,
//! not even the upgrade. `--noconfirm` is never passed to anything.
use std::ffi::OsString;

use crate::news;
use crate::paku::{argv, lines, stopped, Backend, Sys};

/// What `paku maintain` prints (exit 1) for an argument it doesn't take.
pub const USAGE: &str = "paku maintain [--dry-run] [--no-flatpak] [--devel]";

/// The exit code when another `paku maintain` holds the lock (arch-update's code for the same case).
pub const LOCK_HELD: i32 = 17;

/// The reboot reason for services still running on old files.
const OLD_FILES: &str = "services kept running on old files";

/// The exit code when a step failed (the next ones still ran).
pub const STEP_FAILED: i32 = 2;

/// Old versions paccache keeps of an installed package, and of an uninstalled one.
const KEEP_INSTALLED: &str = "3";
const KEEP_UNINSTALLED: &str = "0";

/// The display managers, which checkservices is told to ignore (the same list as arch-update).
const DISPLAY_MANAGERS: &[&str] = &[
    "gdm.service",
    "plasmalogin.service",
    "sddm.service",
    "lightdm.service",
    "lxdm.service",
    "slim.service",
    "xdm.service",
    "greetd.service",
    "nodm.service",
    "ly.service",
    "lemurs.service",
    "display-manager.service",
];

/// What would end the session or the network in the middle of the run (also the units starting with
/// KEEP_PREFIXES): never restarted here, only named.
const SESSION_AND_NETWORK: &[&str] = &[
    "systemd-logind.service",
    "systemd-user-sessions.service",
    "console-getty.service",
    "NetworkManager.service",
    "systemd-networkd.service",
    "iwd.service",
    "wpa_supplicant.service",
];

/// D-Bus, the user managers and the logins on terminals (restarting getty@ttyN ends the login of whoever runs
/// paku there).
const KEEP_PREFIXES: &[&str] = &["dbus", "user@", "getty@", "serial-getty@", "container-getty@"];

/// The services check: -R only lists (it restarts nothing; paku restarts what's left after its own y/N; never
/// -a), -P: no pacdiff (its own step ran), -F: no failed list (its own step too).
const CHECKSERVICES_WORDS: [&str; 4] = ["checkservices", "-F", "-P", "-R"];

/// The journal size from which the flow mentions it.
const JOURNAL_WARN_BYTES: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Default)]
struct Options {
    dry_run: bool,
    no_flatpak: bool,
    devel: bool,
}

fn options(args: &[OsString]) -> Option<Options> {
    let mut o = Options::default();
    for arg in args {
        match arg.to_str()? {
            "--dry-run" => o.dry_run = true,
            "--no-flatpak" => o.no_flatpak = true,
            "--devel" => o.devel = true,
            _ => return None,
        }
    }
    Some(o)
}

/// How the upgrade step went.
#[derive(Debug, PartialEq, Eq)]
enum Upgrade {
    /// Done (or nothing to do); `changed` when packages were upgraded.
    Done { changed: bool },
    /// Answered no.
    Skipped,
    /// Only listed; `packages` when the system upgrade would run (and the services check after it).
    Listed { packages: bool },
    /// The system upgrade failed.
    Failed,
}

/// A Ctrl-C: the flow stops at once.
struct Stopped;

type Step<T = ()> = Result<T, Stopped>;

struct Flow<'a> {
    sys: &'a mut dyn Sys,
    backend: &'a Backend,
    /// Only listing: `--dry-run` or no terminal.
    listing: bool,
    failed: Vec<&'static str>,
    /// Why a reboot is recommended (a new kernel not booted, services kept running on old files): said once,
    /// in the last line.
    reboot: Vec<&'static str>,
}

/// `paku maintain` with its arguments; gives the exit code.
pub fn run(args: &[OsString], sys: &mut dyn Sys, backend: &Backend) -> i32 {
    let Some(o) = options(args) else {
        sys.out(&format!("{USAGE}\n"));
        return 1;
    };
    let listing = o.dry_run || !sys.tty();
    if !o.dry_run && listing {
        sys.out("paku maintain: no terminal, listing only; run it in a terminal\n");
    }
    if !listing && !sys.lock() {
        sys.err("paku maintain: another paku maintain is already running");
        return LOCK_HELD;
    }
    let mut flow = Flow { sys, backend, listing, failed: Vec::new(), reboot: Vec::new() };
    let result = flow.steps(&o);
    let Flow { sys, failed, reboot, .. } = flow;
    if result.is_err() {
        sys.out("\npaku maintain: stopped (Ctrl-C)\n");
        return 130;
    }
    let listed = if listing { " (listing only, nothing was changed)" } else { "" };
    let suffix = if reboot.is_empty() {
        listed.to_string()
    } else {
        format!("{listed}; reboot recommended ({})", reboot.join(", "))
    };
    if failed.is_empty() {
        sys.out(&format!("\npaku maintain: done{suffix}\n"));
        0
    } else {
        let n = failed.len();
        sys.out(&format!(
            "\npaku maintain: done, {n} step{} failed ({}){suffix}\n",
            if n == 1 { "" } else { "s" },
            failed.join(", ")
        ));
        STEP_FAILED
    }
}

impl Flow<'_> {
    fn steps(&mut self, o: &Options) -> Step {
        let (pending, known) = self.pending()?;
        let newest_news = self.news()?;
        let upgrade = self.upgrade(&pending, known, o)?;
        // The news count as seen only once shown and followed by the upgrade
        if let (Some(id), Upgrade::Done { .. }, false) = (newest_news, &upgrade, self.listing) {
            self.sys.save_last_news(&id);
        }
        if upgrade == Upgrade::Failed {
            // The databases are synced but the packages aren't: a partial upgrade, where removing orphans or
            // cached packages is the risky case. Only the information steps
            self.title("Maintenance");
            self.note("skipped: the upgrade failed; fix it and run paku maintain again");
            self.failed_units()?;
            return self.journal();
        }
        let changed = upgrade == Upgrade::Done { changed: true };
        self.orphans()?;
        if !o.no_flatpak {
            self.flatpak_unused()?;
        }
        self.cache()?;
        self.pacnew()?;
        self.failed_units()?;
        self.journal()?;
        if changed {
            self.services()?;
        } else if upgrade == (Upgrade::Listed { packages: true }) {
            self.services_listed();
        }
        self.kernel()
    }

    fn title(&mut self, text: &str) {
        self.sys.out(&format!("\n:: {text}\n"));
    }

    fn note(&mut self, text: &str) {
        self.sys.out(&format!("  {text}\n"));
    }

    /// A command that needs root: through sudo, unless paku runs as root.
    fn as_root(&self, words: &[&str], more: &[String]) -> Vec<OsString> {
        let mut line = if self.backend.root { Vec::new() } else { argv(["sudo"]) };
        line.extend(argv(words));
        line.extend(more.iter().map(OsString::from));
        line
    }

    fn interrupted(&self) -> Step {
        if self.sys.interrupted() {
            Err(Stopped)
        } else {
            Ok(())
        }
    }

    /// The outcome of a command: Err on a Ctrl-C, false (and the step noted as failed) on another failure.
    fn ran(&mut self, step: &'static str, code: i32) -> Step<bool> {
        if stopped(self.sys, code) {
            return Err(Stopped);
        }
        if code != 0 {
            // pacman and the helpers also exit 1 when their own question is answered no
            self.sys.err(&format!("paku maintain: {step}: didn't go through (exit {code})"));
            if !self.failed.contains(&step) {
                self.failed.push(step);
            }
            return Ok(false);
        }
        Ok(true)
    }

    fn capture(&mut self, words: Vec<OsString>) -> Step<Option<(i32, Vec<u8>)>> {
        let result = self.sys.capture(&words);
        self.interrupted()?;
        Ok(result)
    }

    /// The lines of a query, empty when it can't start.
    fn query(&mut self, words: Vec<OsString>) -> Step<Vec<String>> {
        Ok(self.capture(words)?.map(|(_, data)| lines(&data)).unwrap_or_default())
    }

    fn ask(&mut self, question: &str, default: bool) -> Step<bool> {
        let yes = self.sys.ask(question, default);
        self.interrupted()?;
        Ok(yes)
    }

    fn would(&mut self, line: &[OsString]) {
        let text = line.iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" ");
        self.note(&format!("would run: {text}"));
    }

    /// Asks (default no) and runs the commands in order, stopping at the first that fails; only lists them
    /// when listing.
    fn confirm(&mut self, step: &'static str, question: &str, commands: Vec<Vec<OsString>>) -> Step {
        if self.listing {
            for line in &commands {
                self.would(line);
            }
            return Ok(());
        }
        if !self.ask(question, false)? {
            return Ok(());
        }
        for line in commands {
            let code = self.sys.run(&line);
            if !self.ran(step, code)? {
                break;
            }
        }
        Ok(())
    }

    fn skip(&mut self, why: &str) {
        self.note(&format!("skipped: {why}"));
    }

    /// A tool of one of paku's dependencies that isn't there anyway (a broken install, a PATH without it).
    fn missing(&mut self, tool: &str, package: &str) {
        self.skip(&format!("{tool} not found (paku depends on {package}; install it: sudo pacman -S {package})"));
    }

    fn show(&mut self, items: &[String]) {
        for item in items {
            self.note(item);
        }
    }

    /// The pending packages (repos, then AUR), and whether the list is known.
    fn pending(&mut self) -> Step<(Vec<String>, bool)> {
        self.title("Pending updates");
        let mut known = true;
        let mut list = if self.sys.has("checkupdates") && self.sys.has("fakeroot") {
            // pacman-contrib: a fresh copy of the databases, without root (it needs fakeroot, only an optional
            // dependency of pacman-contrib); 2 is "none"
            match self.capture(argv(["checkupdates"]))? {
                Some((0 | 2, data)) => lines(&data),
                _ => {
                    known = false;
                    self.note("couldn't list the repository updates (checkupdates failed)");
                    Vec::new()
                }
            }
        } else {
            // The local databases, maybe out of date until the upgrade syncs them
            self.query(argv(["pacman", "-Qu"]))?
        };
        if let Some(helper) = self.backend.choice.helper {
            list.extend(self.query(argv([helper, "-Qua"]))?);
        }
        if list.is_empty() && known {
            self.note("none, the packages are up to date");
        }
        self.show(&list);
        Ok((list, known))
    }

    /// The Arch news not shown before; gives the id of the newest item of the feed to save after the upgrade.
    fn news(&mut self) -> Step<Option<String>> {
        self.title("Arch Linux news");
        if !self.sys.has("curl") {
            self.missing("curl", "curl");
            return Ok(None);
        }
        let feed = self.capture(argv([
            "curl",
            "-fsS",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-filesize",
            "1M",
            "-m",
            "10",
            news::FEED,
        ]))?;
        let items = match feed {
            Some((0, data)) => news::parse(&String::from_utf8_lossy(&data)),
            _ => Vec::new(),
        };
        let Some(newest) = items.first().map(|i| i.id.clone()) else {
            self.note("couldn't read the news");
            return Ok(None);
        };
        let last = self.sys.last_news();
        let unseen = news::unseen(&items, last.as_deref()).to_vec();
        if unseen.is_empty() {
            self.note("nothing new since the last run");
        }
        for item in unseen {
            self.note(&item.title);
            if let Some(link) = item.link {
                self.note(&format!("  {link}"));
            }
        }
        Ok(Some(newest))
    }

    /// The upgrade (packages, then Flatpak).
    fn upgrade(&mut self, pending: &[String], known: bool, o: &Options) -> Step<Upgrade> {
        self.title("Upgrade");
        let devel: Vec<OsString> = match (o.devel, self.backend.choice.helper) {
            (true, Some(_)) => argv(["--devel"]),
            _ => Vec::new(),
        };
        let system = self.backend.writes("-Syu", &devel);
        let flatpak = (!o.no_flatpak && self.sys.has("flatpak")).then(|| argv(["flatpak", "update"]));
        let packages = !pending.is_empty() || !known;
        if !packages && flatpak.is_none() {
            self.note("nothing to upgrade");
            return Ok(Upgrade::Done { changed: false });
        }
        if self.listing {
            if packages {
                self.would(&system);
            }
            if let Some(line) = &flatpak {
                self.would(line);
            }
            return Ok(Upgrade::Listed { packages });
        }
        let mut changed = false;
        if packages {
            if !self.ask("Proceed with the upgrade?", true)? {
                self.note("the upgrade was skipped");
                return Ok(Upgrade::Skipped);
            }
            let code = self.sys.run(&system);
            if !self.ran("upgrade", code)? {
                // Flatpak only after the system went through, as in `paku upgrade`
                return Ok(Upgrade::Failed);
            }
            changed = true;
        }
        if let Some(line) = flatpak {
            // flatpak lists and asks itself (and says when there's nothing to do)
            let code = self.sys.run(&line);
            self.ran("flatpak", code)?;
        }
        Ok(Upgrade::Done { changed })
    }

    fn orphans(&mut self) -> Step {
        self.title("Orphan packages");
        let orphans = self.query(argv(["pacman", "-Qdtq"]))?;
        if orphans.is_empty() {
            self.note("none");
            return Ok(());
        }
        self.show(&orphans);
        let line = self.as_root(&["pacman", "-Rns", "--"], &orphans);
        self.confirm(
            "orphans",
            "Remove them? Their configuration files go too (no .pacsave), and an orphan can be an optional \
             dependency you still use",
            vec![line],
        )
    }

    fn flatpak_unused(&mut self) -> Step {
        if !self.sys.has("flatpak") {
            return Ok(());
        }
        self.title("Unused Flatpak runtimes");
        // flatpak has no command that only lists them, and its uninstall can't be run just to look: once we get
        // a yes, it lists them and asks itself (or says there are none)
        self.confirm(
            "flatpak-unused",
            "Look for unused Flatpak runtimes? flatpak lists them and asks before removing",
            vec![argv(["flatpak", "uninstall", "--unused"])],
        )
    }

    /// The count paccache gives in a dry run ("==> finished dry run: 4 candidates (disk space saved: …)").
    fn candidates(&mut self, flag: &str) -> Step<u64> {
        let text = self.query(argv(["paccache", flag]))?;
        Ok(text
            .iter()
            .filter_map(|l| l.find(" candidate").and_then(|end| l[..end].rsplit(' ').next()?.parse::<u64>().ok()))
            .sum())
    }

    fn cache(&mut self) -> Step {
        self.title("Package cache");
        if !self.sys.has("paccache") {
            self.missing("paccache", "pacman-contrib");
        } else {
            let old = self.candidates(&format!("-dk{KEEP_INSTALLED}"))?;
            let gone = self.candidates(&format!("-duk{KEEP_UNINSTALLED}"))?;
            if old + gone == 0 {
                self.note("nothing to remove");
            } else {
                let mut commands = Vec::new();
                if old > 0 {
                    self.note(&format!(
                        "{old} old version(s) of installed packages (the last {KEEP_INSTALLED} are kept)"
                    ));
                    commands.push(self.as_root(&["paccache", &format!("-rk{KEEP_INSTALLED}")], &[]));
                }
                if gone > 0 {
                    self.note(&format!("{gone} file(s) of packages no longer installed"));
                    commands.push(self.as_root(&["paccache", &format!("-ruk{KEEP_UNINSTALLED}")], &[]));
                }
                self.confirm("cache", "Remove them from the cache?", commands)?;
            }
        }
        if let Some(helper) = self.backend.choice.helper {
            self.confirm(
                "cache",
                &format!("Also clean {helper}'s caches ({helper} -Sc asks about each one)?"),
                vec![argv([helper, "-Sc"])],
            )?;
        }
        Ok(())
    }

    fn pacnew(&mut self) -> Step {
        self.title("Configuration files to merge (.pacnew, .pacsave)");
        if !self.sys.has("pacdiff") {
            self.missing("pacdiff", "pacman-contrib");
            return Ok(());
        }
        let files = self.query(argv(["pacdiff", "-o"]))?;
        if files.is_empty() {
            self.note("none");
            return Ok(());
        }
        self.show(&files);
        // As the user, with sudo only for the file operations: $DIFFPROG runs as the user
        let line = if self.backend.root { argv(["pacdiff"]) } else { argv(["pacdiff", "--sudo"]) };
        self.confirm("pacnew", "Merge them now with pacdiff?", vec![line])
    }

    fn failed_units(&mut self) -> Step {
        self.title("Failed units");
        let mut any = false;
        for (scope, words) in [
            ("system", &["systemctl", "--failed", "--no-legend", "--plain"][..]),
            ("user", &["systemctl", "--user", "--failed", "--no-legend", "--plain"][..]),
        ] {
            let units = self.query(argv(words))?;
            for unit in units.iter().filter_map(|l| l.split_whitespace().next()) {
                any = true;
                let status = if scope == "user" { "systemctl --user status" } else { "systemctl status" };
                self.note(&format!("{unit} ({scope}): see {status} {unit}"));
            }
        }
        if !any {
            self.note("none");
        }
        Ok(())
    }

    fn journal(&mut self) -> Step {
        let text = self.query(argv(["journalctl", "--disk-usage"]))?;
        let Some(size) =
            text.iter().find_map(|l| l.split("take up ").nth(1)?.split_whitespace().next().map(String::from))
        else {
            return Ok(());
        };
        if bytes(&size).is_some_and(|b| b >= JOURNAL_WARN_BYTES) {
            self.title("Journal");
            self.note(&format!("the journal you can read takes {size}; sudo journalctl --vacuum-size=500M trims it"));
        }
        Ok(())
    }

    fn services(&mut self) -> Step {
        self.title("Services to restart");
        if !self.sys.has("checkservices") {
            self.missing("checkservices", "archlinux-contrib");
            return Ok(());
        }
        let ignored = self.session_units()?;
        // No -i: the units kept running are filtered here, so they can still be named (with a reboot hint)
        // instead of disappearing
        let words = CHECKSERVICES_WORDS;
        // A question left waiting can outlast sudo's timestamp: sudo -v asks for the password again here, on the
        // terminal, since the sudo -n below can only fail. If it fails or is cancelled, the -n below gives the
        // message. (The other root steps run sudo without -n, so it asks itself.)
        if !self.backend.root && !self.listing {
            let code = self.sys.run(&argv(["sudo", "-v"]));
            if stopped(self.sys, code) {
                return Err(Stopped);
            }
        }
        // sudo -n: a question for the password would be hidden in the captured output
        let line = if self.backend.root { argv(words) } else { argv(["sudo", "-n"].iter().chain(&words)) };
        let output = match self.capture(line)? {
            Some((0, data)) => lines(&data),
            _ => {
                self.note("couldn't check them (sudo asked for the password again); run: sudo checkservices");
                return Ok(());
            }
        };
        let mut restart = Vec::new();
        let mut kept = Vec::new();
        for l in output.iter().filter(|l| l.contains(".service")) {
            let unit = l.split('\'').nth(1).unwrap_or(l).trim();
            if !unit.ends_with(".service") || !unit.chars().all(|c| c.is_ascii_graphic()) {
                continue;
            }
            let keep = ignored.iter().any(|u| u == unit)
                || SESSION_AND_NETWORK.contains(&unit)
                || KEEP_PREFIXES.iter().any(|p| unit.starts_with(p));
            let list = if keep { &mut kept } else { &mut restart };
            if !list.iter().any(|u| u == unit) {
                list.push(unit.to_string());
            }
        }
        if !kept.is_empty() {
            self.recommend_reboot(OLD_FILES);
            self.note(&format!(
                "not restarted here (they'd end the session or the network): {}; they still run the old files until \
                 you restart them yourself or reboot",
                kept.join(" ")
            ));
        }
        if restart.is_empty() {
            // After the "not restarted here" line, a "none" would read as if nothing had been found
            if kept.is_empty() {
                self.note("none");
            }
            return Ok(());
        }
        self.show(&restart);
        let line = self.as_root(&["systemctl", "restart", "--"], &restart);
        if self.listing {
            self.would(&line);
            return Ok(());
        }
        // Not restarted (a no, or a failure): they also stay on the old files
        if !self.ask("Restart them all?", false)? {
            self.recommend_reboot(OLD_FILES);
            return Ok(());
        }
        let code = self.sys.run(&line);
        if !self.ran("services", code)? {
            self.recommend_reboot(OLD_FILES);
        }
        Ok(())
    }

    /// The services step of a listing whose upgrade would run: checkservices only means something after the
    /// upgrade, so it says what would run then instead of running it now (as sudo -n, it would fail or list
    /// what an earlier upgrade left).
    fn services_listed(&mut self) {
        self.title("Services to restart");
        if !self.sys.has("checkservices") {
            self.missing("checkservices", "archlinux-contrib");
            return;
        }
        let line = self.as_root(&CHECKSERVICES_WORDS, &[]);
        self.would(&line);
        self.note("after the upgrade, then it asks before restarting the services it lists");
    }

    fn recommend_reboot(&mut self, reason: &'static str) {
        if !self.reboot.contains(&reason) {
            self.reboot.push(reason);
        }
    }

    /// The units checkservices never offers: the display managers, the unit `display-manager.service` points
    /// to, and the service of the login session paku runs in (its PAM service: greetd, sddm, sshd…), so the
    /// restart can't end the session under any display manager.
    fn session_units(&mut self) -> Step<Vec<String>> {
        let mut units: Vec<String> = DISPLAY_MANAGERS.iter().map(|u| u.to_string()).collect();
        let alias = self.query(argv(["systemctl", "show", "-p", "Id", "--value", "display-manager.service"]))?;
        let session = self.query(argv(["loginctl", "show-session", "auto", "-p", "Service", "--value"]))?;
        let candidates = alias.into_iter().take(1).chain(session.into_iter().take(1).map(|s| {
            if s.ends_with(".service") {
                s
            } else {
                format!("{s}.service")
            }
        }));
        for unit in candidates {
            let name = unit.strip_suffix(".service").unwrap_or_default();
            let valid = !name.is_empty()
                && !name.starts_with('-')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.@:\\".contains(c));
            if valid && !units.contains(&unit) {
                units.push(unit);
            }
        }
        Ok(units)
    }

    /// The reboot: from a session on a local seat, as the user (logind lets the active local session reboot
    /// without a password); without a seat (ssh, a container console, no session), polkit would ask for a
    /// password with no agent to answer and time out, so through sudo.
    fn reboot_line(&mut self) -> Step<Vec<OsString>> {
        let seat = self.query(argv(["loginctl", "show-session", "auto", "-p", "Seat", "--value"]))?;
        if self.backend.root || seat.first().is_some_and(|s| !s.trim().is_empty()) {
            return Ok(argv(["systemctl", "reboot"]));
        }
        Ok(argv(["sudo", "systemctl", "reboot"]))
    }

    fn kernel(&mut self) -> Step {
        let release = self.query(argv(["uname", "-r"]))?.into_iter().next().unwrap_or_default();
        if release.is_empty() || !release.chars().all(|c| c.is_ascii_graphic() && c != '/') {
            return Ok(());
        }
        if self.sys.exists(&format!("/usr/lib/modules/{release}/vmlinuz")) {
            return Ok(());
        }
        // A container shares the host's kernel
        if matches!(self.capture(argv(["systemd-detect-virt", "--container", "--quiet"]))?, Some((0, _))) {
            return Ok(());
        }
        self.title("Kernel");
        self.note(&format!("the running kernel ({release}) is no longer installed: reboot to use the new one"));
        if !self.listing && self.ask("Reboot now?", false)? {
            let line = self.reboot_line()?;
            let code = self.sys.run(&line);
            if self.ran("reboot", code)? {
                return Ok(());
            }
        }
        self.recommend_reboot("a new kernel");
        Ok(())
    }
}

/// A size as journalctl prints it ("1.5G", "512.0M", "24.0K", "123B").
fn bytes(size: &str) -> Option<f64> {
    let unit = size.chars().last()?;
    let (number, power) = match unit {
        'B' => (&size[..size.len() - 1], 0),
        'K' => (&size[..size.len() - 1], 1),
        'M' => (&size[..size.len() - 1], 2),
        'G' => (&size[..size.len() - 1], 3),
        'T' => (&size[..size.len() - 1], 4),
        _ => (size, 0),
    };
    Some(number.parse::<f64>().ok()? * 1024f64.powi(power))
}

#[cfg(test)]
mod tests {
    use crate::paku::tests::{paku, with, Fake};

    const FEED: &str = "<rss><channel>\
        <item><title>New one</title><link>https://archlinux.org/news/new/</link></item>\
        <item><title>Seen one</title><link>https://archlinux.org/news/seen/</link></item>\
        </channel></rss>";

    const CURL: &str =
        "curl -fsS --proto =https --proto-redir =https --max-filesize 1M -m 10 https://archlinux.org/feeds/news/";

    const CHECKSERVICES: &str = "checkservices -F -P -R";

    /// A system with everything to do: updates, news, orphans, unused Flatpaks, cache, a .pacnew, a failed
    /// unit, a big journal, services to restart and a new kernel.
    fn busy(programs: &[&'static str]) -> Fake {
        let mut f = with(programs);
        f.tty = true;
        f.last_news = Some("https://archlinux.org/news/seen/".into());
        let o = &mut f.outputs;
        o.insert("checkupdates".into(), (0, "linux 6.17.1 -> 6.17.2\n".into()));
        o.insert("paru -Qua".into(), (0, "foo-git 1 -> 2\n".into()));
        o.insert(CURL.into(), (0, FEED.into()));
        o.insert("pacman -Qdtq".into(), (0, "orphan1\norphan2\n".into()));
        o.insert("paccache -dk3".into(), (0, "==> finished dry run: 4 candidates (disk space saved: 1 GiB)\n".into()));
        o.insert("paccache -duk0".into(), (0, "==> no candidate packages found for pruning\n".into()));
        o.insert("pacdiff -o".into(), (0, "/etc/pacman.conf.pacnew\n".into()));
        o.insert("systemctl --failed --no-legend --plain".into(), (0, "foo.service loaded failed failed Foo\n".into()));
        o.insert("systemctl --user --failed --no-legend --plain".into(), (0, String::new()));
        o.insert(
            "journalctl --disk-usage".into(),
            (0, "Archived and active journals take up 1.5G in the file system.\n".into()),
        );
        o.insert(
            format!("sudo -n {CHECKSERVICES}"),
            (
                0,
                "Found 3 processes\nsystemctl restart 'cups.service'\nsystemctl restart 'dbus-broker.service'\n\
                 systemctl restart 'NetworkManager.service'\nsystemctl restart 'getty@tty2.service'\n"
                    .into(),
            ),
        );
        o.insert("uname -r".into(), (0, "6.17.1-arch1-1\n".into()));
        o.insert("systemd-detect-virt --container --quiet".into(), (1, String::new()));
        // greetd behind the alias (already in the list) and a session over ssh
        o.insert(ALIAS.into(), (0, "greetd.service\n".into()));
        o.insert(SESSION.into(), (0, "sshd\n".into()));
        o.insert(SEAT.into(), (0, "seat0\n".into()));
        f
    }

    const SEAT: &str = "loginctl show-session auto -p Seat --value";

    const ALIAS: &str = "systemctl show -p Id --value display-manager.service";
    const SESSION: &str = "loginctl show-session auto -p Service --value";

    const ALL: &[&str] =
        &["paru", "checkupdates", "fakeroot", "curl", "flatpak", "paccache", "pacdiff", "checkservices", "journalctl"];

    #[test]
    fn the_display_manager_and_the_session_are_named_never_restarted() {
        let mut f = busy(ALL);
        // A display manager not in the list, behind the alias, and a session started by it
        f.outputs.insert(ALIAS.into(), (0, "mydm.service\n".into()));
        f.outputs.insert(SESSION.into(), (0, "mydm-greeter\n".into()));
        f.outputs.insert(
            format!("sudo -n {CHECKSERVICES}"),
            (
                0,
                "systemctl restart 'mydm.service'\nsystemctl restart 'mydm-greeter.service'\n\
                 systemctl restart 'greetd.service'\nsystemctl restart 'cups.service'\n"
                    .into(),
            ),
        );
        f.answers = vec![true; 8];
        assert_eq!(paku(&mut f, &["maintain"]), 0);
        assert!(f.ran.contains(&"sudo systemctl restart -- cups.service".to_string()), "{:?}", f.ran);
        assert!(f.ran.iter().all(|l| !l.contains("restart -- mydm") && !l.contains("greetd")), "{:?}", f.ran);
        assert!(
            f.out.contains(
                "not restarted here (they'd end the session or the network): mydm.service mydm-greeter.service \
                 greetd.service; they still run the old files"
            ),
            "{}",
            f.out
        );
        // checkservices only lists: never restarts nor answers itself
        let check: Vec<&String> = f.ran.iter().filter(|l| l.contains("checkservices")).collect();
        assert_eq!(check, [&format!("sudo -n {CHECKSERVICES}")]);
        // Values that aren't unit names don't count
        let mut f = busy(ALL);
        f.outputs.insert(ALIAS.into(), (0, "-x.service\n".into()));
        f.outputs.insert(SESSION.into(), (0, "a b;c\n".into()));
        f.outputs.insert(format!("sudo -n {CHECKSERVICES}"), (0, "systemctl restart 'cups.service'\n".into()));
        f.answers = vec![true; 8];
        paku(&mut f, &["maintain"]);
        assert!(f.ran.contains(&"sudo systemctl restart -- cups.service".to_string()));
        assert!(!f.out.contains("not restarted here"), "{}", f.out);
    }

    #[test]
    fn all_services_kept_gives_no_none_line() {
        let mut f = busy(ALL);
        f.outputs.insert(format!("sudo -n {CHECKSERVICES}"), (0, "systemctl restart 'greetd.service'\n".into()));
        f.answers = vec![true; 8];
        assert_eq!(paku(&mut f, &["maintain"]), 0);
        let services = f.out.split(":: Services to restart").nth(1).unwrap().split("::").next().unwrap();
        assert!(services.contains("not restarted here"), "{services}");
        assert!(!services.contains("none"), "{services}");
    }

    #[test]
    fn sudo_asks_again_before_checking_the_services() {
        // A long question can outlast sudo's timestamp: sudo -v right before the sudo -n
        let mut f = busy(ALL);
        f.answers = vec![true; 8];
        assert_eq!(paku(&mut f, &["maintain"]), 0);
        let at = |line: &str| f.ran.iter().position(|l| l == line).unwrap_or_else(|| panic!("{line}: {:?}", f.ran));
        assert_eq!(at("sudo -v") + 1, at(&format!("sudo -n {CHECKSERVICES}")));
        // A failed or cancelled sudo -v: the message, and the flow goes on
        let mut f = busy(ALL);
        f.codes.insert("sudo -v".into(), 1);
        f.outputs.remove(&format!("sudo -n {CHECKSERVICES}"));
        f.outputs.insert(format!("sudo -n {CHECKSERVICES}"), (1, String::new()));
        f.answers = vec![true; 8];
        assert_eq!(paku(&mut f, &["maintain"]), 0);
        assert!(f.out.contains("couldn't check them (sudo asked for the password again)"), "{}", f.out);
        // Ctrl-C at sudo's password prompt stops the flow
        let mut f = busy(ALL);
        f.interrupt_on = Some("sudo -v");
        f.answers = vec![true; 8];
        assert_eq!(paku(&mut f, &["maintain"]), 130);
        assert!(f.ran.iter().all(|l| !l.contains("checkservices")), "{:?}", f.ran);
        // Not when listing (it changes nothing) nor as root (no sudo)
        for (args, root) in [(&["maintain", "--dry-run"][..], false), (&["maintain"][..], true)] {
            let mut f = busy(ALL);
            f.root = root;
            f.answers = vec![true; 8];
            paku(&mut f, args);
            assert!(!f.ran.contains(&"sudo -v".to_string()), "{args:?} {root}: {:?}", f.ran);
        }
    }

    #[test]
    fn every_step_with_yes() {
        let mut f = busy(ALL);
        // upgrade, orphans, flatpak, cache, paru -Sc, pacdiff, services, reboot
        f.answers = vec![true; 8];
        assert_eq!(paku(&mut f, &["maintain"]), 0);
        let ran: Vec<&str> = f.ran.iter().map(String::as_str).filter(|l| !l.starts_with("checkupdates")).collect();
        assert!(ran.contains(&"paru -Syu"));
        assert!(ran.contains(&"flatpak update"));
        assert!(ran.contains(&"sudo pacman -Rns -- orphan1 orphan2"));
        assert!(ran.contains(&"flatpak uninstall --unused"));
        assert!(ran.contains(&"sudo paccache -rk3"));
        assert!(!ran.contains(&"sudo paccache -ruk0"), "nothing uninstalled to prune");
        assert!(ran.contains(&"paru -Sc"));
        assert!(ran.contains(&"pacdiff --sudo"));
        assert!(ran.contains(&"sudo systemctl restart -- cups.service"));
        assert!(ran.contains(&"systemctl reboot"));
        // The order of the steps
        let pos = |l: &str| ran.iter().position(|r| *r == l).unwrap();
        assert!(pos("paru -Syu") < pos("flatpak update"));
        assert!(pos("flatpak update") < pos("sudo pacman -Rns -- orphan1 orphan2"));
        assert!(pos("sudo paccache -rk3") < pos("pacdiff --sudo"));
        assert!(pos("pacdiff --sudo") < pos("sudo systemctl restart -- cups.service"));
        assert!(pos("sudo systemctl restart -- cups.service") < pos("systemctl reboot"));
        // Only the unseen news, saved after the upgrade
        assert!(f.out.contains("New one") && !f.out.contains("Seen one"));
        assert_eq!(f.saved_news.as_deref(), Some("https://archlinux.org/news/new/"));
        // Information only
        assert!(f.out.contains("foo.service (system): see systemctl status foo.service"));
        assert!(f.out.contains("takes 1.5G"));
        assert!(f.out.contains(
            "not restarted here (they'd end the session or the network): dbus-broker.service NetworkManager.service \
             getty@tty2.service"
        ));
        // Rebooted for the kernel: only the services kept running are left to say
        assert!(
            f.out.ends_with("paku maintain: done; reboot recommended (services kept running on old files)\n"),
            "{}",
            f.out
        );
        assert!(f.ran.iter().all(|l| !l.contains("--noconfirm")));
    }

    #[test]
    fn every_step_with_no_changes_nothing() {
        let mut f = busy(ALL);
        f.answers = vec![false; 8];
        assert_eq!(paku(&mut f, &["maintain"]), 0);
        let writes = ["-Syu", "flatpak update", "-Rns", "paccache -r", "-Sc", "pacdiff --sudo", "restart", "reboot"];
        assert!(f.ran.iter().all(|l| !writes.iter().any(|w| l.contains(w))), "{:?}", f.ran);
        // Declining the upgrade skips the services and keeps the news unseen
        assert!(f.ran.iter().all(|l| !l.contains("checkservices")));
        assert_eq!(f.saved_news, None);
        assert_eq!(f.asked.len(), 7, "{:?}", f.asked);
    }

    #[test]
    fn every_question_defaults_to_no_except_the_upgrade() {
        let mut f = busy(ALL);
        f.answers = vec![true; 8];
        paku(&mut f, &["maintain"]);
        assert_eq!(f.asked.len(), 8);
        assert!(f.defaults[0], "{}", f.asked[0]);
        assert_eq!(f.asked[0], "Proceed with the upgrade?");
        assert!(f.defaults[1..].iter().all(|d| !d), "{:?}", f.asked);
    }

    #[test]
    fn dry_run_and_no_terminal_only_list() {
        for (args, tty) in [(&["maintain", "--dry-run"][..], true), (&["maintain"][..], false)] {
            let mut f = busy(ALL);
            f.tty = tty;
            assert_eq!(paku(&mut f, args), 0);
            assert!(f.asked.is_empty(), "{:?}", f.asked);
            let writes =
                ["-Syu", "flatpak update", "-Rns", "paccache -r", "-Sc", "pacdiff --sudo", "restart", "reboot"];
            assert!(f.ran.iter().all(|l| !writes.iter().any(|w| l.contains(w))), "{:?}", f.ran);
            assert!(f.out.contains("would run: paru -Syu"));
            assert!(f.out.contains("would run: sudo pacman -Rns -- orphan1 orphan2"));
            assert!(f.out.contains("would run: pacdiff --sudo"));
            assert!(f.out.contains("would run: flatpak uninstall --unused"));
            assert!(
                f.out.ends_with(
                    "paku maintain: done (listing only, nothing was changed); reboot recommended (a new kernel)\n"
                ),
                "{}",
                f.out
            );
            assert_eq!(f.saved_news, None);
            assert_eq!(f.out.contains("no terminal"), !tty);
        }
    }

    #[test]
    fn without_a_seat_the_reboot_goes_through_sudo() {
        // Over ssh (or with no session at all) polkit would ask with no agent and time out
        for seat in [Some(""), None] {
            let mut f = busy(ALL);
            match seat {
                Some(s) => f.outputs.insert(SEAT.into(), (0, format!("{s}\n"))),
                None => f.outputs.remove(SEAT),
            };
            f.answers = vec![true; 8];
            paku(&mut f, &["maintain"]);
            assert!(f.ran.contains(&"sudo systemctl reboot".to_string()), "{seat:?}: {:?}", f.ran);
            assert!(!f.ran.contains(&"systemctl reboot".to_string()));
        }
        // On a local seat, as the user (logind allows it without a password)
        let mut f = busy(ALL);
        f.answers = vec![true; 8];
        paku(&mut f, &["maintain"]);
        assert!(f.ran.contains(&"systemctl reboot".to_string()), "{:?}", f.ran);
        assert!(!f.ran.contains(&"sudo systemctl reboot".to_string()));
    }

    #[test]
    fn a_listing_names_the_services_check_when_the_upgrade_would_run() {
        let mut f = busy(ALL);
        assert_eq!(paku(&mut f, &["maintain", "--dry-run"]), 0);
        assert!(f.out.contains(":: Services to restart\n  would run: sudo checkservices -F -P -R\n"), "{}", f.out);
        // Only listed: checkservices itself doesn't run before the upgrade
        assert!(f.ran.iter().all(|l| !l.contains("checkservices")), "{:?}", f.ran);
        // Without checkservices, the line that says which package to install
        let mut f = busy(&ALL.iter().copied().filter(|p| *p != "checkservices").collect::<Vec<_>>());
        paku(&mut f, &["maintain", "--dry-run"]);
        assert!(f.out.contains("skipped: checkservices not found (paku depends on archlinux-contrib"), "{}", f.out);
        // Nothing to upgrade: no services step, as in a real run
        let mut f = busy(&ALL.iter().copied().filter(|p| *p != "flatpak").collect::<Vec<_>>());
        f.outputs.insert("checkupdates".into(), (2, String::new()));
        f.outputs.insert("paru -Qua".into(), (1, String::new()));
        paku(&mut f, &["maintain", "--dry-run"]);
        assert!(f.out.contains("nothing to upgrade"), "{}", f.out);
        assert!(!f.out.contains("Services to restart"), "{}", f.out);
    }

    #[test]
    fn missing_tools_skip_their_step() {
        let mut f = busy(&[]);
        // Only the upgrade (pacman), orphans and reboot remain
        f.outputs.insert("pacman -Qu".into(), (0, "linux 6.17.1 -> 6.17.2\n".into()));
        f.answers = vec![true, true, false];
        assert_eq!(paku(&mut f, &["maintain"]), 0);
        assert!(f.ran.contains(&"sudo pacman -Syu".to_string()));
        // They're dependencies of the package: missing anyway, each step says which package to install
        for (tool, package) in [
            ("curl", "curl"),
            ("paccache", "pacman-contrib"),
            ("pacdiff", "pacman-contrib"),
            ("checkservices", "archlinux-contrib"),
        ] {
            let note =
                format!("skipped: {tool} not found (paku depends on {package}; install it: sudo pacman -S {package})");
            assert!(f.out.contains(&note), "{note}\n{}", f.out);
        }
        assert!(!f.out.contains("Unused Flatpak"));
        assert!(f.ran.iter().all(|l| !l.starts_with("flatpak") && !l.starts_with("curl")));
    }

    #[test]
    fn as_root_there_is_no_sudo() {
        let mut f = busy(&["checkupdates", "fakeroot", "pacdiff", "paccache", "checkservices"]);
        f.root = true;
        f.outputs.insert(CHECKSERVICES.into(), (0, "systemctl restart 'cups.service'\n".into()));
        f.answers = vec![true; 6];
        assert_eq!(paku(&mut f, &["maintain"]), 0);
        assert!(f.ran.contains(&"pacman -Syu".to_string()));
        assert!(f.ran.contains(&"pacman -Rns -- orphan1 orphan2".to_string()));
        assert!(f.ran.contains(&"paccache -rk3".to_string()));
        assert!(f.ran.contains(&"pacdiff".to_string()));
        assert!(f.ran.contains(&"systemctl restart -- cups.service".to_string()));
        assert!(f.ran.iter().all(|l| !l.starts_with("sudo")), "{:?}", f.ran);
    }

    #[test]
    fn a_failed_step_doesnt_stop_the_others() {
        let mut f = busy(ALL);
        f.codes.insert("sudo pacman -Rns -- orphan1 orphan2".into(), 1);
        f.codes.insert("pacdiff --sudo".into(), 1);
        f.answers = vec![true; 8];
        assert_eq!(paku(&mut f, &["maintain"]), 2);
        assert!(f.ran.contains(&"systemctl reboot".to_string()));
        assert!(
            f.out.ends_with(
                "paku maintain: done, 2 steps failed (orphans, pacnew); reboot recommended (services kept running \
                 on old files)\n"
            ),
            "{}",
            f.out
        );
    }

    #[test]
    fn a_failed_upgrade_leaves_only_the_information_steps() {
        let mut f = busy(ALL);
        f.codes.insert("paru -Syu".into(), 1);
        f.answers = vec![true];
        assert_eq!(paku(&mut f, &["maintain"]), 2);
        assert_eq!(f.asked.len(), 1, "{:?}", f.asked);
        for step in ["flatpak", "pacman -Qdtq", "paccache", "pacdiff", "checkservices", "uname"] {
            assert!(f.ran.iter().all(|l| !l.contains(step)), "{step}: {:?}", f.ran);
        }
        assert!(f.out.contains("skipped: the upgrade failed"));
        assert!(f.out.contains("foo.service (system)"));
        assert!(f.out.contains("takes 1.5G"));
        // No services nor kernel check after a failed upgrade
        assert!(f.out.ends_with("paku maintain: done, 1 step failed (upgrade)\n"), "{}", f.out);
        assert_eq!(f.saved_news, None);
    }

    #[test]
    fn ctrl_c_stops_the_flow() {
        let mut f = busy(ALL);
        f.interrupt_on = Some("paru -Syu");
        f.answers = vec![true];
        assert_eq!(paku(&mut f, &["maintain"]), 130);
        assert_eq!(f.ran.last().map(String::as_str), Some("paru -Syu"));
        assert!(f.out.ends_with("paku maintain: stopped (Ctrl-C)\n"));
        // At a question too
        let mut f = busy(ALL);
        f.interrupt_on = Some("Remove them? Their configuration");
        f.answers = vec![true];
        assert_eq!(paku(&mut f, &["maintain"]), 130);
        assert!(f.ran.iter().all(|l| !l.contains("-Rns") && !l.contains("paccache")));
        // A failure that comes from a signal isn't "failed"
        let mut f = busy(ALL);
        f.codes.insert("paru -Syu".into(), 130);
        f.answers = vec![true];
        assert_eq!(paku(&mut f, &["maintain"]), 130);
    }

    #[test]
    fn another_run_holds_the_lock() {
        let mut f = busy(ALL);
        f.lock_held = true;
        assert_eq!(paku(&mut f, &["maintain"]), 17);
        assert!(f.ran.is_empty());
        assert!(f.err.contains("another paku maintain is already running"));
        // Listing doesn't take it
        let mut f = busy(ALL);
        f.lock_held = true;
        assert_eq!(paku(&mut f, &["maintain", "--dry-run"]), 0);
    }

    #[test]
    fn options() {
        let mut f = busy(ALL);
        f.answers = vec![true, false, false, false, false, false, false, false];
        paku(&mut f, &["maintain", "--devel", "--no-flatpak"]);
        assert!(f.ran.contains(&"paru -Syu --devel".to_string()));
        assert!(f.ran.iter().all(|l| !l.starts_with("flatpak")), "{:?}", f.ran);
        let mut f = busy(ALL);
        assert_eq!(paku(&mut f, &["maintain", "--news"]), 1);
        assert_eq!(f.out, "paku maintain [--dry-run] [--no-flatpak] [--devel]\n");
        assert!(f.ran.is_empty());
    }

    #[test]
    fn nothing_to_do() {
        let mut f = with(&["checkupdates"]);
        f.tty = true;
        f.outputs.insert("checkupdates".into(), (2, String::new()));
        f.outputs.insert("uname -r".into(), (0, "6.17.1-arch1-1\n".into()));
        f.files.push("/usr/lib/modules/6.17.1-arch1-1/vmlinuz".into());
        assert_eq!(paku(&mut f, &["maintain"]), 0);
        assert!(f.asked.is_empty(), "{:?}", f.asked);
        assert!(f.out.contains("none, the packages are up to date"));
        assert!(!f.out.contains("Kernel"));
        assert!(f.ran.iter().all(|l| !l.contains("-Syu")));
    }

    #[test]
    fn no_reboot_question_in_a_container() {
        let mut f = busy(ALL);
        f.outputs.insert("systemd-detect-virt --container --quiet".into(), (0, String::new()));
        f.answers = vec![true; 7];
        assert_eq!(paku(&mut f, &["maintain"]), 0);
        assert!(!f.ran.contains(&"systemctl reboot".to_string()));
    }

    #[test]
    fn one_reboot_line_for_the_kernel_and_the_services() {
        let mut f = busy(ALL);
        // Every question yes, but no to the reboot
        f.answers = vec![true, true, true, true, true, true, true, false];
        assert_eq!(paku(&mut f, &["maintain"]), 0);
        assert!(
            f.out.ends_with(
                "paku maintain: done; reboot recommended (services kept running on old files, a new kernel)\n"
            ),
            "{}",
            f.out
        );
        assert_eq!(f.out.matches("reboot recommended").count(), 1);
        // Nothing kept and the kernel up to date: no line
        let mut f = busy(ALL);
        f.outputs.insert(format!("sudo -n {CHECKSERVICES}"), (0, "systemctl restart 'cups.service'\n".into()));
        f.files.push("/usr/lib/modules/6.17.1-arch1-1/vmlinuz".into());
        f.answers = vec![true; 7];
        assert_eq!(paku(&mut f, &["maintain"]), 0);
        assert!(f.out.ends_with("paku maintain: done\n"), "{}", f.out);
        // A no at "Restart them all?", or a restart that fails, leaves them on the old files too
        for (answer, code) in [(false, 0), (true, 1)] {
            let mut f = busy(ALL);
            f.outputs.insert(format!("sudo -n {CHECKSERVICES}"), (0, "systemctl restart 'cups.service'\n".into()));
            f.files.push("/usr/lib/modules/6.17.1-arch1-1/vmlinuz".into());
            f.codes.insert("sudo systemctl restart -- cups.service".into(), code);
            f.answers = vec![true, true, true, true, true, true, answer];
            paku(&mut f, &["maintain"]);
            assert!(f.out.trim_end().ends_with("reboot recommended (services kept running on old files)"), "{}", f.out);
            assert_eq!(f.out.matches("reboot recommended").count(), 1);
        }
    }

    #[test]
    fn sizes() {
        assert_eq!(super::bytes("1.5G"), Some(1.5 * 1024.0 * 1024.0 * 1024.0));
        assert_eq!(super::bytes("512.0M"), Some(512.0 * 1024.0 * 1024.0));
        assert_eq!(super::bytes("123B"), Some(123.0));
        assert_eq!(super::bytes("x"), None);
    }
}
