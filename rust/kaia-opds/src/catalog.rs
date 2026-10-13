//! Catalogs the user saves, and the defaults.

use std::fmt;

/// A saved catalog. The username and password belong to the catalog. They are
/// sent only to the catalog's own host and port.
#[derive(Clone, PartialEq, Eq)]
pub struct Catalog {
    pub title: String,
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl fmt::Debug for Catalog {
    /// Hides the password.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Catalog")
            .field("title", &self.title)
            .field("url", &self.url)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "<hidden>"))
            .finish()
    }
}

/// The catalogs KOReader ships with, in the same order.
pub fn default_catalogs() -> Vec<Catalog> {
    [
        (
            "Project Gutenberg",
            "https://m.gutenberg.org/ebooks.opds/?format=opds",
        ),
        ("Standard Ebooks", "https://standardebooks.org/feeds/opds"),
        ("ManyBooks", "http://manybooks.net/opds/index.php"),
        ("Internet Archive", "https://bookserver.archive.org/"),
        ("textos.info (Spanish)", "https://www.textos.info/catalogo.atom"),
        ("Gallica (French)", "https://gallica.bnf.fr/opds"),
    ]
    .into_iter()
    .map(|(title, url)| Catalog {
        title: title.to_owned(),
        url: url.to_owned(),
        username: None,
        password: None,
    })
    .collect()
}

/// Adds `http://` to an address typed without a scheme, as KOReader does.
/// An address that already has a scheme (`name://`) stays as it is.
pub fn normalize_url(input: &str) -> String {
    let input = input.trim();
    let has_scheme = input
        .split_once("://")
        .is_some_and(|(scheme, _)| !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphabetic()));
    if has_scheme {
        input.to_owned()
    } else {
        format!("http://{input}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_plugin() {
        let catalogs = default_catalogs();
        assert_eq!(catalogs.len(), 6);
        assert_eq!(catalogs[0].title, "Project Gutenberg");
        assert_eq!(
            catalogs[0].url,
            "https://m.gutenberg.org/ebooks.opds/?format=opds"
        );
        assert_eq!(catalogs[5].url, "https://gallica.bnf.fr/opds");
        assert!(catalogs
            .iter()
            .all(|c| c.username.is_none() && c.password.is_none()));
    }

    #[test]
    fn debug_output_hides_the_password() {
        let catalog = Catalog {
            title: "Calibre".to_owned(),
            url: "http://192.168.1.2:8080/opds".to_owned(),
            username: Some("reader".to_owned()),
            password: Some("hunter2".to_owned()),
        };
        let text = format!("{catalog:?}");
        assert!(text.contains("reader"));
        assert!(!text.contains("hunter2"));
    }

    #[test]
    fn normalize_adds_http_only_without_a_scheme() {
        assert_eq!(normalize_url(" example.org/opds "), "http://example.org/opds");
        assert_eq!(normalize_url("https://example.org"), "https://example.org");
        assert_eq!(
            normalize_url("192.168.1.2:8080/opds"),
            "http://192.168.1.2:8080/opds"
        );
    }
}
