//! Which tool runs the package operations: an AUR helper (paru, yay or pikaur) or pacman. The user picks
//! one with `PAKU_HELPER` or the `helper` key of `~/.config/paku/config`; `auto` (the default) takes the
//! first helper installed, in the order of `HELPERS`, and pacman without any.
use std::ffi::OsString;
use std::path::PathBuf;

/// The AUR helpers paku knows, in the order `auto` looks for them. All three take pacman's operations
/// (`-Syu`, `-S`, `-Ss`, `-Si`, `-Sc`, `-Qu` extended to the AUR) and call sudo themselves.
pub const HELPERS: [&str; 3] = ["paru", "yay", "pikaur"];

/// The values of the setting.
pub const CHOICES: &str = "auto, paru, yay, pikaur or pacman";

/// Where the setting came from, for the warnings.
pub const ENV: &str = "PAKU_HELPER";

/// The `helper` value of the config file: `helper = yay` (quotes optional), `#` starts a comment. The last
/// `helper` line wins; other keys are ignored.
pub fn parse_config(text: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        let Some((key, value)) = line.split_once('=') else { continue };
        if key.trim() != "helper" {
            continue;
        }
        let value = value.trim();
        let unquoted =
            ['"', '\''].iter().find_map(|q| value.strip_prefix(*q).and_then(|v| v.strip_suffix(*q))).unwrap_or(value);
        found = Some(unquoted.trim().to_string());
    }
    found
}

/// The config file: `$XDG_CONFIG_HOME/paku/config`, or `~/.config/paku/config` (an XDG_CONFIG_HOME that
/// isn't absolute is ignored, as the spec says).
pub fn config_path(xdg_config_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let base = match xdg_config_home.map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        _ => PathBuf::from(home.filter(|h| !h.is_empty())?).join(".config"),
    };
    Some(base.join("paku").join("config"))
}

/// The tool chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The AUR helper, or None for pacman.
    pub helper: Option<&'static str>,
    /// pacman because the user asked for it (not because no helper is installed).
    pub pacman_asked: bool,
}

/// The tool for the setting (value, where it came from) and the installed programs; the warning to print
/// when the setting can't be followed.
pub fn choose(setting: Option<(&str, &str)>, has: impl Fn(&str) -> bool) -> (Choice, Option<String>) {
    let auto = Choice { helper: HELPERS.into_iter().find(|h| has(h)), pacman_asked: false };
    let fallback = || auto.helper.unwrap_or("pacman");
    let Some((value, source)) = setting else { return (auto, None) };
    match value {
        "" | "auto" => (auto, None),
        "pacman" => (Choice { helper: None, pacman_asked: true }, None),
        _ => match HELPERS.into_iter().find(|h| *h == value) {
            Some(helper) if has(helper) => (Choice { helper: Some(helper), pacman_asked: false }, None),
            Some(helper) => {
                (auto, Some(format!("paku: {helper} (set in {source}) isn't installed; using {}", fallback())))
            }
            None => (
                auto,
                Some(format!("paku: unknown helper {value:?} in {source} (use {CHOICES}); using {}", fallback())),
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_line() {
        assert_eq!(parse_config("helper = yay\n").as_deref(), Some("yay"));
        assert_eq!(parse_config("# pick one\nhelper=\"pikaur\"  # the AUR\n").as_deref(), Some("pikaur"));
        assert_eq!(parse_config("helper = 'pacman'").as_deref(), Some("pacman"));
        assert_eq!(parse_config("helper = paru\nhelper = yay\n").as_deref(), Some("yay"));
        assert_eq!(parse_config("# helper = yay\nother = 1\n"), None);
        assert_eq!(parse_config(""), None);
    }

    #[test]
    fn the_config_file() {
        let path = |x: Option<&str>, h: Option<&str>| config_path(x.map(OsString::from), h.map(OsString::from));
        assert_eq!(path(Some("/x/cfg"), Some("/home/a")), Some(PathBuf::from("/x/cfg/paku/config")));
        assert_eq!(path(None, Some("/home/a")), Some(PathBuf::from("/home/a/.config/paku/config")));
        assert_eq!(path(Some("rel"), Some("/home/a")), Some(PathBuf::from("/home/a/.config/paku/config")));
        assert_eq!(path(Some(""), Some("/home/a")), Some(PathBuf::from("/home/a/.config/paku/config")));
        assert_eq!(path(None, None), None);
        assert_eq!(path(None, Some("")), None);
    }

    fn installed(programs: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |p| programs.contains(&p)
    }

    #[test]
    fn auto_takes_paru_then_yay_then_pikaur_then_pacman() {
        for (programs, helper) in [
            (&["paru", "yay", "pikaur"][..], Some("paru")),
            (&["yay", "pikaur"][..], Some("yay")),
            (&["pikaur"][..], Some("pikaur")),
            (&[][..], None),
        ] {
            let programs: &'static [&'static str] = programs;
            for setting in [None, Some(("auto", ENV)), Some(("", ENV))] {
                let (choice, warning) = choose(setting, installed(programs));
                assert_eq!(choice, Choice { helper, pacman_asked: false }, "{programs:?} {setting:?}");
                assert_eq!(warning, None);
            }
        }
    }

    #[test]
    fn a_chosen_helper_wins_over_the_order() {
        let (choice, warning) = choose(Some(("pikaur", ENV)), installed(&["paru", "pikaur"]));
        assert_eq!((choice.helper, warning), (Some("pikaur"), None));
        let (choice, warning) = choose(Some(("pacman", "~/.config/paku/config")), installed(&["paru"]));
        assert_eq!(choice, Choice { helper: None, pacman_asked: true });
        assert_eq!(warning, None);
    }

    #[test]
    fn a_missing_or_unknown_helper_warns_and_falls_back_to_auto() {
        let (choice, warning) = choose(Some(("yay", ENV)), installed(&["paru"]));
        assert_eq!(choice.helper, Some("paru"));
        assert_eq!(warning.as_deref(), Some("paku: yay (set in PAKU_HELPER) isn't installed; using paru"));
        let (choice, warning) = choose(Some(("aura", "~/.config/paku/config")), installed(&[]));
        assert_eq!(choice, Choice { helper: None, pacman_asked: false });
        assert_eq!(
            warning.as_deref(),
            Some("paku: unknown helper \"aura\" in ~/.config/paku/config (use auto, paru, yay, pikaur or pacman); using pacman")
        );
    }
}
