use super::*;

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn sample() -> Prefs {
    Prefs {
        wrap: false,
        zoom: 3,
        window: Some(Placement { x: -1200, y: 40, w: 1100, h: 760 }),
        maximized: true,
        recent: vec![p(r"C:\b.md"), p(r"C:\a b\a.txt")],
    }
}

#[test]
fn defaults_are_wrap_on_zoom_zero_and_nothing_else() {
    let d = Prefs::default();
    assert!(d.wrap && d.zoom == 0 && d.window.is_none() && !d.maximized && d.recent.is_empty());
}

#[test]
fn serialise_then_parse_gives_the_same_prefs() {
    assert_eq!(parse(&serialise(&sample())), sample());
    assert_eq!(parse(&serialise(&Prefs::default())), Prefs::default());
}

#[test]
fn an_empty_or_garbage_file_means_defaults() {
    assert_eq!(parse(""), Prefs::default());
    assert_eq!(parse("\0\u{1}\u{2}garbage without equals\n[[[\n===\n"), Prefs::default());
    assert_eq!(parse("{\"json\": true}"), Prefs::default());
}

#[test]
fn a_bad_value_spoils_only_itself() {
    let text = "[view]\nwrap=maybe\nzoom=lots\n[window]\nx=1\ny=2\nw=wide\nh=500\nmaximized=1\n[recent]\n1=C:\\a.md\n";
    let got = parse(text);
    assert!(got.wrap, "an unreadable wrap keeps the default");
    assert_eq!(got.zoom, 0);
    assert_eq!(got.window, None, "a half-readable rectangle is dropped whole");
    assert!(got.maximized);
    assert_eq!(got.recent, vec![p(r"C:\a.md")]);
}

#[test]
fn zoom_is_clamped_and_a_tiny_window_is_dropped() {
    assert_eq!(parse("[view]\nzoom=999\n").zoom, ZOOM_MAX);
    assert_eq!(parse("[view]\nzoom=-999\n").zoom, ZOOM_MIN);
    assert_eq!(parse("[window]\nx=0\ny=0\nw=3\nh=3\n").window, None);
}

#[test]
fn unknown_keys_comments_a_bom_and_case_are_tolerated() {
    let got = parse("\u{feff}; hi\n# there\n[VIEW]\nWrap = 0\nfuture=1\n[Other]\nwrap=1\n");
    assert!(!got.wrap);
}

#[test]
fn recent_is_read_in_numeric_order_without_duplicates_and_capped() {
    let mut text = String::from("[recent]\n2=C:\\two.md\n1=C:\\one.md\n3=c:\\ONE.md\n");
    for i in 4..30 {
        text += &format!("{i}=C:\\f{i}.md\n");
    }
    let got = parse(&text).recent;
    assert_eq!(got.len(), MAX_RECENT);
    assert_eq!(got[..2], [p(r"C:\one.md"), p(r"C:\two.md")]);
    assert_eq!(got.iter().filter(|x| x.to_string_lossy().eq_ignore_ascii_case(r"C:\one.md")).count(), 1);
}

#[test]
fn a_path_with_spaces_and_equals_survives() {
    let prefs = Prefs { recent: vec![p(r"C:\my notes\a=b & c.md")], ..Prefs::default() };
    assert_eq!(parse(&serialise(&prefs)).recent, prefs.recent);
}

#[test]
fn opening_a_file_puts_it_on_top() {
    let mut list = vec![p(r"C:\a.md"), p(r"C:\b.md")];
    push_recent(&mut list, Path::new(r"C:\c.md"));
    assert_eq!(list, [p(r"C:\c.md"), p(r"C:\a.md"), p(r"C:\b.md")]);
}

#[test]
fn a_file_already_listed_moves_up_whatever_its_case() {
    let mut list = vec![p(r"C:\a.md"), p(r"C:\b.md")];
    push_recent(&mut list, Path::new(r"c:\B.MD"));
    assert_eq!(list, [p(r"c:\B.MD"), p(r"C:\a.md")]);
}

#[test]
fn the_list_keeps_the_last_ten() {
    let mut list = Vec::new();
    for i in 0..15 {
        push_recent(&mut list, Path::new(&format!(r"C:\f{i}.md")));
    }
    assert_eq!(list.len(), MAX_RECENT);
    assert_eq!(list[0], p(r"C:\f14.md"));
    assert_eq!(list[9], p(r"C:\f5.md"));
}

#[test]
fn removing_drops_the_file_and_ignores_strangers() {
    let mut list = vec![p(r"C:\a.md"), p(r"C:\b.md")];
    remove_recent(&mut list, Path::new(r"C:\A.md"));
    assert_eq!(list, [p(r"C:\b.md")]);
    remove_recent(&mut list, Path::new(r"C:\zzz.md"));
    assert_eq!(list.len(), 1);
}

#[test]
fn labels_number_from_one_with_the_tenth_as_zero_and_double_the_ampersand() {
    assert_eq!(menu_label(0, Path::new(r"C:\a.md")), r"&1  C:\a.md");
    assert_eq!(menu_label(9, Path::new(r"C:\a.md")), r"1&0  C:\a.md");
    assert_eq!(menu_label(1, Path::new(r"C:\R&D.md")), r"&2  C:\R&&D.md");
}

#[test]
fn a_long_path_is_shortened_in_the_middle_keeping_the_file_name() {
    let long = format!(r"C:\{}\notes.md", "deep\\".repeat(30));
    let label = menu_label(0, Path::new(&long));
    assert!(label.chars().count() < 80, "{label}");
    assert!(label.contains('\u{2026}') && label.ends_with(r"notes.md"));
}

fn wa(x: i32, y: i32, w: i32, h: i32) -> Placement {
    Placement { x, y, w, h }
}

#[test]
fn a_visible_rectangle_is_kept() {
    let want = wa(100, 100, 800, 600);
    assert_eq!(clamp_to_work_areas(want, &[wa(0, 0, 1920, 1040)]), Some(want));
}

#[test]
fn a_rectangle_hanging_off_an_edge_is_moved_in() {
    let got = clamp_to_work_areas(wa(1500, 900, 800, 600), &[wa(0, 0, 1920, 1040)]);
    assert_eq!(got, Some(wa(1120, 440, 800, 600)));
}

#[test]
fn a_rectangle_bigger_than_the_work_area_is_shrunk() {
    let got = clamp_to_work_areas(wa(-50, -50, 3000, 2000), &[wa(0, 0, 1920, 1040)]);
    assert_eq!(got, Some(wa(0, 0, 1920, 1040)));
}

#[test]
fn a_rectangle_on_a_monitor_that_is_gone_is_placed_by_default() {
    assert_eq!(clamp_to_work_areas(wa(2000, 0, 800, 600), &[wa(0, 0, 1920, 1040)]), None);
    assert_eq!(clamp_to_work_areas(wa(0, 0, 800, 600), &[]), None);
}

#[test]
fn the_monitor_with_most_of_the_window_wins() {
    let areas = [wa(0, 0, 1920, 1040), wa(1920, 0, 1920, 1040)];
    let got = clamp_to_work_areas(wa(1800, 100, 1000, 600), &areas);
    assert_eq!(got, Some(wa(1920, 100, 1000, 600)));
    // A monitor left of the primary has negative coordinates.
    let left = [wa(-1920, 0, 1920, 1040), wa(0, 0, 1920, 1040)];
    assert_eq!(clamp_to_work_areas(wa(-1500, 50, 800, 600), &left), Some(wa(-1500, 50, 800, 600)));
}

fn os(s: &str) -> Option<std::ffi::OsString> {
    Some(s.into())
}

#[test]
fn the_prefs_folder_is_under_local_app_data_in_release() {
    let got = dir_for(false, os(r"D:\tmp"), true, os(r"C:\Users\m\AppData\Local"));
    assert_eq!(got, Some(p(r"C:\Users\m\AppData\Local").join("scrivi")), "release ignores both test variables");
}

#[test]
fn a_debug_override_wins_and_a_test_instance_alone_turns_prefs_off() {
    assert_eq!(dir_for(true, os(r"D:\tmp"), true, os(r"C:\L")), Some(p(r"D:\tmp")));
    assert_eq!(dir_for(true, None, true, os(r"C:\L")), None);
    assert_eq!(dir_for(true, None, false, os(r"C:\L")), Some(p(r"C:\L").join("scrivi")));
    assert_eq!(dir_for(true, None, false, None), None);
}

#[test]
fn save_then_load_round_trips_through_a_file_in_a_temp_dir() {
    let tmp = std::env::temp_dir().join(format!("scrivi-prefs-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    // Write and read the file by hand: the env-based `dir()` is shared by the whole test process.
    std::fs::write(tmp.join(FILE), serialise(&sample())).unwrap();
    let text = std::fs::read_to_string(tmp.join(FILE)).unwrap();
    assert_eq!(parse(&text), sample());
    let _ = std::fs::remove_dir_all(&tmp);
}
