# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

pacu: a Rust binary wrapping `paru` (or `pacman` without it) with a smaller, friendlier subcommand surface (`pacu upgrade`, `pacu install`, `pacu remove`, ...). It's the successor of the fish function `pak`, whose history this repo keeps. No dependencies besides std, so the vendored tarball for OBS stays small.

- `src/commands.rs`: the subcommand table (name, aliases, help line, usage message, shell description, what to complete). The help, the dispatch and the completions all come from it: an alias added there works and completes everywhere.
- `src/pacu.rs`: what each subcommand runs, through the `Sys` trait (run, capture, has, is_root, out, err). The tests use a fake `Sys` that records the command lines.
- `src/completions.rs`: the bash, zsh and fish scripts, generated from the table. Package names come from the hidden `pacu __complete repo|installed`.
- `src/audit.rs`: parsing of the AUR compromised list and the match.
- `src/main.rs`: the real `Sys` (std::process::Command, never a shell; SIGINT/SIGQUIT ignored while a child runs, reset in the child).
- `pacu.1`: the man page, by hand.
- `tests/packaging.rs`: the PKGBUILD promises (provides `pacu-wrapper` and the paku upgrade path; pkgver = the crate's version).
- `CHANGELOG.md`: one entry per release.
- `assets/`: the pacu logo (pacu.svg) and the wordmark (pacu-wordmark.svg), drawn by hand.

## Build and test

```sh
cargo build --release
cargo test
PACU_REQUIRE_SHELLS=1 cargo test   # fail instead of skipping when bash, zsh or fish is missing
```

The CI (.github/workflows/ci.yml) runs the tests in an `archlinux` container with the three shells.

## Rules

- Keep the help text and exit codes of the fish function (the test `the_help_is_the_one_of_the_fish_function` pins the help).
- Commands are argument vectors; never `sh -c`.
- The package provides `pacu-wrapper` (Swaydesk's swaydesk-desktop depends on it); keep it.
- Without paru: pacman with sudo for writes (not as root), official repos only; never install an AUR helper.

## Runtime assumptions

- `pacman` on PATH (Arch / Arch-derived). `paru`, `flatpak`, `cachy-update`/`arch-update`, `curl` and `checkupdates` are optional and looked up on the PATH.
- `pacu aur-audit` fetches the "Arch Linux AUR Compromised" list from the HedgeDoc `/download` endpoint (`https://md.archlinux.org/s/SxbqukK6IA/download`, raw markdown) with `curl -fsSL --max-time 20`, drops the code-fence and empty lines, and intersects it with `pacman -Qmq`. Exits non-zero if any installed AUR package matches, or if the list can't be fetched.
