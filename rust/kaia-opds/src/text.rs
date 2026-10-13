//! Plain text from feed content.

/// Removes HTML tags, decodes the common entities and collapses white space.
///
/// `<` only starts a tag when a letter, `/` or `!` follows it, so text such
/// as `a < b` keeps its content.
pub(crate) fn plain_text(html: &str) -> String {
    let chars: Vec<char> = html.chars().collect();
    let mut stripped = String::with_capacity(html.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let starts_tag = c == '<'
            && chars
                .get(i + 1)
                .is_some_and(|next| next.is_ascii_alphabetic() || *next == '/' || *next == '!');
        if starts_tag {
            match chars[i..].iter().position(|&ch| ch == '>') {
                Some(end) => {
                    let name: String = chars[i + 1..i + end]
                        .iter()
                        .skip_while(|&&ch| ch == '/')
                        .take_while(|ch| ch.is_ascii_alphanumeric())
                        .map(|ch| ch.to_ascii_lowercase())
                        .collect();
                    if is_block(&name) {
                        stripped.push(' ');
                    }
                    i += end + 1;
                }
                // An unclosed tag at the end of the text: keep the rest.
                None => {
                    stripped.extend(&chars[i..]);
                    break;
                }
            }
            continue;
        }
        stripped.push(c);
        i += 1;
    }
    collapse(&decode_entities(&stripped))
}

/// Elements that start a new line of text.
pub(crate) fn is_block(local: &str) -> bool {
    matches!(
        local,
        "p" | "div"
            | "br"
            | "li"
            | "ul"
            | "ol"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "tr"
            | "td"
            | "th"
            | "blockquote"
            | "section"
            | "article"
            | "table"
            | "dt"
            | "dd"
    )
}

/// Trims the ends and turns every run of white space into one space.
pub(crate) fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest.find(';').filter(|&end| end <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..semi];
        match decode_name(name) {
            Some(ch) => {
                out.push(ch);
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

fn decode_name(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse::<u32>().ok()?,
            };
            char::from_u32(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_decodes_entities() {
        assert_eq!(
            plain_text("<p>Tom &amp; Jerry</p>\n<p>caf&#233; &#x2014;</p>"),
            "Tom & Jerry caf\u{e9} \u{2014}"
        );
    }

    #[test]
    fn inline_tags_join_words_and_block_tags_split_them() {
        assert_eq!(
            plain_text("A novel about <em>manners</em>."),
            "A novel about manners."
        );
        assert_eq!(plain_text("<p>One</p><p>Two</p>"), "One Two");
    }

    #[test]
    fn keeps_a_bare_less_than_sign() {
        assert_eq!(plain_text("a < b and c > d"), "a < b and c > d");
    }

    #[test]
    fn unknown_entity_stays_as_written() {
        assert_eq!(plain_text("x &bogus; y"), "x &bogus; y");
    }
}
