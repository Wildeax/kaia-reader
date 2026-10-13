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
    let mut out = String::new();
    if first > last || last >= text.chars.len() {
        return out;
    }
    for i in first..=last {
        if i > first && text.lines.iter().any(|l| l.first_char == i) {
            out.push(' ');
        }
        out.push(text.chars[i].ch);
    }
    out.trim().to_string()
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
