//! Converts HTML and XDXF markup to plain text for display.

/// Removes markup and returns plain text with paragraph and list breaks kept.
///
/// - `p`, `h1`-`h6`, `table`, `ul`, `ol`, `dl`, `blockquote` and similar start a new paragraph.
/// - `div`, `tr`, `dt`, `dd` and `br` start a new line. `hr` starts a new paragraph.
/// - `li` starts a new line with `- ` in front.
/// - `td` and `th` put a space between cells.
/// - `script` and `style` elements are dropped with their content. Comments and the
///   doctype are dropped.
/// - `<![CDATA[...]]>` content is kept as text, as XDXF uses it.
/// - Whitespace collapses to single spaces. At most one blank line separates paragraphs.
/// - Entities `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;` and numeric references
///   are decoded. Other named entities are kept as written.
///
/// A `<` that does not start a tag is kept as text.
pub fn strip_html(input: &str) -> String {
    let mut out = Output::default();
    let mut i = 0;
    while i < input.len() {
        let rest = &input[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").map_or(rest.len(), |end| end + 3);
        } else if let Some(body) = rest.strip_prefix("<![CDATA[") {
            let end = body.find("]]>").unwrap_or(body.len());
            out.text(&body[..end]);
            i += 9 + (end + 3).min(body.len());
        } else if rest.starts_with('<') && starts_tag(rest) {
            // An unclosed tag at the end of the input is dropped.
            let Some(end) = rest.find('>') else {
                break;
            };
            let tag = &rest[1..end];
            i += end + 1;
            let name = tag_name(tag);
            let closing = tag.starts_with('/');
            if !closing && matches!(name.as_str(), "script" | "style") {
                i += skip_element(&input[i..], &name);
                out.break_lines(2);
            } else {
                out.tag(&name, closing);
            }
        } else if rest.starts_with('&') {
            match decode_entity(rest) {
                Some((decoded, used)) => {
                    out.text(&decoded);
                    i += used;
                }
                None => {
                    out.text("&");
                    i += 1;
                }
            }
        } else {
            // The first character is not a tag or entity start, so search after it. Both
            // `<` and `&` are ASCII, so the match is on a char boundary.
            let first = rest.chars().next().map_or(1, char::len_utf8);
            let next = rest[first..].find(['<', '&']).map_or(rest.len(), |p| p + first);
            out.text(&rest[..next]);
            i += next;
        }
    }
    out.finish()
}

/// A `<` starts a tag when a letter, `/`, `!` or `?` follows it.
fn starts_tag(rest: &str) -> bool {
    rest[1..]
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?'))
}

/// Lowercase element name from the text between `<` and `>`.
fn tag_name(tag: &str) -> String {
    tag.trim_start_matches('/')
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Bytes to skip from just after an opening `<script>` or `<style>` tag, through its
/// closing tag. Skips to the end of the input when the closing tag is missing.
fn skip_element(after_open: &str, name: &str) -> usize {
    let lower = after_open.to_ascii_lowercase();
    match lower.find(&format!("</{name}")) {
        Some(pos) => {
            let rest = &after_open[pos..];
            pos + rest.find('>').map_or(rest.len(), |e| e + 1)
        }
        None => after_open.len(),
    }
}

/// Decodes one entity at the start of `s`. Returns the text and the bytes used.
fn decode_entity(s: &str) -> Option<(String, usize)> {
    let end = s.find(';').filter(|&e| e <= 10)?;
    let text = match &s[1..end] {
        "amp" => "&".to_owned(),
        "lt" => "<".to_owned(),
        "gt" => ">".to_owned(),
        "quot" => "\"".to_owned(),
        "apos" => "'".to_owned(),
        "nbsp" => "\u{a0}".to_owned(),
        name => decode_numeric(name)?.to_string(),
    };
    Some((text, end + 1))
}

fn decode_numeric(name: &str) -> Option<char> {
    let digits = name.strip_prefix('#')?;
    let code = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => digits.parse().ok()?,
    };
    char::from_u32(code)
}

/// Accumulates text and the layout breaks that are waiting for the next visible character.
#[derive(Default)]
struct Output {
    text: String,
    /// Line breaks requested by block tags, at most two.
    pending_breaks: usize,
    /// Whitespace was seen since the last visible character.
    pending_space: bool,
    /// A list item started and its `- ` marker is still to be written.
    list_marker: bool,
}

impl Output {
    fn text(&mut self, s: &str) {
        for c in s.chars() {
            if c.is_whitespace() {
                self.pending_space = true;
            } else {
                self.apply_layout();
                self.text.push(c);
            }
        }
    }

    /// Writes the waiting breaks, space, or list marker in front of a visible character.
    fn apply_layout(&mut self) {
        let at_start = self.text.is_empty();
        if self.pending_breaks > 0 && !at_start {
            while self.text.ends_with(' ') {
                self.text.pop();
            }
            let have = self.text.chars().rev().take_while(|&c| c == '\n').count();
            for _ in have..self.pending_breaks {
                self.text.push('\n');
            }
        } else if self.pending_space && !at_start && !self.text.ends_with('\n') {
            self.text.push(' ');
        }
        if self.list_marker && (at_start || self.text.ends_with('\n')) {
            self.text.push_str("- ");
            self.list_marker = false;
        }
        self.pending_breaks = 0;
        self.pending_space = false;
    }

    fn break_lines(&mut self, n: usize) {
        self.pending_breaks = self.pending_breaks.max(n.min(2));
    }

    fn tag(&mut self, name: &str, closing: bool) {
        match name {
            "br" => self.break_lines(1),
            "hr" => self.break_lines(2),
            "li" => {
                self.break_lines(1);
                self.list_marker = !closing;
            }
            // Dictionaries put one item per div, so a div closes a line, not a paragraph.
            "div" | "dt" | "dd" | "tr" => self.break_lines(1),
            "td" | "th" => self.pending_space = true,
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "table" | "ul" | "ol" | "dl" | "blockquote"
            | "pre" | "section" | "article" | "header" | "footer" => self.break_lines(2),
            _ => {}
        }
    }

    fn finish(self) -> String {
        self.text.trim_end().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::strip_html;

    #[test]
    fn paragraphs_keep_one_blank_line() {
        assert_eq!(
            strip_html("<p>First   para\n with words.</p>\n\n<p>Second</p>"),
            "First para with words.\n\nSecond"
        );
    }

    #[test]
    fn line_breaks_and_list_items_stay_on_their_own_lines() {
        assert_eq!(
            strip_html("noun<br/>sense one<br>sense two"),
            "noun\nsense one\nsense two"
        );
        assert_eq!(
            strip_html("<ul><li>apple</li><li>pear</li></ul>"),
            "- apple\n- pear"
        );
    }

    #[test]
    fn divs_end_lines_and_do_not_add_blank_lines() {
        assert_eq!(strip_html("<div>a</div><div>b</div>"), "a\nb");
        assert_eq!(
            strip_html(
                "<div><ol><li><div lang=\"de\">Geschlecht</div></li><li><div>Familie</div></li></ol></div>"
            ),
            "- Geschlecht\n- Familie"
        );
        assert_eq!(
            strip_html("<div>Haus</div><div><a>houses</a> <a>open house</a></div>"),
            "Haus\nhouses open house"
        );
    }

    #[test]
    fn scripts_styles_comments_and_doctype_are_dropped() {
        let html = "<!DOCTYPE html><!-- note --><style>p{color:red}</style>\
                    <script>if (a < b) { x(); }</script>Visible";
        assert_eq!(strip_html(html), "Visible");
    }

    #[test]
    fn entities_are_decoded() {
        assert_eq!(
            strip_html("a &amp; b &lt;c&gt; &quot;d&quot; &#65;&#x42; &nbsp;e"),
            "a & b <c> \"d\" AB e"
        );
        assert_eq!(strip_html("&unknown; stays"), "&unknown; stays");
        assert_eq!(strip_html("fish &amp chips"), "fish &amp chips");
    }

    #[test]
    fn stray_less_than_signs_are_text() {
        assert_eq!(strip_html("2 < 3 and 5 > 4"), "2 < 3 and 5 > 4");
    }

    #[test]
    fn cdata_is_text_and_inline_tags_vanish() {
        assert_eq!(
            strip_html("<ex><![CDATA[<b>raw</b>]]></ex> <b>bold</b> word"),
            "<b>raw</b> bold word"
        );
    }

    #[test]
    fn unicode_text_passes_through() {
        assert_eq!(strip_html("<p>Épuisant, adj.</p>"), "Épuisant, adj.");
    }

    #[test]
    fn empty_and_tag_only_input() {
        assert_eq!(strip_html(""), "");
        assert_eq!(strip_html("<p></p><br>"), "");
    }
}
