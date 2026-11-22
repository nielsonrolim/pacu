//! What the package promises to the packages that depend on it.

const PKGBUILD: &str = include_str!("../packaging/PKGBUILD");

/// The values of an array of the PKGBUILD (`name=(a "b=$pkgver")`), on one line.
fn array(name: &str) -> Vec<String> {
    let line = PKGBUILD.lines().find(|l| l.starts_with(&format!("{name}=("))).unwrap_or_else(|| panic!("no {name}"));
    let inner = line.split_once('(').unwrap().1.rsplit_once(')').unwrap().0;
    inner.split_whitespace().map(|v| v.trim_matches(|c| c == '"' || c == '\'').to_string()).collect()
}

#[test]
fn swaydesk_depends_on_a_name_only_this_package_provides() {
    // swaydesk-desktop depends on swaydesk-pacu: BlackArch's pacu has an epoch and would win a plain pacu
    assert!(array("provides").contains(&"swaydesk-pacu=$pkgver".to_string()), "{:?}", array("provides"));
}

#[test]
fn the_paku_upgrade_path_stays() {
    assert!(array("provides").contains(&"paku=$pkgver".to_string()));
    assert_eq!(array("replaces"), ["paku"]);
    assert_eq!(array("conflicts"), ["paku"]);
}

#[test]
fn the_versions_agree() {
    let pkgver = PKGBUILD.lines().find_map(|l| l.strip_prefix("pkgver=")).unwrap();
    assert_eq!(pkgver, env!("CARGO_PKG_VERSION"));
}
