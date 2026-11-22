<p align="center"><img src="assets/paku-wordmark.svg" alt="paku" height="96"></p>

# paku

A small package-management wrapper around an AUR helper ([`paru`](https://github.com/Morganamilo/paru), [`yay`](https://github.com/Jguer/yay) or [`pikaur`](https://github.com/actionless/pikaur)) and `pacman`, for Arch Linux and its derivatives (CachyOS, Swaydesk Arch, ...). It exposes a simpler, easier-to-remember subcommand surface for everyday package management, in any shell.

paku is the Rust successor of the fish function [pak](https://github.com/nielsonrolim/pak); its history is kept here.

**pacu is now paku.** The name went from paku (0.1.0) to pacu (0.1.1, 0.1.2) and back to paku in 0.2.0, which no other package uses (BlackArch has an unrelated `pacu`). If you have pacu from the repository, install paku once: `sudo pacman -Sy paku`, and answer `y` when pacman asks to remove pacu. After that, type `paku` instead of `pacu`. BlackArch's `pacu` can stay installed next to paku.

```sh
paku upgrade             # paru -Syu (and flatpak upgrade if available, unless --no-flatpak)
paku maintain            # upgrade, then orphans, caches, pacnew, services (cachy-update if installed)
paku check               # list pending updates without applying them
paku update              # paru -Sy (refresh package DB only)
paku search <pkg>        # paru -Ss <pkg>
paku install <pkg>...    # paru -S <pkg>
paku remove <pkg>...     # paru -Rns <pkg>
paku info <pkg>          # paru -Si <pkg>
paku owns <file>         # pacman -Qo <file>
paku files <pkg>         # pacman -Ql <pkg>
paku list                # explicitly installed packages (pacman -Qe)
paku aur                 # AUR/foreign packages (pacman -Qm)
paku aur-audit           # check installed AUR packages against the compromised list
paku clean               # paru -Sc
paku autoremove          # remove orphaned packages
paku completions <shell> # print the bash, zsh or fish completions
paku helper              # print the tool paku uses: paru, yay, pikaur or pacman
```

## With or without an AUR helper

paku uses an AUR helper, so the AUR is included: `paru`, `yay` or `pikaur`, the first one on your PATH in that order, or the one you choose (below). Without one, paku uses `pacman` and the official repos only, with `sudo` for the operations that change the system (`upgrade`, `update`, `install`, `remove`, `clean`, `autoremove`). paku never installs an AUR helper for you.

The table shows paru; yay and pikaur get the same operations (`yay -Syu`, `pikaur -S`, ...).

| Subcommand   | With paru                 | Without a helper                                   |
| ------------ | ------------------------- | -------------------------------------------------- |
| `upgrade`    | `paru -Syu`               | `sudo pacman -Syu`                                 |
| `update`     | `paru -Sy`                | `sudo pacman -Sy`                                  |
| `search`     | `paru -Ss`                | `pacman -Ss`                                       |
| `install`    | `paru -S`                 | `sudo pacman -S`                                   |
| `remove`     | `paru -Rns`               | `sudo pacman -Rns`                                 |
| `info`       | `paru -Si`                | `pacman -Si`, then `pacman -Qi` if not in the repos |
| `clean`      | `paru -Sc`                | `sudo pacman -Sc`                                  |
| `autoremove` | `paru -Rns` the orphans   | `sudo pacman -Rns` the orphans                     |
| `check`      | `paru -Qu`                | `checkupdates` (pacman-contrib), else `pacman -Qu` |

`owns`, `files`, `list` and `aur` always use `pacman`. When `search`, `install` or `info` fail without a helper, paku adds that the AUR wasn't used. As root, paku runs `pacman` without `sudo`.

`check` and `maintain` hand off to `cachy-update` when it's installed.

### Choosing the helper

Set `PAKU_HELPER`, or put a `helper` line in `~/.config/paku/config` (`$XDG_CONFIG_HOME/paku/config`):

```toml
# auto, paru, yay, pikaur or pacman
helper = "yay"
```

`auto` (the default, also with no file) takes the first helper installed: paru, then yay, then pikaur, then pacman. `pacman` uses pacman even when a helper is installed. A chosen helper that isn't installed, or an unknown value, gets a warning on stderr and counts as `auto`. `PAKU_HELPER` wins over the file.

`paku helper` prints the tool paku will use, one word on stdout (`paru`, `yay`, `pikaur` or `pacman`), with the warning on stderr when the setting can't be followed: scripts can show it before running `paku upgrade`.

`paku update` only refreshes the package databases (`-Sy`), as the fish function did. Installing a package right after it, without upgrading, is a partial upgrade, which Arch doesn't support: prefer `paku upgrade`, or `paku check` to see what's pending.

Every command runs directly, as an argument list (never through a shell), and paku exits with its code. Ctrl-C goes to the helper or pacman, and paku waits for them to finish.

## Requirements

- `pacman` (Arch or an Arch-derived distro)
- optional: an AUR helper, [`paru`](https://github.com/Morganamilo/paru), [`yay`](https://github.com/Jguer/yay) or [`pikaur`](https://github.com/actionless/pikaur), `flatpak` (`paku upgrade` also runs `flatpak upgrade`), `curl` (`paku aur-audit`, the news in `paku maintain`), `pacman-contrib` (`paku check` without a helper; `paccache`, `pacdiff` and `checkservices` in `paku maintain`), [`cachy-update`](https://github.com/CachyOS/cachy-update) (`paku maintain` and `paku check` hand off to it)

## Install

From source, with Rust's `cargo`:

```sh
git clone https://github.com/nielsonrolim/paku.git
cd paku
cargo build --release
sudo install -Dm755 target/release/paku /usr/local/bin/paku
sudo install -Dm644 paku.1 /usr/local/share/man/man1/paku.1
```

### Shell completions

`paku completions bash|zsh|fish` prints the completion script. Package names come from the repos (`install`, `search`), the installed packages (`remove`, `files`) or both (`info`); `owns` completes paths.

```sh
paku completions bash | sudo tee /usr/share/bash-completion/completions/paku >/dev/null
paku completions zsh  | sudo tee /usr/share/zsh/site-functions/_paku >/dev/null
paku completions fish | sudo tee /usr/share/fish/vendor_completions.d/paku.fish >/dev/null
```

### Coming from the fish function pak

paku replaces the fish function `pak`, with the same subcommands and aliases under the new name. Remove the old function and its completions (the pak repo's `./uninstall.fish` does the same):

```sh
rm -f ~/.config/fish/functions/pak.fish ~/.config/fish/completions/pak.fish
```

Then open a new fish shell.

## Usage

Run `paku` with no arguments for the help summary (it exits 1, like the fish function did); `paku --help` prints it and exits 0, and `paku --version` prints the version.

### Upgrade everything

```sh
paku upgrade
```

Runs `paru -Syu` (or `sudo pacman -Syu`) and then, if `flatpak` is on your PATH and the system upgrade went through, `flatpak upgrade`. A failed or interrupted system upgrade stops there, with its exit code. `paku upgrade --no-flatpak` runs only the system upgrade (for a tool that upgrades the Flatpaks itself).

### Run a full maintenance pass

```sh
paku maintain
```

With [`cachy-update`](https://github.com/CachyOS/cachy-update) installed, it runs `cachy-update` with the arguments as given. Otherwise paku runs its own flow, in this order:

1. the pending updates (`checkupdates`, else `pacman -Qu`, plus the helper's `-Qua`);
2. the Arch Linux news not shown before (titles and links from the RSS feed, with `curl`);
3. the upgrade, after a `[Y/n]`: as `paku upgrade`, then `flatpak update`;
4. orphans (`sudo pacman -Rns`);
5. unused Flatpak runtimes (`flatpak uninstall --unused`);
6. the package cache (`sudo paccache -rk3` and `-ruk0`), then the helper's own caches (`paru -Sc`);
7. `.pacnew` and `.pacsave` files (`pacdiff --sudo`, so `$DIFFPROG` runs as you);
8. failed system and user units, and the journal size when over 1 GiB, as information only;
9. after an upgrade, the services running old libraries (`checkservices`), restarted all or none; the display managers, D-Bus, logind, the network services and `user@` units are only named, never restarted;
10. when the running kernel is no longer installed, an offer to reboot.

Steps 4 to 10 ask `[y/N]`: a plain Enter keeps everything. A step whose tool is missing (`curl`, `flatpak`, `pacman-contrib`) is skipped with a note. Without a terminal, or with `--dry-run`, every step only lists what it would do, the upgrade included. `--no-flatpak` leaves Flatpak out, and `--devel` is passed to the helper. paku never passes `--noconfirm`.

The last line says how it went: `paku maintain: done`, `paku maintain: done, 1 step failed (orphans)` or `paku maintain: stopped (Ctrl-C)`. Exit codes: 0 done (including steps skipped or answered no), 1 bad usage, 2 a step failed (the next steps still ran), 17 another `paku maintain` is running, 130 Ctrl-C (which stops the flow at once). With `cachy-update`, its own codes.

The flow follows [arch-update](https://github.com/Antiz96/arch-update)'s (version 4.4.7) and is reimplemented here: no code of it is used. It differs in the defaults (arch-update asks `[Y/n]` before cleaning the cache and merging `.pacnew` files) and in the information steps (failed units and the journal).

### Check for pending updates

```sh
paku check
```

Lists available updates without applying them: `cachy-update --list` when it's installed, otherwise `paru -Qu` (without a helper, `checkupdates`, else `pacman -Qu`). paku's own list has packages only (repos and AUR), no Flatpak; `cachy-update --list` can include Flatpak updates.

### Search, install, inspect and remove

```sh
paku search firefox
paku install neovim ripgrep fd
paku info neovim
paku remove neovim
```

`remove` uses `-Rns`, so unused dependencies and saved configs go with the package. Extra flags are passed on, e.g. `paku install --needed base-devel`.

### Files and ownership

```sh
paku owns /usr/bin/fish     # pacman -Qo
paku files fish             # pacman -Ql
```

### Lists

```sh
paku list                   # explicitly installed packages (pacman -Qe)
paku aur                    # AUR / foreign packages (pacman -Qm)
```

### Audit installed AUR packages against the compromised list

```sh
paku aur-audit
```

Fetches the [Arch Linux AUR compromised-package list](https://md.archlinux.org/s/SxbqukK6IA) with `curl` and compares it against your installed AUR/foreign packages (`pacman -Qmq`). It prints a summary and, if any installed package is on the list, a `WARNING:` block naming them. Exit codes: 0 when nothing matches (or no AUR package is installed); 1 when a package matches, when the list can't be fetched or when `curl` is missing.

### Clean the cache and remove orphans

```sh
paku clean
paku autoremove
```

`autoremove` finds unrequired dependencies with `pacman -Qdtq` and removes them with `-Rns`; with none, it says so and exits 0.

## Subcommand aliases

| Subcommand   | Aliases                      |
| ------------ | ---------------------------- |
| `upgrade`    | `up`                         |
| `maintain`   | `cu`, `full`, `full-upgrade` |
| `check`      | `co`                         |
| `update`     | `refresh`                    |
| `search`     | `s`                          |
| `install`    | `add`, `i`                   |
| `remove`     | `rm`, `uninstall`            |
| `list`       | `explicit`                   |
| `aur`        | `aur-list`                   |
| `aur-audit`  | `aur-sec`, `aur-security`    |
| `autoremove` | `orphans`                    |

## The name

paku is named after the [pacu](https://en.wikipedia.org/wiki/Pacu), the South American fish with human-like teeth, spelled with a k (the logo is still a pacu); it starts like `pac`man. From 0.1.1 to 0.1.2 it was called pacu, which clashed with an unrelated tool of that name.

## License

[Apache License 2.0](LICENSE)
