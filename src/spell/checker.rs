//! The ISpellChecker adapter: one checker per installed language, with a
//! cache of what each said about a word.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Globalization::{ISpellChecker, ISpellCheckerFactory, SpellCheckerFactory};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};

/// The languages scrivi checks, in the order suggestions are listed.
pub const LANGUAGES: [&str; 2] = ["it-IT", "en-US"];

/// Windows' spell checkers for the installed languages among `LANGUAGES`.
/// Single-threaded: COM objects of the UI thread's apartment.
pub struct Checkers {
    list: Vec<(&'static str, ISpellChecker)>,
    /// Per language, per word: did that checker reject it.
    cache: RefCell<Vec<HashMap<String, bool>>>,
    /// Words added this session, lowercase: Windows applies `Add` a moment
    /// later, and the flag must stop at once.
    added: RefCell<HashSet<String>>,
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

impl Checkers {
    /// Open the checkers for `tags`, skipping languages that aren't installed.
    pub fn open(tags: &[&'static str]) -> Checkers {
        let mut list = Vec::new();
        unsafe {
            // Already initialised (S_FALSE) or by another model: both are fine.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if let Ok(factory) = CoCreateInstance::<_, ISpellCheckerFactory>(
                &SpellCheckerFactory,
                None,
                CLSCTX_INPROC_SERVER,
            ) {
                for tag in tags {
                    let wide = HSTRING::from(*tag);
                    if factory.IsSupported(&wide).is_ok_and(|b| b.as_bool()) {
                        if let Ok(checker) = factory.CreateSpellChecker(&wide) {
                            list.push((*tag, checker));
                        }
                    }
                }
            }
        }
        let cache = RefCell::new(vec![HashMap::new(); list.len()]);
        Checkers {
            list,
            cache,
            added: RefCell::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// `None` if the checker failed to answer.
    fn reject_uncached(checker: &ISpellChecker, word: &str) -> Option<bool> {
        let wide = to_wide(word);
        unsafe {
            let errors = checker.Check(PCWSTR(wide.as_ptr())).ok()?;
            let mut error = None;
            // S_OK: an error was returned; S_FALSE: the text is clean.
            Some(errors.Next(&mut error).0 == 0 && error.is_some())
        }
    }

    /// Whether the checker at `index` rejects `word`.
    fn rejects(&self, index: usize, word: &str) -> bool {
        if let Some(known) = self.cache.borrow()[index].get(word) {
            return *known;
        }
        // A failed check says nothing: don't remember it.
        let Some(rejected) = Self::reject_uncached(&self.list[index].1, word) else {
            return false;
        };
        self.cache.borrow_mut()[index].insert(word.to_string(), rejected);
        rejected
    }

    /// Whether every checker rejects `word` or its elided tail (the combine rule).
    pub fn is_misspelled(&self, word: &str) -> bool {
        let normal = word.replace('\u{2019}', "'");
        let all_reject = |w: &str| {
            !self.added.borrow().contains(&w.to_lowercase())
                && super::combine((0..self.list.len()).map(|i| self.rejects(i, w)))
        };
        super::is_misspelled(&normal, &[&all_reject])
    }

    /// Up to `max` suggestions from either language, alternating, no repeats.
    pub fn suggest(&self, word: &str, max: usize) -> Vec<String> {
        let normal = word.replace('\u{2019}', "'");
        let lists: Vec<Vec<String>> = self
            .list
            .iter()
            .map(|(_, c)| suggestions_of(c, &normal))
            .collect();
        let mut out: Vec<String> = Vec::new();
        for round in 0..lists.iter().map(Vec::len).max().unwrap_or(0) {
            for list in &lists {
                if let Some(s) = list.get(round) {
                    if out.len() < max && !out.contains(s) {
                        out.push(s.clone());
                    }
                }
            }
        }
        out
    }

    /// Add `word` to every checker's dictionary, and forget what was cached.
    pub fn add(&self, word: &str) {
        let wide = to_wide(&word.replace('\u{2019}', "'"));
        for (_, checker) in &self.list {
            unsafe {
                let _ = checker.Add(PCWSTR(wide.as_ptr()));
            }
        }
        self.added
            .borrow_mut()
            .insert(word.replace('\u{2019}', "'").to_lowercase());
        for cache in self.cache.borrow_mut().iter_mut() {
            cache.clear();
        }
    }
}

fn suggestions_of(checker: &ISpellChecker, word: &str) -> Vec<String> {
    let wide = to_wide(word);
    let mut out = Vec::new();
    unsafe {
        let Ok(list) = checker.Suggest(PCWSTR(wide.as_ptr())) else {
            return out;
        };
        loop {
            let mut item = [PWSTR::null()];
            if list.Next(&mut item, None).0 != 0 || item[0].is_null() {
                break;
            }
            if let Ok(s) = item[0].to_string() {
                out.push(s);
            }
            CoTaskMemFree(Some(item[0].0 as *const _));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdstyle::style_runs;
    use crate::spell::words;

    /// The checkers, or `None` (the test skips) unless both languages exist.
    fn both() -> Option<Checkers> {
        let c = Checkers::open(&LANGUAGES);
        if c.list.len() == LANGUAGES.len() {
            Some(c)
        } else {
            eprintln!(
                "skipped: only {} of {} languages installed",
                c.list.len(),
                LANGUAGES.len()
            );
            None
        }
    }

    fn flagged<'a>(c: &Checkers, text: &'a str) -> Vec<&'a str> {
        let runs = style_runs(text);
        words(text, &runs, 0..text.len())
            .into_iter()
            .map(|r| &text[r])
            .filter(|w| c.is_misspelled(w))
            .collect()
    }

    #[test]
    fn italian_sentence_flags_only_the_typo() {
        let Some(c) = both() else { return };
        assert_eq!(flagged(&c, "Prenotare il tagliandoo"), ["tagliandoo"]);
    }

    #[test]
    fn english_sentence_flags_only_the_typo() {
        let Some(c) = both() else { return };
        assert_eq!(flagged(&c, "Please book the servise today"), ["servise"]);
    }

    #[test]
    fn preventivo_is_never_flagged() {
        let Some(c) = both() else { return };
        assert!(flagged(&c, "preventivo Preventivo preventivi il preventivo").is_empty());
    }

    #[test]
    fn code_spans_are_not_checked() {
        let Some(c) = both() else { return };
        assert_eq!(flagged(&c, "Usa `tagliandoo` e tagliandoo"), ["tagliandoo"]);
        assert!(flagged(&c, "```\ntagliandoo\n```\n").is_empty());
    }

    #[test]
    fn elided_italian_words_are_not_flagged() {
        let Some(c) = both() else { return };
        assert!(flagged(&c, "l'albero e dell'anno").is_empty());
    }

    #[test]
    fn suggestions_come_from_either_language() {
        let Some(c) = both() else { return };
        assert!(c.suggest("tagliandoo", 5).iter().any(|s| s == "tagliando"));
        assert!(c.suggest("servise", 5).iter().any(|s| s == "service"));
    }

    #[test]
    fn one_language_alone_still_checks() {
        let c = Checkers::open(&LANGUAGES[..1]);
        if c.is_empty() {
            return;
        }
        assert!(c.is_misspelled("tagliandoo"));
        assert!(!c.is_misspelled("tagliando"));
    }

    #[test]
    fn adding_a_word_stops_the_flag_at_once() {
        use windows::core::Interface;
        use windows::Win32::Globalization::ISpellChecker2;
        let Some(c) = both() else { return };
        let word = "zqxvwkpl";
        // Leaves the user's dictionaries as they were, even if an assert fails.
        struct Cleanup<'a>(&'a Checkers, &'static str);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                let wide = to_wide(self.1);
                for (_, checker) in &self.0.list {
                    if let Ok(two) = checker.cast::<ISpellChecker2>() {
                        unsafe {
                            let _ = two.Remove(PCWSTR(wide.as_ptr()));
                        }
                    }
                }
            }
        }
        let _cleanup = Cleanup(&c, word);
        assert!(c.is_misspelled(word));
        c.add(word);
        assert!(!c.is_misspelled(word));
        assert!(!c.is_misspelled("dell'zqxvwkpl"));
    }

    #[test]
    fn no_languages_flags_nothing() {
        let c = Checkers::open(&[]);
        assert!(!c.is_misspelled("tagliandoo"));
    }
}
