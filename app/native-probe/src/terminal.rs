//! VT parsing and grid ownership; independent of a window or Web bridge.
use alacritty_terminal::{
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point, Side},
    selection::{Selection, SelectionType},
    term::{cell::Flags, Config, TermMode},
    vte::ansi::{ClearMode, Color, Handler, NamedColor, Processor},
    Term,
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<u8>>>);
impl EventListener for Events {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = event {
            self.0.lock().unwrap().extend_from_slice(text.as_bytes());
        }
    }
}
struct Size(usize, usize);
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.1
    }
    fn screen_lines(&self) -> usize {
        self.1
    }
    fn columns(&self) -> usize {
        self.0
    }
}

pub struct Cell {
    pub column: usize,
    pub row: usize,
    pub text: String,
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub wide: bool,
    pub bold: bool,
    pub underline: bool,
}
pub struct Terminal {
    term: Term<Events>,
    parser: Processor,
    events: Events,
}
impl Terminal {
    pub fn new(columns: usize, rows: usize) -> Self {
        assert!((2..=1000).contains(&columns) && (1..=500).contains(&rows));
        let events = Events::default();
        let config = Config {
            scrolling_history: 1000,
            ..Default::default()
        };
        Self {
            term: Term::new(config, &Size(columns, rows), events.clone()),
            parser: Processor::new(),
            events,
        }
    }
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }
    pub fn resize(&mut self, columns: usize, rows: usize) {
        if (2..=1000).contains(&columns) && (1..=500).contains(&rows) {
            self.term.resize(Size(columns, rows));
        }
    }
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }
    pub fn clear(&mut self) {
        self.term.clear_screen(ClearMode::All);
        self.term.clear_screen(ClearMode::Saved);
        self.term.selection = None;
    }
    pub fn cursor_visible(&self) -> bool {
        self.term.mode().contains(TermMode::SHOW_CURSOR)
    }
    pub fn select(&mut self, start: (usize, usize), end: (usize, usize)) {
        let offset = self.term.grid().display_offset() as i32;
        let point = |(column, row): (usize, usize)| {
            Point::new(
                Line(row.min(self.term.screen_lines() - 1) as i32 - offset),
                Column(column.min(self.term.columns() - 1)),
            )
        };
        let mut selection = Selection::new(SelectionType::Simple, point(start), Side::Left);
        selection.update(point(end), Side::Right);
        self.term.selection = Some(selection);
    }
    pub fn selected_text(&self) -> Option<String> {
        self.term.selection_to_string()
    }
    pub fn responses(&self) -> Vec<u8> {
        std::mem::take(&mut *self.events.0.lock().unwrap())
    }
    pub fn paste(&self, text: &str) -> Vec<u8> {
        if self.term.mode().contains(TermMode::BRACKETED_PASTE) {
            format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")).into_bytes()
        } else {
            text.replace("\r\n", "\n").replace('\n', "\r").into_bytes()
        }
    }
    pub fn command(&self, text: &str) -> Vec<u8> {
        let mut bytes = self.paste(&text.replace("\r\n", "\n").replace('\n', "\r"));
        bytes.push(13);
        bytes
    }
    pub fn arrow(&self, code: char) -> Vec<u8> {
        format!(
            "\x1b{}{code}",
            if self.term.mode().contains(TermMode::APP_CURSOR) {
                'O'
            } else {
                '['
            }
        )
        .into_bytes()
    }
    pub fn text(&self) -> String {
        let cells = self.cells();
        let mut lines = vec![String::new(); self.term.screen_lines()];
        for cell in cells {
            lines[cell.row].push_str(&cell.text);
        }
        lines
            .into_iter()
            .map(|line| line.trim_end().to_owned())
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn cells(&self) -> Vec<Cell> {
        let content = self.term.renderable_content();
        content
            .display_iter
            .filter_map(|indexed| {
                let c = indexed.cell;
                if c.flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    return None;
                }
                let row = (indexed.point.line.0 + content.display_offset as i32) as usize;
                let mut foreground = color(c.fg, true);
                let mut background = color(c.bg, false);
                if c.flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut foreground, &mut background);
                }
                if content
                    .selection
                    .is_some_and(|range| range.contains(indexed.point))
                {
                    background = [48, 80, 123];
                }
                let mut text = c.c.to_string();
                if let Some(extra) = c.zerowidth() {
                    text.extend(extra);
                }
                Some(Cell {
                    column: indexed.point.column.0,
                    row,
                    text,
                    foreground,
                    background,
                    wide: c.flags.contains(Flags::WIDE_CHAR),
                    bold: c.flags.contains(Flags::BOLD),
                    underline: c
                        .flags
                        .intersects(Flags::UNDERLINE | Flags::DOUBLE_UNDERLINE),
                })
            })
            .collect()
    }
    pub fn cursor(&self) -> (usize, usize) {
        let content = self.term.renderable_content();
        (
            content.cursor.point.column.0,
            (content.cursor.point.line.0 + content.display_offset as i32).max(0) as usize,
        )
    }
}

fn color(value: Color, foreground: bool) -> [u8; 3] {
    const ANSI: [[u8; 3]; 16] = [
        [24, 28, 36],
        [224, 96, 96],
        [113, 190, 132],
        [222, 191, 112],
        [112, 165, 232],
        [185, 133, 219],
        [113, 193, 201],
        [216, 223, 235],
        [101, 113, 134],
        [246, 133, 133],
        [153, 220, 163],
        [244, 219, 154],
        [155, 192, 246],
        [219, 170, 244],
        [153, 224, 229],
        [248, 250, 254],
    ];
    match value {
        Color::Spec(rgb) => [rgb.r, rgb.g, rgb.b],
        Color::Named(NamedColor::Foreground) => ANSI[7],
        Color::Named(NamedColor::Background) => ANSI[0],
        Color::Named(named) if (named as usize) < 16 => ANSI[named as usize],
        Color::Indexed(n) if n < 16 => ANSI[n as usize],
        Color::Indexed(n) if n < 232 => {
            let n = n - 16;
            let level = |v| if v == 0 { 0 } else { 55 + 40 * v };
            [level(n / 36), level(n / 6 % 6), level(n % 6)]
        }
        Color::Indexed(n) => [8 + 10 * (n - 232); 3],
        _ => {
            if foreground {
                ANSI[7]
            } else {
                ANSI[0]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_utf8_wide_and_combining_characters() {
        let mut term = Terminal::new(20, 4);
        for byte in "中A e\u{301}🙂".as_bytes() {
            term.feed(&[*byte]);
        }
        let cells = term.cells();
        assert_eq!(cells[0].text, "中");
        assert!(cells[0].wide);
        assert_eq!(cells[1].column, 2);
        assert!(term.text().contains("中A e\u{301}🙂"));
    }
    #[test]
    fn ansi_colors_cursor_and_erasure() {
        let mut term = Terminal::new(20, 4);
        term.feed(b"old\rnew\x1b[K\x1b[2;3H\x1b[38;2;10;20;30mX");
        let x = term.cells().into_iter().find(|c| c.text == "X").unwrap();
        assert_eq!((x.column, x.row, x.foreground), (2, 1, [10, 20, 30]));
        assert!(term.text().starts_with("new"));
    }
    #[test]
    fn alternate_screen_restores_primary() {
        let mut term = Terminal::new(20, 4);
        term.feed(b"primary\x1b[?1049h\x1b[2J\x1b[Halternate");
        assert!(term.text().contains("alternate"));
        term.feed(b"\x1b[?1049l");
        assert!(term.text().contains("primary"));
        assert!(!term.text().contains("alternate"));
    }
    #[test]
    fn scrollback_is_bounded_and_wheel_does_not_write_to_remote() {
        let mut term = Terminal::new(30, 4);
        for n in 0..1500 {
            term.feed(format!("line-{n}\r\n").as_bytes());
        }
        assert_eq!(term.term.grid().history_size(), 1000);
        term.scroll(10);
        assert!(!term.text().contains("line-1499"));
        assert!(term.responses().is_empty());
    }
    #[test]
    fn resize_and_selection_preserve_unicode() {
        let mut term = Terminal::new(20, 4);
        term.feed("你好world".as_bytes());
        term.select((0, 0), (3, 0));
        assert_eq!(term.selected_text().as_deref(), Some("你好"));
        term.resize(40, 8);
        assert_eq!((term.term.columns(), term.term.screen_lines()), (40, 8));
    }
    #[test]
    fn input_modes_and_terminal_replies() {
        let mut term = Terminal::new(20, 4);
        assert_eq!(term.arrow('A'), b"\x1b[A");
        term.feed(b"\x1b[?1h\x1b[?2004h\x1b[6n");
        assert_eq!(term.arrow('A'), b"\x1bOA");
        assert_eq!(
            term.paste("中\x1b[201~"),
            "\x1b[200~中[201~\x1b[201~".as_bytes()
        );
        assert_eq!(term.responses(), b"\x1b[1;1R");
        assert_eq!(term.command("a\nb"), b"\x1b[200~a\rb\x1b[201~\r");
    }
    #[test]
    fn local_clear_removes_history_without_changing_remote_modes() {
        let mut term = Terminal::new(30, 4);
        for _ in 0..20 {
            term.feed(b"history\r\n");
        }
        term.feed(b"\x1b[?2004h\x1b[?25l");
        term.clear();
        assert!(term.text().trim().is_empty());
        assert_eq!(term.term.grid().history_size(), 0);
        assert!(!term.cursor_visible());
        assert_eq!(term.paste("x"), b"\x1b[200~x\x1b[201~");
        assert!(term.responses().is_empty());
    }
}
