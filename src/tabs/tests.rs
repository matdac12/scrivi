//! One test per Tab rule in spec §3, §7 and §8. Each name states the rule.

use super::*;

fn names(tabs: &Tabs) -> Vec<String> {
    tabs.iter().map(Tab::name).collect()
}

/// The id of the Tab a command just added.
fn added(actions: &[Action]) -> TabId {
    actions
        .iter()
        .find_map(|a| match a {
            Action::CreateDocument(id) | Action::Load { id, .. } => Some(*id),
            _ => None,
        })
        .expect("no Tab was added")
}

fn path(p: &str) -> PathBuf {
    PathBuf::from(p)
}

fn stat(size: u64, secs: u64) -> FileStat {
    FileStat {
        size,
        modified: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
    }
}

fn file(size: u64) -> LoadedFile {
    LoadedFile {
        encoding: Encoding::Utf8,
        eol: Eol::Lf,
        stat: stat(size, 100),
        file_id: None,
        text_len: size as usize,
        lossy: false,
    }
}

/// Opens `p` and completes its load as a small UTF-8 file.
fn open_file(tabs: &mut Tabs, p: &str) -> TabId {
    open_sized(tabs, p, 10)
}

fn open_sized(tabs: &mut Tabs, p: &str, size: u64) -> TabId {
    let id = added(&tabs.open(&[path(p)]));
    tabs.loaded(id, file(size));
    id
}

// --- §3 Opening files -----------------------------------------------------

#[test]
fn opening_a_path_adds_a_tab_that_watches_the_file_before_loading_it() {
    let mut tabs = Tabs::new();
    let actions = tabs.open(&[path(r"C:\notes\todo.md")]);
    let id = added(&actions);
    assert_eq!(
        actions,
        [
            Action::Watch(path(r"C:\notes\todo.md")),
            Action::Load {
                id,
                path: path(r"C:\notes\todo.md"),
                encoding: None,
                keep_caret_line: false,
            },
        ]
    );
    assert_eq!(names(&tabs), ["todo.md"]);
}

#[test]
fn a_loaded_file_is_shown_and_moves_to_the_top_of_recent_files() {
    let mut tabs = Tabs::new();
    let id = added(&tabs.open(&[path(r"C:\notes\todo.md")]));
    let actions = tabs.loaded(id, file(10));
    assert_eq!(
        actions,
        [
            Action::Changed(id),
            Action::Activate(id),
            Action::AddRecent(path(r"C:\notes\todo.md")),
        ]
    );
    assert_eq!(tabs.active(), Some(id));
}

#[test]
fn opening_several_paths_adds_a_tab_for_each_and_shows_the_last() {
    let mut tabs = Tabs::new();
    let actions = tabs.open(&[path(r"C:\a.txt"), path(r"C:\b.txt")]);
    let ids: Vec<TabId> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Load { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 2);
    tabs.loaded(ids[0], file(1));
    tabs.loaded(ids[1], file(1));
    assert_eq!(names(&tabs), ["a.txt", "b.txt"]);
    assert_eq!(tabs.active(), Some(ids[1]));
}

#[test]
fn opening_a_path_already_open_switches_to_its_tab_and_moves_it_to_the_top_of_recent_files() {
    let mut tabs = Tabs::new();
    let notes = open_file(&mut tabs, r"C:\notes\todo.md");
    tabs.new_untitled();
    let actions = tabs.open(&[path(r"C:\notes\todo.md")]);
    assert_eq!(
        actions,
        [
            Action::Activate(notes),
            Action::AddRecent(path(r"C:\notes\todo.md"))
        ]
    );
    assert_eq!(tabs.iter().count(), 2);
}

#[test]
fn paths_match_ignoring_case_and_slash_direction() {
    let mut tabs = Tabs::new();
    let notes = open_file(&mut tabs, r"C:\Notes\Todo.md");
    tabs.new_untitled();
    assert_eq!(
        tabs.open(&[path("c:/notes/TODO.MD")])[0],
        Action::Activate(notes)
    );
}

#[test]
fn a_file_that_fails_to_load_leaves_no_tab() {
    let mut tabs = Tabs::new();
    let keep = added(&tabs.new_untitled());
    let id = added(&tabs.open(&[path(r"C:\locked.txt")]));
    let actions = tabs.load_failed(id);
    assert_eq!(
        actions,
        [
            Action::Closed { id, document: None },
            Action::Unwatch(path(r"C:\locked.txt")),
        ]
    );
    assert_eq!(tabs.active(), Some(keep));
}

#[test]
fn when_the_only_file_fails_to_load_an_untitled_tab_takes_its_place() {
    let mut tabs = Tabs::new();
    let id = added(&tabs.open(&[path(r"C:\locked.txt")]));
    let actions = tabs.load_failed(id);
    assert!(!actions.contains(&Action::CloseWindow));
    assert_eq!(names(&tabs), ["Untitled 1"]);
    assert!(tabs.active().is_some());
}

// --- Kinds ----------------------------------------------------------------

#[test]
fn md_and_markdown_files_are_markdown_files_in_any_case() {
    let mut tabs = Tabs::new();
    for p in [r"C:\a.md", r"C:\b.MARKDOWN", r"C:\c.Md"] {
        let id = open_file(&mut tabs, p);
        assert_eq!(tabs.get(id).unwrap().kind(), Kind::Markdown, "{p}");
    }
}

#[test]
fn other_files_are_text_files() {
    let mut tabs = Tabs::new();
    for p in [r"C:\a.txt", r"C:\b.log", r"C:\Makefile", r"C:\c.md.bak"] {
        let id = open_file(&mut tabs, p);
        assert_eq!(tabs.get(id).unwrap().kind(), Kind::Text, "{p}");
    }
}

#[test]
fn untitled_tabs_count_as_markdown_files() {
    let mut tabs = Tabs::new();
    let id = added(&tabs.new_untitled());
    assert_eq!(tabs.get(id).unwrap().kind(), Kind::Markdown);
}

// --- §3 Untitled tabs -----------------------------------------------------

#[test]
fn new_untitled_tabs_are_numbered_from_one() {
    let mut tabs = Tabs::new();
    tabs.new_untitled();
    tabs.new_untitled();
    assert_eq!(names(&tabs), ["Untitled 1", "Untitled 2"]);
}

#[test]
fn new_untitled_creates_a_document_and_shows_the_tab() {
    let mut tabs = Tabs::new();
    let actions = tabs.new_untitled();
    let id = added(&actions);
    assert_eq!(actions, [Action::CreateDocument(id), Action::Activate(id)]);
    assert_eq!(tabs.active(), Some(id));
}

#[test]
fn new_untitled_takes_the_lowest_free_number() {
    let mut tabs = Tabs::new();
    tabs.new_untitled();
    let two = added(&tabs.new_untitled());
    tabs.new_untitled();
    tabs.close(two);
    tabs.new_untitled();
    assert_eq!(names(&tabs), ["Untitled 1", "Untitled 3", "Untitled 2"]);
}

// --- Closing a Tab --------------------------------------------------------

#[test]
fn closing_a_tab_with_no_unsaved_edits_closes_it_and_hands_back_its_document() {
    let mut tabs = Tabs::new();
    let keep = added(&tabs.new_untitled());
    let gone = added(&tabs.new_untitled());
    tabs.set_document(gone, EditorDoc(7));
    let actions = tabs.close(gone);
    assert_eq!(
        actions,
        [
            Action::Closed {
                id: gone,
                document: Some(EditorDoc(7))
            },
            Action::Activate(keep),
        ]
    );
    assert!(tabs.get(gone).is_none());
}

#[test]
fn closing_the_active_tab_shows_the_tab_to_its_right() {
    let mut tabs = Tabs::new();
    let a = added(&tabs.new_untitled());
    let b = added(&tabs.new_untitled());
    let _c = added(&tabs.new_untitled());
    tabs.activate(a);
    tabs.close(a);
    assert_eq!(tabs.active(), Some(b));
}

#[test]
fn closing_the_rightmost_active_tab_shows_the_tab_to_its_left() {
    let mut tabs = Tabs::new();
    let _a = added(&tabs.new_untitled());
    let b = added(&tabs.new_untitled());
    let c = added(&tabs.new_untitled());
    tabs.close(c);
    assert_eq!(tabs.active(), Some(b));
}

#[test]
fn closing_an_inactive_tab_keeps_the_active_one() {
    let mut tabs = Tabs::new();
    let a = added(&tabs.new_untitled());
    let b = added(&tabs.new_untitled());
    let actions = tabs.close(a);
    assert_eq!(
        actions,
        [Action::Closed {
            id: a,
            document: None
        }]
    );
    assert_eq!(tabs.active(), Some(b));
}

#[test]
fn closing_the_last_tab_closes_the_window() {
    let mut tabs = Tabs::new();
    let only = added(&tabs.new_untitled());
    let actions = tabs.close(only);
    assert_eq!(
        actions,
        [
            Action::Closed {
                id: only,
                document: None
            },
            Action::CloseWindow
        ]
    );
}

// --- Edits and saving -----------------------------------------------------

fn t0() -> Instant {
    Instant::now()
}

/// A Tab with unsaved edits.
fn edited_file(tabs: &mut Tabs, p: &str) -> TabId {
    let id = open_file(tabs, p);
    tabs.edited(id, 11);
    id
}

fn edited_untitled(tabs: &mut Tabs) -> TabId {
    let id = added(&tabs.new_untitled());
    tabs.edited(id, 5);
    id
}

#[test]
fn the_first_edit_marks_the_tab_unsaved() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.txt");
    assert_eq!(tabs.edited(id, 11), [Action::Changed(id)]);
    assert!(tabs.get(id).unwrap().unsaved());
    assert_eq!(tabs.edited(id, 12), []);
}

#[test]
fn save_writes_the_file_in_its_encoding_and_line_ending() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.txt");
    assert_eq!(
        tabs.save(id),
        [Action::Write {
            id,
            path: path(r"C:\a.txt"),
            encoding: Encoding::Utf8,
            eol: Eol::Lf,
        }]
    );
}

#[test]
fn a_finished_save_clears_the_unsaved_mark_and_moves_the_file_to_the_top_of_recent_files() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.txt");
    let actions = tabs.saved(id, path(r"C:\a.txt"), stat(11, 200), t0());
    assert_eq!(
        actions,
        [Action::Changed(id), Action::AddRecent(path(r"C:\a.txt"))]
    );
    assert!(!tabs.get(id).unwrap().unsaved());
}

#[test]
fn save_on_an_untitled_tab_opens_save_as() {
    let mut tabs = Tabs::new();
    let id = edited_untitled(&mut tabs);
    assert!(matches!(tabs.save(id)[..], [Action::AskSaveAs { id: asked, .. }] if asked == id));
}

#[test]
fn save_as_on_an_untitled_tab_defaults_to_md_in_documents() {
    let mut tabs = Tabs::new();
    let id = edited_untitled(&mut tabs);
    assert_eq!(
        tabs.save_as_default(id).unwrap(),
        SaveAsDefault {
            folder: SaveAsFolder::Documents,
            file_name: "Untitled 1.md".into()
        }
    );
}

#[test]
fn save_as_on_a_file_defaults_to_its_own_folder_and_name() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\notes\a.txt");
    assert_eq!(
        tabs.save_as(id),
        [Action::AskSaveAs {
            id,
            default: SaveAsDefault {
                folder: SaveAsFolder::Folder(path(r"C:\notes")),
                file_name: "a.txt".into(),
            },
        }]
    );
}

#[test]
fn the_chosen_save_as_path_is_written() {
    let mut tabs = Tabs::new();
    let id = edited_untitled(&mut tabs);
    assert_eq!(
        tabs.save_as_chosen(id, path(r"C:\docs\idea.md")),
        [Action::Write {
            id,
            path: path(r"C:\docs\idea.md"),
            encoding: Encoding::Utf8,
            eol: Eol::Lf,
        }]
    );
}

#[test]
fn saving_an_untitled_tab_names_it_after_the_file_and_watches_it() {
    let mut tabs = Tabs::new();
    let id = edited_untitled(&mut tabs);
    let actions = tabs.saved(id, path(r"C:\docs\idea.md"), stat(5, 200), t0());
    assert_eq!(
        actions,
        [
            Action::Watch(path(r"C:\docs\idea.md")),
            Action::Changed(id),
            Action::AddRecent(path(r"C:\docs\idea.md")),
        ]
    );
    assert_eq!(names(&tabs), ["idea.md"]);
}

#[test]
fn a_saved_untitled_tab_frees_its_number() {
    let mut tabs = Tabs::new();
    let id = edited_untitled(&mut tabs);
    tabs.saved(id, path(r"C:\docs\idea.md"), stat(5, 200), t0());
    tabs.new_untitled();
    assert_eq!(names(&tabs), ["idea.md", "Untitled 1"]);
}

#[test]
fn save_as_to_a_new_path_moves_the_watch() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.txt");
    let actions = tabs.saved(id, path(r"C:\b.txt"), stat(10, 200), t0());
    assert_eq!(
        actions[..2],
        [
            Action::Unwatch(path(r"C:\a.txt")),
            Action::Watch(path(r"C:\b.txt"))
        ]
    );
}

#[test]
fn saving_as_txt_switches_markdown_off() {
    let mut tabs = Tabs::new();
    let id = edited_untitled(&mut tabs);
    tabs.saved(id, path(r"C:\docs\idea.txt"), stat(5, 200), t0());
    let tab = tabs.get(id).unwrap();
    assert_eq!(tab.kind(), Kind::Text);
    assert!(!tab.styled());
}

#[test]
fn saving_a_text_file_as_md_switches_markdown_on() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\idea.txt");
    tabs.saved(id, path(r"C:\idea.md"), stat(10, 200), t0());
    let tab = tabs.get(id).unwrap();
    assert_eq!(tab.kind(), Kind::Markdown);
    assert!(tab.styled());
}

// --- §8 Close a tab with unsaved edits ------------------------------------

#[test]
fn closing_a_tab_with_unsaved_edits_asks_save_dont_save_cancel() {
    let mut tabs = Tabs::new();
    let id = edited_untitled(&mut tabs);
    assert_eq!(tabs.close(id), [Action::AskSaveBeforeClose(id)]);
    assert!(tabs.get(id).is_some());
}

#[test]
fn closing_an_unsaved_tab_in_the_background_shows_it_before_asking() {
    let mut tabs = Tabs::new();
    let id = edited_untitled(&mut tabs);
    tabs.new_untitled();
    assert_eq!(
        tabs.close(id),
        [Action::Activate(id), Action::AskSaveBeforeClose(id)]
    );
}

#[test]
fn dont_save_closes_the_tab_and_drops_its_edits() {
    let mut tabs = Tabs::new();
    let keep = added(&tabs.new_untitled());
    let id = edited_untitled(&mut tabs);
    tabs.close(id);
    let actions = tabs.close_reply(id, CloseReply::Discard);
    assert_eq!(
        actions,
        [
            Action::Closed { id, document: None },
            Action::Activate(keep)
        ]
    );
}

#[test]
fn cancel_keeps_the_tab_and_its_edits() {
    let mut tabs = Tabs::new();
    let id = edited_untitled(&mut tabs);
    tabs.close(id);
    assert_eq!(tabs.close_reply(id, CloseReply::Cancel), []);
    assert!(tabs.get(id).unwrap().unsaved());
}

#[test]
fn save_in_the_close_dialog_writes_the_file_then_closes_the_tab() {
    let mut tabs = Tabs::new();
    let keep = added(&tabs.new_untitled());
    let id = edited_file(&mut tabs, r"C:\a.txt");
    tabs.close(id);
    assert!(matches!(
        tabs.close_reply(id, CloseReply::Save)[..],
        [Action::Write { .. }]
    ));
    let actions = tabs.saved(id, path(r"C:\a.txt"), stat(11, 200), t0());
    assert!(actions.contains(&Action::Closed { id, document: None }));
    assert_eq!(tabs.active(), Some(keep));
}

#[test]
fn save_in_the_close_dialog_on_an_untitled_tab_goes_through_save_as() {
    let mut tabs = Tabs::new();
    let id = edited_untitled(&mut tabs);
    tabs.close(id);
    assert!(matches!(
        tabs.close_reply(id, CloseReply::Save)[..],
        [Action::AskSaveAs { .. }]
    ));
    tabs.save_as_chosen(id, path(r"C:\docs\idea.md"));
    let actions = tabs.saved(id, path(r"C:\docs\idea.md"), stat(5, 200), t0());
    assert_eq!(actions.last(), Some(&Action::CloseWindow));
}

#[test]
fn a_cancelled_or_failed_save_in_the_close_dialog_keeps_the_tab() {
    let mut tabs = Tabs::new();
    let id = edited_untitled(&mut tabs);
    tabs.close(id);
    tabs.close_reply(id, CloseReply::Save);
    assert_eq!(tabs.save_failed(id), []);
    // A later, ordinary save must not close the Tab.
    tabs.save_as_chosen(id, path(r"C:\docs\idea.md"));
    let actions = tabs.saved(id, path(r"C:\docs\idea.md"), stat(5, 200), t0());
    assert!(!actions.iter().any(|a| matches!(a, Action::Closed { .. })));
}

// --- §8 Close the Window with unsaved tabs --------------------------------

#[test]
fn closing_the_window_with_no_unsaved_tabs_closes_it() {
    let mut tabs = Tabs::new();
    open_file(&mut tabs, r"C:\a.txt");
    tabs.new_untitled();
    assert_eq!(tabs.close_window(), [Action::CloseWindow]);
}

#[test]
fn closing_the_window_with_unsaved_tabs_asks_once_listing_them() {
    let mut tabs = Tabs::new();
    let a = edited_file(&mut tabs, r"C:\a.txt");
    open_file(&mut tabs, r"C:\clean.txt");
    let c = edited_untitled(&mut tabs);
    assert_eq!(tabs.close_window(), [Action::AskSaveAll(vec![a, c])]);
}

#[test]
fn discard_all_closes_the_window() {
    let mut tabs = Tabs::new();
    edited_untitled(&mut tabs);
    tabs.close_window();
    assert_eq!(
        tabs.close_window_reply(CloseReply::Discard),
        [Action::CloseWindow]
    );
}

#[test]
fn cancel_on_the_window_dialog_keeps_every_tab() {
    let mut tabs = Tabs::new();
    edited_untitled(&mut tabs);
    tabs.close_window();
    assert_eq!(tabs.close_window_reply(CloseReply::Cancel), []);
    assert_eq!(tabs.iter().count(), 1);
}

#[test]
fn save_all_saves_each_unsaved_tab_in_turn_then_closes_the_window() {
    let mut tabs = Tabs::new();
    let a = edited_file(&mut tabs, r"C:\a.txt");
    let b = edited_untitled(&mut tabs);
    tabs.activate(a);
    tabs.close_window();

    let actions = tabs.close_window_reply(CloseReply::Save);
    assert!(matches!(actions[..], [Action::Write { id, .. }] if id == a));

    // Saving an Untitled tab opens Save As, with the tab shown.
    let actions = tabs.saved(a, path(r"C:\a.txt"), stat(11, 200), t0());
    assert!(!actions.contains(&Action::CloseWindow));
    assert!(matches!(
        actions[actions.len() - 2..],
        [Action::Activate(shown), Action::AskSaveAs { id, .. }] if shown == b && id == b
    ));

    tabs.save_as_chosen(b, path(r"C:\b.md"));
    let actions = tabs.saved(b, path(r"C:\b.md"), stat(5, 200), t0());
    assert_eq!(actions.last(), Some(&Action::CloseWindow));
}

#[test]
fn a_cancelled_save_as_during_save_all_keeps_the_window_open() {
    let mut tabs = Tabs::new();
    let a = edited_untitled(&mut tabs);
    let b = edited_untitled(&mut tabs);
    tabs.close_window();
    tabs.close_window_reply(CloseReply::Save);
    assert_eq!(tabs.save_failed(a), []);
    // b's later save is an ordinary one.
    tabs.save_as_chosen(b, path(r"C:\b.md"));
    let actions = tabs.saved(b, path(r"C:\b.md"), stat(5, 200), t0());
    assert!(!actions.contains(&Action::CloseWindow));
}

// --- §8 Disk changes ------------------------------------------------------

/// The stat `open_file` loads with.
fn as_loaded() -> Option<FileStat> {
    Some(stat(10, 100))
}

fn changed_on_disk() -> Option<FileStat> {
    Some(stat(42, 300))
}

fn reload(id: TabId, p: &str) -> Action {
    Action::Load {
        id,
        path: path(p),
        encoding: None,
        keep_caret_line: true,
    }
}

#[test]
fn disk_change_on_a_tab_with_no_unsaved_edits_reloads_it_keeping_the_caret_line() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    let actions = tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0());
    assert_eq!(actions, [reload(id, r"C:\a.md")]);
    assert_eq!(tabs.get(id).unwrap().notice(), None);
}

#[test]
fn disk_change_on_a_tab_with_unsaved_edits_shows_the_disk_change_notice() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.md");
    let actions = tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0());
    assert_eq!(actions, [Action::Changed(id)]);
    let tab = tabs.get(id).unwrap();
    assert_eq!(tab.notice(), Some(Notice::DiskChange));
    assert!(tab.unsaved());
}

#[test]
fn the_disk_change_notice_is_kept_by_a_tab_that_is_not_shown() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.md");
    let other = added(&tabs.new_untitled());
    tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0());
    assert_eq!(tabs.get(other).unwrap().notice(), None);
    tabs.activate(id);
    assert_eq!(tabs.get(id).unwrap().notice(), Some(Notice::DiskChange));
}

#[test]
fn a_repeated_event_for_the_same_disk_change_does_nothing_more() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.md");
    tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0());
    tabs.notice_action(id, NoticeAction::KeepMine);
    assert_eq!(
        tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0()),
        []
    );
    assert_eq!(tabs.get(id).unwrap().notice(), None);
}

#[test]
fn an_event_that_finds_the_file_as_the_tab_knows_it_does_nothing() {
    let mut tabs = Tabs::new();
    open_file(&mut tabs, r"C:\a.md");
    assert_eq!(tabs.disk_event(&path(r"C:\a.md"), as_loaded(), t0()), []);
}

#[test]
fn reload_on_the_disk_change_notice_rereads_the_file_and_drops_the_edits() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.md");
    tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0());
    assert_eq!(
        tabs.notice_action(id, NoticeAction::Reload),
        [reload(id, r"C:\a.md")]
    );
    tabs.loaded(id, file(42));
    let tab = tabs.get(id).unwrap();
    assert_eq!(tab.notice(), None);
    assert!(!tab.unsaved());
}

#[test]
fn keep_mine_dismisses_the_notice_and_the_next_save_overwrites_the_file() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.md");
    tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0());
    assert_eq!(
        tabs.notice_action(id, NoticeAction::KeepMine),
        [Action::Changed(id)]
    );
    let tab = tabs.get(id).unwrap();
    assert_eq!(tab.notice(), None);
    assert!(tab.unsaved());
    assert!(matches!(tabs.save(id)[..], [Action::Write { .. }]));
}

#[test]
fn reloading_a_file_does_not_move_it_in_recent_files() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0());
    assert!(!tabs
        .loaded(id, file(42))
        .iter()
        .any(|a| matches!(a, Action::AddRecent(_))));
}

#[test]
fn disk_events_for_files_not_open_do_nothing() {
    let mut tabs = Tabs::new();
    open_file(&mut tabs, r"C:\a.md");
    assert_eq!(tabs.disk_event(&path(r"C:\other.md"), None, t0()), []);
}

#[test]
fn a_failed_automatic_reload_shows_the_disk_change_notice_instead_of_taking_the_change_as_read() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0());
    assert_eq!(tabs.load_failed(id), [Action::Changed(id)]);
    assert_eq!(tabs.get(id).unwrap().notice(), Some(Notice::DiskChange));
    // Its Reload button tries again.
    assert_eq!(
        tabs.notice_action(id, NoticeAction::Reload),
        [reload(id, r"C:\a.md")]
    );
}

#[test]
fn a_later_change_after_a_failed_automatic_reload_reloads_again() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0());
    tabs.load_failed(id);
    assert_eq!(
        tabs.disk_event(&path(r"C:\a.md"), Some(stat(43, 301)), t0()),
        [reload(id, r"C:\a.md")]
    );
}

// --- §8 File deleted or renamed on disk -----------------------------------

/// Reports `p` missing, then still missing once the grace has passed.
fn delete_on_disk(tabs: &mut Tabs, p: &str) -> Vec<Action> {
    let now = t0();
    tabs.disk_event(&path(p), None, now);
    tabs.disk_event(&path(p), None, now + MISSING_GRACE)
}

#[test]
fn a_missing_file_is_rechecked_before_it_counts_as_deleted() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    assert_eq!(
        tabs.disk_event(&path(r"C:\a.md"), None, t0()),
        [Action::Recheck {
            path: path(r"C:\a.md"),
            after: MISSING_GRACE
        }]
    );
    let tab = tabs.get(id).unwrap();
    assert!(!tab.unsaved());
    assert_eq!(tab.notice(), None);
}

#[test]
fn a_file_deleted_on_disk_keeps_its_text_marked_unsaved_with_the_deleted_notice() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    assert_eq!(delete_on_disk(&mut tabs, r"C:\a.md"), [Action::Changed(id)]);
    let tab = tabs.get(id).unwrap();
    assert!(tab.unsaved());
    assert_eq!(tab.notice(), Some(Notice::DeletedOnDisk));
}

#[test]
fn a_file_replaced_by_delete_then_create_reloads_a_tab_without_edits_silently() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    tabs.disk_event(&path(r"C:\a.md"), None, t0());
    assert_eq!(
        tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0()),
        [reload(id, r"C:\a.md")]
    );
    let tab = tabs.get(id).unwrap();
    assert!(!tab.unsaved());
    assert_eq!(tab.notice(), None);
}

#[test]
fn save_on_the_deleted_notice_writes_the_file_again_at_its_old_path() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    delete_on_disk(&mut tabs, r"C:\a.md");
    assert_eq!(
        tabs.notice_action(id, NoticeAction::Save),
        [Action::Write {
            id,
            path: path(r"C:\a.md"),
            encoding: Encoding::Utf8,
            eol: Eol::Lf
        }]
    );
    tabs.saved(id, path(r"C:\a.md"), stat(10, 400), t0());
    assert_eq!(tabs.get(id).unwrap().notice(), None);
}

#[test]
fn a_deleted_file_that_comes_back_changed_reloads_a_tab_without_edits() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    delete_on_disk(&mut tabs, r"C:\a.md");
    assert_eq!(
        tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0()),
        [reload(id, r"C:\a.md")]
    );
    tabs.loaded(id, file(42));
    let tab = tabs.get(id).unwrap();
    assert!(!tab.unsaved());
    assert_eq!(tab.notice(), None);
}

#[test]
fn a_deleted_file_that_comes_back_changed_under_edits_shows_the_disk_change_notice() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.md");
    delete_on_disk(&mut tabs, r"C:\a.md");
    tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0());
    let tab = tabs.get(id).unwrap();
    assert_eq!(tab.notice(), Some(Notice::DiskChange));
    assert!(tab.unsaved());
}

#[test]
fn notice_buttons_that_do_not_belong_to_the_shown_notice_do_nothing() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    delete_on_disk(&mut tabs, r"C:\a.md");
    assert_eq!(tabs.notice_action(id, NoticeAction::Reload), []);
    assert_eq!(tabs.notice_action(id, NoticeAction::KeepMine), []);
    assert_eq!(tabs.get(id).unwrap().notice(), Some(Notice::DeletedOnDisk));
}

// --- §8 The own Save ------------------------------------------------------

#[test]
fn scrivis_own_save_does_not_trigger_a_disk_change() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.md");
    let now = t0();
    tabs.saved(id, path(r"C:\a.md"), stat(11, 500), now);
    tabs.edited(id, 12); // typing again right after the save
    let later = now + Duration::from_secs(10);
    assert_eq!(
        tabs.disk_event(&path(r"C:\a.md"), Some(stat(11, 500)), later),
        []
    );
    assert_eq!(tabs.get(id).unwrap().notice(), None);
}

#[test]
fn a_replace_style_own_save_absorbs_the_file_briefly_missing() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    let now = t0();
    tabs.saved(id, path(r"C:\a.md"), stat(11, 500), now);
    let soon = now + Duration::from_millis(300);
    assert_eq!(
        tabs.disk_event(&path(r"C:\a.md"), None, soon),
        [Action::Recheck {
            path: path(r"C:\a.md"),
            after: OWN_SAVE_WINDOW - Duration::from_millis(300)
        }]
    );
    assert_eq!(
        tabs.disk_event(&path(r"C:\a.md"), Some(stat(11, 500)), soon),
        []
    );
    let tab = tabs.get(id).unwrap();
    assert_eq!(tab.notice(), None);
    assert!(!tab.unsaved());
}

#[test]
fn after_the_own_save_window_a_missing_file_gets_only_the_usual_grace() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    let now = t0();
    tabs.saved(id, path(r"C:\a.md"), stat(11, 500), now);
    let late = now + OWN_SAVE_WINDOW + Duration::from_millis(1);
    assert_eq!(
        tabs.disk_event(&path(r"C:\a.md"), None, late),
        [Action::Recheck {
            path: path(r"C:\a.md"),
            after: MISSING_GRACE
        }]
    );
    tabs.disk_event(&path(r"C:\a.md"), None, late + MISSING_GRACE);
    assert_eq!(tabs.get(id).unwrap().notice(), Some(Notice::DeletedOnDisk));
}

#[test]
fn once_a_stat_shows_the_own_save_a_missing_file_gets_only_the_usual_grace() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    let now = t0();
    tabs.saved(id, path(r"C:\a.md"), stat(11, 500), now);
    tabs.disk_event(&path(r"C:\a.md"), Some(stat(11, 500)), now);
    let soon = now + Duration::from_millis(100);
    assert_eq!(
        tabs.disk_event(&path(r"C:\a.md"), None, soon),
        [Action::Recheck {
            path: path(r"C:\a.md"),
            after: MISSING_GRACE
        }]
    );
    tabs.disk_event(&path(r"C:\a.md"), None, soon + MISSING_GRACE);
    assert_eq!(tabs.get(id).unwrap().notice(), Some(Notice::DeletedOnDisk));
}

#[test]
fn another_programs_write_right_after_the_own_save_is_a_disk_change() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    let now = t0();
    tabs.saved(id, path(r"C:\a.md"), stat(11, 500), now);
    let actions = tabs.disk_event(&path(r"C:\a.md"), Some(stat(99, 501)), now);
    assert_eq!(actions, [reload(id, r"C:\a.md")]);
}

#[test]
fn save_as_onto_a_file_open_in_another_tab_is_refused_and_both_tabs_are_untouched() {
    let mut tabs = Tabs::new();
    let other = edited_file(&mut tabs, r"C:\notes\a.md");
    let id = edited_untitled(&mut tabs);
    assert_eq!(
        tabs.save_as_chosen(id, path("c:/NOTES/A.md")),
        [Action::OpenInAnotherTab {
            name: "a.md".into()
        }]
    );
    let (mine, theirs) = (tabs.get(id).unwrap(), tabs.get(other).unwrap());
    assert!(mine.is_untitled() && mine.unsaved());
    assert_eq!(theirs.path(), Some(path(r"C:\notes\a.md").as_path()));
    assert!(theirs.unsaved());
    assert_eq!(theirs.notice(), None);
}

#[test]
fn a_refused_save_as_calls_off_the_close_waiting_on_it() {
    let mut tabs = Tabs::new();
    open_file(&mut tabs, r"C:\a.md");
    let id = edited_untitled(&mut tabs);
    tabs.close(id);
    tabs.close_reply(id, CloseReply::Save);
    tabs.save_as_chosen(id, path(r"C:\a.md"));
    // A later, ordinary save must not close the Tab.
    tabs.save_as_chosen(id, path(r"C:\b.md"));
    let actions = tabs.saved(id, path(r"C:\b.md"), stat(5, 200), t0());
    assert!(!actions.iter().any(|a| matches!(a, Action::Closed { .. })));
}

#[test]
fn save_as_onto_the_tabs_own_file_is_allowed() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.md");
    assert!(matches!(
        tabs.save_as_chosen(id, path(r"C:\a.md"))[..],
        [Action::Write { .. }]
    ));
}

// --- §7 Encoding and line endings -----------------------------------------

#[test]
fn new_files_are_utf8_without_bom_with_lf() {
    let mut tabs = Tabs::new();
    let id = added(&tabs.new_untitled());
    let tab = tabs.get(id).unwrap();
    assert_eq!((tab.encoding(), tab.eol()), (Encoding::Utf8, Eol::Lf));
}

#[test]
fn an_existing_file_is_saved_back_in_its_own_encoding_and_line_ending() {
    let mut tabs = Tabs::new();
    let id = added(&tabs.open(&[path(r"C:\old.ini")]));
    tabs.loaded(
        id,
        LoadedFile {
            encoding: Encoding::Windows1252,
            eol: Eol::Crlf,
            ..file(10)
        },
    );
    tabs.edited(id, 11);
    assert_eq!(
        tabs.save(id),
        [Action::Write {
            id,
            path: path(r"C:\old.ini"),
            encoding: Encoding::Windows1252,
            eol: Eol::Crlf,
        }]
    );
}

#[test]
fn choosing_a_line_ending_converts_the_whole_tab_and_marks_it_unsaved() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.txt");
    assert_eq!(
        tabs.set_eol(id, Eol::Crlf),
        [
            Action::ConvertEol { id, eol: Eol::Crlf },
            Action::Changed(id)
        ]
    );
    let tab = tabs.get(id).unwrap();
    assert_eq!(tab.eol(), Eol::Crlf);
    assert!(tab.unsaved());
}

#[test]
fn choosing_the_line_ending_the_tab_already_has_does_nothing() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.txt");
    assert_eq!(tabs.set_eol(id, Eol::Lf), []);
    assert!(!tabs.get(id).unwrap().unsaved());
}

#[test]
fn choosing_an_encoding_changes_how_the_next_save_encodes_and_marks_the_tab_unsaved() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.txt");
    assert_eq!(
        tabs.set_encoding(id, Encoding::Utf16Le),
        [Action::Changed(id)]
    );
    assert!(tabs.get(id).unwrap().unsaved());
    assert!(matches!(
        tabs.save(id)[..],
        [Action::Write {
            encoding: Encoding::Utf16Le,
            ..
        }]
    ));
}

#[test]
fn reopen_with_encoding_rereads_the_file_in_that_encoding() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.txt");
    assert_eq!(
        tabs.reopen_with_encoding(id, Encoding::Windows1252),
        [Action::Load {
            id,
            path: path(r"C:\a.txt"),
            encoding: Some(Encoding::Windows1252),
            keep_caret_line: true,
        }]
    );
}

#[test]
fn reopen_with_encoding_asks_first_when_the_tab_has_unsaved_edits() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.txt");
    assert_eq!(
        tabs.reopen_with_encoding(id, Encoding::Utf16Be),
        [Action::AskDiscardForReopen {
            id,
            encoding: Encoding::Utf16Be
        }]
    );
    assert!(matches!(
        tabs.reopen_confirmed(id, Encoding::Utf16Be)[..],
        [Action::Load {
            encoding: Some(Encoding::Utf16Be),
            ..
        }]
    ));
}

#[test]
fn reopen_with_encoding_does_nothing_for_an_untitled_tab() {
    let mut tabs = Tabs::new();
    let id = added(&tabs.new_untitled());
    assert_eq!(tabs.reopen_with_encoding(id, Encoding::Utf16Le), []);
}

// --- §7 The save point (SCRIVI-16) ---------------------------------------

#[test]
fn leaving_the_save_point_marks_the_tab_unsaved() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.txt");
    assert_eq!(tabs.save_point_left(id), [Action::Changed(id)]);
    assert!(tabs.get(id).unwrap().unsaved());
    assert_eq!(tabs.save_point_left(id), []);
}

#[test]
fn undoing_back_to_the_save_point_clears_the_unsaved_dot() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.txt");
    assert_eq!(tabs.save_point_reached(id), [Action::Changed(id)]);
    assert!(!tabs.get(id).unwrap().unsaved());
    assert_eq!(tabs.save_point_reached(id), []);
}

#[test]
fn back_at_the_save_point_a_line_ending_choice_still_needs_saving() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.txt");
    tabs.set_eol(id, Eol::Crlf);
    tabs.save_point_left(id);
    tabs.save_point_reached(id);
    assert!(tabs.get(id).unwrap().unsaved());
}

#[test]
fn back_at_the_save_point_an_encoding_choice_still_needs_saving() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.txt");
    tabs.set_encoding(id, Encoding::Utf16Le);
    assert_eq!(tabs.save_point_reached(id), []);
    assert!(tabs.get(id).unwrap().unsaved());
}

#[test]
fn back_at_the_save_point_a_file_deleted_on_disk_stays_unsaved() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\notes.md");
    delete_on_disk(&mut tabs, r"C:\notes.md");
    tabs.save_point_reached(id);
    assert!(tabs.get(id).unwrap().unsaved());
}

#[test]
fn a_save_clears_line_ending_and_encoding_choices_too() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.txt");
    tabs.set_eol(id, Eol::Crlf);
    tabs.set_encoding(id, Encoding::Utf8Bom);
    tabs.saved(id, path(r"C:\a.txt"), stat(12, 200), t0());
    assert!(!tabs.get(id).unwrap().unsaved());
    // Undoing past the new save point is an edit again.
    tabs.save_point_left(id);
    assert!(tabs.get(id).unwrap().unsaved());
}

#[test]
fn the_save_point_of_a_read_only_tab_is_ignored() {
    let mut tabs = Tabs::new();
    let id = open_sized(&mut tabs, r"C:\large.log", LARGE_FILE_BYTES + 1);
    assert_eq!(tabs.save_point_left(id), []);
    assert!(!tabs.get(id).unwrap().unsaved());
}

// --- File identity (SCRIVI-16) ------------------------------------------------

fn id_of(index: u64) -> Option<FileId> {
    Some(FileId { volume: 7, index })
}

fn open_with_id(tabs: &mut Tabs, p: &str, file_id: Option<FileId>, lossy: bool) -> TabId {
    let id = added(&tabs.open(&[path(p)]));
    tabs.loaded(id, LoadedFile { file_id, lossy, ..file(10) });
    id
}

fn loads(actions: &[Action]) -> bool {
    actions.iter().any(|a| matches!(a, Action::Load { .. }))
}

#[test]
fn opening_an_alias_of_an_open_file_switches_to_its_tab() {
    let mut tabs = Tabs::new();
    let first = open_with_id(&mut tabs, r"C:\real\a.md", id_of(5), false);
    open_with_id(&mut tabs, r"C:\other.md", id_of(6), false);
    let actions = tabs.open_with_ids(&[(path(r"C:\junction\a.md"), id_of(5))]);
    assert!(actions.contains(&Action::Activate(first)), "{actions:?}");
    assert!(!loads(&actions));
    assert_eq!(tabs.iter().count(), 2);
}

#[test]
fn opening_a_different_file_with_a_different_identity_adds_a_tab() {
    let mut tabs = Tabs::new();
    open_with_id(&mut tabs, r"C:\a.md", id_of(5), false);
    assert!(loads(&tabs.open_with_ids(&[(path(r"C:\b.md"), id_of(6))])));
}

#[test]
fn save_as_onto_an_alias_of_an_open_tabs_file_is_refused() {
    let mut tabs = Tabs::new();
    let other = open_with_id(&mut tabs, r"C:\real\a.md", id_of(5), false);
    let id = edited_untitled(&mut tabs);
    assert_eq!(
        tabs.save_as_chosen_with_id(id, path(r"C:\junction\a.md"), id_of(5)),
        [Action::OpenInAnotherTab { name: "a.md".into() }]
    );
    assert_eq!(tabs.get(other).unwrap().notice(), None);
}

#[test]
fn save_as_onto_an_alias_of_a_lossy_tabs_own_file_is_refused() {
    let mut tabs = Tabs::new();
    let id = open_with_id(&mut tabs, r"C:\real\odd.txt", id_of(5), true);
    let actions = tabs.save_as_chosen_with_id(id, path(r"C:\junction\odd.txt"), id_of(5));
    assert!(!actions.iter().any(|a| matches!(a, Action::Write { .. })), "{actions:?}");
}

#[test]
fn the_same_path_still_matches_when_the_identity_changed() {
    // A replaced file (an editor's save) gets a new index.
    let mut tabs = Tabs::new();
    open_with_id(&mut tabs, r"C:\a.md", id_of(5), false);
    assert!(!loads(&tabs.open_with_ids(&[(path(r"c:\A.md"), id_of(9))])));
}

#[test]
fn without_an_identity_paths_compare_by_normalised_string() {
    let mut tabs = Tabs::new();
    open_with_id(&mut tabs, r"C:\a.md", id_of(5), false);
    assert!(!loads(&tabs.open_with_ids(&[(path("c:/A.MD"), None)])));
}

#[test]
fn a_saved_tab_remembers_the_identity_of_the_file_it_wrote() {
    let mut tabs = Tabs::new();
    let id = edited_untitled(&mut tabs);
    tabs.saved_with_id(id, path(r"C:\new.md"), stat(5, 1), id_of(8), t0());
    assert!(!loads(&tabs.open_with_ids(&[(path(r"C:\alias\new.md"), id_of(8))])));
}

// --- §7 Bytes the encoding cannot hold (SCRIVI-16) -------------------------

fn open_lossy(tabs: &mut Tabs, p: &str) -> TabId {
    let id = added(&tabs.open(&[path(p)]));
    tabs.loaded(id, LoadedFile { lossy: true, ..file(10) });
    id
}

#[test]
fn a_file_with_bytes_its_encoding_cannot_hold_opens_read_only_with_a_note() {
    let mut tabs = Tabs::new();
    let id = open_lossy(&mut tabs, r"C:\odd.txt");
    let tab = tabs.get(id).unwrap();
    assert!(tab.read_only());
    assert!(tab.wrap_allowed());
    assert_eq!(tab.status_note(), Some(StatusNote::InvalidBytes));
    assert!(!tab.unsaved());
}

#[test]
fn a_lossy_tab_refuses_edits_and_encoding_or_line_ending_changes() {
    let mut tabs = Tabs::new();
    let id = open_lossy(&mut tabs, r"C:\odd.txt");
    assert_eq!(tabs.edited(id, 1), []);
    assert_eq!(tabs.save_point_left(id), []);
    assert_eq!(tabs.set_eol(id, Eol::Crlf), []);
    assert_eq!(tabs.set_encoding(id, Encoding::Utf16Le), []);
    assert!(!tabs.get(id).unwrap().unsaved());
}

#[test]
fn saving_a_lossy_tab_over_its_file_is_refused_with_its_note() {
    let mut tabs = Tabs::new();
    let id = open_lossy(&mut tabs, r"C:\odd.txt");
    assert_eq!(tabs.save(id), [Action::Flash(StatusNote::InvalidBytes)]);
    assert_eq!(
        tabs.save_as_chosen(id, path(r"C:\ODD.txt")),
        [Action::Flash(StatusNote::InvalidBytes)]
    );
    assert!(matches!(
        tabs.save_as_chosen(id, path(r"C:\copy.txt"))[..],
        [Action::Write { .. }]
    ));
}

#[test]
fn a_refused_lossy_save_calls_off_the_close_waiting_on_it() {
    let mut tabs = Tabs::new();
    let id = open_lossy(&mut tabs, r"C:\odd.txt");
    delete_on_disk(&mut tabs, r"C:\odd.txt");
    assert!(matches!(tabs.close(id)[..], [.., Action::AskSaveBeforeClose(_)]));
    assert_eq!(
        tabs.close_reply(id, CloseReply::Save),
        [Action::Flash(StatusNote::InvalidBytes)]
    );
    // The close was called off: saving a copy later does not close the Tab.
    tabs.save_as_chosen(id, path(r"C:\copy.txt"));
    let actions = tabs.saved(id, path(r"C:\copy.txt"), stat(10, 300), t0());
    assert!(!actions.iter().any(|a| matches!(a, Action::Closed { .. })));
}

#[test]
fn a_saved_copy_of_a_lossy_tab_is_editable_it_holds_what_the_tab_shows() {
    let mut tabs = Tabs::new();
    let id = open_lossy(&mut tabs, r"C:\odd.txt");
    tabs.save_as_chosen(id, path(r"C:\copy.txt"));
    tabs.saved(id, path(r"C:\copy.txt"), stat(10, 300), t0());
    let tab = tabs.get(id).unwrap();
    assert!(!tab.read_only());
    assert_eq!(tab.status_note(), None);
}

#[test]
fn save_as_utf8_instead_switches_the_encoding_even_of_a_read_only_tab() {
    let mut tabs = Tabs::new();
    let id = added(&tabs.open(&[path(r"C:\odd.txt")]));
    tabs.loaded(
        id,
        LoadedFile { encoding: Encoding::Other(encoding_rs::SHIFT_JIS), lossy: true, ..file(10) },
    );
    assert_eq!(tabs.use_utf8_instead(id), [Action::Changed(id)]);
    let tab = tabs.get(id).unwrap();
    assert_eq!(tab.encoding(), Encoding::Utf8);
    assert!(tab.unsaved());
    assert_eq!(tabs.use_utf8_instead(id), []);
}

#[test]
fn reopening_a_lossy_tab_in_an_encoding_that_fits_makes_it_editable() {
    let mut tabs = Tabs::new();
    let id = open_lossy(&mut tabs, r"C:\odd.txt");
    assert!(matches!(
        tabs.reopen_with_encoding(id, Encoding::Windows1252)[..],
        [Action::Load { encoding: Some(Encoding::Windows1252), .. }]
    ));
    tabs.loaded(id, LoadedFile { encoding: Encoding::Windows1252, ..file(10) });
    let tab = tabs.get(id).unwrap();
    assert!(!tab.read_only());
    assert_eq!(tab.status_note(), None);
}

// --- §7 Large files -------------------------------------------------------

#[test]
fn a_file_over_50_mb_opens_read_only_without_wrap_styling_or_spell_check() {
    let mut tabs = Tabs::new();
    let id = open_sized(&mut tabs, r"C:\large.md", LARGE_FILE_BYTES + 1);
    let tab = tabs.get(id).unwrap();
    assert!(tab.read_only());
    assert!(!tab.wrap_allowed());
    assert!(!tab.styled());
    assert!(!tab.spell_checked());
    assert!(!tab.can_render());
    assert_eq!(tab.notice(), Some(Notice::LargeFile));
    assert_eq!(tab.status_note(), Some(StatusNote::ReadOnly));
}

#[test]
fn a_file_of_exactly_50_mb_is_not_a_large_file() {
    let mut tabs = Tabs::new();
    let id = open_sized(&mut tabs, r"C:\exactly-50-mb.log", LARGE_FILE_BYTES);
    let tab = tabs.get(id).unwrap();
    assert!(!tab.read_only());
    assert_eq!(tab.notice(), None);
}

#[test]
fn the_large_file_notice_closes_with_its_x() {
    let mut tabs = Tabs::new();
    let id = open_sized(&mut tabs, r"C:\large.log", LARGE_FILE_BYTES + 1);
    assert_eq!(
        tabs.notice_action(id, NoticeAction::Dismiss),
        [Action::Changed(id)]
    );
    let tab = tabs.get(id).unwrap();
    assert_eq!(tab.notice(), None);
    assert!(tab.read_only());
}

#[test]
fn a_large_file_refuses_edits_and_encoding_or_line_ending_changes() {
    let mut tabs = Tabs::new();
    let id = open_sized(&mut tabs, r"C:\large.log", LARGE_FILE_BYTES + 1);
    assert_eq!(tabs.edited(id, 1), []);
    assert_eq!(tabs.set_eol(id, Eol::Crlf), []);
    assert_eq!(tabs.set_encoding(id, Encoding::Utf16Le), []);
    assert!(!tabs.get(id).unwrap().unsaved());
}

// --- §5 / §6 The 1 MB rule and the Rendered view --------------------------

#[test]
fn a_markdown_file_over_1_mb_gets_no_styling_or_spell_check() {
    let mut tabs = Tabs::new();
    let id = open_sized(&mut tabs, r"C:\long.md", MARKDOWN_LIMIT_BYTES as u64 + 1);
    let tab = tabs.get(id).unwrap();
    assert!(!tab.styled());
    assert!(!tab.spell_checked());
    assert_eq!(tab.status_note(), Some(StatusNote::StylingOff));
}

#[test]
fn a_markdown_file_of_exactly_1_mb_is_styled() {
    let mut tabs = Tabs::new();
    let id = open_sized(&mut tabs, r"C:\long.md", MARKDOWN_LIMIT_BYTES as u64);
    let tab = tabs.get(id).unwrap();
    assert!(tab.styled());
    assert_eq!(tab.status_note(), None);
}

#[test]
fn a_text_file_over_1_mb_has_no_status_note() {
    let mut tabs = Tabs::new();
    let id = open_sized(&mut tabs, r"C:\long.txt", MARKDOWN_LIMIT_BYTES as u64 + 1);
    assert_eq!(tabs.get(id).unwrap().status_note(), None);
}

#[test]
fn an_untitled_tab_is_styled_and_spell_checked() {
    let mut tabs = Tabs::new();
    let id = added(&tabs.new_untitled());
    let tab = tabs.get(id).unwrap();
    assert!(tab.styled());
    assert!(tab.spell_checked());
}

#[test]
fn an_untitled_tab_edited_past_1_mb_loses_styling() {
    let mut tabs = Tabs::new();
    let id = edited_untitled(&mut tabs);
    assert_eq!(
        tabs.edited(id, MARKDOWN_LIMIT_BYTES + 1),
        [Action::Changed(id)]
    );
    assert!(!tabs.get(id).unwrap().styled());
}

#[test]
fn ctrl_e_switches_a_markdown_tab_to_the_rendered_view_and_back() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    assert_eq!(tabs.toggle_view(id), [Action::Changed(id)]);
    let tab = tabs.get(id).unwrap();
    assert_eq!(tab.view(), View::Rendered);
    assert_eq!(tab.status_note(), Some(StatusNote::RenderedView));
    assert!(tabs.any_rendered());
    tabs.toggle_view(id);
    assert_eq!(tabs.get(id).unwrap().view(), View::Source);
    assert!(!tabs.any_rendered());
}

#[test]
fn ctrl_e_renders_an_untitled_tab() {
    let mut tabs = Tabs::new();
    let id = added(&tabs.new_untitled());
    tabs.toggle_view(id);
    assert_eq!(tabs.get(id).unwrap().view(), View::Rendered);
}

#[test]
fn ctrl_e_does_nothing_in_a_text_file() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.txt");
    assert_eq!(tabs.toggle_view(id), []);
    assert_eq!(tabs.get(id).unwrap().view(), View::Source);
}

#[test]
fn ctrl_e_on_a_markdown_file_over_1_mb_says_too_large_to_render() {
    let mut tabs = Tabs::new();
    let id = open_sized(&mut tabs, r"C:\long.md", MARKDOWN_LIMIT_BYTES as u64 + 1);
    assert_eq!(
        tabs.toggle_view(id),
        [Action::Flash(StatusNote::TooLargeToRender)]
    );
    assert_eq!(tabs.get(id).unwrap().view(), View::Source);
}

#[test]
fn each_tab_remembers_its_view() {
    let mut tabs = Tabs::new();
    let a = open_file(&mut tabs, r"C:\a.md");
    tabs.toggle_view(a);
    let b = open_file(&mut tabs, r"C:\b.md");
    tabs.activate(a);
    assert_eq!(tabs.get(a).unwrap().view(), View::Rendered);
    assert_eq!(tabs.get(b).unwrap().view(), View::Source);
}

#[test]
fn a_reload_past_1_mb_returns_a_rendered_tab_to_the_source_view() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    tabs.toggle_view(id);
    tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0());
    tabs.loaded(id, file(MARKDOWN_LIMIT_BYTES as u64 + 1));
    assert_eq!(tabs.get(id).unwrap().view(), View::Source);
}

#[test]
fn saving_a_rendered_markdown_file_as_txt_returns_it_to_the_source_view() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    tabs.toggle_view(id);
    tabs.saved(id, path(r"C:\a.txt"), stat(10, 200), t0());
    assert_eq!(tabs.get(id).unwrap().view(), View::Source);
}

// --- Switching tabs -------------------------------------------------------

#[test]
fn next_and_prev_cycle_through_the_tabs_and_wrap_around() {
    let mut tabs = Tabs::new();
    let a = added(&tabs.new_untitled());
    let b = added(&tabs.new_untitled());
    let c = added(&tabs.new_untitled());
    assert_eq!(tabs.next(), [Action::Activate(a)]);
    assert_eq!(tabs.next(), [Action::Activate(b)]);
    assert_eq!(tabs.prev(), [Action::Activate(a)]);
    assert_eq!(tabs.prev(), [Action::Activate(c)]);
}

#[test]
fn next_with_one_tab_does_nothing() {
    let mut tabs = Tabs::new();
    tabs.new_untitled();
    assert_eq!(tabs.next(), []);
    assert_eq!(tabs.prev(), []);
}

#[test]
fn activating_the_shown_tab_does_nothing() {
    let mut tabs = Tabs::new();
    let id = added(&tabs.new_untitled());
    assert_eq!(tabs.activate(id), []);
}

// --- Stale ids ------------------------------------------------------------

#[test]
fn commands_for_a_closed_tab_do_nothing() {
    let mut tabs = Tabs::new();
    tabs.new_untitled();
    let gone = open_file(&mut tabs, r"C:\a.md");
    tabs.close(gone);
    let now = t0();
    assert_eq!(tabs.edited(gone, 1), []);
    assert_eq!(tabs.loaded(gone, file(1)), []);
    assert_eq!(tabs.load_failed(gone), []);
    assert_eq!(tabs.saved(gone, path(r"C:\a.md"), stat(1, 1), now), []);
    assert_eq!(tabs.save(gone), []);
    assert_eq!(tabs.save_as(gone), []);
    assert_eq!(tabs.save_as_chosen(gone, path(r"C:\b.md")), []);
    assert_eq!(tabs.close(gone), []);
    assert_eq!(tabs.close_reply(gone, CloseReply::Save), []);
    assert_eq!(tabs.notice_action(gone, NoticeAction::Reload), []);
    assert_eq!(tabs.set_eol(gone, Eol::Cr), []);
    assert_eq!(tabs.set_encoding(gone, Encoding::Utf16Le), []);
    assert_eq!(tabs.reopen_with_encoding(gone, Encoding::Utf16Le), []);
    assert_eq!(tabs.reopen_confirmed(gone, Encoding::Utf16Le), []);
    assert_eq!(tabs.toggle_view(gone), []);
    assert_eq!(tabs.activate(gone), []);
    assert_eq!(tabs.save_as_default(gone), None);
    assert_eq!(tabs.iter().count(), 1);
}

#[test]
fn a_document_for_a_closed_tab_is_handed_straight_back() {
    let mut tabs = Tabs::new();
    tabs.new_untitled();
    let gone = added(&tabs.new_untitled());
    tabs.close(gone);
    assert_eq!(tabs.set_document(gone, EditorDoc(9)), Some(EditorDoc(9)));
}

// --- Edge cases ----------------------------------------------------------

#[test]
fn a_file_still_missing_when_rechecked_after_the_own_save_window_is_deleted_on_disk() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    let now = t0();
    tabs.saved(id, path(r"C:\a.md"), stat(11, 500), now);
    let soon = now + Duration::from_millis(300);
    tabs.disk_event(&path(r"C:\a.md"), None, soon);
    tabs.disk_event(&path(r"C:\a.md"), None, soon + OWN_SAVE_WINDOW);
    assert_eq!(tabs.get(id).unwrap().notice(), Some(Notice::DeletedOnDisk));
}

#[test]
fn when_the_last_of_several_files_fails_to_load_an_earlier_one_is_shown() {
    let mut tabs = Tabs::new();
    let actions = tabs.open(&[path(r"C:\a.txt"), path(r"C:\locked.txt")]);
    let ids: Vec<TabId> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Load { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    tabs.loaded(ids[0], file(1));
    let actions = tabs.load_failed(ids[1]);
    assert_eq!(actions.last(), Some(&Action::Activate(ids[0])));
    assert_eq!(tabs.active(), Some(ids[0]));
}

#[test]
fn a_failed_save_of_another_tab_does_not_call_off_a_pending_close() {
    let mut tabs = Tabs::new();
    let other = edited_file(&mut tabs, r"C:\other.txt");
    let id = edited_file(&mut tabs, r"C:\a.txt");
    tabs.close(id);
    tabs.close_reply(id, CloseReply::Save);
    tabs.save_failed(other);
    let actions = tabs.saved(id, path(r"C:\a.txt"), stat(11, 200), t0());
    assert!(actions.contains(&Action::Closed { id, document: None }));
}

#[test]
fn esc_returns_a_rendered_tab_to_the_source_view_and_does_nothing_in_the_source_view() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    assert_eq!(tabs.show_source(id), []);
    tabs.toggle_view(id);
    assert_eq!(tabs.show_source(id), [Action::Changed(id)]);
    assert_eq!(tabs.get(id).unwrap().view(), View::Source);
}

#[test]
fn the_large_file_notice_can_show_the_file_size() {
    let mut tabs = Tabs::new();
    let id = open_sized(&mut tabs, r"C:\large.log", LARGE_FILE_BYTES + 1);
    assert_eq!(
        tabs.get(id).unwrap().file_size(),
        Some(LARGE_FILE_BYTES + 1)
    );
    let untitled = added(&tabs.new_untitled());
    assert_eq!(tabs.get(untitled).unwrap().file_size(), None);
}

#[test]
fn a_deleted_file_that_comes_back_as_it_was_leaves_a_tab_without_edits_clean() {
    let mut tabs = Tabs::new();
    let id = open_file(&mut tabs, r"C:\a.md");
    delete_on_disk(&mut tabs, r"C:\a.md");
    assert_eq!(
        tabs.disk_event(&path(r"C:\a.md"), as_loaded(), t0()),
        [Action::Changed(id)]
    );
    let tab = tabs.get(id).unwrap();
    assert_eq!(tab.notice(), None);
    assert!(!tab.unsaved());
}

#[test]
fn a_deleted_file_that_comes_back_as_it_was_keeps_the_edits_unsaved() {
    let mut tabs = Tabs::new();
    let id = edited_file(&mut tabs, r"C:\a.md");
    delete_on_disk(&mut tabs, r"C:\a.md");
    tabs.disk_event(&path(r"C:\a.md"), as_loaded(), t0());
    let tab = tabs.get(id).unwrap();
    assert_eq!(tab.notice(), None);
    assert!(tab.unsaved());
}

// --- Watching and loading -------------------------------------------------

#[test]
fn a_change_seen_while_the_file_was_loading_reloads_it_once_loaded() {
    let mut tabs = Tabs::new();
    let id = added(&tabs.open(&[path(r"C:\a.md")]));
    assert_eq!(
        tabs.disk_event(&path(r"C:\a.md"), changed_on_disk(), t0()),
        []
    );
    let actions = tabs.loaded(id, file(10));
    assert!(actions.contains(&reload(id, r"C:\a.md")));
    // The reload's own load does not start another.
    assert!(!tabs.loaded(id, file(42)).contains(&reload(id, r"C:\a.md")));
}

#[test]
fn a_change_seen_while_loading_that_the_load_already_read_needs_no_reload() {
    let mut tabs = Tabs::new();
    let id = added(&tabs.open(&[path(r"C:\a.md")]));
    tabs.disk_event(&path(r"C:\a.md"), as_loaded(), t0());
    assert!(!tabs.loaded(id, file(10)).contains(&reload(id, r"C:\a.md")));
}

#[test]
fn opening_a_path_whose_tab_is_still_loading_shows_it_once_loaded() {
    let mut tabs = Tabs::new();
    let actions = tabs.open(&[path(r"C:\a.md"), path(r"C:\A.MD")]);
    let id = added(&actions);
    assert_eq!(actions.len(), 2, "{actions:?}");
    assert_eq!(tabs.open(&[path(r"C:\a.md")]), []);
    let actions = tabs.loaded(id, file(10));
    assert_eq!(
        actions,
        [
            Action::Changed(id),
            Action::Activate(id),
            Action::AddRecent(path(r"C:\a.md")),
        ]
    );
}

#[test]
fn status_notes_read_as_the_spec_words_them() {
    assert_eq!(StatusNote::ReadOnly.text(), "Read-only");
    assert_eq!(StatusNote::StylingOff.text(), "Markdown styling off (over 1 MB)");
    assert_eq!(StatusNote::RenderedView.text(), "Rendered view \u{b7} Esc to edit");
    assert_eq!(StatusNote::TooLargeToRender.text(), "Too large to render");
    assert_eq!(StatusNote::SavedInPlace.text(), "Saved in place: the safe replace failed");
    assert_eq!(
        StatusNote::InvalidBytes.text(),
        "Read-only: some bytes don\u{2019}t fit this encoding \u{b7} Encoding \u{203a} Reopen with encoding\u{2026}"
    );
}
