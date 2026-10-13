//! Commands of the reader surface: `reader:overlay`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Shows the overlay: the Back button and the page number.
    Overlay,
}

/// Parses a reader action id. None for every other id.
pub fn parse(id: &str) -> Option<Command> {
    match id {
        "reader:overlay" => Some(Command::Overlay),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_overlay_only() {
        assert_eq!(parse("reader:overlay"), Some(Command::Overlay));
        assert_eq!(parse("reader:next"), None);
        assert_eq!(parse("library:next"), None);
    }
}
