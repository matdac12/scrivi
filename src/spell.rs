//! Spell check through ISpellChecker, Italian and English combined (spec §5).
//! This file is the pure part: the combine rule and word extraction over
//! `mdstyle`'s runs. The COM adapter is in `checker`.

mod checker;

pub use checker::{Checkers, LANGUAGES};

use std::ops::Range;

use crate::mdstyle::Style;

/// The combine rule: flagged only if there is at least one checker and every
/// one of them rejects the word.
pub fn combine(rejections: impl IntoIterator<Item = bool>) -> bool {
    let mut any = false;
    for rejected in rejections {
        if !rejected {
            return false;
        }
        any = true;
    }
    any
}

fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '\u{2019}'
}

/// The part of `word` after its last apostrophe: Italian elisions ("dell'anno")
/// are checked by what follows the apostrophe too.
pub fn elision_tail(word: &str) -> Option<&str> {
    let (i, c) = word.char_indices().rev().find(|(_, c)| is_apostrophe(*c))?;
    let tail = &word[i + c.len_utf8()..];
    (!tail.is_empty()).then_some(tail)
}

/// Whether `word` is flagged, given one `rejects` function per checker. A word
/// with an apostrophe is let through if its whole or its tail is accepted.
pub fn is_misspelled(word: &str, rejects: &[&dyn Fn(&str) -> bool]) -> bool {
    let all_reject = |w: &str| combine(rejects.iter().map(|r| r(w)));
    all_reject(word) && elision_tail(word).is_none_or(all_reject)
}

/// The byte ranges of `0..len` whose text is prose: everything except the
/// runs `Style::is_spell_checked` rules out.
pub fn checkable_ranges(len: usize, runs: &[(Range<usize>, Style)]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut at = 0;
    for (range, style) in runs {
        if style.is_spell_checked() {
            continue;
        }
        let (start, end) = (range.start.min(len), range.end.min(len));
        if start > at {
            out.push(at..start);
        }
        at = at.max(end);
    }
    if at < len {
        out.push(at..len);
    }
    out
}

/// A token is worth checking: letters only, two or more of them, not an
/// acronym (all capitals) or an identifier (a capital inside the word).
fn worth_checking(token: &str) -> bool {
    let letters: Vec<char> = token.chars().filter(|c| !is_apostrophe(*c)).collect();
    if letters.len() < 2 || letters.iter().any(|c| !c.is_alphabetic()) {
        return false;
    }
    if letters.iter().all(|c| c.is_uppercase()) {
        return false;
    }
    !letters[1..].iter().zip(&letters).any(|(c, prev)| c.is_uppercase() && prev.is_lowercase())
}

/// Whether the chunk of text between two spaces is an email address, a URL or
/// a path (and not prose like "and/or").
fn is_address(chunk: &str) -> bool {
    chunk.contains('@')
        || chunk.contains("//")
        || chunk.contains(":\\")
        || chunk.contains(":/")
        || chunk.starts_with(['/', '\\', '~'])
}

/// The words of `text` to check inside `window`: byte ranges of tokens that
/// lie in prose per `runs`. `window` must start and end on character
/// boundaries (line edges do).
pub fn words(text: &str, runs: &[(Range<usize>, Style)], window: Range<usize>) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    for part in checkable_ranges(text.len(), runs) {
        let (start, end) = (part.start.max(window.start), part.end.min(window.end));
        if start >= end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            continue;
        }
        let mut token_start = None;
        let flush = |from: usize, to: usize, out: &mut Vec<Range<usize>>| {
            // Leading and trailing apostrophes are quotes, not part of the word.
            let token = text[from..to].trim_matches(is_apostrophe);
            if token.is_empty() {
                return;
            }
            let from = from + text[from..to].find(token).unwrap_or(0);
            let to = from + token.len();
            // The chunk around the token, within this stretch of prose.
            let lo = text[start..from].rfind(char::is_whitespace).map_or(start, |i| start + i);
            let hi = text[to..end].find(char::is_whitespace).map_or(end, |i| to + i);
            if is_address(text[lo..hi].trim_start()) {
                return;
            }
            if worth_checking(token) {
                out.push(from..to);
            }
        };
        for (i, c) in text[start..end].char_indices() {
            let i = start + i;
            let in_token = c.is_alphanumeric() || is_apostrophe(c) || c == '_';
            match (in_token, token_start) {
                (true, None) => token_start = Some(i),
                (false, Some(from)) => {
                    flush(from, i, &mut out);
                    token_start = None;
                }
                _ => {}
            }
        }
        if let Some(from) = token_start {
            flush(from, end, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mdstyle::style_runs;

    fn found(text: &str) -> Vec<&str> {
        let runs = style_runs(text);
        words(text, &runs, 0..text.len()).into_iter().map(|r| &text[r]).collect()
    }

    #[test]
    fn combine_flags_only_when_every_checker_rejects() {
        assert!(combine([true, true]));
        assert!(!combine([true, false]));
        assert!(!combine([false, true]));
        assert!(!combine([false, false]));
    }

    #[test]
    fn combine_with_one_checker_follows_it() {
        assert!(combine([true]));
        assert!(!combine([false]));
    }

    #[test]
    fn combine_with_no_checker_never_flags() {
        assert!(!combine([]));
    }

    #[test]
    fn a_word_is_flagged_only_if_both_reject_it() {
        let italian_knows = |w: &str| w != "preventivo";
        let english_knows = |w: &str| w != "quote";
        let both: [&dyn Fn(&str) -> bool; 2] = [&italian_knows, &english_knows];
        assert!(!is_misspelled("preventivo", &both));
        assert!(!is_misspelled("quote", &both));
        assert!(is_misspelled("tagliandoo", &[&|_| true, &|_| true]));
    }

    #[test]
    fn an_elided_word_passes_when_its_tail_does() {
        assert_eq!(elision_tail("dell'anno"), Some("anno"));
        assert_eq!(elision_tail("anno"), None);
        // Whole word rejected, tail accepted: not flagged.
        let rejects_whole = |w: &str| w == "dell'anno";
        assert!(!is_misspelled("dell'anno", &[&rejects_whole]));
        // Whole word and tail both rejected: flagged.
        let rejects_both = |w: &str| w == "dell'annno" || w == "annno";
        assert!(is_misspelled("dell'annno", &[&rejects_both]));
    }

    #[test]
    fn prose_words_are_found() {
        assert_eq!(found("Prenotare il tagliandoo"), ["Prenotare", "il", "tagliandoo"]);
    }

    #[test]
    fn code_is_not_checked() {
        assert_eq!(found("Use `fooo` now"), ["Use", "now"]);
        assert_eq!(found("text\n\n```\nbaaad code\n```\n\nmore"), ["text", "more"]);
    }

    #[test]
    fn urls_and_link_addresses_are_not_checked() {
        assert_eq!(found("see https://exampple.org/pagee now"), ["see", "now"]);
        assert_eq!(found("[the sitte](http://exampple.org) ok"), ["the", "sitte", "ok"]);
    }

    #[test]
    fn headings_and_emphasis_are_checked() {
        assert_eq!(found("# Titlo\n\n**bold** and _ital_"), ["Titlo", "bold", "and", "ital"]);
    }

    #[test]
    fn apostrophes_stay_inside_words_and_quotes_do_not() {
        assert_eq!(found("dell'anno and don't, 'quoted'"), ["dell'anno", "and", "don't", "quoted"]);
        assert_eq!(found("l\u{2019}albero"), ["l\u{2019}albero"]);
    }

    #[test]
    fn numbers_acronyms_identifiers_and_single_letters_are_skipped() {
        assert_eq!(found("abc123 HTTP GitHub snake_case a x 2024 fine"), ["fine"]);
    }

    #[test]
    fn email_and_path_pieces_are_skipped() {
        assert_eq!(found("mail foo@barr.example or C:\\tmpp\\filee ok"), ["mail", "or", "ok"]);
        assert_eq!(found("see /usr/locall/bin and ~/dokuments ok"), ["see", "and", "ok"]);
        assert_eq!(found("clientee/fornitore"), ["clientee", "fornitore"]);
    }

    #[test]
    fn accented_letters_are_word_letters() {
        assert_eq!(found("perché è così"), ["perché", "così"]);
    }

    #[test]
    fn the_window_limits_the_words() {
        let text = "uno due\ntre quattro\ncinque";
        let runs = style_runs(text);
        let window = 8..19;
        let got: Vec<&str> = words(text, &runs, window).into_iter().map(|r| &text[r]).collect();
        assert_eq!(got, ["tre", "quattro"]);
    }

    #[test]
    fn checkable_ranges_cut_out_code() {
        let runs = vec![(2..4, Style::InlineCode), (6..7, Style::Bold), (8..9, Style::Url)];
        assert_eq!(checkable_ranges(10, &runs), vec![0..2, 4..8, 9..10]);
        assert_eq!(checkable_ranges(0, &[]), Vec::<Range<usize>>::new());
        assert_eq!(checkable_ranges(5, &[]), vec![0..5]);
    }
}
