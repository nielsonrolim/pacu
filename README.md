<p align="center"><img src="assets/pacu-wordmark.svg" alt="pacu" height="96"></p>

# pacu

A small package-management wrapper around [`paru`](https://github.com/Morganamilo/paru) and `pacman`, for Arch Linux and its derivatives (CachyOS, Swaydesk Arch, ...). It exposes a simpler, easier-to-remember subcommand surface for everyday package management, in any shell.

pacu is the Rust successor of the fish function [pak](https://github.com/nielsonrolim/pak); its history is kept here.

**paku is now pacu.** Version 0.1.0 was published as `paku`; from 0.1.1 the package and the command are `pacu`. With the repository configured, `sudo pacman -Syu` replaces paku with pacu on its own (the package `replaces` it); after that, type `pacu` instead of `paku`.

```sh
pacu upgrade             # paru -Syu (and flatpak upgrade if available)
pacu maintain [args]     # full Cachy-Update maintenance run (cachy-update)
pacu check               # list pending updates without applying them
pacu update              # paru -Sy (refresh package DB only)
pacu search <pkg>        # paru -Ss <pkg>
pacu install <pkg>...    # paru -S <pkg>
pacu remove <pkg>...     # paru -Rns <pkg>
pacu info <pkg>          # paru -Si <pkg>
pacu owns <file>         # pacman -Qo <file>
pacu files <pkg>         # pacman -Ql <pkg>
pacu list                # explicitly installed packages (pacman -Qe)
pacu aur                 # AUR/foreign packages (pacman -Qm)
pacu aur-audit           # check installed AUR packages against the compromised list
pacu clean               # paru -Sc
pacu autoremove          # remove orphaned packages
pacu completions <shell> # print the bash, zsh or fish completions
```

## With or without paru

pacu uses `paru` when it's on your PATH, so the AUR is included. Without it, pacu uses `pacman` and the official repos only, with `sudo` for the operations that change the system (`upgrade`, `update`, `install`, `remove`, `clean`, `autoremove`). pacu never installs an AUR helper for you.

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

`owns`, `files`, `list` and `aur` always use `pacman`. When `search`, `install` or `info` fail without paru, pacu adds that the AUR wasn't used. As root, pacu runs `pacman` without `sudo`.

`check` and `maintain` prefer `cachy-update`, then `arch-update`, when installed.

`pacu update` only refreshes the package databases (`-Sy`), as the fish function did. Installing a package right after it, without upgrading, is a partial upgrade, which Arch doesn't support: prefer `pacu upgrade`, or `pacu check` to see what's pending.

Every command runs directly, as an argument list (never through a shell), and pacu exits with its code. Ctrl-C goes to paru or pacman, and pacu waits for them to finish.

### Known conflict: BlackArch's pacu

[BlackArch](https://blackarch.org) has an unrelated package called `pacu` (Rhino Security Labs' AWS exploitation framework), and pip installs the same tool as a `pacu` command. Both can't be installed at once: they'd both own `/usr/bin/pacu`. With both repositories in `/etc/pacman.conf`, pacman looks a package up in the first repository that has its name: keep `[home_nielsonrolim_Arch]` above `[blackarch]`. If `[blackarch]` comes first, `pacman -Syu` replaces this pacu with theirs, as an upgrade and without asking (its version has an epoch, `1:…`, which is always newer). A package that needs this pacu should depend on `pacu-wrapper`, which only this package provides (Swaydesk's `swaydesk-desktop` does): it always resolves to this pacu, and with `[blackarch]` first, `pacman -Syu` stops with "installing pacu (1:…) breaks dependency 'pacu-wrapper'" instead of swapping it. Then move `[home_nielsonrolim_Arch]` above `[blackarch]`, or add `IgnorePkg = pacu` to `/etc/pacman.conf`. A `pacu` from pip in `~/.local/bin` hides this one when `~/.local/bin` comes before `/usr/bin` in the PATH.

## Requirements

- `pacman` (Arch or an Arch-derived distro)
- optional: [`paru`](https://github.com/Morganamilo/paru) (AUR), `flatpak` (`pacu upgrade` also runs `flatpak upgrade`), [`cachy-update`](https://github.com/CachyOS/cachy-update) or `arch-update` (`pacu maintain`), `curl` (`pacu aur-audit`), `pacman-contrib` (`pacu check` without paru)

## Install

From source, with Rust's `cargo`:

```sh
git clone https://github.com/nielsonrolim/pacu.git
cd pacu
cargo build --release
sudo install -Dm755 target/release/pacu /usr/local/bin/pacu
sudo install -Dm644 pacu.1 /usr/local/share/man/man1/pacu.1
```

### Shell completions

`pacu completions bash|zsh|fish` prints the completion script. Package names come from the repos (`install`, `search`), the installed packages (`remove`, `files`) or both (`info`); `owns` completes paths.

```sh
pacu completions bash | sudo tee /usr/share/bash-completion/completions/pacu >/dev/null
pacu completions zsh  | sudo tee /usr/share/zsh/site-functions/_pacu >/dev/null
pacu completions fish | sudo tee /usr/share/fish/vendor_completions.d/pacu.fish >/dev/null
```

### Coming from the fish function pak

pacu replaces the fish function `pak`, with the same subcommands and aliases under the new name. Remove the old function and its completions (the pak repo's `./uninstall.fish` does the same):

```sh
rm -f ~/.config/fish/functions/pak.fish ~/.config/fish/completions/pak.fish
```

Then open a new fish shell.

## Usage

Run `pacu` with no arguments for the help summary (it exits 1, like the fish function did); `pacu --help` prints it and exits 0, and `pacu --version` prints the version.

### Upgrade everything

```sh
pacu upgrade
```

Runs `paru -Syu` (or `sudo pacman -Syu`) and then, if `flatpak` is on your PATH and the system upgrade went through, `flatpak upgrade`. A failed or interrupted system upgrade stops there, with its exit code.

### Run a full maintenance pass

```sh
pacu maintain
```

Delegates to [`cachy-update`](https://github.com/CachyOS/cachy-update) (falling back to `arch-update`) for a complete maintenance flow: repo + AUR updates plus news, orphan removal, cache cleanup, pacnew handling, and kernel/reboot and service-restart checks. Any extra arguments are passed straight through, e.g. `pacu maintain --devel` or `pacu maintain --news 10`. If neither tool is installed, it prints how to install `cachy-update` and exits 1.

### Check for pending updates

```sh
pacu check
```

Lists available updates without applying them: `cachy-update --list` or `arch-update --list` when available, otherwise `paru -Qu` (without paru, `checkupdates`, else `pacman -Qu`).

### Search, install, inspect and remove

```sh
pacu search firefox
pacu install neovim ripgrep fd
pacu info neovim
pacu remove neovim
```

`remove` uses `-Rns`, so unused dependencies and saved configs go with the package. Extra flags are passed on, e.g. `pacu install --needed base-devel`.

### Files and ownership

```sh
pacu owns /usr/bin/fish     # pacman -Qo
pacu files fish             # pacman -Ql
```

### Lists

```sh
pacu list                   # explicitly installed packages (pacman -Qe)
pacu aur                    # AUR / foreign packages (pacman -Qm)
```

### Audit installed AUR packages against the compromised list

```sh
pacu aur-audit
```

Fetches the [Arch Linux AUR compromised-package list](https://md.archlinux.org/s/SxbqukK6IA) with `curl` and compares it against your installed AUR/foreign packages (`pacman -Qmq`). It prints a summary and, if any installed package is on the list, a `WARNING:` block naming them. Exit codes: 0 when nothing matches (or no AUR package is installed); 1 when a package matches, when the list can't be fetched or when `curl` is missing.

### Clean the cache and remove orphans

```sh
pacu clean
pacu autoremove
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

pacu is named after the [pacu](https://en.wikipedia.org/wiki/Pacu), the South American fish with human-like teeth, and it starts like `pac`man.

## License

[Apache License 2.0](LICENSE)
