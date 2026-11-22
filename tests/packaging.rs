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
