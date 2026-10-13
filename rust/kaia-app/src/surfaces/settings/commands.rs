//! Commands of the settings surface: `settings:<section>` opens a section.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Open the section with this name, such as `reading`.
    Section(String),
}

/// Parses a settings action id. None for every other id, and for an empty name.
pub fn parse(id: &str) -> Option<Command> {
    let name = id.strip_prefix("settings:")?;
    if name.is_empty() {
        return None;
    }
    Some(Command::Section(name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_section_names() {
        assert_eq!(
            parse("settings:reading"),
            Some(Command::Section("reading".into()))
        );
        assert_eq!(parse("settings:"), None);
        assert_eq!(parse("nav:settings"), None);
    }
}
