#!/usr/bin/env bash
# Sends a make-source.sh folder to the OBS package paku (an osc commit). Only releases: HEAD must be the
# tag v<version>. A version already on the OBS with the same content is skipped; the same version with a
# different content, or a lower one, is refused (pacman wouldn't move to it). osc uses the configured
# account (osc whois), with the password in the keyring: this script never asks for it.
# usage: packaging/obs/push.sh OUT
#        OBS_PROJECT=home:other packaging/obs/push.sh OUT   (default: home:nielsonrolim)
set -euo pipefail
src=$(cd "${1:?usage: push.sh OUT}" && pwd)
root=$(cd "$(dirname "$(readlink -f "$0")")/../.." && pwd)
project=${OBS_PROJECT:-home:nielsonrolim}
field() { sed -n "s/^$1=//p" "$2"; }
version=$(field pkgver "$src/PKGBUILD") rel=$(field pkgrel "$src/PKGBUILD")
tarball=paku-$version.tar.xz
[[ -f $src/$tarball ]] || { echo "push: no $tarball in $src" >&2; exit 1; }
tag=$(git -C "$root" describe --exact-match --tags HEAD 2>/dev/null || true)
[[ $tag == "v$version" ]] || { echo "push: HEAD isn't the tag v$version (only releases go to the OBS)" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"
osc checkout --output-dir paku "$project" paku >/dev/null
cd paku
if [[ -f PKGBUILD ]]; then
  old="$(field pkgver PKGBUILD)-$(field pkgrel PKGBUILD)" new="$version-$rel"
  if [[ $old == "$new" ]]; then
    # The bytes of a tarball made again are the same (make-source.sh is reproducible), so compare them
    if cmp -s "$tarball" "$src/$tarball" && cmp -s PKGBUILD "$src/PKGBUILD"; then
      echo "push: $project/paku: $new is already there, with the same content: skipped"
      exit 0
    fi
    echo "push: $project/paku: same version ($new), different content: raise the pkgrel or the version" >&2
    exit 1
  fi
  if [[ $(printf '%s\n%s\n' "$old" "$new" | sort -V | tail -n1) != "$new" ]]; then
    echo "push: $project/paku: $new is below $old, already on the OBS" >&2
    exit 1
  fi
fi
# Only the package's files: .osc is osc's, and the ones starting with _ (a _service) aren't ours
find . -maxdepth 1 -type f ! -name '_*' -delete
cp -- "$src/PKGBUILD" "$src/$tarball" .
osc addremove >/dev/null
osc status
osc commit --message "paku $version-$rel"
