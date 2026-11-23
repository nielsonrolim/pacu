# Changelog

## 0.3.4 (2026-11-23)

- `paku clean` and the cache step of `paku maintain` first remove the empty `download-*` directories pacman 7
  (`DownloadUser = alpm`) leaves in the cache directory after installing a local package file, which is how
  paru and yay install an AUR package. `pacman -Sc` stopped on them with `could not open file
  /var/cache/pacman/pkg/download-XXXXXX: Error reading fd 8`. Only empty ones go (`find -empty -delete`, through
  `sudo`), in every `CacheDir` of `pacman-conf`, and nothing is touched while pacman's `db.lck` exists. One
  line says how many were removed.

## 0.3.3 (2026-11-23)

- `paku maintain`: "Reboot now?" from a session without a local seat (ssh, for example) reboots through
  `sudo systemctl reboot`. A plain `systemctl reboot` asked polkit for a password with no agent to answer it
  and timed out. From a local session it stays `systemctl reboot`, as the user.
- `paku maintain --dry-run` (and the listing without a terminal) now shows the services step when the upgrade
  would run: the `checkservices` command it would run after the upgrade, or the note naming the package when
  `checkservices` is missing. Before, the step didn't appear at all.

## 0.3.2 (2026-11-23)

- The tools of `paku maintain`'s and `paku check`'s full flow are now dependencies of the package:
  `pacman-contrib` (checkupdates, paccache, pacdiff), `fakeroot` (checkupdates needs it), `archlinux-contrib`
  (checkservices) and `curl` (the news, `paku aur-audit`). The AUR helpers, `flatpak`, `cachy-update` and
  `sudo` stay optional. A step whose tool is missing anyway says which package to install.

## 0.3.1 (2026-11-22)

- `paku maintain`: a Ctrl-C at a question now prints `paku maintain: stopped (Ctrl-C)` and shows the cursor
  again before paku dies of SIGINT, as a Ctrl-C during a command already did. A shell around it stops either
  way and sees 130. The man page says how a caller can carry on (`trap : INT`, not ignoring SIGINT).
- `paku maintain`: `sudo -v` right before the services check, so a password timestamp that ran out while a
  question waited asks again instead of skipping the check (the message stays when sudo fails or is cancelled).
- `paku maintain`: no "none" right after the "not restarted here" line when every service found is kept.
## 0.3.0 (2026-11-22)

- **`paku maintain` has its own flow** when cachy-update isn't installed: the pending updates, the Arch news
  not shown before, the upgrade, then orphans, unused Flatpak runtimes, the package cache, `.pacnew` files,
  failed units and the journal size (information only), services to restart and a reboot for a new kernel.
  Every step but the upgrade asks `[y/N]`; without a terminal, or with `--dry-run`, it only lists. It follows
  arch-update 4.4.7's flow, reimplemented. Exit codes 0/1/2/17/130.
- `paku maintain` and `paku check` hand off to cachy-update only; arch-update is no longer used.
- `checkupdates` is used only with `fakeroot` installed (pacman-contrib lists it as optional, and checkupdates
  fails without it); otherwise `pacman -Qu`. `checkservices` comes from archlinux-contrib.

## 0.2.0 (2026-11-22)

- **pacu is paku again.** The package, the command, the completions and the man page are `paku`; the
  repository is github.com/nielsonrolim/paku. The name pacu clashed with BlackArch's unrelated pacu. Move
  once with `sudo pacman -Sy paku` (answer `y` to remove pacu). The package conflicts only with our pacu
  0.1.x (`pacu<1:0`), so BlackArch's pacu can stay installed; it no longer provides `pacu-wrapper`.
- AUR helpers: paru, yay or pikaur, the first one installed, or the one set with `PAKU_HELPER` or
  `helper = …` in `~/.config/paku/config` (`pacman` forces pacman). A setting that can't be followed warns
  and falls back to the first one installed.
- `paku helper` prints the tool paku uses.
- `paku upgrade --no-flatpak` runs only the system upgrade.

## 0.1.2 (2026-11-22)

- The package provides `pacu-wrapper`, a name for other packages to depend on: BlackArch's unrelated `pacu`
  has an epoch, so a dependency on plain `pacu` could take theirs from a repository listed above ours.
  Swaydesk's `swaydesk-desktop` depends on it.

## 0.1.1 (2026-11-21)

- paku is now pacu: the package, the command, the completions and the man page. The package replaces
  paku, so `pacman -Syu` moves paku 0.1.0 to pacu.

## 0.1.0 (2026-11-21)

- First release, as paku: the fish function `pak` as a Rust binary, with paru or pacman.
