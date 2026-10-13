//! Re-anchoring of reflowable stickers by text quote.
//!
//! Rules from `STORAGE.md`:
//!
//! 1. The first occurrence of `prefix + exact + suffix` wins.
//! 2. Otherwise, the occurrence of `exact` whose surrounding text matches
//!    `prefix` and `suffix` at the most character positions wins. Ties go to
//!    the earliest occurrence.
//! 3. Otherwise there is no match.
//!
//! Positions are char indexes (Unicode scalar values), not byte offsets.

/// Returns the char index of the best match for `exact` in `haystack`, or `None`.
///
/// An empty `exact` never matches. The haystack is converted to chars once,
/// and candidates are compared in place without allocating.
pub fn locate(haystack: &str, exact: &str, prefix: &str, suffix: &str) -> Option<usize> {
    if exact.is_empty() {
        return None;
    }
    let h: Vec<char> = haystack.chars().collect();
    let e: Vec<char> = exact.chars().collect();
    let p: Vec<char> = prefix.chars().collect();
    let s: Vec<char> = suffix.chars().collect();
    let (n, m) = (h.len(), e.len());
    if m > n {
        return None;
    }

    let mut best: Option<(usize, usize)> = None; // (score, index), earliest wins ties
    for i in 0..=n - m {
        if h[i] != e[0] || h[i..i + m] != e[..] {
            continue;
        }
        if ends_with_chars(&h[..i], &p) && h[i + m..].starts_with(&s) {
            return Some(i);
        }
        let score = context_score(&h, i, m, &p, &s);
        match best {
            Some((best_score, _)) if best_score >= score => {}
            _ => best = Some((score, i)),
        }
    }
    best.map(|(_, i)| i)
}

fn ends_with_chars(before: &[char], p: &[char]) -> bool {
    before.ends_with(p)
}

/// Counts matching positions. `prefix` is aligned to the chars just before the
/// match (right to left), and `suffix` to the chars just after it (left to right).
fn context_score(h: &[char], i: usize, m: usize, p: &[char], s: &[char]) -> usize {
    let prefix_hits = p
        .iter()
        .rev()
        .zip(h[..i].iter().rev())
        .filter(|(expected, actual)| expected == actual)
        .count();
    let suffix_hits = s
        .iter()
        .zip(h[i + m..].iter())
        .filter(|(expected, actual)| expected == actual)
        .count();
    prefix_hits + suffix_hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn empty_exact_never_matches() {
        assert_eq!(locate("anything", "", "a", "b"), None);
        assert_eq!(locate("", "", "", ""), None);
    }

    #[test]
    fn missing_quote_is_none() {
        assert_eq!(locate("the cat sat", "dog", "", ""), None);
        assert_eq!(locate("short", "much longer quote", "", ""), None);
        assert_eq!(locate("", "x", "", ""), None);
    }

    #[test]
    fn finds_simple_match_with_context() {
        let text = "one two three four";
        assert_eq!(locate(text, "three", "two ", " four"), Some(8));
    }

    #[test]
    fn index_counts_chars_not_bytes() {
        let text = "éèà target here";
        assert_eq!(text.find("target").map(|b| text[..b].chars().count()), Some(4));
        assert_eq!(locate(text, "target", "à ", " here"), Some(4));
    }

    #[test]
    fn full_context_match_beats_earlier_bare_match() {
        // First "cat" has no context. The second has the full prefix and suffix.
        let text = "cat dog. the cat sat.";
        assert_eq!(locate(text, "cat", "the ", " sat"), Some(13));
    }

    #[test]
    fn full_context_match_is_earliest_among_full_matches() {
        let text = "x-cat-y and x-cat-y";
        assert_eq!(locate(text, "cat", "x-", "-y"), Some(2));
    }

    #[test]
    fn partial_context_prefers_higher_score() {
        // Neither occurrence has the full context. The second one matches more characters.
        let text = "zzz cat qqq ... abc cat xyz";
        // Prefix "abc " and suffix "xyz". Second "cat" matches 4 prefix chars and 3 suffix chars.
        // First "cat" matches 1 prefix char ("z" vs " " is no match, so 0) and 0 suffix chars.
        assert_eq!(locate(text, "cat", "abc ", "xyz"), Some(20));
    }

    #[test]
    fn partial_context_ties_go_to_earliest() {
        let text = "aa cat bb ... aa cat bb";
        // Both occurrences score the same against prefix "qq" and suffix "zz": 0 hits.
        assert_eq!(locate(text, "cat", "qq", "zz"), Some(3));
    }

    #[test]
    fn empty_context_returns_first_occurrence() {
        assert_eq!(locate("abcabc", "bc", "", ""), Some(1));
    }

    #[test]
    fn insertion_before_quote_is_found_at_new_position() {
        let before = "Alpha beta. The needle sits here. Omega.";
        let after = "Inserted sentence in front. Alpha beta. The needle sits here. Omega.";
        assert_eq!(locate(before, "needle", "The ", " sits"), Some(16));
        let shifted = locate(after, "needle", "The ", " sits").unwrap();
        assert_eq!(after.chars().skip(shifted).take(6).collect::<String>(), "needle");
        assert_eq!(shifted, 16 + "Inserted sentence in front. ".chars().count());
    }

    #[test]
    fn removed_quote_is_none() {
        let before = "Alpha beta. The needle sits here. Omega.";
        let after = "Alpha beta. The pin sits here. Omega.";
        assert!(locate(before, "needle", "The ", " sits").is_some());
        assert_eq!(locate(after, "needle", "The ", " sits"), None);
    }

    #[test]
    fn exact_longer_than_haystack_is_none() {
        assert_eq!(locate("ab", "abc", "", ""), None);
    }

    #[test]
    fn match_at_end_of_text_with_suffix_longer_than_remaining() {
        assert_eq!(locate("start end", "end", "start ", "more text after"), Some(6));
    }

    proptest! {
        #[test]
        fn finds_quote_inserted_into_random_text(
            before in "\\PC{0,200}",
            exact in "\\PC{1,40}",
            after in "\\PC{0,200}",
        ) {
            let haystack = format!("{before}{exact}{after}");
            let b: Vec<char> = before.chars().collect();
            let a: Vec<char> = after.chars().collect();
            let prefix: String = b[b.len().saturating_sub(64)..].iter().collect();
            let suffix: String = a[..a.len().min(64)].iter().collect();

            let found = locate(&haystack, &exact, &prefix, &suffix);
            let idx = found.expect("quote was inserted, so it must be found");
            // The first full match is at or before the inserted position.
            prop_assert!(idx <= b.len());
            let hc: Vec<char> = haystack.chars().collect();
            let ec: Vec<char> = exact.chars().collect();
            prop_assert_eq!(&hc[idx..idx + ec.len()], &ec[..]);
        }

        #[test]
        fn never_panics_on_arbitrary_unicode(
            haystack in any::<String>(),
            exact in any::<String>(),
            prefix in any::<String>(),
            suffix in any::<String>(),
        ) {
            if let Some(idx) = locate(&haystack, &exact, &prefix, &suffix) {
                let hlen = haystack.chars().count();
                let elen = exact.chars().count();
                prop_assert!(idx + elen <= hlen);
            }
        }
    }
}
