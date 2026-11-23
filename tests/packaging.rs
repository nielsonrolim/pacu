//! What the package promises to the packages around it.

const PKGBUILD: &str = include_str!("../packaging/PKGBUILD");

/// The values of an array of the PKGBUILD (`name=(a 'b<1:0')`), on one line; empty when it isn't there.
fn array(name: &str) -> Vec<String> {
    let Some(line) = PKGBUILD.lines().find(|l| l.starts_with(&format!("{name}=("))) else { return Vec::new() };
    let inner = line.split_once('(').unwrap().1.rsplit_once(')').unwrap().0;
    inner.split_whitespace().map(|v| v.trim_matches(|c| c == '"' || c == '\'').to_string()).collect()
}

#[test]
fn it_conflicts_with_our_old_pacu_only() {
    // Our pacu 0.1.x has no epoch; BlackArch's unrelated pacu has epoch 1 and must stay installable next to paku
    assert_eq!(array("conflicts"), ["pacu<1:0"]);
}

#[test]
fn it_replaces_and_provides_nothing() {
    // A replaces=(pacu) would make pacman -Syu offer to swap BlackArch's pacu for paku
    assert!(array("replaces").is_empty(), "{:?}", array("replaces"));
    assert!(array("provides").is_empty(), "{:?}", array("provides"));
}

#[test]
fn the_versions_agree() {
    let pkgver = PKGBUILD.lines().find_map(|l| l.strip_prefix("pkgver=")).unwrap();
    assert_eq!(pkgver, env!("CARGO_PKG_VERSION"));
}

#[test]
fn the_full_flow_tools_are_dependencies() {
    // paku maintain and paku check: checkupdates (with fakeroot), paccache, pacdiff, checkservices, curl
    let depends = array("depends");
    for package in ["pacman", "pacman-contrib", "fakeroot", "archlinux-contrib", "curl"] {
        assert!(depends.iter().any(|d| d == package), "{package} not in {depends:?}");
    }
    // The helpers, flatpak, cachy-update and sudo stay the user's choice
    let optional: Vec<String> = array_lines("optdepends");
    for package in ["paru", "yay", "pikaur", "flatpak", "cachy-update", "sudo"] {
        assert!(optional.iter().any(|o| o.starts_with(&format!("{package}:"))), "{package} not in {optional:?}");
    }
    for package in ["pacman-contrib", "fakeroot", "archlinux-contrib", "curl"] {
        assert!(!optional.iter().any(|o| o.starts_with(&format!("{package}:"))), "{package} still optional");
    }
}

/// The values of an array that spans lines (`optdepends=('a: x'\n 'b: y')`).
fn array_lines(name: &str) -> Vec<String> {
    let start = PKGBUILD.find(&format!("\n{name}=(")).unwrap() + name.len() + 3;
    let body = &PKGBUILD[start..start + PKGBUILD[start..].find(")\n").unwrap()];
    body.split('\'').skip(1).step_by(2).map(str::to_string).collect()
}
