# Packaging

paku is built by the openSUSE Build Service in the project `home:nielsonrolim`, the one of the
[Swaydesk](https://swaydesk.nielsonrolim.com) packages, in its `Arch` repository only (`obs/meta/paku.xml`
turns the project's other repositories off for this package).

- `PKGBUILD`: the package: `/usr/bin/paku`, the man page, the bash, zsh and fish completions (made by the
  built binary) and the license.
- `make-source.sh OUT`: the source of the current commit for the OBS: `paku-<version>.tar.xz` (with the
  crates in `vendor/` when there are any, since the OBS builds without network) and the `PKGBUILD` with its
  sha256. The same commit always gives the same tarball.
- `obs/push.sh OUT`: sends that folder to the OBS with `osc`. Only from a release tag; it skips a version
  already there with the same content and refuses a different content under the same version, or a lower
  version.
- `obs/meta/paku.xml`: the package's meta on the OBS.

## Build by hand

```sh
packaging/make-source.sh /tmp/paku-src
cd /tmp/paku-src && makepkg -si
```

## Release X.Y.Z

1. Set the version in `Cargo.toml` (and `Cargo.lock`), the `pkgver` of the `PKGBUILD` (`pkgrel=1`) and the
   header of `paku.1`, in a PR.
2. `git tag -a vX.Y.Z -m "paku X.Y.Z"` on the merged commit, and `git push origin vX.Y.Z`.
3. Once only, the package on the OBS: `osc meta pkg -F packaging/obs/meta/paku.xml home:nielsonrolim paku`.
4. `packaging/make-source.sh OUT && packaging/obs/push.sh OUT`, then wait for the `Arch` build to be green.
5. A packaging-only fix keeps the version and raises the `pkgrel`, from a tag `vX.Y.Z` branch.

## Installing from the repository

The repository is the one of Swaydesk; on Arch, CachyOS and other derivatives, add it to
`/etc/pacman.conf` and trust its key as described on the
[Swaydesk install page](https://swaydesk.nielsonrolim.com/install/) (the Arch section), then
`sudo pacman -Sy paku`.
