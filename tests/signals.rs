//! Ctrl-C in `paku maintain`, with the real binary in a pseudo-terminal and fakes of pacman and sudo: at a
//! question and during the upgrade, paku prints "stopped (Ctrl-C)" and then dies of SIGINT itself, so a shell
//! around it stops too (and sees 130 in `$?`).

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

extern "C" {
    fn openpty(master: *mut i32, slave: *mut i32, name: *mut u8, termp: *const u8, winp: *const u8) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
}

const SIGINT: i32 = 2;

struct Run {
    child: Child,
    master: File,
    output: Arc<Mutex<Vec<u8>>>,
    _dir: TempDir,
}

/// A folder removed at the end, under the target dir (never /tmp).
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// `paku maintain --no-flatpak` with one pending package; the fake `sudo pacman -Syu` prints "transferring"
/// and waits, exiting 130 on SIGINT like pacman.
fn start(name: &str) -> Run {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("signals-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    for d in [&bin, &dir.join("run"), &dir.join("state"), &dir.join("home")] {
        fs::create_dir_all(d).unwrap();
    }
    script(&bin, "pacman", "[ \"$1\" = -Qu ] && echo 'foo 1-1 -> 2-1'\nexit 0\n");
    script(
        &bin,
        "sudo",
        "if [ \"$2\" = -Syu ]; then\n  trap 'exit 130' INT\n  echo transferring\n  /bin/sleep 30 &\n  wait\nfi\nexit 0\n",
    );
    let (mut m, mut s) = (0, 0);
    // SAFETY: openpty fills the two descriptors; null name, termios and winsize are allowed
    assert_eq!(unsafe { openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) }, 0);
    // SAFETY: both descriptors were just opened and are owned here
    let (master, slave) = unsafe { (File::from_raw_fd(m), File::from_raw_fd(s)) };
    let child = Command::new(env!("CARGO_BIN_EXE_paku"))
        .args(["maintain", "--no-flatpak"])
        .env_clear()
        .env("PATH", &bin)
        .env("HOME", dir.join("home"))
        .env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave.try_clone().unwrap()))
        .stderr(Stdio::from(slave))
        // Its own group, so the test sends Ctrl-C to the group as the terminal would
        .process_group(0)
        .spawn()
        .unwrap();
    let output = Arc::new(Mutex::new(Vec::new()));
    let (sink, mut reader) = (output.clone(), master.try_clone().unwrap());
    std::thread::spawn(move || {
        let mut buf = [0; 4096];
        // EIO once the last process with the slave open is gone
        while let Ok(n @ 1..) = reader.read(&mut buf) {
            sink.lock().unwrap().extend_from_slice(&buf[..n]);
        }
    });
    Run { child, master, output, _dir: TempDir(dir) }
}

impl Run {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }

    fn wait_for(&self, what: &str) {
        let end = Instant::now() + Duration::from_secs(20);
        while !self.text().contains(what) {
            assert!(Instant::now() < end, "no {what:?} in: {}", self.text());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn ctrl_c(&self) {
        // SAFETY: a signal to the child's own process group
        assert_eq!(unsafe { kill(-(self.child.id() as i32), SIGINT) }, 0);
    }

    fn died_of_sigint(mut self) -> String {
        let status = self.child.wait().unwrap();
        assert_eq!(status.signal(), Some(SIGINT), "{status:?}: {}", self.text());
        // The last bytes after paku is gone
        std::thread::sleep(Duration::from_millis(100));
        self.text()
    }
}

#[test]
fn ctrl_c_at_a_question_prints_the_line_then_dies_of_the_signal() {
    let run = start("question");
    run.wait_for("Proceed with the upgrade? [Y/n]");
    run.ctrl_c();
    let text = run.died_of_sigint();
    assert!(text.contains("paku maintain: stopped (Ctrl-C)"), "{text}");
    // The cursor is shown again before it goes
    assert!(text.contains("\x1b[?25h"), "{text:?}");
}

#[test]
fn ctrl_c_during_the_upgrade_prints_the_line_then_dies_of_the_signal() {
    let mut run = start("upgrade");
    run.wait_for("Proceed with the upgrade? [Y/n]");
    run.master.write_all(b"y\n").unwrap();
    run.wait_for("transferring");
    run.ctrl_c();
    let text = run.died_of_sigint();
    assert!(text.contains("paku maintain: stopped (Ctrl-C)"), "{text}");
}
