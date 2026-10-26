//! Find, replace and go to line (spec §11): the search rules, pure, over a
//! [`Text`]. The Source view's `Text` is Scintilla's `SCI_SEARCHINTARGET`;
//! the tests use a `String`.
//!
//! Positions are byte offsets, as in Scintilla. No regex.

use std::ops::Range;

use crate::editor::{sci::*, Editor};

/// Matches counted (and highlighted) at most: "n of m" reads "n of 10000+"
/// beyond it. Next and previous search the document itself, so they still
/// reach every match.
pub const MATCH_CAP: usize = 10_000;

/// What the search rules need of a document.
pub trait Text {
    /// The first match of `needle` within `from..to`; `from > to` searches
    /// backwards, from `from` down to `to`.
    fn find(&self, needle: &str, from: usize, to: usize, match_case: bool) -> Option<Range<usize>>;
    fn len(&self) -> usize;
    /// Replace `range` with `with`.
    fn replace(&mut self, range: Range<usize>, with: &str);
    /// Edits between these two are one undo step.
    fn begin_edit(&mut self) {}
    fn end_edit(&mut self) {}
}

/// The matches of a needle, in order, up to [`MATCH_CAP`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Matches {
    pub list: Vec<Range<usize>>,
    /// There are more than the list holds.
    pub capped: bool,
}

impl Matches {
    /// Which match `selection` is (0-based), if it is exactly one.
    pub fn index_of(&self, selection: &Range<usize>) -> Option<usize> {
        let i = self.list.partition_point(|m| m.start < selection.start);
        self.list.get(i).filter(|m| *m == selection).map(|_| i)
    }
}

/// Every match of `needle` in `t`, an empty list for an empty needle.
pub fn all_matches(t: &impl Text, needle: &str, match_case: bool) -> Matches {
    let mut out = Matches::default();
    if needle.is_empty() {
        return out;
    }
    let (len, mut at) = (t.len(), 0);
    while let Some(m) = t.find(needle, at, len, match_case) {
        if out.list.len() == MATCH_CAP {
            out.capped = true;
            break;
        }
        at = m.end.max(m.start + 1);
        out.list.push(m);
    }
    out
}

/// The first match starting at or after `from`, wrapping to the top.
pub fn next_match(t: &impl Text, needle: &str, match_case: bool, from: usize) -> Option<Range<usize>> {
    if needle.is_empty() {
        return None;
    }
    let len = t.len();
    t.find(needle, from.min(len), len, match_case).or_else(|| t.find(needle, 0, len, match_case))
}

/// The last match ending at or before `before`, wrapping to the bottom.
pub fn prev_match(t: &impl Text, needle: &str, match_case: bool, before: usize) -> Option<Range<usize>> {
    if needle.is_empty() {
        return None;
    }
    let len = t.len();
    t.find(needle, before.min(len), 0, match_case).or_else(|| t.find(needle, len, 0, match_case))
}

/// Replace `selection` if it is a match of `needle`. Returns where the text
/// after the replacement starts, to search on from; `None` if the selection
/// is no match (nothing changed).
pub fn replace_selected(
    t: &mut impl Text,
    needle: &str,
    with: &str,
    match_case: bool,
    selection: Range<usize>,
) -> Option<usize> {
    if needle.is_empty() || t.find(needle, selection.start, selection.end, match_case) != Some(selection.clone()) {
        return None;
    }
    t.replace(selection.clone(), with);
    Some(selection.start + with.len())
}

/// Replace every match as one undo step; how many there were. The
/// replacement is not searched again.
pub fn replace_all(t: &mut impl Text, needle: &str, with: &str, match_case: bool) -> usize {
    if needle.is_empty() {
        return 0;
    }
    t.begin_edit();
    let (mut at, mut count) = (0, 0);
    while let Some(m) = t.find(needle, at, t.len(), match_case) {
        t.replace(m.clone(), with);
        at = m.start + with.len();
        count += 1;
    }
    t.end_edit();
    count
}

/// The line (0-based) that Go to line's input names. The input is 1-based;
/// a line past the end goes to the last one, and 0 to the first. `None`
/// for an empty or non-numeric input.
pub fn go_to_line(input: &str, line_count: usize) -> Option<usize> {
    let digits = input.trim();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Longer than any line count: the last line.
    let n = digits.parse::<usize>().unwrap_or(usize::MAX);
    Some(n.saturating_sub(1).min(line_count.saturating_sub(1)))
}

/// The bar's "n of m": nothing for an empty needle.
pub fn count_label(needle_empty: bool, current: Option<usize>, matches: &Matches) -> String {
    let total = matches.list.len();
    let m = if matches.capped { format!("{total}+") } else { total.to_string() };
    match (needle_empty, total, current) {
        (true, ..) => String::new(),
        (false, 0, _) => "No results".into(),
        (false, _, Some(n)) => format!("{} of {m}", n + 1),
        (false, ..) => format!("{m} found"),
    }
}

/// The Source view's document, as the search sees it. Calls are plain
/// Scintilla messages: the search moves the target, never the selection.
impl Text for Editor {
    fn find(&self, needle: &str, from: usize, to: usize, match_case: bool) -> Option<Range<usize>> {
        self.call(SCI_SETSEARCHFLAGS, if match_case { SCFIND_MATCHCASE } else { 0 }, 0);
        self.call(SCI_SETTARGETRANGE, from, to as isize);
        let at = self.call(SCI_SEARCHINTARGET, needle.len(), needle.as_ptr() as isize);
        if at < 0 {
            return None;
        }
        let (start, end) = (self.call(SCI_GETTARGETSTART, 0, 0), self.call(SCI_GETTARGETEND, 0, 0));
        Some(start as usize..end as usize)
    }

    fn len(&self) -> usize {
        self.text_len()
    }

    fn replace(&mut self, range: Range<usize>, with: &str) {
        self.call(SCI_SETTARGETRANGE, range.start, range.end as isize);
        self.call(SCI_REPLACETARGET, with.len(), with.as_ptr() as isize);
    }

    fn begin_edit(&mut self) {
        self.call(SCI_BEGINUNDOACTION, 0, 0);
    }

    fn end_edit(&mut self) {
        self.call(SCI_ENDUNDOACTION, 0, 0);
    }
}

#[cfg(test)]
mod tests;
