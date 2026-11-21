#!/usr/bin/env bash
# Sends a make-source.sh folder to the OBS package pacu (an osc commit). Only releases: HEAD must be the
# tag v<version>. A version already on the OBS with the same content is skipped; the same version with a
# different content, or a lower one, is refused (pacman wouldn't move to it). osc uses the configured
# account (osc whois), with the password in the keyring: this script never asks for it.
# OUT must be what make-source.sh makes from HEAD now: push.sh makes it again and compares, so a folder made
# before the last commit never goes.
# usage: packaging/obs/push.sh OUT
#        OBS_PROJECT=home:other packaging/obs/push.sh OUT   (default: home:nielsonrolim)
set -euo pipefail
src=$(cd "${1:?usage: push.sh OUT}" && pwd)
root=$(cd "$(dirname "$(readlink -f "$0")")/../.." && pwd)
project=${OBS_PROJECT:-home:nielsonrolim}
field() { sed -n "s/^$1=//p" "$2"; }
version=$(field pkgver "$src/PKGBUILD") rel=$(field pkgrel "$src/PKGBUILD")
tarball=pacu-$version.tar.xz
[[ -f $src/$tarball ]] || { echo "push: no $tarball in $src" >&2; exit 1; }
tag=$(git -C "$root" describe --exact-match --tags HEAD 2>/dev/null || true)
[[ $tag == "v$version" ]] || { echo "push: HEAD isn't the tag v$version (only releases go to the OBS)" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
"$root/packaging/make-source.sh" "$work/fresh" >/dev/null
if ! cmp -s "$work/fresh/$tarball" "$src/$tarball" || ! cmp -s "$work/fresh/PKGBUILD" "$src/PKGBUILD"; then
  echo "push: $src isn't the source of HEAD (v$version): run packaging/make-source.sh again" >&2
  exit 1
fi
# pacman's order when vercmp is there (a 0.2.0rc1 comes before 0.2.0); sort -V otherwise
newer() { # newer OLD NEW: NEW comes after OLD
  if command -v vercmp >/dev/null; then (($(vercmp "$2" "$1") > 0)); else
    [[ $(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n1) == "$2" && $1 != "$2" ]]
  fi
}
cd "$work"
osc checkout --output-dir pacu "$project" pacu >/dev/null
cd pacu
if [[ -f PKGBUILD ]]; then
  old="$(field pkgver PKGBUILD)-$(field pkgrel PKGBUILD)" new="$version-$rel"
  if [[ $old == "$new" ]]; then
    # The bytes of a tarball made again are the same (make-source.sh is reproducible), so compare them
    if cmp -s "$tarball" "$src/$tarball" && cmp -s PKGBUILD "$src/PKGBUILD"; then
      echo "push: $project/pacu: $new is already there, with the same content: skipped"
      exit 0
    fi
    echo "push: $project/pacu: same version ($new), different content: raise the pkgrel or the version" >&2
    exit 1
  fi
  if ! newer "$old" "$new"; then
    echo "push: $project/pacu: $new is below $old, already on the OBS" >&2
    exit 1
  fi
fi
# Only the package's files: .osc is osc's, and the ones starting with _ (a _service) aren't ours
find . -maxdepth 1 -type f ! -name '_*' -delete
cp -- "$src/PKGBUILD" "$src/$tarball" .
osc addremove >/dev/null
osc status
osc commit --message "pacu $version-$rel"
