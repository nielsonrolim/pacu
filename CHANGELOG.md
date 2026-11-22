# Changelog

## 0.1.2 (2026-11-22)

- The package provides `pacu-wrapper`, a name for other packages to depend on: BlackArch's unrelated `pacu`
  has an epoch, so a dependency on plain `pacu` could take theirs from a repository listed above ours.
  Swaydesk's `swaydesk-desktop` depends on it.

## 0.1.1 (2026-11-21)

- paku is now pacu: the package, the command, the completions and the man page. The package replaces
  paku, so `pacman -Syu` moves paku 0.1.0 to pacu.

## 0.1.0 (2026-11-21)

- First release, as paku: the fish function `pak` as a Rust binary, with paru or pacman.
