# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

paku: a Rust binary wrapping an AUR helper (`paru`, `yay` or `pikaur`; `pacman` without one) with a smaller, friendlier subcommand surface (`paku upgrade`, `paku install`, `paku remove`, ...). It's the successor of the fish function `pak`, whose history this repo keeps. No dependencies besides std, so the vendored tarball for OBS stays small.

- `src/commands.rs`: the subcommand table (name, aliases, help line, usage message, shell description, what to complete). The help, the dispatch and the completions all come from it: an alias added there works and completes everywhere.
- `src/paku.rs`: what each subcommand runs, through the `Sys` trait (run, capture, has, is_root, helper_setting, out, err). The tests use a fake `Sys` that records the command lines.
- `src/completions.rs`: the bash, zsh and fish scripts, generated from the table. Package names come from the hidden `paku __complete repo|installed`.
- `src/helper.rs`: which tool runs (`PAKU_HELPER`, then `helper =` in `$XDG_CONFIG_HOME/paku/config`; `auto` = paru → yay → pikaur → pacman), the config parsing and the warnings.
- `src/audit.rs`: parsing of the AUR compromised list and the match.
- `src/main.rs`: the real `Sys` (std::process::Command, never a shell; SIGINT/SIGQUIT ignored while a child runs, reset in the child).
- `paku.1`: the man page, by hand.
- `tests/packaging.rs`: the PKGBUILD promises (`conflicts=('pacu<1:0')`, no replaces/provides; pkgver = the crate's version).
- `CHANGELOG.md`: one entry per release.
- `assets/`: the paku logo (paku.svg) and the wordmark (paku-wordmark.svg), drawn by hand.

## Build and test

```sh
cargo build --release
cargo test
PAKU_REQUIRE_SHELLS=1 cargo test   # fail instead of skipping when bash, zsh or fish is missing
```

The CI (.github/workflows/ci.yml) runs the tests in an `archlinux` container with the three shells.

## Rules

- Keep the help text and exit codes of the fish function (the test `the_help_is_the_one_of_the_fish_function` pins the help).
- Commands are argument vectors; never `sh -c`.
- The package conflicts only with our old pacu (`pacu<1:0`, epoch 0), never with BlackArch's pacu (epoch 1), and has no `replaces`: a replaces would offer to swap theirs. Swaydesk's swaydesk-desktop depends on `paku>=0.2.0`.
- Without a helper: pacman with sudo for writes (not as root), official repos only; never install an AUR helper.
- The helpers get the same pacman operations; one that needs different flags gets its own case in `Backend`, with a test.
- The help keeps the fish function's lines (it still says `paru`; the README and the man page say it stands for the helper in use); new subcommands and flags add to it, and the pinning test follows.

## Runtime assumptions

- `pacman` on PATH (Arch / Arch-derived). `paru`, `yay`, `pikaur`, `flatpak`, `cachy-update`, `curl` and `pacman-contrib` (`checkupdates`, `paccache`, `pacdiff`, `checkservices`) are optional and looked up on the PATH.
- `paku aur-audit` fetches the "Arch Linux AUR Compromised" list from the HedgeDoc `/download` endpoint (`https://md.archlinux.org/s/SxbqukK6IA/download`, raw markdown) with `curl -fsSL --max-time 20`, drops the code-fence and empty lines, and intersects it with `pacman -Qmq`. Exits non-zero if any installed AUR package matches, or if the list can't be fetched.
