use linkify::{LinkFinder, LinkKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Offsets in characters, the unit the text buffer uses.
    pub start: usize,
    pub end: usize,
    pub href: String,
    pub is_email: bool,
}

/// The web addresses and email addresses in a text.
pub fn find(text: &str) -> Vec<Link> {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url, LinkKind::Email]);
    finder
        .links(text)
        .map(|l| {
            let is_email = *l.kind() == LinkKind::Email;
            let href = if is_email { format!("mailto:{}", l.as_str()) } else { l.as_str().to_string() };
            Link {
                start: text[..l.start()].chars().count(),
                end: text[..l.end()].chars().count(),
                href,
                is_email,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_are_in_characters() {
        let links = find("è qui: https://bwlab.it/x e info@bwlab.it");
        assert_eq!(links.len(), 2);
        assert_eq!((links[0].start, links[0].end), (7, 25));
        assert_eq!(links[0].href, "https://bwlab.it/x");
        assert!(links[1].is_email && links[1].href == "mailto:info@bwlab.it");
    }
}
