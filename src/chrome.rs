//! Title row, Notices, find bar and status bar: pure layout and hit testing, plus drawing (spec §4).

mod paint;

pub use paint::{Painter, TabView, View};

/// A rectangle in client pixels; `right` and `bottom` are exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// What the layout is computed from.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// Client size in pixels.
    pub width: i32,
    pub height: i32,
    /// DPI / 96.
    pub scale: f32,
    /// Height of the Notice band in DIPs; 0 when the Tab shows none (10, 11 fill it).
    pub notice_h: f32,
    /// Height of the find bar in DIPs; 0 when hidden (12 fills it).
    pub find_h: f32,
    /// Number of Tabs in the strip.
    pub tabs: usize,
    /// Text widths of "File", "Edit", "View" in DIPs, measured by the painter.
    pub menu_text: [f32; 3],
    pub maximized: bool,
    /// Thickness of the native resize border in pixels. Left, right and
    /// bottom lie outside the client; the top one is the top of the title row.
    pub resize_border: i32,
}

impl Default for Frame {
    fn default() -> Self {
        Frame {
            width: 0,
            height: 0,
            scale: 1.0,
            notice_h: 0.0,
            find_h: 0.0,
            tabs: 1,
            menu_text: [22.0, 22.0, 26.0],
            maximized: false,
            resize_border: 0,
        }
    }
}

/// The Window's bands, top to bottom, and the pieces of the title row, in
/// client pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub title: Rect,
    pub notice: Rect,
    pub find: Rect,
    pub editor: Rect,
    pub status: Rect,

    pub icon: Rect,
    /// File, Edit, View.
    pub menus: [Rect; 3],
    /// The hairline after the menus.
    pub divider: Rect,
    /// Where Tabs are drawn; Tabs past its right edge are clipped.
    pub strip: Rect,
    pub tabs: Vec<Rect>,
    /// The × (or unsaved dot) of each Tab.
    pub tab_closes: Vec<Rect>,
    /// "+": a new Tab.
    pub plus: Rect,
    /// Empty caption: drags the Window.
    pub drag: Rect,
    /// Minimise, maximise, close.
    pub buttons: [Rect; 3],

    pub maximized: bool,
    resize_border: i32,
    /// How far a corner reaches along each edge.
    corner: i32,
    cell_pad: i32,
}

/// What a client point (possibly outside the client, over the native
/// border) is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Resize(Edge),
    /// Empty title row: drag, double-click maximises.
    Caption,
    Icon,
    /// File, Edit, View.
    Menu(usize),
    Tab(usize),
    TabClose(usize),
    NewTab,
    Minimize,
    Maximize,
    Close,
    Notice,
    Find,
    Editor,
    Status,
    Nowhere,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Hit {
    /// The caption button behind a non-client hit-test code, if any.
    pub fn caption_button(code: u32) -> Option<Hit> {
        [Hit::Minimize, Hit::Maximize, Hit::Close].into_iter().find(|h| h.nc_code() == code)
    }

    /// Whether hovering this piece highlights it: only what can be clicked.
    pub fn lights_up(self) -> bool {
        matches!(
            self,
            Hit::Menu(_) | Hit::Tab(_) | Hit::TabClose(_) | Hit::NewTab | Hit::Minimize | Hit::Maximize | Hit::Close
        )
    }

    /// The `WM_NCHITTEST` answer. Everything we handle ourselves is HTCLIENT.
    /// The icon drags like the caption: no system menu on click, and a
    /// double-click maximises rather than closing.
    pub fn nc_code(self) -> u32 {
        use windows::Win32::UI::WindowsAndMessaging as wm;
        match self {
            Hit::Resize(e) => match e {
                Edge::Left => wm::HTLEFT,
                Edge::Right => wm::HTRIGHT,
                Edge::Top => wm::HTTOP,
                Edge::Bottom => wm::HTBOTTOM,
                Edge::TopLeft => wm::HTTOPLEFT,
                Edge::TopRight => wm::HTTOPRIGHT,
                Edge::BottomLeft => wm::HTBOTTOMLEFT,
                Edge::BottomRight => wm::HTBOTTOMRIGHT,
            },
            Hit::Caption | Hit::Icon => wm::HTCAPTION,
            Hit::Minimize => wm::HTMINBUTTON,
            Hit::Maximize => wm::HTMAXBUTTON,
            Hit::Close => wm::HTCLOSE,
            Hit::Nowhere => wm::HTNOWHERE,
            _ => wm::HTCLIENT,
        }
    }
}

const CORNER: f32 = 16.0;
const CELL_PAD: f32 = 14.0;

/// What the status bar shows: a note on the left, cells on the right.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// "Read-only", "Markdown styling off (over 1 MB)", ...; empty for none.
    pub note: String,
    pub cells: Vec<String>,
}

impl Status {
    /// The Source view's cells: `Ln x, Col y` | zoom (when not 100 %) | EOL | encoding.
    pub fn source(line: usize, col: usize, eol: &str, encoding: &str, zoom_percent: i32) -> Status {
        let mut cells = vec![format!("Ln {line}, Col {col}")];
        if zoom_percent != 100 {
            cells.push(format!("{zoom_percent}%"));
        }
        cells.push(eol.to_string());
        cells.push(encoding.to_string());
        Status { note: String::new(), cells }
    }
}

// Sizes in DIPs, from variant E of the window-look prototype.
pub const TITLE_H: f32 = 42.0;
pub const STATUS_H: f32 = 26.0;
const ICON_L: f32 = 14.0;
const ICON: f32 = 16.0;
const MENUS_L: f32 = 44.0;
const MENU_PAD: f32 = 9.0;
const MENU_H: f32 = 24.0;
const MENUS_PAD_R: f32 = 8.0;
/// Margin and flex gap between the divider and the first Tab.
const STRIP_GAP_L: f32 = 8.0;
const GAP: f32 = 2.0;
const TAB_W: f32 = 178.0;
const TAB_MIN_W: f32 = 120.0;
const TAB_H: f32 = 31.0;
const TAB_PAD_R: f32 = 10.0;
const TAB_CLOSE: f32 = 18.0;
const PLUS_W: f32 = 30.0;
pub const DRAG_MIN: f32 = 48.0;
const BUTTON_W: f32 = 46.0;

impl Frame {
    /// DIPs to pixels.
    pub fn px(&self, dip: f32) -> i32 {
        (dip * self.scale).round() as i32
    }
}

impl Layout {
    pub fn new(f: &Frame) -> Layout {
        let (w, h) = (f.width.max(0), f.height.max(0));
        let band = |top: i32, bottom: i32| Rect { left: 0, top, right: w, bottom };
        let title_b = f.px(TITLE_H).min(h);
        let status_t = (h - f.px(STATUS_H)).max(title_b);
        let notice_b = (title_b + f.px(f.notice_h)).min(status_t);
        let find_b = (notice_b + f.px(f.find_h)).min(status_t);
        let rect = |left: i32, top: i32, right: i32, bottom: i32| Rect {
            left,
            top,
            right: right.max(left),
            bottom: bottom.max(top),
        };
        // Centred vertically in the title row.
        let centred = |left: i32, right: i32, height: f32| {
            let top = (title_b - f.px(height)) / 2;
            rect(left, top, right, top + f.px(height))
        };

        // Right to left: the window buttons, then the left side.
        let bw = f.px(BUTTON_W);
        let close_l = (w - bw).max(0);
        let max_l = (close_l - bw).max(0);
        let min_l = (max_l - bw).max(0);
        let buttons = [rect(min_l, 0, max_l, title_b), rect(max_l, 0, close_l, title_b), rect(close_l, 0, w, title_b)];

        let icon_l = f.px(ICON_L);
        let icon = centred(icon_l, icon_l + f.px(ICON), ICON);
        let mut x = f.px(MENUS_L);
        let menus = f.menu_text.map(|t| {
            let m = centred(x, x + f.px(t + 2.0 * MENU_PAD), MENU_H);
            x = m.right;
            m
        });
        let div_l = x + f.px(MENUS_PAD_R);
        let divider = centred(div_l, div_l + f.px(1.0), ICON);
        let strip_l = divider.right + f.px(STRIP_GAP_L);

        // Tabs share what is left once "+" and the drag area fit.
        let (gap, plus_w) = (f.px(GAP), f.px(PLUS_W));
        let strip_max = (min_l - gap - f.px(DRAG_MIN) - gap - plus_w - gap).max(strip_l);
        let n = f.tabs as i32;
        let tab_w = if n == 0 {
            0
        } else {
            ((strip_max - strip_l - (n - 1) * gap) / n).clamp(f.px(TAB_MIN_W), f.px(TAB_W))
        };
        let tab_t = title_b - f.px(1.0) - f.px(TAB_H);
        let tabs: Vec<Rect> =
            (0..n).map(|i| rect(strip_l + i * (tab_w + gap), tab_t, strip_l + i * (tab_w + gap) + tab_w, title_b)).collect();
        let tab_closes = tabs
            .iter()
            .map(|t| {
                let right = t.right - f.px(TAB_PAD_R);
                let top = tab_t + (f.px(TAB_H) - f.px(TAB_CLOSE)) / 2;
                rect(right - f.px(TAB_CLOSE), top, right, top + f.px(TAB_CLOSE))
            })
            .collect();
        let strip_r = tabs.last().map_or(strip_l, |t| t.right).min(strip_max);
        let plus_l = strip_r + gap;
        let plus = rect(plus_l, tab_t, plus_l + plus_w, title_b - f.px(1.0));

        Layout {
            title: band(0, title_b),
            notice: band(title_b, notice_b),
            find: band(notice_b, find_b),
            editor: band(find_b, status_t),
            status: band(status_t, h),
            icon,
            menus,
            divider,
            strip: rect(strip_l, 0, strip_r, title_b),
            tabs,
            tab_closes,
            plus,
            drag: rect(plus.right, 0, min_l, title_b),
            buttons,
            maximized: f.maximized,
            resize_border: f.resize_border,
            corner: f.px(CORNER).max(f.resize_border),
            cell_pad: f.px(CELL_PAD),
        }
    }

    /// Status bar cells for texts `text_widths` pixels wide, left to right,
    /// packed against the right edge.
    pub fn status_cells(&self, text_widths: &[i32]) -> Vec<Rect> {
        let s = self.status;
        let mut right = s.right;
        let mut cells: Vec<Rect> = text_widths
            .iter()
            .rev()
            .map(|w| {
                let left = (right - w - 2 * self.cell_pad).max(s.left);
                let r = Rect { left, top: s.top, right, bottom: s.bottom };
                right = left;
                r
            })
            .collect();
        cells.reverse();
        cells
    }

    /// The note's text box: from the left padding to the first cell.
    pub fn status_note(&self, cells: &[Rect]) -> Rect {
        let s = self.status;
        let right = cells.first().map_or(s.right - self.cell_pad, |c| c.left);
        let left = (s.left + self.cell_pad).min(right);
        Rect { left, top: s.top, right, bottom: s.bottom }
    }

    pub fn hit_test(&self, x: i32, y: i32) -> Hit {
        let (w, h) = (self.status.right, self.status.bottom);
        let inside = x >= 0 && x < w && y >= 0 && y < h;
        if self.maximized {
            if !inside {
                return Hit::Nowhere;
            }
        } else if let Some(e) = self.edge(x, y, w, h) {
            return Hit::Resize(e);
        } else if !inside {
            return Hit::Nowhere;
        }
        let at = |r: &Rect| x >= r.left && x < r.right && y >= r.top && y < r.bottom;
        if at(&self.title) {
            let pieces = [
                (self.buttons[0], Hit::Minimize),
                (self.buttons[1], Hit::Maximize),
                (self.buttons[2], Hit::Close),
                (self.icon, Hit::Icon),
                (self.plus, Hit::NewTab),
            ];
            if let Some((_, hit)) = pieces.iter().find(|(r, _)| at(r)) {
                return *hit;
            }
            if let Some(i) = self.menus.iter().position(at) {
                return Hit::Menu(i);
            }
            if at(&self.strip) {
                if let Some(i) = self.tab_closes.iter().position(at) {
                    return Hit::TabClose(i);
                }
                if let Some(i) = self.tabs.iter().position(at) {
                    return Hit::Tab(i);
                }
            }
            return Hit::Caption;
        }
        [(self.notice, Hit::Notice), (self.find, Hit::Find), (self.editor, Hit::Editor)]
            .into_iter()
            .find(|(r, _)| at(r))
            .map_or(Hit::Status, |(_, hit)| hit)
    }

    /// The resize edge under a point of a restored Window, if any.
    fn edge(&self, x: i32, y: i32, w: i32, h: i32) -> Option<Edge> {
        let c = self.corner;
        let (left, right) = (x < 0, x >= w);
        let (top, bottom) = (y < self.resize_border, y >= h);
        let (near_top, near_bottom) = (y < c, y >= h - c);
        let (near_left, near_right) = (x < c, x >= w - c);
        Some(match () {
            _ if (left && near_top) || (top && near_left) => Edge::TopLeft,
            _ if (right && near_top) || (top && near_right) => Edge::TopRight,
            _ if (left && near_bottom) || (bottom && near_left) => Edge::BottomLeft,
            _ if (right && near_bottom) || (bottom && near_right) => Edge::BottomRight,
            _ if left => Edge::Left,
            _ if right => Edge::Right,
            _ if top => Edge::Top,
            _ if bottom => Edge::Bottom,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: i32, height: i32, scale: f32) -> Frame {
        Frame { width, height, scale, ..Frame::default() }
    }

    fn r(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect { left, top, right, bottom }
    }

    #[test]
    fn bands_stack_title_editor_status() {
        let l = Layout::new(&frame(1000, 700, 1.0));
        assert_eq!(l.title, r(0, 0, 1000, 42));
        assert_eq!(l.notice, r(0, 42, 1000, 42));
        assert_eq!(l.find, r(0, 42, 1000, 42));
        assert_eq!(l.editor, r(0, 42, 1000, 674));
        assert_eq!(l.status, r(0, 674, 1000, 700));
    }

    fn row(tabs: usize, width: i32) -> Frame {
        Frame { tabs, menu_text: [22.0, 22.0, 26.0], ..frame(width, 700, 1.0) }
    }

    #[test]
    fn title_row_follows_variant_e() {
        let l = Layout::new(&row(1, 1000));
        assert_eq!(l.icon, r(14, 13, 30, 29));
        // Text width + 9 px padding each side, from x = 44.
        assert_eq!(l.menus, [r(44, 9, 84, 33), r(84, 9, 124, 33), r(124, 9, 168, 33)]);
        assert_eq!(l.divider, r(176, 13, 177, 29));
        // One tab at its full 178 px, bottom-aligned, 31 px tall, joined to the editor.
        assert_eq!(l.tabs, vec![r(185, 10, 363, 42)]);
        assert_eq!(l.tab_closes, vec![r(335, 16, 353, 34)]);
        assert_eq!(l.plus, r(365, 10, 395, 41));
        assert_eq!(l.drag, r(395, 0, 862, 42));
        assert_eq!(l.buttons, [r(862, 0, 908, 42), r(908, 0, 954, 42), r(954, 0, 1000, 42)]);
    }

    #[test]
    fn tabs_shrink_to_share_the_strip() {
        let l = Layout::new(&row(4, 1000));
        let lefts: Vec<i32> = l.tabs.iter().map(|t| t.left).collect();
        // (593 px strip - 3 gaps) / 4, rounded down.
        assert_eq!(lefts, [185, 333, 481, 629]);
        assert!(l.tabs.iter().all(|t| t.right - t.left == 146));
        assert_eq!(l.plus, r(777, 10, 807, 41));
        assert!(l.drag.right - l.drag.left >= 48);
    }

    #[test]
    fn tabs_stop_shrinking_at_120_px_and_the_strip_clips_them() {
        let l = Layout::new(&row(8, 1000));
        assert!(l.tabs.iter().all(|t| t.right - t.left == 120));
        // The strip ends where the plus and a 48 px drag area still fit.
        assert_eq!(l.strip, r(185, 0, 778, 42));
        assert_eq!(l.plus.left, 780);
        // 48 px plus the 2 px gaps either side.
        assert_eq!(l.drag, r(810, 0, 862, 42));
    }

    fn restored() -> Layout {
        Layout::new(&Frame { resize_border: 8, ..row(1, 1000) })
    }

    #[test]
    fn all_eight_resize_edges() {
        let l = restored();
        let edge = |x, y| l.hit_test(x, y);
        // Left, right and bottom lie in the native frame, outside the client.
        assert_eq!(edge(-3, 300), Hit::Resize(Edge::Left));
        assert_eq!(edge(1003, 300), Hit::Resize(Edge::Right));
        assert_eq!(edge(500, 703), Hit::Resize(Edge::Bottom));
        // The top border is ours: the first 8 px of the title row.
        assert_eq!(edge(500, 2), Hit::Resize(Edge::Top));
        assert_eq!(edge(-3, 2), Hit::Resize(Edge::TopLeft));
        assert_eq!(edge(5, 3), Hit::Resize(Edge::TopLeft));
        assert_eq!(edge(-3, 12), Hit::Resize(Edge::TopLeft));
        assert_eq!(edge(1003, 2), Hit::Resize(Edge::TopRight));
        assert_eq!(edge(995, 3), Hit::Resize(Edge::TopRight));
        assert_eq!(edge(-3, 698), Hit::Resize(Edge::BottomLeft));
        assert_eq!(edge(5, 703), Hit::Resize(Edge::BottomLeft));
        assert_eq!(edge(1003, 703), Hit::Resize(Edge::BottomRight));
        assert_eq!(edge(990, 703), Hit::Resize(Edge::BottomRight));
        // Resizing wins over the close button along the top edge.
        assert_eq!(edge(970, 3), Hit::Resize(Edge::Top));
    }

    #[test]
    fn title_row_pieces() {
        let l = restored();
        assert_eq!(l.hit_test(20, 20), Hit::Icon);
        assert_eq!(l.hit_test(60, 20), Hit::Menu(0));
        assert_eq!(l.hit_test(100, 20), Hit::Menu(1));
        assert_eq!(l.hit_test(130, 20), Hit::Menu(2));
        assert_eq!(l.hit_test(200, 25), Hit::Tab(0));
        assert_eq!(l.hit_test(340, 25), Hit::TabClose(0));
        assert_eq!(l.hit_test(380, 25), Hit::NewTab);
        assert_eq!(l.hit_test(880, 20), Hit::Minimize);
        assert_eq!(l.hit_test(930, 20), Hit::Maximize);
        assert_eq!(l.hit_test(980, 20), Hit::Close);
    }

    #[test]
    fn empty_title_row_drags() {
        let l = restored();
        assert_eq!(l.hit_test(600, 20), Hit::Caption);
        // Above a Tab, between the menus and the strip, and left of the icon.
        assert_eq!(l.hit_test(200, 9), Hit::Caption);
        assert_eq!(l.hit_test(180, 20), Hit::Caption);
        assert_eq!(l.hit_test(8, 20), Hit::Caption);
    }

    #[test]
    fn bands_below_the_title_row() {
        let f = Frame { resize_border: 8, notice_h: 38.0, find_h: 40.0, ..row(1, 1000) };
        let l = Layout::new(&f);
        assert_eq!(l.hit_test(500, 50), Hit::Notice);
        assert_eq!(l.hit_test(500, 100), Hit::Find);
        assert_eq!(l.hit_test(500, 300), Hit::Editor);
        assert_eq!(l.hit_test(500, 690), Hit::Status);
    }

    #[test]
    fn maximised_has_no_resize_edges_and_buttons_reach_the_screen_edge() {
        let l = Layout::new(&Frame { resize_border: 8, maximized: true, ..row(1, 1000) });
        assert_eq!(l.hit_test(999, 0), Hit::Close);
        assert_eq!(l.hit_test(500, 2), Hit::Caption);
        assert_eq!(l.hit_test(-3, 300), Hit::Nowhere);
        assert_eq!(l.hit_test(500, 703), Hit::Nowhere);
    }

    #[test]
    fn a_clipped_tab_is_not_hit_past_the_strip() {
        let l = Layout::new(&Frame { resize_border: 8, ..row(8, 1000) });
        assert_eq!(l.hit_test(740, 25), Hit::Tab(4));
        assert_eq!(l.hit_test(800, 25), Hit::NewTab);
        assert_eq!(l.hit_test(830, 25), Hit::Caption);
    }

    #[test]
    fn hits_map_to_native_hit_test_codes() {
        // Values from WinUser.h.
        assert_eq!(Hit::Nowhere.nc_code(), 0);
        assert_eq!(Hit::Editor.nc_code(), 1);
        assert_eq!(Hit::Menu(0).nc_code(), 1);
        assert_eq!(Hit::Tab(0).nc_code(), 1);
        assert_eq!(Hit::Caption.nc_code(), 2);
        assert_eq!(Hit::Icon.nc_code(), 2);
        assert_eq!(Hit::Minimize.nc_code(), 8);
        // Snap Layouts appears only over HTMAXBUTTON.
        assert_eq!(Hit::Maximize.nc_code(), 9);
        assert_eq!(Hit::Close.nc_code(), 20);
        let edges = [
            (Edge::Left, 10),
            (Edge::Right, 11),
            (Edge::Top, 12),
            (Edge::TopLeft, 13),
            (Edge::TopRight, 14),
            (Edge::Bottom, 15),
            (Edge::BottomLeft, 16),
            (Edge::BottomRight, 17),
        ];
        for (e, code) in edges {
            assert_eq!(Hit::Resize(e).nc_code(), code, "{e:?}");
        }
    }

    #[test]
    fn status_cells_read_position_eol_encoding() {
        let s = Status::source(10, 43, "LF", "UTF-8", 100);
        assert_eq!(s.note, "");
        assert_eq!(s.cells, ["Ln 10, Col 43", "LF", "UTF-8"]);
    }

    #[test]
    fn zoom_shows_only_when_not_100_percent() {
        assert_eq!(Status::source(1, 1, "CRLF", "UTF-16 LE", 120).cells, ["Ln 1, Col 1", "120%", "CRLF", "UTF-16 LE"]);
        assert_eq!(Status::source(1, 1, "LF", "UTF-8", 100).cells.len(), 3);
    }

    #[test]
    fn status_cells_are_right_aligned_with_14_px_padding() {
        let l = Layout::new(&frame(1000, 700, 1.0));
        let cells = l.status_cells(&[80, 14, 36]);
        assert_eq!(cells, [r(786, 674, 894, 700), r(894, 674, 936, 700), r(936, 674, 1000, 700)]);
        // The note takes what is left on the left.
        assert_eq!(l.status_note(&cells), r(14, 674, 786, 700));
        assert_eq!(l.status_note(&[]), r(14, 674, 986, 700));
    }

    #[test]
    fn caption_buttons_come_back_from_their_codes() {
        for hit in [Hit::Minimize, Hit::Maximize, Hit::Close] {
            assert_eq!(Hit::caption_button(hit.nc_code()), Some(hit));
        }
        assert_eq!(Hit::caption_button(2), None);
        assert_eq!(Hit::caption_button(1), None);
    }

    #[test]
    fn only_clickable_pieces_light_up_on_hover() {
        for hit in [Hit::Menu(1), Hit::Tab(0), Hit::TabClose(0), Hit::NewTab, Hit::Close] {
            assert!(hit.lights_up(), "{hit:?}");
        }
        for hit in [Hit::Caption, Hit::Icon, Hit::Editor, Hit::Status, Hit::Resize(Edge::Top), Hit::Nowhere] {
            assert!(!hit.lights_up(), "{hit:?}");
        }
    }

    #[test]
    fn notice_and_find_bar_push_the_editor_down() {
        let f = Frame { notice_h: 38.0, find_h: 76.0, ..frame(1000, 700, 1.0) };
        let l = Layout::new(&f);
        assert_eq!(l.notice, r(0, 42, 1000, 80));
        assert_eq!(l.find, r(0, 80, 1000, 156));
        assert_eq!(l.editor, r(0, 156, 1000, 674));
    }

    #[test]
    fn bands_scale_with_dpi() {
        let l = Layout::new(&frame(1500, 1050, 1.5));
        assert_eq!(l.title, r(0, 0, 1500, 63));
        assert_eq!(l.status, r(0, 1011, 1500, 1050));
        // 125 %: 52.5 rounds away from zero.
        assert_eq!(Layout::new(&frame(1250, 875, 1.25)).title.bottom, 53);
    }

    #[test]
    fn a_tiny_window_never_inverts_a_band() {
        let l = Layout::new(&Frame { notice_h: 38.0, ..frame(10, 50, 1.0) });
        for b in [l.title, l.notice, l.find, l.editor, l.status] {
            assert!(b.top <= b.bottom && b.left <= b.right, "{b:?}");
        }
        assert_eq!(l.status.bottom, 50);
    }
}
