use super::*;

/// A document in a `String`. Case-insensitive matching folds ASCII only.
struct Doc {
    text: String,
    /// Undo steps opened and closed, to check Replace all is one.
    steps: usize,
    open: bool,
}

fn doc(s: &str) -> Doc {
    Doc {
        text: s.into(),
        steps: 0,
        open: false,
    }
}

impl Text for Doc {
    fn find(&self, needle: &str, from: usize, to: usize, match_case: bool) -> Option<Range<usize>> {
        let fold = |s: &str| {
            if match_case {
                s.to_string()
            } else {
                s.to_ascii_lowercase()
            }
        };
        let (hay, needle) = (fold(&self.text), fold(needle));
        let fits = |at: usize| hay.is_char_boundary(at) && hay[at..].starts_with(&needle);
        let hit = |at: usize| Some(at..at + needle.len());
        if from <= to {
            (from..=to.saturating_sub(needle.len()))
                .filter(|_| to >= needle.len())
                .find(|at| fits(*at))
                .and_then(hit)
        } else {
            // Backwards: matches that end at or before `from`, down to `to`.
            (to..=from.saturating_sub(needle.len()))
                .rev()
                .filter(|_| from >= needle.len())
                .find(|at| fits(*at))
                .and_then(hit)
        }
    }

    fn len(&self) -> usize {
        self.text.len()
    }

    fn replace(&mut self, range: Range<usize>, with: &str) {
        self.text.replace_range(range, with);
    }

    fn begin_edit(&mut self) {
        assert!(!self.open);
        self.open = true;
    }

    fn end_edit(&mut self) {
        assert!(self.open);
        self.open = false;
        self.steps += 1;
    }
}

#[test]
fn all_matches_lists_each_one_in_order() {
    let m = all_matches(&doc("a test, a Test, a TEST"), "test", false);
    assert_eq!(m.list, vec![2..6, 10..14, 18..22]);
    assert!(!m.capped);
}

#[test]
fn match_case_narrows_the_matches() {
    let m = all_matches(&doc("a test, a Test, a TEST"), "test", true);
    assert_eq!(m.list, vec![2..6]);
}

#[test]
fn matches_do_not_overlap() {
    assert_eq!(all_matches(&doc("aaaa"), "aa", true).list, vec![0..2, 2..4]);
}

#[test]
fn an_empty_needle_matches_nothing() {
    assert!(all_matches(&doc("abc"), "", false).list.is_empty());
    assert_eq!(next_match(&doc("abc"), "", false, 0), None);
    assert_eq!(prev_match(&doc("abc"), "", false, 3), None);
}

#[test]
fn the_list_stops_at_the_cap() {
    let m = all_matches(&doc(&"x".repeat(MATCH_CAP + 5)), "x", true);
    assert_eq!(m.list.len(), MATCH_CAP);
    assert!(m.capped);
    let exact = all_matches(&doc(&"x".repeat(MATCH_CAP)), "x", true);
    assert_eq!(exact.list.len(), MATCH_CAP);
    assert!(!exact.capped);
}

#[test]
fn next_goes_forward_and_wraps() {
    let d = doc("one two one two one");
    assert_eq!(next_match(&d, "one", true, 0), Some(0..3));
    assert_eq!(next_match(&d, "one", true, 3), Some(8..11));
    assert_eq!(next_match(&d, "one", true, 16), Some(16..19));
    // Past the last match: back to the first.
    assert_eq!(next_match(&d, "one", true, 17), Some(0..3));
}

#[test]
fn prev_goes_back_and_wraps() {
    let d = doc("one two one two one");
    assert_eq!(prev_match(&d, "one", true, 19), Some(16..19));
    assert_eq!(prev_match(&d, "one", true, 16), Some(8..11));
    // Before the first match: round to the last.
    assert_eq!(prev_match(&d, "one", true, 2), Some(16..19));
}

#[test]
fn a_needle_with_no_match_finds_nothing() {
    let d = doc("one two");
    assert_eq!(next_match(&d, "three", false, 0), None);
    assert_eq!(prev_match(&d, "three", false, 7), None);
}

#[test]
fn a_single_match_wraps_onto_itself() {
    let d = doc("a needle here");
    assert_eq!(next_match(&d, "needle", true, 8), Some(2..8));
    assert_eq!(prev_match(&d, "needle", true, 2), Some(2..8));
}

#[test]
fn index_of_finds_the_selected_match() {
    let m = all_matches(&doc("ab ab ab"), "ab", true);
    assert_eq!(m.index_of(&(3..5)), Some(1));
    assert_eq!(m.index_of(&(0..2)), Some(0));
    assert_eq!(m.index_of(&(3..4)), None);
    assert_eq!(m.index_of(&(2..3)), None);
    assert_eq!(m.index_of(&(9..9)), None);
}

#[test]
fn replace_selected_replaces_only_a_selected_match() {
    let mut d = doc("cat and cat");
    assert_eq!(replace_selected(&mut d, "cat", "dog", true, 0..3), Some(3));
    assert_eq!(d.text, "dog and cat");
    // Not a match: nothing changes.
    assert_eq!(replace_selected(&mut d, "cat", "dog", true, 0..3), None);
    assert_eq!(replace_selected(&mut d, "cat", "dog", true, 4..6), None);
    assert_eq!(d.text, "dog and cat");
}

#[test]
fn replace_selected_follows_the_case_rule() {
    let mut d = doc("Cat");
    assert_eq!(replace_selected(&mut d, "cat", "dog", true, 0..3), None);
    assert_eq!(replace_selected(&mut d, "cat", "dog", false, 0..3), Some(3));
    assert_eq!(d.text, "dog");
}

#[test]
fn replace_all_is_one_undo_step() {
    let mut d = doc("a b a b a");
    assert_eq!(replace_all(&mut d, "a", "xyz", true), 3);
    assert_eq!(d.text, "xyz b xyz b xyz");
    assert_eq!(d.steps, 1);
}

/// A document that ignores edits, like a read-only one.
struct Frozen(Doc);

impl Text for Frozen {
    fn find(&self, needle: &str, from: usize, to: usize, match_case: bool) -> Option<Range<usize>> {
        self.0.find(needle, from, to, match_case)
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn replace(&mut self, _: Range<usize>, _: &str) {}
}

#[test]
fn replace_all_in_a_document_that_refuses_edits_ends_with_nothing_replaced() {
    let mut d = Frozen(doc("foo foo"));
    assert_eq!(replace_all(&mut d, "foo", "", true), 0);
    assert_eq!(d.0.text, "foo foo");
}

#[test]
fn replace_all_does_not_search_its_own_output() {
    let mut d = doc("aa");
    assert_eq!(replace_all(&mut d, "a", "aa", true), 2);
    assert_eq!(d.text, "aaaa");
}

#[test]
fn replace_all_with_nothing_deletes_the_matches() {
    let mut d = doc("a-b-c");
    assert_eq!(replace_all(&mut d, "-", "", true), 2);
    assert_eq!(d.text, "abc");
}

#[test]
fn replace_all_with_no_match_changes_nothing() {
    let mut d = doc("abc");
    assert_eq!(replace_all(&mut d, "x", "y", true), 0);
    assert_eq!(replace_all(&mut d, "", "y", true), 0);
    assert_eq!(d.text, "abc");
}

#[test]
fn replace_all_handles_non_ascii() {
    let mut d = doc("caffè e caffè");
    assert_eq!(replace_all(&mut d, "caffè", "tè", true), 2);
    assert_eq!(d.text, "tè e tè");
}

#[test]
fn go_to_line_is_one_based_and_clamped() {
    assert_eq!(go_to_line("1", 100), Some(0));
    assert_eq!(go_to_line(" 42 ", 100), Some(41));
    assert_eq!(go_to_line("0", 100), Some(0));
    assert_eq!(go_to_line("100", 100), Some(99));
    assert_eq!(go_to_line("101", 100), Some(99));
    assert_eq!(go_to_line("999999999999999999999999999", 100), Some(99));
    // A document always has one line.
    assert_eq!(go_to_line("5", 0), Some(0));
}

#[test]
fn go_to_line_ignores_what_is_not_a_number() {
    assert_eq!(go_to_line("", 10), None);
    assert_eq!(go_to_line("  ", 10), None);
    assert_eq!(go_to_line("abc", 10), None);
    assert_eq!(go_to_line("-3", 10), None);
    assert_eq!(go_to_line("1x", 10), None);
}

#[test]
fn the_count_reads_n_of_m() {
    let three = all_matches(&doc("a a a"), "a", true);
    assert_eq!(count_label(false, Some(1), &three), "2 of 3");
    assert_eq!(count_label(false, None, &three), "3 found");
    assert_eq!(count_label(false, None, &Matches::default()), "No results");
    assert_eq!(count_label(true, None, &Matches::default()), "");
    let capped = Matches {
        list: vec![0..1; MATCH_CAP],
        capped: true,
    };
    assert_eq!(count_label(false, Some(4), &capped), "5 of 10000+");
}
