//! The Arch news for `paku maintain`: the RSS feed of archlinux.org, read without a parser crate. The text comes
//! from the network and goes to the terminal, so the reader is defensive: anything unexpected gives no news (the
//! step is skipped), control characters become "?", and a link is kept only when it is on archlinux.org.

/// The feed (HTTPS only; curl gets `--proto =https`).
pub const FEED: &str = "https://archlinux.org/feeds/news/";

/// At most this many items are shown.
pub const MAX: usize = 5;

/// One news item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    /// The link, only when it starts with `https://archlinux.org/`.
    pub link: Option<String>,
    /// What identifies the item in the state file: the link as published (or the title without one).
    pub id: String,
}

/// The items of the feed, newest first as published. Empty for anything that isn't an RSS feed with items.
pub fn parse(feed: &str) -> Vec<Item> {
    if !feed.contains("<rss") {
        return Vec::new();
    }
    let mut items = Vec::new();
    for chunk in feed.split("<item>").skip(1) {
        let Some(body) = chunk.split("</item>").next() else { continue };
        let Some(title) = tag(body, "title").map(|t| clean(&t)).filter(|t| !t.is_empty()) else { continue };
        let raw_link = tag(body, "link").map(|l| l.trim().to_string()).unwrap_or_default();
        let link = (raw_link.starts_with("https://archlinux.org/") && raw_link.chars().all(|c| c.is_ascii_graphic()))
            .then(|| raw_link.clone());
        let id = if raw_link.is_empty() { title.clone() } else { clean(&raw_link) };
        items.push(Item { title, link, id });
    }
    items
}

/// The items newer than the one seen last (`last` is its id): the ones before it in the feed, at most MAX. With
/// no `last` (the first run), the MAX newest; with a `last` no longer in the feed, also the MAX newest.
pub fn unseen<'a>(items: &'a [Item], last: Option<&str>) -> &'a [Item] {
    let end = last.and_then(|id| items.iter().position(|i| i.id == id)).unwrap_or(items.len());
    &items[..end.min(MAX)]
}

/// The text of `<name>…</name>` in an item, with CDATA unwrapped and the entities decoded.
fn tag(body: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = body.find(&open)? + open.len();
    let len = body[start..].find(&close)?;
    let raw = body[start..start + len].trim();
    Some(match raw.strip_prefix("<![CDATA[").and_then(|r| r.strip_suffix("]]>")) {
        Some(cdata) => cdata.to_string(),
        None => decode(raw),
    })
}

/// The basic XML entities and numeric references; an unknown one stays as it is.
fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// One line for the terminal: control characters (ESC included) become "?", runs of whitespace one space.
fn clean(text: &str) -> String {
    let mapped: String = text.chars().map(|c| if c.is_control() && c != '\n' && c != '\t' { '?' } else { c }).collect();
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED_SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0"><channel><title>Arch Linux: Recent news updates</title>
<item><title>Manual intervention for pacman 7.2 &amp; libalpm</title><link>https://archlinux.org/news/manual-intervention/</link><description>&lt;p&gt;body&lt;/p&gt;</description></item>
<item><title><![CDATA[The <b>valkey</b> & redis switch]]></title><link>https://archlinux.org/news/valkey/</link></item>
<item><title>Evil &#27;[31mred&#x1b;[0m</title><link>https://evil.example/x</link></item>
<item><title>Old news</title><link>https://archlinux.org/news/old/</link></item>
</channel></rss>"#;

    #[test]
    fn it_reads_titles_links_cdata_and_entities() {
        let items = parse(FEED_SAMPLE);
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].title, "Manual intervention for pacman 7.2 & libalpm");
        assert_eq!(items[0].link.as_deref(), Some("https://archlinux.org/news/manual-intervention/"));
        assert_eq!(items[1].title, "The <b>valkey</b> & redis switch");
    }

    #[test]
    fn control_characters_and_foreign_links_never_reach_the_terminal() {
        let items = parse(FEED_SAMPLE);
        // &#27; and &#x1b; are ESC: shown as "?"
        assert_eq!(items[2].title, "Evil ?[31mred?[0m");
        assert!(!items[2].title.chars().any(|c| c.is_control()));
        assert_eq!(items[2].link, None);
        assert_eq!(items[2].id, "https://evil.example/x");
    }

    #[test]
    fn anything_else_gives_no_news() {
        for text in ["", "<html>Bad gateway</html>", "<rss><item><link>x</link></item></rss>", "<rss><item><title>"] {
            assert!(parse(text).is_empty(), "{text:?}");
        }
        // A broken entity doesn't panic
        assert_eq!(decode("a & b &#xZZ; &#99999999; &unknown;"), "a & b &#xZZ; &#99999999; &unknown;");
    }

    #[test]
    fn only_the_unseen_ones_at_most_five() {
        let items = parse(FEED_SAMPLE);
        assert_eq!(unseen(&items, None).len(), 4);
        assert_eq!(unseen(&items, Some("https://archlinux.org/news/valkey/")).len(), 1);
        assert!(unseen(&items, Some("https://archlinux.org/news/manual-intervention/")).is_empty());
        assert_eq!(unseen(&items, Some("https://archlinux.org/news/gone/")).len(), 4);
        let many: Vec<Item> =
            (0..9).map(|n| Item { title: format!("t{n}"), link: None, id: format!("t{n}") }).collect();
        assert_eq!(unseen(&many, None).len(), MAX);
    }
}
