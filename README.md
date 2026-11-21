<p align="center"><img src="assets/paku-wordmark.svg" alt="paku" height="96"></p>

# paku

A small package-management wrapper around [`paru`](https://github.com/Morganamilo/paru) and `pacman`, for Arch Linux and its derivatives (CachyOS, Swaydesk Arch, ...). It exposes a simpler, easier-to-remember subcommand surface for everyday package management, in any shell.

paku is the Rust successor of the fish function [pak](https://github.com/nielsonrolim/pak); its history is kept here.

```sh
paku upgrade             # paru -Syu (and flatpak upgrade if available)
paku maintain [args]     # full Cachy-Update maintenance run (cachy-update)
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
```

## With or without paru

paku uses `paru` when it's on your PATH, so the AUR is included. Without it, paku uses `pacman` and the official repos only, with `sudo` for the operations that change the system (`upgrade`, `update`, `install`, `remove`, `clean`, `autoremove`). paku never installs an AUR helper for you.

| Subcommand   | With paru                 | Without paru                                       |
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

`owns`, `files`, `list` and `aur` always use `pacman`. When `search`, `install` or `info` fail without paru, paku adds that the AUR wasn't used. As root, paku runs `pacman` without `sudo`.

`check` and `maintain` prefer `cachy-update`, then `arch-update`, when installed.

Every command runs directly, as an argument list (never through a shell), and paku exits with its code. Ctrl-C goes to paru or pacman, and paku waits for them to finish.

## Requirements

- `pacman` (Arch or an Arch-derived distro)
- optional: [`paru`](https://github.com/Morganamilo/paru) (AUR), `flatpak` (`paku upgrade` also runs `flatpak upgrade`), [`cachy-update`](https://github.com/CachyOS/cachy-update) or `arch-update` (`paku maintain`), `curl` (`paku aur-audit`), `pacman-contrib` (`paku check` without paru)

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

Runs `paru -Syu` (or `sudo pacman -Syu`) and, if `flatpak` is on your PATH, `flatpak upgrade`. The exit code is the first one that failed.

### Run a full maintenance pass

```sh
paku maintain
```

Delegates to [`cachy-update`](https://github.com/CachyOS/cachy-update) (falling back to `arch-update`) for a complete maintenance flow: repo + AUR updates plus news, orphan removal, cache cleanup, pacnew handling, and kernel/reboot and service-restart checks. Any extra arguments are passed straight through, e.g. `paku maintain --devel` or `paku maintain --news 10`. If neither tool is installed, it prints how to install `cachy-update` and exits 1.

### Check for pending updates

```sh
paku check
```

Lists available updates without applying them: `cachy-update --list` or `arch-update --list` when available, otherwise `paru -Qu` (without paru, `checkupdates`, else `pacman -Qu`).

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

paku is a [pacu](https://en.wikipedia.org/wiki/Pacu), the South American fish with human-like teeth, and a nod to `pac`man.

## License

[Apache License 2.0](LICENSE)
