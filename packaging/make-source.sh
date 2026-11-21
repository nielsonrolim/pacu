#!/usr/bin/env bash
# The OBS source of the current commit (HEAD, not the working tree: commit first) in OUT:
# - pacu-<version>.tar.xz: git archive, plus the crates in vendor/ and the .cargo/config.toml that uses
#   them when the crate has dependencies (the OBS builds without network). Reproducible: the same commit
#   gives the same bytes (sorted names, the commit's date, no owners);
# - PKGBUILD: packaging/PKGBUILD with the tarball's sha256.
# The version is the one of Cargo.toml, which must match the pkgver of the PKGBUILD.
# usage: packaging/make-source.sh OUT
set -euo pipefail
# The modes in the tarball can't depend on the machine's umask (002 on Ubuntu and Mint, 022 in containers)
umask 022
root=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
out=$(mkdir -p "${1:?usage: make-source.sh OUT}" && cd "$1" && pwd)
version=$(git -C "$root" show HEAD:Cargo.toml | sed -n 's/^version = "\(.*\)"$/\1/p' | head -n1)
pkgver=$(git -C "$root" show HEAD:packaging/PKGBUILD | sed -n 's/^pkgver=//p')
[[ -n $version && $version == "$pkgver" ]] || { echo "make-source: Cargo.toml has $version, the PKGBUILD $pkgver" >&2; exit 1; }
name=pacu-$version
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git -C "$root" archive --format=tar --prefix="$name/" HEAD | tar -x -C "$work"
(
  cd "$work/$name"
  mkdir .cargo
  # Its messages only when it fails (on success, the progress lines would hide the output path)
  if ! cargo vendor --locked --versioned-dirs vendor > .cargo/config.toml 2> "$work/vendor.log"; then
    cat "$work/vendor.log" >&2
    exit 1
  fi
  # No dependency: no vendor/, and the config would point at nothing
  if [[ ! -d vendor ]]; then
    rm -rf .cargo
  elif ! grep -q vendored-sources .cargo/config.toml; then
    echo "make-source: cargo vendor wrote no config" >&2
    exit 1
  fi
)
mtime=$(git -C "$root" log -1 --format=%ct HEAD)
tar -C "$work" --sort=name --mtime="@$mtime" --owner=0 --group=0 --numeric-owner \
  --mode=u+rwX,go+rX,go-w -cf - "$name" | xz -9 > "$out/$name.tar.xz"
sum=$(sha256sum "$out/$name.tar.xz" | cut -d' ' -f1)
git -C "$root" show HEAD:packaging/PKGBUILD | sed "s/^sha256sums=('SKIP')$/sha256sums=('$sum')/" > "$out/PKGBUILD"
grep -q "^sha256sums=('$sum')$" "$out/PKGBUILD" || { echo "make-source: the PKGBUILD has no sha256sums=('SKIP') line" >&2; exit 1; }
echo "$out/$name.tar.xz"
