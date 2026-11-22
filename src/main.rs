//! pacu: a smaller, friendlier surface over an AUR helper (paru, yay or pikaur) and pacman. Every command runs as an argument vector,
//! never through a shell.
mod audit;
mod commands;
mod completions;
mod helper;
mod pacu;

use std::ffi::OsString;
use std::io::Write;
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
    }
}

/// What SIGINT and SIGQUIT did when pacu started: the children get it back (a pacu started with them ignored,
/// in the background of a script, keeps them ignored for its children too).
static INT_BEFORE: AtomicUsize = AtomicUsize::new(libc::SIG_DFL);
static QUIT_BEFORE: AtomicUsize = AtomicUsize::new(libc::SIG_DFL);

/// The SIGINT or SIGQUIT the terminal sent pacu (the last one), or 0.
static GOT_SIGNAL: AtomicI32 = AtomicI32::new(0);

extern "C" fn on_signal(sig: i32) {
    // An atomic store is async-signal-safe
    GOT_SIGNAL.store(sig, Ordering::Relaxed);
}

/// Catches a signal (only noting it, so pacu waits for its child) and gives what it did before (SIG_DFL if
/// that can't be read). One that was ignored stays ignored. A caught signal goes back to the default at
/// exec, so the child gets it as before pacu
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
        // pacu notes Ctrl-C and Ctrl-\ while a child runs (see main); the child gets back what they did before
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
        eprintln!("pacu: couldn't run {}: {e}", argv[0].to_string_lossy());
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

impl pacu::Sys for System {
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
        // A closed pipe (`pacu __complete repo | head`) isn't an error worth a panic
        let _ = std::io::stdout().write_all(text.as_bytes());
    }

    fn err(&mut self, text: &str) {
        let _ = writeln!(std::io::stderr(), "{text}");
    }
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

fn main() {
    // The terminal sends Ctrl-C to the whole foreground group: paru or pacman decide what to do with it,
    // and pacu waits for them instead of leaving them running behind the prompt, noting that it came
    INT_BEFORE.store(catch(libc::SIGINT), Ordering::Relaxed);
    QUIT_BEFORE.store(catch(libc::SIGQUIT), Ordering::Relaxed);
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let code = pacu::run(&args, &mut System);
    let _ = std::io::stdout().flush();
    // Stopped by Ctrl-C (pacu got it, or the code is 130: a child killed by SIGINT, or sudo when pacman, in
    // sudo's own pty, died of a Ctrl-C pacu never saw): pacu dies of SIGINT too, so a shell loop around it
    // stops. bash only stops a loop when the command itself was killed by SIGINT, not on an exit code. Not for
    // SIGQUIT: its default action dumps core, and pacu would show up as a crash next to the child; it just
    // exits with the code. paru exits 1 on a Ctrl-C under sudo's pty, which no one can tell from a failure
    let got_int = GOT_SIGNAL.load(Ordering::Relaxed) == libc::SIGINT;
    if (got_int || code == 130) && INT_BEFORE.load(Ordering::Relaxed) == libc::SIG_DFL {
        // SAFETY: back to the default action, then the signal to pacu itself
        unsafe {
            libc::signal(libc::SIGINT, libc::SIG_DFL);
            libc::kill(libc::getpid(), libc::SIGINT);
        }
    }
    std::process::exit(code);
}
