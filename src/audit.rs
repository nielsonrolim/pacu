//! The "Arch Linux AUR Compromised" list: one package name per line, inside a markdown code fence.

/// The package names of the list: the fence lines (```) and the empty ones left out.
pub fn parse_list(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("```")).map(String::from).collect()
}

/// The installed packages that are on the list, in the installed order.
pub fn hits(installed: &[String], list: &[String]) -> Vec<String> {
    let list: std::collections::HashSet<&str> = list.iter().map(String::as_str).collect();
    installed.iter().filter(|p| list.contains(p.as_str())).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_as_published() {
        // The head of the real download, with a CRLF line and a blank one
        let text = "```\n123pan-bin\n1code\r\n\n8188eu-dkms\n```\n";
        assert_eq!(parse_list(text), ["123pan-bin", "1code", "8188eu-dkms"]);
        assert!(parse_list("").is_empty());
        assert!(parse_list("```\n```\n").is_empty());
    }

    #[test]
    fn only_exact_names_match() {
        let list = parse_list("```\n1code\nfoo-bin\n```\n");
        let installed: Vec<String> = ["foo", "foo-bin", "1code-git", "1code"].map(String::from).to_vec();
        assert_eq!(hits(&installed, &list), ["foo-bin", "1code"]);
    }
}
