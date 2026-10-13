//! Lookup stages, result types, and the folder-level `Library`.

use std::collections::{BTreeSet, HashSet};
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
    /// The headword is a prefix of the query, or differs from it by one edit.
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
    /// `t` and `y`: phonetic transcription, shown as plain text.
    Phonetic,
    /// Any other lowercase type letter. Its text is kept as plain text.
    Other(char),
}

/// A definition's text and what the text is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub kind: DefinitionKind,
    /// Raw text in the dictionary's format.
    pub text: String,
}

impl Definition {
    /// The text without markup, ready for display. Markup kinds go through [`strip_html`].
    pub fn plain_text(&self) -> String {
        match self.kind {
            DefinitionKind::Html | DefinitionKind::Xdxf | DefinitionKind::Pango => strip_html(&self.text),
            _ => self.text.clone(),
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
    /// Most entries the fuzzy stage returns, across all dictionaries.
    pub max_fuzzy: usize,
    /// Only search dictionaries with these names. `None` searches all of them.
    pub dictionaries: Option<Vec<String>>,
}

impl Default for LookupOptions {
    fn default() -> Self {
        LookupOptions {
            fuzzy: true,
            max_fuzzy: 20,
            dictionaries: None,
        }
    }
}

/// Every dictionary found under one folder.
#[derive(Debug)]
pub struct Library {
    dicts: Vec<Dictionary>,
    errors: Vec<(PathBuf, Error)>,
}

impl Library {
    /// Opens every `.ifo` file under `dir`, recursively, in path order.
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
    /// Each dictionary reports the results of its first stage that matches (exact, case
    /// insensitive, synonym, inflection, fuzzy), in dictionary order. Entries whose
    /// definition cannot be read are skipped.
    pub fn lookup_with(&self, word: &str, opts: &LookupOptions) -> Vec<Entry> {
        let refs: Vec<&Dictionary> = self.dicts.iter().collect();
        run(&refs, word, opts)
    }
}

fn collect_ifos(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    const MAX_DEPTH: usize = 8;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        // `file_type` does not follow symbolic links, so a link cycle cannot recurse forever.
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if depth < MAX_DEPTH {
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
    let stages = [
        Match::Exact,
        Match::CaseInsensitive,
        Match::Synonym,
        Match::Morphology,
        Match::Fuzzy,
    ];
    let mut out = Vec::new();
    let mut fuzzy_returned = 0;
    for dict in dicts {
        if let Some(names) = &opts.dictionaries {
            if !names.iter().any(|n| n == dict.name()) {
                continue;
            }
        }
        for stage in stages {
            let limit = if stage == Match::Fuzzy {
                if !opts.fuzzy || fuzzy_returned >= opts.max_fuzzy {
                    break;
                }
                opts.max_fuzzy - fuzzy_returned
            } else {
                usize::MAX
            };
            let entries: Vec<Entry> = stage_hits(dict, stage, word, limit)
                .into_iter()
                .filter_map(|rec| dict.entry(rec, stage))
                .collect();
            if !entries.is_empty() {
                if stage == Match::Fuzzy {
                    fuzzy_returned += entries.len();
                }
                out.extend(entries);
                break;
            }
        }
    }
    out
}

/// Record indexes (file order) that one stage finds in one dictionary.
fn stage_hits(dict: &Dictionary, stage: Match, word: &str, limit: usize) -> Vec<usize> {
    let mut hits = Hits::new(limit);
    match stage {
        Match::Exact => {
            for i in dict.index.exact(word.as_bytes()) {
                hits.add(i);
            }
        }
        Match::CaseInsensitive => folded(&dict.index, word, &mut hits),
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
                folded(&dict.index, &lemma, &mut hits);
            }
        }
        Match::Fuzzy => fuzzy(&dict.index, word, &mut hits),
    }
    hits.out
}

/// Collects matches for `word` ignoring ASCII case, plus matches in the common Unicode
/// case forms. The ASCII-folded search covers StarDict's own rule; the Unicode forms cover
/// accented headwords such as `Épuisant`.
fn folded<T>(table: &WordTable<T>, word: &str, hits: &mut Hits) {
    for i in table.ascii_fold(word.as_bytes()) {
        hits.add(i);
    }
    for variant in case_variants(word) {
        for i in table.exact(variant.as_bytes()) {
            hits.add(i);
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

/// Words within one edit of the query's prefixes and of the query itself. Prefixes must keep
/// at least three characters, and the edit search needs a query of at least four.
fn fuzzy<T>(table: &WordTable<T>, word: &str, hits: &mut Hits) {
    let chars: Vec<char> = word.chars().collect();
    for drop in 1..=3 {
        if chars.len() < drop + 3 {
            break;
        }
        let prefix: String = chars[..chars.len() - drop].iter().collect();
        folded(table, &prefix, hits);
    }
    if chars.len() >= 4 {
        for variant in edits_within_one(&word.to_lowercase()) {
            folded(table, &variant, hits);
        }
    }
}

/// All strings one insertion, deletion, substitution or transposition away from `word`.
fn edits_within_one(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut alphabet: Vec<char> = ('a'..='z').collect();
    for &c in &chars {
        if !c.is_ascii() && !alphabet.contains(&c) {
            alphabet.push(c);
        }
    }
    let joined = |v: &[char]| v.iter().collect::<String>();
    let mut out = BTreeSet::new();
    for i in 0..chars.len() {
        let mut deleted = chars.clone();
        deleted.remove(i);
        out.insert(joined(&deleted));
        if i + 1 < chars.len() {
            let mut swapped = chars.clone();
            swapped.swap(i, i + 1);
            out.insert(joined(&swapped));
        }
        for &c in &alphabet {
            let mut replaced = chars.clone();
            replaced[i] = c;
            out.insert(joined(&replaced));
        }
    }
    for i in 0..=chars.len() {
        for &c in &alphabet {
            let mut inserted = chars.clone();
            inserted.insert(i, c);
            out.insert(joined(&inserted));
        }
    }
    out.remove(word);
    out.into_iter().filter(|s| !s.is_empty()).collect()
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
    fn edits_include_one_insert_delete_substitute_and_swap() {
        let edits = edits_within_one("helo");
        assert!(edits.contains(&"hello".to_owned()));
        assert!(edits.contains(&"help".to_owned()));
        assert!(edits.contains(&"hlo".to_owned()));
        assert!(edits.contains(&"hleo".to_owned()));
        assert!(!edits.contains(&"helo".to_owned()));
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
    }
}
