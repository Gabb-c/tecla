use std::ops::Range;
use std::time::{Duration, Instant};

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Position, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::engine::{GraphemeState, Session};

const MAX_WIDTH: u16 = 80;
const VISIBLE_LINES: u16 = 3;

/// The typing screen: timer, wrapped target text and key hints.
pub struct TypingView<'a> {
    pub session: &'a Session,
    pub now: Instant,
}

impl Widget for TypingView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [timer, text, hints] = layout(area);
        Line::from(seconds(self.session.remaining(self.now)).to_string())
            .yellow()
            .render(timer, buf);
        Paragraph::new(self.visible_lines(text.width)).render(text, buf);
        Line::from("tab restart · esc quit")
            .dark_gray()
            .centered()
            .render(hints, buf);
    }
}

impl TypingView<'_> {
    /// Where the terminal cursor should sit when rendered into `area`, if the caret is visible.
    pub fn caret_position(&self, area: Rect) -> Option<Position> {
        let [_, text, _] = layout(area);
        let target = self.target();
        let cursor = self.session.cursor();
        visible_ranges(&target, cursor, text.width)
            .into_iter()
            .zip(text.y..)
            .find(|(line, _)| line.contains(&cursor))
            .and_then(|(line, y)| {
                let offset: usize = target[line.start..cursor]
                    .iter()
                    .map(|g| Span::raw(*g).width())
                    .sum();
                let offset = u16::try_from(offset).ok().filter(|&x| x < text.width)?;
                Some(Position::new(text.x + offset, y))
            })
    }

    fn target(&self) -> Vec<&str> {
        self.session.target().iter().map(String::as_str).collect()
    }

    fn visible_lines(&self, width: u16) -> Vec<Line<'_>> {
        let target = self.target();
        visible_ranges(&target, self.session.cursor(), width)
            .into_iter()
            .map(|line| {
                line.map(|i| Span::styled(target[i], self.style_at(i, target[i])))
                    .collect()
            })
            .collect()
    }

    fn style_at(&self, index: usize, grapheme: &str) -> Style {
        match self.session.state_at(index) {
            GraphemeState::Untyped => Style::new().dark_gray(),
            GraphemeState::Correct => Style::new(),
            GraphemeState::Incorrect if grapheme == " " => Style::new().on_red(),
            GraphemeState::Incorrect => Style::new().red(),
        }
    }
}

/// Splits the typing screen into timer, text and hints rows.
fn layout(area: Rect) -> [Rect; 3] {
    let width = area.width.saturating_sub(4).min(MAX_WIDTH);
    let [column] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [timer, _, text, _, hints] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(VISIBLE_LINES),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .flex(Flex::Center)
    .areas(column);
    [timer, text, hints]
}

/// The wrapped lines that fit on screen while keeping `cursor` visible.
fn visible_ranges(target: &[&str], cursor: usize, width: u16) -> Vec<Range<usize>> {
    let lines = wrap(target, usize::from(width));
    let cursor_line = lines
        .iter()
        .position(|line| line.contains(&cursor))
        .unwrap_or(lines.len().saturating_sub(1));
    // keep one line of already-typed context above the cursor
    let first = cursor_line.saturating_sub(1);
    lines
        .into_iter()
        .skip(first)
        .take(usize::from(VISIBLE_LINES))
        .collect()
}

fn seconds(remaining: Duration) -> u128 {
    remaining.as_millis().div_ceil(1000)
}

/// Splits graphemes into line ranges no wider than `width`, breaking after spaces.
/// A single word wider than `width` gets its own line and is clipped.
fn wrap(graphemes: &[&str], width: usize) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut line_start = 0;
    let mut line_width = 0;
    let mut i = 0;
    while i < graphemes.len() {
        let word_end = graphemes[i..]
            .iter()
            .position(|g| *g == " ")
            .map_or(graphemes.len(), |p| i + p + 1);
        let word = &graphemes[i..word_end];
        let word_width: usize = word.iter().map(|g| Span::raw(*g).width()).sum();
        // a trailing space may hang past the edge
        let visible_width = word_width - usize::from(word.last() == Some(&" "));
        if line_width > 0 && line_width + visible_width > width {
            lines.push(line_start..i);
            line_start = i;
            line_width = 0;
        }
        line_width += word_width;
        i = word_end;
    }
    if line_start < graphemes.len() {
        lines.push(line_start..graphemes.len());
    }
    lines
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::engine::Input;

    fn rendered(session: &Session, now: Instant, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| frame.render_widget(TypingView { session, now }, frame.area()))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn shows_target_text_and_timer() {
        let now = Instant::now();
        let session = Session::new("hello world", Duration::from_secs(30));
        let screen = rendered(&session, now, 40, 10).join("\n");

        assert!(screen.contains("hello world"), "{screen}");
        assert!(screen.contains("30"), "{screen}");
    }

    #[test]
    fn timer_counts_down_after_first_key() {
        let start = Instant::now();
        let mut session = Session::new("hello world", Duration::from_secs(30));
        session.handle(Input::Char('h'), start);
        let screen = rendered(&session, start + Duration::from_millis(5500), 40, 10).join("\n");

        assert!(screen.contains("25"), "{screen}");
    }

    #[test]
    fn scrolls_to_keep_cursor_visible() {
        let now = Instant::now();
        let text = "aaaa bbbb cccc dddd eeee ffff";
        let mut session = Session::new(text, Duration::from_secs(30));
        for c in "aaaa bbbb cccc dddd ".chars() {
            session.handle(Input::Char(c), now);
        }
        let screen = rendered(&session, now, 14, 10).join("\n");

        assert!(!screen.contains("aaaa"), "{screen}");
        assert!(screen.contains("eeee"), "{screen}");
    }

    #[test]
    fn caret_sits_on_next_grapheme() {
        let now = Instant::now();
        let mut session = Session::new("hello world", Duration::from_secs(30));
        for c in "hello ".chars() {
            session.handle(Input::Char(c), now);
        }
        let screen = rendered(&session, now, 40, 10);
        let row = screen
            .iter()
            .position(|l| l.contains("hello world"))
            .expect("text row");
        let column = screen[row].find('w').expect("next grapheme");

        let caret = TypingView {
            session: &session,
            now,
        }
        .caret_position(Rect::new(0, 0, 40, 10))
        .expect("caret visible");

        assert_eq!(
            caret,
            Position::new(u16::try_from(column).unwrap(), u16::try_from(row).unwrap())
        );
    }

    #[test]
    fn caret_follows_wrapped_line() {
        let now = Instant::now();
        let mut session = Session::new("aaaa bbbb cccc", Duration::from_secs(30));
        for c in "aaaa b".chars() {
            session.handle(Input::Char(c), now);
        }
        let area = Rect::new(0, 0, 9, 10);
        let [_, text, _] = layout(area);

        let caret = TypingView {
            session: &session,
            now,
        }
        .caret_position(area)
        .expect("caret visible");

        assert_eq!(caret, Position::new(text.x + 1, text.y + 1));
    }

    #[test]
    fn wraps_after_spaces() {
        let text: Vec<&str> = "ab cd ef".split("").filter(|g| !g.is_empty()).collect();
        assert_eq!(wrap(&text, 5), [0..6, 6..8]);
        assert_eq!(wrap(&text, 2), [0..3, 3..6, 6..8]);
        assert_eq!(wrap(&text, 80).first(), Some(&(0..8)));
        assert_eq!(wrap(&text, 80).len(), 1);
        assert_eq!(wrap(&[], 10), Vec::<Range<usize>>::new());
    }

    #[test]
    fn overlong_word_gets_own_line() {
        let text: Vec<&str> = "a bcdefg h".split("").filter(|g| !g.is_empty()).collect();
        assert_eq!(wrap(&text, 3), [0..2, 2..9, 9..10]);
    }
}
