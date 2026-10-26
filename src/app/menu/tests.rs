//! The menus as data, and the menu bar from the keyboard. Each name states the rule.

use super::*;
use crate::app::cmd;

fn state() -> State {
    State {
        encoding: Encoding::Utf8,
        eol: Eol::Lf,
        read_only: false,
        untitled: false,
        can_undo: true,
        can_redo: true,
        selection: true,
        can_paste: true,
        markdown: true,
        rendered: false,
        wrap: true,
        wrap_allowed: true,
    }
}

/// Labels without access-key ampersands, separators as "-".
fn labels(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .map(|i| match i {
            Item::Separator => "-".to_string(),
            Item::Command { label, .. } | Item::Sub { label, .. } => label.replace('&', ""),
        })
        .collect()
}

fn sub(items: &[Item], name: &str) -> (bool, Vec<Item>) {
    items
        .iter()
        .find_map(|i| match i {
            Item::Sub { label, enabled, items } if label.replace('&', "") == name => {
                Some((*enabled, items.clone()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {name} submenu"))
}

fn command(items: &[Item], id: u16) -> &Item {
    items.iter().find(|i| matches!(i, Item::Command { id: c, .. } if *c == id)).expect("no such command")
}

fn enabled(items: &[Item], id: u16) -> bool {
    matches!(command(items, id), Item::Command { enabled: true, .. })
}

fn checked(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| match i {
            Item::Command { label, checked: true, .. } => Some(label.replace('&', "")),
            _ => None,
        })
        .collect()
}

#[test]
fn the_file_menu_reads_as_the_spec_lists_it() {
    assert_eq!(
        labels(&items(FILE, &state())),
        [
            "New tab\tCtrl+T",
            "Open…\tCtrl+O",
            "Recent files",
            "-",
            "Save\tCtrl+S",
            "Save as…\tCtrl+Shift+S",
            "-",
            "Encoding",
            "Line endings",
            "-",
            "Close tab\tCtrl+W",
            "Exit",
        ]
    );
}

#[test]
fn recent_files_is_an_empty_disabled_submenu_while_the_list_is_empty() {
    let (enabled, items) = sub(&items(FILE, &state()), "Recent files");
    assert!(!enabled);
    assert!(items.is_empty());
}

#[test]
fn the_encoding_submenu_checks_the_tabs_encoding_and_ends_with_reopen() {
    let s = State { encoding: Encoding::Utf16Le, ..state() };
    let (enabled, items) = sub(&items(FILE, &s), "Encoding");
    assert!(enabled);
    assert_eq!(
        labels(&items),
        ["UTF-8", "UTF-8 with BOM", "UTF-16 LE", "UTF-16 BE", "Windows-1252 (ANSI)", "-", "Reopen with encoding…"]
    );
    assert_eq!(checked(&items), ["UTF-16 LE"]);
    assert!(enabled_all(&items));
}

fn enabled_all(items: &[Item]) -> bool {
    items.iter().all(|i| !matches!(i, Item::Command { enabled: false, .. }))
}

#[test]
fn an_encoding_outside_the_menu_shows_checked_at_the_top() {
    let s = State { encoding: Encoding::Other(encoding_rs::SHIFT_JIS), ..state() };
    let (_, items) = sub(&items(FILE, &s), "Encoding");
    assert_eq!(labels(&items)[..2], ["Shift_JIS".to_string(), "-".to_string()]);
    assert_eq!(checked(&items), ["Shift_JIS"]);
}

#[test]
fn a_read_only_tab_can_only_reopen_with_another_encoding() {
    let s = State { read_only: true, ..state() };
    let file = items(FILE, &s);
    let (_, encodings) = sub(&file, "Encoding");
    for (i, _) in Encoding::MENU.iter().enumerate() {
        assert!(!enabled(&encodings, cmd::ENCODING + i as u16));
    }
    assert!(enabled(&encodings, cmd::REOPEN));
    let (_, eols) = sub(&file, "Line endings");
    assert!(!enabled_all(&eols));
}

#[test]
fn an_untitled_tab_has_no_file_to_reopen() {
    let s = State { untitled: true, ..state() };
    let (_, items) = sub(&items(FILE, &s), "Encoding");
    assert!(!enabled(&items, cmd::REOPEN));
}

#[test]
fn the_line_endings_submenu_checks_the_tabs_line_ending() {
    let s = State { eol: Eol::Crlf, ..state() };
    let (_, items) = sub(&items(FILE, &s), "Line endings");
    assert_eq!(labels(&items), ["LF", "CRLF", "CR"]);
    assert_eq!(checked(&items), ["CRLF"]);
}

#[test]
fn the_edit_menu_has_the_clipboard_and_undo_commands() {
    let edit = items(EDIT, &state());
    assert_eq!(
        labels(&edit)[..7],
        [
            "Undo\tCtrl+Z",
            "Redo\tCtrl+Y",
            "-",
            "Cut\tCtrl+X",
            "Copy\tCtrl+C",
            "Paste\tCtrl+V",
            "Select all\tCtrl+A",
        ]
    );
    for id in [cmd::UNDO, cmd::REDO, cmd::CUT, cmd::COPY, cmd::PASTE, cmd::SELECT_ALL] {
        assert!(enabled(&edit, id));
    }
}

#[test]
fn edit_commands_follow_the_editor() {
    let s = State { can_undo: false, can_redo: false, selection: false, can_paste: false, ..state() };
    let edit = items(EDIT, &s);
    for id in [cmd::UNDO, cmd::REDO, cmd::CUT, cmd::COPY, cmd::PASTE] {
        assert!(!enabled(&edit, id), "{id}");
    }
    assert!(enabled(&edit, cmd::SELECT_ALL));
    // Read-only text can be copied, not cut or pasted into.
    let edit = items(EDIT, &State { read_only: true, ..state() });
    assert!(enabled(&edit, cmd::COPY));
    assert!(!enabled(&edit, cmd::CUT));
    assert!(!enabled(&edit, cmd::PASTE));
}

#[test]
fn find_replace_go_to_line_and_the_view_menu_are_on() {
    // Even a read-only Tab can be searched.
    let edit = items(EDIT, &State { read_only: true, ..state() });
    assert_eq!(labels(&edit)[7..], ["-", "Find…\tCtrl+F", "Replace…\tCtrl+H", "Go to line…\tCtrl+G"]);
    for id in [cmd::FIND, cmd::REPLACE, cmd::GO_TO_LINE] {
        assert!(enabled(&edit, id));
    }
    let view = items(VIEW, &state());
    assert_eq!(
        labels(&view),
        [
            "Rendered view\tCtrl+E",
            "Word wrap\tAlt+Z",
            "-",
            "Zoom in\tCtrl++",
            "Zoom out\tCtrl+\u{2212}",
            "Reset zoom\tCtrl+0"
        ]
    );
    for id in [cmd::WORD_WRAP, cmd::ZOOM_IN, cmd::ZOOM_OUT, cmd::ZOOM_RESET] {
        assert!(enabled(&view, id), "{id}");
    }
}

fn is_checked(items: &[Item], id: u16) -> bool {
    items.iter().any(|i| matches!(i, Item::Command { id: i, checked: true, .. } if *i == id))
}

#[test]
fn find_replace_and_go_to_line_are_greyed_while_the_rendered_view_shows() {
    let edit = items(EDIT, &State { rendered: true, read_only: true, ..state() });
    for id in [cmd::FIND, cmd::REPLACE, cmd::GO_TO_LINE] {
        assert!(!enabled(&edit, id), "{id}");
    }
}

#[test]
fn word_wrap_is_checked_when_on_and_greyed_for_a_large_file() {
    assert!(is_checked(&items(VIEW, &state()), cmd::WORD_WRAP));
    assert!(!is_checked(&items(VIEW, &State { wrap: false, ..state() }), cmd::WORD_WRAP));
    let large = items(VIEW, &State { wrap_allowed: false, ..state() });
    assert!(!enabled(&large, cmd::WORD_WRAP));
    assert!(!is_checked(&large, cmd::WORD_WRAP));
}

/// Every command id, submenus included.
fn ids(items: &[Item], out: &mut Vec<u16>) {
    for i in items {
        match i {
            Item::Command { id, .. } => out.push(*id),
            Item::Sub { items, .. } => ids(items, out),
            Item::Separator => {}
        }
    }
}

#[test]
fn command_ids_are_distinct_and_clear_of_the_tab_shortcuts() {
    let mut all = Vec::new();
    let s = State { encoding: Encoding::Other(encoding_rs::SHIFT_JIS), ..state() };
    for menu in MENUS {
        ids(&items(menu, &s), &mut all);
    }
    for tab in [cmd::NEW_TAB, cmd::CLOSE_TAB] {
        all.retain(|id| *id != tab);
    }
    let mut sorted = all.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), all.len(), "{all:?}");
    assert!(!all.contains(&cmd::NEXT_TAB) && !all.contains(&cmd::PREV_TAB));
}

fn access_keys(items: &[Item]) -> Vec<char> {
    items
        .iter()
        .filter_map(|i| match i {
            Item::Command { label, .. } | Item::Sub { label, .. } => {
                let at = label.find('&')?;
                label[at + 1..].chars().next().map(|c| c.to_ascii_lowercase())
            }
            Item::Separator => None,
        })
        .collect()
}

#[test]
fn every_item_has_an_access_key_unique_in_its_popup() {
    let s = state();
    let file = items(FILE, &s);
    let mut popups = vec![file.clone(), items(EDIT, &s), items(VIEW, &s)];
    popups.push(sub(&file, "Encoding").1);
    popups.push(sub(&file, "Line endings").1);
    for popup in popups {
        let keys = access_keys(&popup);
        let named = popup.iter().filter(|i| !matches!(i, Item::Separator)).count();
        assert_eq!(keys.len(), named, "{:?}", labels(&popup));
        let mut unique = keys.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), keys.len(), "{:?}", labels(&popup));
    }
}

// --- The menu bar from the keyboard ------------------------------------

#[test]
fn alt_f_e_and_v_name_the_three_menus() {
    assert_eq!(access_key('f'), Some(0));
    assert_eq!(access_key('E'), Some(1));
    assert_eq!(access_key('v'), Some(2));
    assert_eq!(access_key('x'), None);
    assert_eq!(access_key(' '), None);
}

#[test]
fn arrows_move_across_the_bar_and_wrap() {
    assert_eq!(on_bar(0, Key::Right), Bar::Focus(1));
    assert_eq!(on_bar(2, Key::Right), Bar::Focus(0));
    assert_eq!(on_bar(0, Key::Left), Bar::Focus(2));
    assert_eq!(on_bar(1, Key::Left), Bar::Focus(0));
}

#[test]
fn enter_and_up_or_down_open_the_highlighted_menu() {
    for key in [Key::Enter, Key::Down, Key::Up] {
        assert_eq!(on_bar(1, key), Bar::Open(1));
    }
}

#[test]
fn a_menus_letter_opens_it() {
    assert_eq!(on_bar(0, Key::Letter('v')), Bar::Open(2));
    assert_eq!(on_bar(2, Key::Letter('F')), Bar::Open(0));
}

#[test]
fn esc_alt_and_f10_leave_the_bar() {
    for key in [Key::Esc, Key::Alt, Key::F10] {
        assert_eq!(on_bar(1, key), Bar::Leave);
    }
}

#[test]
fn other_keys_keep_the_bar_highlighted() {
    assert_eq!(on_bar(1, Key::Letter('q')), Bar::Focus(1));
    assert_eq!(on_bar(1, Key::Other), Bar::Focus(1));
}

#[test]
fn in_an_open_menu_left_and_right_move_to_the_next_menu() {
    let top = Popup { in_submenu: false, on_submenu_item: false };
    assert_eq!(in_popup(0, Key::Right, top), Some(1));
    assert_eq!(in_popup(0, Key::Left, top), Some(2));
    assert_eq!(in_popup(1, Key::Down, top), None);
}

#[test]
fn right_on_a_submenu_item_opens_the_submenu_instead() {
    let on_sub = Popup { in_submenu: false, on_submenu_item: true };
    assert_eq!(in_popup(0, Key::Right, on_sub), None);
    assert_eq!(in_popup(0, Key::Left, on_sub), Some(2));
}

#[test]
fn left_in_a_submenu_closes_it_and_right_moves_on() {
    let inside = Popup { in_submenu: true, on_submenu_item: false };
    assert_eq!(in_popup(0, Key::Left, inside), None);
    assert_eq!(in_popup(0, Key::Right, inside), Some(1));
}
