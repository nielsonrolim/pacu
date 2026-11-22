//! paku: a smaller, friendlier surface over an AUR helper (paru, yay or pikaur) and pacman. Every command runs as an argument vector,
//! never through a shell.
mod audit;
mod commands;
mod completions;
mod helper;
mod maintain;
mod news;
mod paku;

use std::ffi::OsString;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

mod libc {
    pub const SIGINT: i32 = 2;
    pub const SIGQUIT: i32 = 3;
    pub const SIG_DFL: usize = 0;
    pub const SIG_IGN: usize = 1;
    pub const SIG_ERR: usize = usize::MAX;
    extern "C" {
        pub fn signal(signum: i32, handler: usize) -> usize;
        pub fn geteuid() -> u32;
        pub fn getpid() -> i32;
        pub fn kill(pid: i32, sig: i32) -> i32;
        pub fn write(fd: i32, buf: *const std::ffi::c_void, count: usize) -> isize;
        pub fn isatty(fd: i32) -> i32;
        pub fn flock(fd: i32, operation: i32) -> i32;
    }
    pub const LOCK_EX: i32 = 2;
    pub const LOCK_NB: i32 = 4;
}

/// What SIGINT and SIGQUIT did when paku started: the children get it back (a paku started with them ignored,
/// in the background of a script, keeps them ignored for its children too).
static INT_BEFORE: AtomicUsize = AtomicUsize::new(libc::SIG_DFL);
static QUIT_BEFORE: AtomicUsize = AtomicUsize::new(libc::SIG_DFL);

/// The SIGINT or SIGQUIT the terminal sent paku (the last one), or 0.
static GOT_SIGNAL: AtomicI32 = AtomicI32::new(0);

extern "C" fn on_signal(sig: i32) {
    // An atomic store is async-signal-safe
    GOT_SIGNAL.store(sig, Ordering::Relaxed);
}

/// What a Ctrl-C at a question prints before paku dies of it: the cursor shown again (a child's progress bar may
/// have hidden it; the line discipline's echo is never turned off by paku), then the same line as a Ctrl-C
/// during a step.
const STOPPED_AT_QUESTION: &[u8] = b"\x1b[?25h\npaku maintain: stopped (Ctrl-C)\n";

/// SIGINT during a question (no child runs): the line, then paku dies of the signal itself, as during a step,
/// so a shell around it sees a command killed by SIGINT. Only async-signal-safe calls: write, signal, kill
/// (SIGINT is blocked while the handler runs, so the kill takes effect when it returns, with the default action).
extern "C" fn on_signal_at_question(_sig: i32) {
    // SAFETY: write, signal, getpid and kill are async-signal-safe; the buffer is a static
    unsafe {
        libc::write(1, STOPPED_AT_QUESTION.as_ptr().cast(), STOPPED_AT_QUESTION.len());
        libc::signal(libc::SIGINT, libc::SIG_DFL);
        libc::kill(libc::getpid(), libc::SIGINT);
    }
}

/// Catches a signal (only noting it, so paku waits for its child) and gives what it did before (SIG_DFL if
/// that can't be read). One that was ignored stays ignored. A caught signal goes back to the default at
/// exec, so the child gets it as before paku
fn catch(sig: i32) -> usize {
    // SAFETY: called from main before any thread starts; the handler only stores an atomic
    unsafe {
        let before = libc::signal(sig, libc::SIG_IGN);
        if before == libc::SIG_ERR {
            return libc::SIG_DFL;
        }
        if before == libc::SIG_DFL {
            libc::signal(sig, on_signal as extern "C" fn(i32) as usize);
        }
        before
    }
}

/// The real system.
struct System;

impl System {
    fn command(argv: &[OsString]) -> Command {
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        // paku notes Ctrl-C and Ctrl-\ while a child runs (see main); the child gets back what they did before
        let (int, quit) = (INT_BEFORE.load(Ordering::Relaxed), QUIT_BEFORE.load(Ordering::Relaxed));
        // SAFETY: signal() is async-signal-safe, the only call made between fork and exec
        unsafe {
            command.pre_exec(move || {
                libc::signal(libc::SIGINT, int);
                libc::signal(libc::SIGQUIT, quit);
                Ok(())
            });
        }
        command
    }

    fn report_spawn_error(argv: &[OsString], e: &std::io::Error) -> i32 {
        eprintln!("paku: couldn't run {}: {e}", argv[0].to_string_lossy());
        if e.kind() == std::io::ErrorKind::NotFound {
            127
        } else {
            126
        }
    }
}

/// The code a shell would give: the exit code, or 128 + the signal that killed the child.
fn exit_code(status: std::process::ExitStatus) -> i32 {
    status.code().or_else(|| status.signal().map(|s| 128 + s)).unwrap_or(1)
}

impl paku::Sys for System {
    fn run(&mut self, argv: &[OsString]) -> i32 {
        match Self::command(argv).status() {
            Ok(status) => exit_code(status),
            Err(e) => Self::report_spawn_error(argv, &e),
        }
    }

    fn capture(&mut self, argv: &[OsString]) -> Option<(i32, Vec<u8>)> {
        let output = Self::command(argv).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
        Some((exit_code(output.status), output.stdout))
    }

    fn has(&self, program: &str) -> bool {
        let Some(path) = std::env::var_os("PATH") else { return false };
        std::env::split_paths(&path).any(|dir| is_executable(&dir.join(program)))
    }

    fn is_root(&self) -> bool {
        // SAFETY: geteuid has no preconditions and can't fail
        unsafe { libc::geteuid() == 0 }
    }

    fn helper_setting(&self) -> Option<(String, String)> {
        if let Some(value) = std::env::var_os(helper::ENV) {
            return Some((value.to_string_lossy().trim().to_string(), helper::ENV.to_string()));
        }
        let path = helper::config_path(std::env::var_os("XDG_CONFIG_HOME"), std::env::var_os("HOME"))?;
        // A missing or unreadable file is auto
        let text = std::fs::read_to_string(&path).ok()?;
        Some((helper::parse_config(&text)?, path.display().to_string()))
    }

    fn interrupted(&self) -> bool {
        GOT_SIGNAL.load(Ordering::Relaxed) != 0
    }

    fn out(&mut self, text: &str) {
        // A closed pipe (`paku __complete repo | head`) isn't an error worth a panic
        let _ = std::io::stdout().write_all(text.as_bytes());
    }

    fn err(&mut self, text: &str) {
        let _ = writeln!(std::io::stderr(), "{text}");
    }

    fn tty(&self) -> bool {
        // SAFETY: isatty only looks at the descriptor
        unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
    }

    fn ask(&mut self, question: &str, default: bool) -> bool {
        // No child runs during a question: Ctrl-C ends paku right there instead of waiting for Enter, with the
        // "stopped" line and then the signal itself (a shell around it sees it). A SIGINT paku inherited as
        // ignored (the background of a script) stays ignored
        let restore = INT_BEFORE.load(Ordering::Relaxed) == libc::SIG_DFL;
        if restore {
            // SAFETY: no other thread; the handler only makes async-signal-safe calls
            unsafe { libc::signal(libc::SIGINT, on_signal_at_question as extern "C" fn(i32) as usize) };
        }
        let answer = loop {
            print!("{question} {} ", if default { "[Y/n]" } else { "[y/N]" });
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            match std::io::stdin().read_line(&mut line) {
                // End of input: no
                Ok(0) | Err(_) => {
                    println!();
                    break false;
                }
                Ok(_) => match line.trim().to_lowercase().as_str() {
                    "" => break default,
                    "y" | "yes" => break true,
                    "n" | "no" => break false,
                    _ => println!("Please answer y or n."),
                },
            }
        };
        if restore {
            // SAFETY: as in catch
            unsafe { libc::signal(libc::SIGINT, on_signal as extern "C" fn(i32) as usize) };
        }
        answer
    }

    fn lock(&mut self) -> bool {
        // $XDG_RUNTIME_DIR, else /run/user/UID (under sudo or su without the variable), else ~/.cache/paku.
        // As root, /run: never a file in a directory a user owns (a planted symlink), even under sudo -E.
        // pacman's own lock still guards its database across users
        // SAFETY: geteuid has no preconditions
        let uid = unsafe { libc::geteuid() };
        let dir = (uid == 0)
            .then(|| OsString::from("/run"))
            .or_else(|| std::env::var_os("XDG_RUNTIME_DIR"))
            .map(std::path::PathBuf::from)
            .filter(|d| d.is_absolute() && d.is_dir())
            .or_else(|| Some(std::path::PathBuf::from(format!("/run/user/{uid}"))).filter(|d| d.is_dir()))
            .or_else(|| {
                let dir = std::path::PathBuf::from(std::env::var_os("HOME")?).join(".cache/paku");
                std::fs::create_dir_all(&dir).ok()?;
                Some(dir)
            });
        // Without a place for it, the run goes on unlocked
        let Some(dir) = dir else { return true };
        let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("paku-maintain.lock"))
        else {
            return true;
        };
        // SAFETY: a valid descriptor; the lock lasts as long as the file stays open, so it's kept until exit
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        if locked {
            std::mem::forget(file);
        }
        locked
    }

    fn last_news(&self) -> Option<String> {
        let text = std::fs::read_to_string(news_state()?).ok()?;
        Some(text.trim().to_string()).filter(|t| !t.is_empty())
    }

    fn save_last_news(&mut self, id: &str) {
        // Best effort: without it, the same news show again next time
        if let Some(path) = news_state() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, format!("{id}\n"));
        }
    }

    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }
}

/// Where `maintain` keeps the newest news item it showed: $XDG_STATE_HOME/paku/last-news (~/.local/state);
/// as root, /var/lib/paku/last-news (never a root-owned file in a user's home, even under sudo -E).
fn news_state() -> Option<std::path::PathBuf> {
    // SAFETY: geteuid has no preconditions
    if unsafe { libc::geteuid() } == 0 {
        return Some(std::path::PathBuf::from("/var/lib/paku/last-news"));
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .filter(|d| d.is_absolute())
        .or_else(|| Some(std::path::PathBuf::from(std::env::var_os("HOME")?).join(".local/state")))?;
    Some(base.join("paku/last-news"))
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

fn main() {
    // The terminal sends Ctrl-C to the whole foreground group: paru or pacman decide what to do with it,
    // and paku waits for them instead of leaving them running behind the prompt, noting that it came
    INT_BEFORE.store(catch(libc::SIGINT), Ordering::Relaxed);
    QUIT_BEFORE.store(catch(libc::SIGQUIT), Ordering::Relaxed);
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let code = paku::run(&args, &mut System);
    let _ = std::io::stdout().flush();
    // Stopped by Ctrl-C (paku got it, or the code is 130: a child killed by SIGINT, or sudo when pacman, in
    // sudo's own pty, died of a Ctrl-C paku never saw): paku dies of SIGINT too, so a shell loop around it
    // stops. bash only stops a loop when the command itself was killed by SIGINT, not on an exit code. Not for
    // SIGQUIT: its default action dumps core, and paku would show up as a crash next to the child; it just
    // exits with the code. paru exits 1 on a Ctrl-C under sudo's pty, which no one can tell from a failure
    let got_int = GOT_SIGNAL.load(Ordering::Relaxed) == libc::SIGINT;
    if (got_int || code == 130) && INT_BEFORE.load(Ordering::Relaxed) == libc::SIG_DFL {
        // SAFETY: isatty only looks at the descriptor; then back to the default action, and the signal to paku
        // itself. The cursor first: a child stopped in its progress bar may have left it hidden (pacman shows it
        // again itself; a helper may not)
        unsafe {
            if libc::isatty(1) == 1 {
                let show = b"\x1b[?25h";
                libc::write(1, show.as_ptr().cast(), show.len());
            }
            libc::signal(libc::SIGINT, libc::SIG_DFL);
            libc::kill(libc::getpid(), libc::SIGINT);
        }
    }
    std::process::exit(code);
}
