//! Pure functions over page text: hit testing, word boundaries, text and
//! boxes of a span, and case-insensitive search. The engine thread calls them
//! on [`PageText`] values, so they need no document.

use std::ops::Range;

use kaia_doc::{PageText, PointF, RectF, TextChar, TextLine};

/// Characters that belong to a word for selection. Hyphens and apostrophes
/// stay inside a word.
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '\'' | '\u{2019}' | '-')
}

/// Builds page text from characters with their line numbers. Consecutive
/// characters with the same line number form one line. CREngine reports
/// character boxes this way.
pub fn page_text_from_boxes(boxes: &[(char, RectF, i32)]) -> PageText {
    let mut out = PageText::default();
    let mut current: Option<i32> = None;
    let mut line_start = 0usize;
    let mut line_box: Option<RectF> = None;
    for &(ch, bbox, line) in boxes {
        if current != Some(line) {
            if current.is_some() {
                finish_line(&mut out, line_start, line_box.take());
            }
            current = Some(line);
            line_start = out.chars.len();
        }
        out.text.push(ch);
        out.chars.push(TextChar { ch, bbox });
        line_box = Some(match line_box {
            Some(lb) => union(lb, bbox),
            None => bbox,
        });
    }
    if current.is_some() {
        finish_line(&mut out, line_start, line_box);
    }
    out
}

fn finish_line(out: &mut PageText, first_char: usize, bbox: Option<RectF>) {
    let char_count = out.chars.len() - first_char;
    out.lines.push(TextLine {
        bbox: bbox.unwrap_or_default(),
        first_char,
        char_count,
    });
    out.text.push('\n');
}

fn union(a: RectF, b: RectF) -> RectF {
    RectF::new(a.x0.min(b.x0), a.y0.min(b.y0), a.x1.max(b.x1), a.y1.max(b.y1))
}

/// The index of the character nearest to `p`. A point inside a box is at
/// distance 0. Ties go to the earlier character. None when the page has no text.
pub fn nearest_char(text: &PageText, p: PointF) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, c) in text.chars.iter().enumerate() {
        let b = c.bbox;
        let dx = (b.x0 - p.x).max(0.0).max(p.x - b.x1);
        let dy = (b.y0 - p.y).max(0.0).max(p.y - b.y1);
        let d = dx * dx + dy * dy;
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// The line that holds character `i`.
pub fn line_of(text: &PageText, i: usize) -> Option<&TextLine> {
    text.lines
        .iter()
        .find(|l| i >= l.first_char && i < l.first_char + l.char_count)
}

/// The inclusive character range of the word around character `i`, within its
/// line. None when character `i` is not part of a word.
pub fn word_span(text: &PageText, i: usize) -> Option<(usize, usize)> {
    let line = line_of(text, i)?;
    let last_in_line = line.first_char + line.char_count - 1;
    if !is_word_char(text.chars.get(i)?.ch) {
        return None;
    }
    let mut s = i;
    while s > line.first_char && is_word_char(text.chars[s - 1].ch) {
        s -= 1;
    }
    let mut e = i;
    while e < last_in_line && is_word_char(text.chars[e + 1].ch) {
        e += 1;
    }
    Some((s, e))
}

/// The text of the inclusive character range `first..=last`. A line break
/// becomes one space. The result has no leading or trailing spaces.
pub fn span_text(text: &PageText, first: usize, last: usize) -> String {
    span_text_with_lines(text, first, last, ' ')
}

/// Text of a selection with the line breaks KOReader keeps in highlights.
pub fn span_selected_text(text: &PageText, first: usize, last: usize) -> String {
    span_text_with_lines(text, first, last, '\n')
}

fn span_text_with_lines(text: &PageText, first: usize, last: usize, line_separator: char) -> String {
    let mut out = String::new();
    if first > last || last >= text.chars.len() {
        return out;
    }
    for i in first..=last {
        if i > first && text.lines.iter().any(|l| l.first_char == i) {
            out.push(line_separator);
        }
        out.push(text.chars[i].ch);
    }
    if line_separator == '\n' {
        out.lines()
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    } else {
        out.trim().to_string()
    }
}

/// One box per line for the inclusive character range `first..=last`. Each box
/// is the union of the character boxes on that line.
pub fn span_rects(text: &PageText, first: usize, last: usize) -> Vec<RectF> {
    let mut rects: Vec<RectF> = Vec::new();
    if first > last || last >= text.chars.len() {
        return rects;
    }
    let mut current: Option<usize> = None;
    for i in first..=last {
        let line = line_index(text, i);
        let bbox = text.chars[i].bbox;
        match (current, line, rects.last_mut()) {
            (Some(c), Some(l), Some(r)) if c == l => *r = union(*r, bbox),
            _ => rects.push(bbox),
        }
        current = line;
    }
    rects
}

fn line_index(text: &PageText, i: usize) -> Option<usize> {
    text.lines
        .iter()
        .position(|l| i >= l.first_char && i < l.first_char + l.char_count)
}

/// One character of [`Flat`]. Line ends are spaces with no character index.
struct Item {
    orig: char,
    folded: char,
    index: Option<usize>,
}

/// Page text as one sequence of characters, so a search can match across a line break.
pub struct Flat {
    items: Vec<Item>,
}

/// Lower-cases a character for comparison. Multi-character case folds keep the first character.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

impl Flat {
    pub fn new(text: &PageText) -> Self {
        let mut items = Vec::with_capacity(text.chars.len() + text.lines.len());
        for (n, line) in text.lines.iter().enumerate() {
            if n > 0 {
                items.push(Item {
                    orig: ' ',
                    folded: ' ',
                    index: None,
                });
            }
            for k in line.first_char..line.first_char + line.char_count {
                let orig = text.chars[k].ch;
                items.push(Item {
                    orig,
                    folded: fold(orig),
                    index: Some(k),
                });
            }
        }
        Self { items }
    }

    /// Flat ranges of every match of `needle`. Matching ignores case. Runs of
    /// whitespace in the needle match one space. An empty needle has no matches.
    pub fn find_all(&self, needle: &str) -> Vec<Range<usize>> {
        let needle: Vec<char> = needle
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .map(fold)
            .collect();
        let mut hits = Vec::new();
        if needle.is_empty() || needle.len() > self.items.len() {
            return hits;
        }
        for s in 0..=self.items.len() - needle.len() {
            if needle
                .iter()
                .enumerate()
                .all(|(k, &c)| self.items[s + k].folded == c)
            {
                hits.push(s..s + needle.len());
            }
        }
        hits
    }

    /// The first and last character index inside a flat range, or None when the range holds no character.
    pub fn span(&self, range: &Range<usize>) -> Option<(usize, usize)> {
        let mut indices = self.items[range.clone()].iter().filter_map(|item| item.index);
        let first = indices.next()?;
        let last = indices.next_back().unwrap_or(first);
        Some((first, last))
    }

    /// Text around a flat range, with `context` characters on each side, and
    /// the range of the match inside that text (in characters).
    pub fn snippet(&self, range: &Range<usize>, context: usize) -> (String, Range<usize>) {
        let start = range.start.saturating_sub(context);
        let end = (range.end + context).min(self.items.len());
        let mut text = String::new();
        let mut match_start = 0;
        let mut match_end = 0;
        for (i, item) in self.items[start..end].iter().enumerate() {
            let at = start + i;
            if at == range.start {
                match_start = text.chars().count();
            }
            text.push(item.orig);
            if at + 1 == range.end {
                match_end = text.chars().count();
            }
        }
        let lead = text.chars().take_while(|c| c.is_whitespace()).count();
        let trimmed = text.trim().to_string();
        (
            trimmed,
            match_start.saturating_sub(lead)..match_end.saturating_sub(lead),
        )
    }
}

/// Characters kept on each side of a word when its sentence is looked up.
const SENTENCE_WINDOW: usize = 200;

/// The sentence that holds `needle` on a page. Matching ignores case and line
/// breaks, as [`Flat::find_all`] does. When the text occurs more than once, the
/// occurrence whose first character is nearest `near` wins. A sentence ends at
/// `.`, `!` or `?` followed by a space. When the window of text around the word
/// has no sentence end, the sentence is cut at the edge of the window.
pub fn sentence_around(page: &PageText, needle: &str, near: Option<RectF>) -> Option<String> {
    let flat = Flat::new(page);
    let hit = selection_range(page, &flat, needle, near)?;
    let (window, found) = flat.snippet(&hit, SENTENCE_WINDOW);
    let chars: Vec<char> = window.chars().collect();
    let found = found.start.min(chars.len())..found.end.min(chars.len());
    let ends_sentence =
        |i: usize| matches!(chars[i], '.' | '!' | '?') && chars.get(i + 1).is_none_or(|c| c.is_whitespace());
    let start = (0..found.start)
        .rev()
        .find(|&i| ends_sentence(i))
        .map_or(0, |i| i + 1);
    let end = (found.end..chars.len())
        .find(|&i| ends_sentence(i))
        .map_or(chars.len(), |i| i + 1);
    let sentence = chars[start..end]
        .iter()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!sentence.is_empty()).then_some(sentence)
}

/// Text on either side of the selected occurrence, bounded by the current page.
pub fn selection_context(page: &PageText, needle: &str, near: Option<RectF>) -> Option<(String, String)> {
    let flat = Flat::new(page);
    let hit = selection_range(page, &flat, needle, near)?;
    let (text, range) = flat.snippet(&hit, usize::MAX / 2);
    let chars: Vec<_> = text.chars().collect();
    Some((
        chars[..range.start].iter().collect(),
        chars[range.end..].iter().collect(),
    ))
}

fn selection_range(page: &PageText, flat: &Flat, needle: &str, near: Option<RectF>) -> Option<Range<usize>> {
    let hits = flat.find_all(needle);
    match near {
        Some(target) => hits
            .into_iter()
            .filter_map(|range| {
                let (first, _) = flat.span(&range)?;
                let distance = center_distance(page.chars.get(first)?.bbox, target);
                Some((distance, range))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, range)| range),
        None => hits.into_iter().next(),
    }
}

/// The distance between the centres of two boxes, in page points.
fn center_distance(a: RectF, b: RectF) -> f32 {
    let (ax, ay) = ((a.x0 + a.x1) / 2.0, (a.y0 + a.y1) / 2.0);
    let (bx, by) = ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0);
    ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt()
}

#[cfg(test)]
mod sentence_tests {
    use super::*;

    /// One line of characters per string. Each character is 10 points wide and
    /// each line is 20 points tall.
    fn page_of(lines: &[&str]) -> PageText {
        let mut boxes = Vec::new();
        for (row, line) in lines.iter().enumerate() {
            for (col, ch) in line.chars().enumerate() {
                let x = col as f32 * 10.0;
                let y = row as f32 * 20.0;
                boxes.push((ch, RectF::new(x, y, x + 10.0, y + 20.0), row as i32));
            }
        }
        page_text_from_boxes(&boxes)
    }

    #[test]
    fn the_sentence_holds_the_word_and_ignores_case() {
        let page = page_of(&["The quarrel is over. Their men fight. Then peace."]);
        assert_eq!(
            sentence_around(&page, "Their", None).as_deref(),
            Some("Their men fight.")
        );
        assert_eq!(
            sentence_around(&page, "their", None).as_deref(),
            Some("Their men fight.")
        );
    }

    #[test]
    fn a_sentence_can_cross_a_line_break() {
        let page = page_of(&["It was late. Their men", "fight in the street. Then"]);
        assert_eq!(
            sentence_around(&page, "men fight", None).as_deref(),
            Some("Their men fight in the street.")
        );
    }

    #[test]
    fn the_occurrence_nearest_the_selection_wins() {
        let page = page_of(&["Cat sat. Cat ran."]);
        let near = RectF::new(90.0, 0.0, 100.0, 20.0);
        assert_eq!(
            sentence_around(&page, "Cat", Some(near)).as_deref(),
            Some("Cat ran.")
        );
        assert_eq!(sentence_around(&page, "Cat", None).as_deref(), Some("Cat sat."));
    }

    #[test]
    fn text_not_on_the_page_has_no_sentence() {
        let page = page_of(&["Nothing here."]);
        assert_eq!(sentence_around(&page, "absent", None), None);
        assert_eq!(sentence_around(&page, "   ", None), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two lines: "Hi there" and "friend.", one box per character, 10 units wide.
    fn sample() -> PageText {
        let mut boxes = Vec::new();
        let mut x = 0.0;
        for ch in "Hi there".chars() {
            boxes.push((ch, RectF::new(x, 0.0, x + 10.0, 12.0), 0));
            x += 10.0;
        }
        x = 0.0;
        for ch in "friend.".chars() {
            boxes.push((ch, RectF::new(x, 14.0, x + 10.0, 26.0), 1));
            x += 10.0;
        }
        page_text_from_boxes(&boxes)
    }

    #[test]
    fn builds_lines_and_text() {
        let t = sample();
        assert_eq!(t.lines.len(), 2);
        assert_eq!(t.text, "Hi there\nfriend.\n");
        assert_eq!(t.chars.len(), 15);
    }

    #[test]
    fn word_at_point_and_boundaries() {
        let t = sample();
        let i = nearest_char(&t, PointF { x: 35.0, y: 5.0 }).expect("char");
        assert_eq!(t.chars[i].ch, 't');
        assert_eq!(word_span(&t, i), Some((3, 7)));
        assert_eq!(span_text(&t, 3, 7), "there");
        assert_eq!(word_span(&t, 2), None, "a space is not part of a word");
    }

    #[test]
    fn span_rects_split_by_line() {
        let t = sample();
        let rects = span_rects(&t, 6, 10);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0], RectF::new(60.0, 0.0, 80.0, 12.0));
        assert_eq!(rects[1], RectF::new(0.0, 14.0, 30.0, 26.0));
        assert_eq!(span_text(&t, 6, 10), "re fri");
        assert_eq!(span_selected_text(&t, 6, 10), "re\nfri");
    }

    #[test]
    fn search_crosses_line_breaks_and_ignores_case() {
        let t = sample();
        let flat = Flat::new(&t);
        let hits = flat.find_all("HI   THERE friend");
        assert_eq!(hits.len(), 1);
        let (first, last) = flat.span(&hits[0]).expect("span");
        assert_eq!((first, last), (0, 13));
        let (snip, m) = flat.snippet(&hits[0], 4);
        assert_eq!(snip, "Hi there friend.");
        let matched: String = snip.chars().skip(m.start).take(m.len()).collect();
        assert_eq!(matched, "Hi there friend");
        assert!(flat.find_all("   ").is_empty());
    }
}
