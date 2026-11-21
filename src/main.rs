//! paku: a smaller, friendlier surface over paru and pacman. Every command runs as an argument vector,
//! never through a shell.
mod audit;
mod commands;
mod completions;
mod paku;

use std::ffi::OsString;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

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

/// What SIGINT and SIGQUIT did when paku started: the children get it back (a paku started with them ignored,
/// in the background of a script, keeps them ignored for its children too).
static INT_BEFORE: AtomicUsize = AtomicUsize::new(libc::SIG_DFL);
static QUIT_BEFORE: AtomicUsize = AtomicUsize::new(libc::SIG_DFL);
/// The signal (SIGINT or SIGQUIT) that killed the last child, or 0.
static KILLED_BY: AtomicI32 = AtomicI32::new(0);

/// Whether the terminal sent paku a SIGINT or SIGQUIT.
static GOT_SIGNAL: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: i32) {
    // An atomic store is async-signal-safe
    GOT_SIGNAL.store(true, Ordering::Relaxed);
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

/// The code a shell would give: the exit code, or 128 + the signal that killed the child. A child killed by
/// SIGINT or SIGQUIT is remembered, for paku to end the same way.
fn exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(sig @ (libc::SIGINT | libc::SIGQUIT)) = status.signal() {
        KILLED_BY.store(sig, Ordering::Relaxed);
    }
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

    fn interrupted(&self) -> bool {
        GOT_SIGNAL.load(Ordering::Relaxed)
    }

    fn out(&mut self, text: &str) {
        // A closed pipe (`paku __complete repo | head`) isn't an error worth a panic
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
    // and paku waits for them instead of leaving them running behind the prompt, noting that it came
    INT_BEFORE.store(catch(libc::SIGINT), Ordering::Relaxed);
    QUIT_BEFORE.store(catch(libc::SIGQUIT), Ordering::Relaxed);
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let code = paku::run(&args, &mut System);
    let _ = std::io::stdout().flush();
    // A child killed by Ctrl-C: paku dies of SIGINT too, so a shell loop around it stops (bash only stops a
    // loop when the command itself was killed by SIGINT, not on exit code 130). Not for SIGQUIT: its default
    // action dumps core, and paku would show up as a crash next to the child; it just exits 131
    if code == 130
        && KILLED_BY.load(Ordering::Relaxed) == libc::SIGINT
        && INT_BEFORE.load(Ordering::Relaxed) == libc::SIG_DFL
    {
        // SAFETY: back to the default action, then the signal to paku itself
        unsafe {
            libc::signal(libc::SIGINT, libc::SIG_DFL);
            libc::kill(libc::getpid(), libc::SIGINT);
        }
    }
    std::process::exit(code);
}
