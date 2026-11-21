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

mod libc {
    pub const SIGINT: i32 = 2;
    pub const SIGQUIT: i32 = 3;
    pub const SIG_DFL: usize = 0;
    pub const SIG_IGN: usize = 1;
    extern "C" {
        pub fn signal(signum: i32, handler: usize) -> usize;
        pub fn geteuid() -> u32;
    }
}

/// The real system.
struct System;

impl System {
    fn command(argv: &[OsString]) -> Command {
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        // paku ignores Ctrl-C and Ctrl-\ while a child runs (see main); the child gets them back
        // SAFETY: signal() is async-signal-safe, the only call made between fork and exec
        unsafe {
            command.pre_exec(|| {
                libc::signal(libc::SIGINT, libc::SIG_DFL);
                libc::signal(libc::SIGQUIT, libc::SIG_DFL);
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
    // and paku waits for them instead of leaving them running behind the prompt
    // SAFETY: setting a signal to SIG_IGN before any thread starts
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGQUIT, libc::SIG_IGN);
    }
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let code = paku::run(&args, &mut System);
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}
