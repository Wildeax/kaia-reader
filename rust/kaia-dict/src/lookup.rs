//! Lookup stages, result types, and the folder-level `Library`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::dict::Dictionary;
use crate::error::Error;
use crate::html::strip_html;
use crate::index::WordTable;

/// How a result matched the query. Stages run in this order within each dictionary, and the
/// first stage with results in that dictionary wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    /// The headword equals the query byte for byte.
    Exact,
    /// The headword equals the query ignoring ASCII case, or in a common Unicode case form.
    CaseInsensitive,
    /// The query is a synonym from the `.syn` file.
    Synonym,
    /// The query is an English inflection of a headword, such as `dogs` for `dog`.
    Morphology,
    /// The headword comes from sdcv-style edit-distance fallback.
    Fuzzy,
}

/// The kind of a definition, from its StarDict type letter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionKind {
    /// `m`: plain UTF-8 text.
    Plain,
    /// `h`: HTML. Use [`strip_html`] or [`Definition::plain_text`] before showing it.
    Html,
    /// `x`: XDXF markup.
    Xdxf,
    /// `g`: Pango markup.
    Pango,
    /// `t` and `y`: phonetic transcription, wrapped in brackets for display.
    Phonetic,
    /// Any other lowercase type letter. Its text is kept as plain text.
    Other(char),
}

/// A definition's text and what the text is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub kind: DefinitionKind,
    /// Joined text fields. Phonetic fields have brackets and XDXF fields are text.
    pub text: String,
}

impl Definition {
    /// The text without markup, ready for display.
    pub fn plain_text(&self) -> String {
        self.display_text()
    }

    /// KOReader's dictionary display text. HTML keeps paragraph and list layout;
    /// other types keep source line breaks and turn `<br>` into a line break.
    pub fn display_text(&self) -> String {
        if self.kind == DefinitionKind::Html {
            strip_html(&self.text)
        } else {
            crate::html::strip_inline(&self.text, self.kind == DefinitionKind::Xdxf)
        }
    }
}

/// One definition found by a lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Name of the dictionary, from `bookname`.
    pub dict_name: String,
    /// The word whose definition this is. For a synonym or inflection, the target word.
    pub headword: String,
    pub definition: Definition,
    /// The stage that produced this entry.
    pub matched: Match,
}

/// Options for a lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupOptions {
    /// Run the fuzzy stage when nothing matches exactly, by case, by synonym or by inflection.
    pub fuzzy: bool,
    /// Most fuzzy headwords to try, across all dictionaries.
    pub max_fuzzy: usize,
    /// Preferred dictionary names in order. Unlisted dictionaries follow by name.
    pub dictionaries: Option<Vec<String>>,
    /// If true, search only the dictionaries listed above.
    pub exclusive: bool,
    /// Dictionary names excluded from lookup.
    pub disabled: HashSet<String>,
}

impl Default for LookupOptions {
    fn default() -> Self {
        LookupOptions {
            fuzzy: true,
            max_fuzzy: 10,
            dictionaries: None,
            exclusive: false,
            disabled: HashSet::new(),
        }
    }
}

/// Every dictionary found under one folder.
#[derive(Debug, Default)]
pub struct Library {
    dicts: Vec<Dictionary>,
    kobo: Vec<crate::kobo::KoboDictionary>,
    errors: Vec<(PathBuf, Error)>,
}

impl Library {
    /// Opens every `.ifo` file under `dir`, recursively. Keep this instance open
    /// while the app performs lookups so indexes and dictzip handles are reused.
    ///
    /// A dictionary that fails to open does not stop the others. It goes into
    /// [`Library::errors`]. The call fails only when `dir` itself cannot be read.
    pub fn open_dir(dir: impl AsRef<Path>) -> Result<Self, Error> {
        let dir = dir.as_ref();
        let mut ifos = Vec::new();
        collect_ifos(dir, 0, &mut ifos).map_err(|e| Error::from(e).at(dir))?;
        ifos.sort();
        let mut library = Library {
            dicts: Vec::new(),
            kobo: Vec::new(),
            errors: Vec::new(),
        };
        for ifo in ifos {
            match Dictionary::open(&ifo) {
                Ok(dict) => library.dicts.push(dict),
                Err(error) => library.errors.push((ifo, error)),
            }
        }
        Ok(library)
    }

    /// Combines sources, keeping only one copy of a file reached through aliases.
    pub fn append(&mut self, other: Self) {
        for dict in other.dicts {
            if !self
                .dicts
                .iter()
                .any(|d| fs::canonicalize(d.path()).ok() == fs::canonicalize(dict.path()).ok())
            {
                self.dicts.push(dict);
            }
        }
        self.kobo.extend(other.kobo);
        self.errors.extend(other.errors);
    }

    /// Reads Nickel's dictionary folder without creating or changing anything.
    pub fn add_kobo_dir(&mut self, dir: &Path) {
        let Ok(files) = fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<_> = files
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.starts_with("dicthtml") && s.ends_with(".zip"))
            })
            .collect();
        paths.sort();
        for path in paths {
            match crate::kobo::KoboDictionary::open(&path) {
                Ok(dict) => self.kobo.push(dict),
                Err(error) => self.errors.push((path, error)),
            }
        }
    }

    /// Names of StarDict and readable Kobo dictionaries.
    pub fn names(&self) -> Vec<String> {
        self.dicts
            .iter()
            .map(|d| d.name().to_owned())
            .chain(self.kobo.iter().map(|d| d.name().to_owned()))
            .collect()
    }

    /// The dictionaries that opened, in lookup order.
    pub fn dictionaries(&self) -> &[Dictionary] {
        &self.dicts
    }

    /// Dictionaries that failed to open, with the reason.
    pub fn errors(&self) -> &[(PathBuf, Error)] {
        &self.errors
    }

    /// Looks up `word` with the default options.
    pub fn lookup(&self, word: &str) -> Vec<Entry> {
        self.lookup_with(word, &LookupOptions::default())
    }

    /// Looks up `word` across all dictionaries.
    ///
    /// Each dictionary reports its first matching simple stage. If none matches,
    /// fuzzy headwords are chosen across the library. Unreadable entries are skipped.
    pub fn lookup_with(&self, word: &str, opts: &LookupOptions) -> Vec<Entry> {
        let mut refs: Vec<&Dictionary> = self.dicts.iter().collect();
        refs.sort_by(|a, b| {
            let rank = |name: &str| {
                opts.dictionaries
                    .as_ref()
                    .and_then(|v| v.iter().position(|n| n == name))
            };
            rank(a.name())
                .unwrap_or(usize::MAX)
                .cmp(&rank(b.name()).unwrap_or(usize::MAX))
                .then_with(|| a.name().cmp(b.name()))
                .then_with(|| a.path().cmp(b.path()))
        });
        let mut results = run(&refs, word, opts);
        let mut kobo_matches = Vec::new();
        for dict in &self.kobo {
            if opts.disabled.contains(dict.name())
                || (opts.exclusive
                    && !opts
                        .dictionaries
                        .as_ref()
                        .is_some_and(|names| names.iter().any(|n| n == dict.name())))
            {
                continue;
            }
            if let Ok(entries) = dict.lookup(word) {
                kobo_matches.extend(entries);
            }
        }
        if kobo_matches.is_empty() {
            return results;
        }
        // Fuzzy guesses apply only when no source has a direct match.
        results.retain(|entry| entry.matched != Match::Fuzzy);
        results.extend(kobo_matches);
        results.sort_by_key(|entry| {
            (
                opts.dictionaries
                    .as_ref()
                    .and_then(|names| names.iter().position(|n| n == &entry.dict_name))
                    .unwrap_or(usize::MAX),
                entry.dict_name.clone(),
            )
        });
        results
    }
}

fn collect_ifos(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    const MAX_DEPTH: usize = 8;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        // Follow symbolic links, with a depth limit to stop cycles.
        let file_type = fs::metadata(&path)?.file_type();
        if file_type.is_dir() {
            if depth < MAX_DEPTH && entry.file_name() != "res" && entry.file_name() != ".partial" {
                // An unreadable subfolder is skipped; the other folders still load.
                let _ = collect_ifos(&path, depth + 1, out);
            }
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "ifo") {
            out.push(path);
        }
    }
    Ok(())
}

/// Runs the lookup stages over `dicts`.
///
/// Each dictionary is searched on its own. It reports the results of its first stage that
/// matches, as sdcv does. So a dictionary with an exact match and another with only an
/// inflected match both contribute to the result.
pub(crate) fn run(dicts: &[&Dictionary], word: &str, opts: &LookupOptions) -> Vec<Entry> {
    let word = word.trim();
    if word.is_empty() {
        return Vec::new();
    }
    let forced_fuzzy = word.starts_with('/');
    let word = word.strip_prefix('/').unwrap_or(word);
    if !opts.fuzzy && !forced_fuzzy && !word.contains(['*', '?']) {
        let eligible = |dict: &&Dictionary| {
            !opts.disabled.contains(dict.name())
                && (!opts.exclusive
                    || opts
                        .dictionaries
                        .as_ref()
                        .is_some_and(|names| names.iter().any(|n| n == dict.name())))
        };
        for candidate in [word] {
            let mut result = Vec::new();
            for dict in dicts.iter().filter(|dict| eligible(dict)) {
                let index_hits: Vec<_> = dict.index.exact(candidate.as_bytes()).collect();
                for rec in index_hits {
                    if let Some(entry) = dict.entry(rec, Match::Exact) {
                        result.push(entry);
                    }
                }
                if let Some(syn) = &dict.syn {
                    for rec in syn.exact(candidate.as_bytes()) {
                        if let Some(entry) = dict.entry(syn.item(rec).value as usize, Match::Synonym) {
                            result.push(entry);
                        }
                    }
                }
            }
            if !result.is_empty() {
                return result;
            }
        }
        return Vec::new();
    }
    let wildcard = word.contains(['*', '?']);
    let eligible = |dict: &&Dictionary| {
        !opts.disabled.contains(dict.name())
            && (!opts.exclusive
                || opts
                    .dictionaries
                    .as_ref()
                    .is_some_and(|names| names.iter().any(|n| n == dict.name())))
    };
    let dicts: Vec<_> = dicts.iter().copied().filter(eligible).collect();
    if forced_fuzzy {
        return fuzzy_results(&dicts, word, opts.max_fuzzy, opts.fuzzy);
    }
    if wildcard {
        let mut words = Vec::new();
        for dict in &dicts {
            for rec in wildcard_hits(&dict.index, word, 100) {
                let candidate = String::from_utf8_lossy(dict.index.word(rec)).into_owned();
                if !words.contains(&candidate) {
                    words.push(candidate);
                }
            }
        }
        words.sort_by_key(|candidate: &String| candidate.to_lowercase());
        let mut out = Vec::new();
        for candidate in words {
            for dict in &dicts {
                for stage in [Match::Exact, Match::CaseInsensitive, Match::Morphology] {
                    if !opts.fuzzy && stage != Match::Exact {
                        break;
                    }
                    let entries: Vec<_> = stage_hits(dict, stage, &candidate, usize::MAX)
                        .into_iter()
                        .filter_map(|rec| dict.entry(rec, stage))
                        .collect();
                    if !entries.is_empty() {
                        out.extend(entries);
                        break;
                    }
                }
            }
        }
        return out;
    }
    let stages = [Match::Exact, Match::CaseInsensitive, Match::Morphology];
    let mut out = Vec::new();
    for dict in &dicts {
        for stage in stages {
            let synonym_only = matches!(stage, Match::Exact | Match::CaseInsensitive)
                && (if stage == Match::Exact {
                    dict.index.exact(word.as_bytes()).next().is_none()
                } else {
                    dict.index.ascii_fold(word.as_bytes()).next().is_none()
                        && case_variants(word)
                            .iter()
                            .all(|v| dict.index.exact(v.as_bytes()).next().is_none())
                });
            let actual_match = if synonym_only { Match::Synonym } else { stage };
            let entries: Vec<_> = stage_hits(dict, stage, word, usize::MAX)
                .into_iter()
                .filter_map(|rec| dict.entry(rec, actual_match))
                .collect();
            if !entries.is_empty() {
                out.extend(entries);
                break;
            }
        }
    }
    if out.is_empty() && opts.fuzzy {
        fuzzy_results(&dicts, word, opts.max_fuzzy, opts.fuzzy)
    } else {
        out
    }
}

/// Record indexes (file order) that one stage finds in one dictionary.
fn stage_hits(dict: &Dictionary, stage: Match, word: &str, limit: usize) -> Vec<usize> {
    let mut hits = Hits::new(limit);
    match stage {
        Match::Exact => {
            for i in dict.index.exact(word.as_bytes()) {
                hits.add(i);
            }
            if let Some(syn) = &dict.syn {
                for i in syn.exact(word.as_bytes()) {
                    hits.add(syn.item(i).value as usize);
                }
            }
        }
        Match::CaseInsensitive => {
            folded(&dict.index, word, &mut hits);
            if let Some(syn) = &dict.syn {
                let mut found = Hits::new(usize::MAX);
                folded(syn, word, &mut found);
                for i in found.out {
                    hits.add(syn.item(i).value as usize);
                }
            }
        }
        Match::Synonym => {
            if let Some(syn) = &dict.syn {
                let mut found = Hits::new(usize::MAX);
                folded(syn, word, &mut found);
                for i in found.out {
                    hits.add(syn.item(i).value as usize);
                }
            }
        }
        Match::Morphology => {
            for lemma in lemmas(word) {
                let exact: Vec<_> = dict.index.exact(lemma.as_bytes()).collect();
                if exact.is_empty() {
                    folded(&dict.index, &lemma, &mut hits);
                } else {
                    for i in exact {
                        hits.add(i);
                    }
                }
                if !hits.out.is_empty() {
                    break;
                }
            }
        }
        Match::Fuzzy => {}
    }
    hits.out
}

fn wildcard_hits<T>(table: &WordTable<T>, pattern: &str, limit: usize) -> Vec<usize> {
    (0..table.len())
        .filter(|&i| wildcard_match(pattern.as_bytes(), table.word(i)))
        .take(limit)
        .collect()
}

fn wildcard_match(pattern: &[u8], word: &[u8]) -> bool {
    let (mut p, mut w, mut star, mut retry) = (0, 0, None, 0);
    while w < word.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == word[w]) {
            p += 1;
            w += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = w;
        } else if let Some(s) = star {
            retry += 1;
            w = retry;
            p = s + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

/// Cleans a selected word or phrase using `ReaderDictionary:cleanSelection`.
pub fn clean_selection(text: &str) -> String {
    let text = text.replace('\u{a0}', " ");
    let text = text.trim().replace('\u{2019}', "'");
    let mut text: String = text
        .chars()
        .filter(|c| !('\u{2000}'..='\u{206f}').contains(c))
        .collect();
    text = text
        .trim_start_matches(|c: char| c.is_ascii_punctuation())
        .to_owned();
    text = text
        .trim_end_matches(|c: char| c.is_ascii_punctuation())
        .to_owned();
    for mark in ['·', '|', '↑', 'ˈ', 'ˌ'] {
        text = text.replace(mark, "");
    }
    if let Some(stripped) = text.strip_suffix("'s") {
        text = stripped.to_owned();
    }
    if text.len() >= 2 && text.as_bytes()[1] == b'\'' && b"LSDMNTlsdmnt".contains(&text.as_bytes()[0]) {
        text.drain(..2);
    }
    if text.len() >= 3 && text[..2].eq_ignore_ascii_case("qu") && text.as_bytes()[2] == b'\'' {
        text.drain(..3);
    }
    text.trim().to_owned()
}

/// Try the lowercase, uppercase, and title case forms in sdcv's order.
fn folded<T>(table: &WordTable<T>, word: &str, hits: &mut Hits) {
    for variant in case_variants(word) {
        let found: Vec<_> = table.exact(variant.as_bytes()).collect();
        if !found.is_empty() {
            for i in found {
                hits.add(i);
            }
            break;
        }
    }
}

fn case_variants(word: &str) -> Vec<String> {
    let mut capitalised = String::new();
    for (i, c) in word.chars().enumerate() {
        if i == 0 {
            capitalised.extend(c.to_uppercase());
        } else {
            capitalised.extend(c.to_lowercase());
        }
    }
    let candidates = [word.to_lowercase(), capitalised, word.to_uppercase()];
    let mut out: Vec<String> = Vec::new();
    for candidate in candidates {
        if candidate != word && !out.contains(&candidate) {
            out.push(candidate);
        }
    }
    out
}

/// sdcv ranks at most ten fuzzy headwords across all dictionaries, then
/// looks up each headword across every dictionary.
fn fuzzy_results(dicts: &[&Dictionary], word: &str, limit: usize, similar: bool) -> Vec<Entry> {
    let query: Vec<char> = word.to_lowercase().chars().collect();
    if limit == 0 {
        return Vec::new();
    }
    let mut candidates: Vec<(usize, Option<String>)> = vec![(3, None); limit];
    let mut max_distance = 3;
    for dict in dicts {
        for i in 0..dict.index.len() {
            let headword = String::from_utf8_lossy(dict.index.word(i)).into_owned();
            let length = headword.chars().count();
            if length.abs_diff(query.len()) >= max_distance {
                continue;
            }
            let truncated: Vec<char> = headword.to_lowercase().chars().take(query.len()).collect();
            let distance = edit_distance(&truncated, &query, max_distance);
            if distance >= max_distance || distance >= query.len() {
                continue;
            }
            if candidates
                .iter()
                .any(|(_, existing)| existing.as_deref() == Some(&headword))
            {
                continue;
            }
            let replace = candidates
                .iter()
                .rposition(|(distance, _)| *distance == max_distance)
                .unwrap_or(0);
            candidates[replace] = (distance, Some(headword));
            max_distance = candidates
                .iter()
                .map(|(distance, _)| *distance)
                .max()
                .unwrap_or(3);
        }
    }
    candidates.sort_by(|a, b| {
        a.0.cmp(&b.0).then_with(|| {
            a.1.as_ref()
                .map(|s| s.to_lowercase())
                .cmp(&b.1.as_ref().map(|s| s.to_lowercase()))
        })
    });
    let mut out = Vec::new();
    for (_, candidate) in candidates {
        let Some(candidate) = candidate else {
            break;
        };
        for dict in dicts {
            for stage in [Match::Exact, Match::CaseInsensitive, Match::Morphology] {
                if !similar && stage != Match::Exact {
                    break;
                }
                let entries: Vec<_> = stage_hits(dict, stage, &candidate, usize::MAX)
                    .into_iter()
                    .filter_map(|rec| dict.entry(rec, Match::Fuzzy))
                    .collect();
                if !entries.is_empty() {
                    out.extend(entries);
                    break;
                }
            }
        }
    }
    out
}

fn edit_distance(a: &[char], b: &[char], limit: usize) -> usize {
    if a.len().abs_diff(b.len()) > limit {
        return limit + 1;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, &left) in a.iter().enumerate() {
        let mut current = vec![i + 1; b.len() + 1];
        let mut row_min = current[0];
        for (j, &right) in b.iter().enumerate() {
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + usize::from(left != right));
            row_min = row_min.min(current[j + 1]);
        }
        if row_min > limit {
            return limit + 1;
        }
        previous = current;
    }
    previous[b.len()]
}

/// Headword candidates for an English inflection: `dogs`, `boxes`, `liked`, `running`.
fn lemmas(word: &str) -> Vec<String> {
    if !word.is_ascii() {
        return Vec::new();
    }
    let lower = word.to_ascii_lowercase();
    let len = word.len();
    let cut = |n: usize| word[..len - n].to_owned();
    let mut stems: Vec<String> = Vec::new();
    if lower.ends_with("ies") && len > 4 {
        stems.push(format!("{}y", cut(3)));
    }
    if lower.ends_with("ied") && len > 4 {
        stems.push(format!("{}y", cut(3)));
    }
    if lower.ends_with("er") && len > 3 {
        stems.push(cut(2));
    }
    if lower.ends_with("est") && len > 4 {
        stems.push(cut(3));
    }
    if lower.ends_with("ly") && len > 3 {
        stems.push(cut(2));
    }
    if lower.ends_with("es") && len > 3 {
        stems.push(cut(2));
    }
    if lower.ends_with('s') && !lower.ends_with("ss") && len > 3 {
        stems.push(cut(1));
    }
    if lower.ends_with("ed") && len > 3 {
        stems.push(cut(2));
    }
    if lower.ends_with("ing") && len > 4 {
        stems.push(cut(3));
    }
    let mut out: Vec<String> = Vec::new();
    for stem in stems {
        let mut forms = vec![stem.clone(), format!("{stem}e")];
        let bytes = stem.as_bytes();
        if bytes.len() >= 3 && bytes[bytes.len() - 1] == bytes[bytes.len() - 2] {
            forms.push(stem[..stem.len() - 1].to_owned());
        }
        for form in forms {
            if form.len() >= 2 && form != lower && !out.contains(&form) {
                out.push(form);
            }
        }
    }
    out
}

/// Collects record indexes, skipping repeats and stopping at `limit`.
struct Hits {
    out: Vec<usize>,
    seen: HashSet<usize>,
    limit: usize,
}

impl Hits {
    fn new(limit: usize) -> Self {
        Hits {
            out: Vec::new(),
            seen: HashSet::new(),
            limit,
        }
    }

    fn add(&mut self, index: usize) {
        if self.out.len() < self.limit && self.seen.insert(index) {
            self.out.push(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_variants_cover_unicode_forms() {
        assert_eq!(case_variants("Épuisant"), ["épuisant", "ÉPUISANT"]);
        assert_eq!(case_variants("apple"), ["Apple", "APPLE"]);
        assert!(case_variants("ÉPUISANT").contains(&"Épuisant".to_owned()));
    }

    #[test]
    fn edit_distance_covers_short_and_long_typos() {
        assert_eq!(
            edit_distance(&['h', 'e', 'l', 'o'], &['h', 'e', 'l', 'l', 'o'], 3),
            1
        );
        assert_eq!(edit_distance(&['c', 'a', 'e', 't'], &['c', 'a', 'f', 'e'], 3), 2);
    }

    #[test]
    fn lemmas_handle_common_english_inflections() {
        assert!(lemmas("dogs").contains(&"dog".to_owned()));
        assert!(lemmas("boxes").contains(&"box".to_owned()));
        assert!(lemmas("liked").contains(&"like".to_owned()));
        assert!(lemmas("stopped").contains(&"stop".to_owned()));
        assert!(lemmas("running").contains(&"run".to_owned()));
        assert!(lemmas("glass").is_empty());
        assert!(lemmas("épuisantes").is_empty());
        for (word, lemma) in [
            ("cried", "cry"),
            ("faster", "fast"),
            ("fastest", "fast"),
            ("quickly", "quick"),
        ] {
            assert!(lemmas(word).contains(&lemma.to_owned()), "{word}");
        }
    }

    #[test]
    fn selection_matches_luajit_clean_selection_examples() {
        for (input, expected) in [
            ("qu’autrefois,", "autrefois"),
            ("l’amour", "amour"),
            ("dog's", "dog"),
            ("word one", "word one"),
            (" ˈhel·lo|↑ ", "hello"),
            ("d'abord", "abord"),
            ("qu'ami", "ami"),
            ("heˌllo", "hello"),
            ("hi\u{2014}there", "hithere"),
            ("(word)", "word"),
            ("’word’", "word"),
        ] {
            assert_eq!(clean_selection(input), expected, "{input}");
        }
    }

    #[test]
    fn wildcard_matches_star_and_question_mark() {
        assert!(wildcard_match(b"a*e", b"apple"));
        assert!(wildcard_match(b"a??le", b"apple"));
        assert!(!wildcard_match(b"a??le", b"apples"));
    }
}
