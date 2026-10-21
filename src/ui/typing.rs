use std::borrow::Cow;
use std::ops::Range;
use std::time::{Duration, Instant};

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Position, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::config::{Config, Mode};
use crate::engine::{LetterState, Session};
use crate::settings::Lengths;
use crate::stats::Live;

const MAX_WIDTH: u16 = 80;
const VISIBLE_LINES: u16 = 3;

/// The typing screen: the next test or live stats, wrapped target text and any notice.
pub struct TypingView<'a> {
    pub session: &'a Session,
    pub config: &'a Config,
    pub lengths: &'a Lengths,
    pub now: Instant,
    pub notice: Option<&'a str>,
}

impl Widget for TypingView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_with_caret(area, buf);
    }
}

impl TypingView<'_> {
    /// Renders into `area` and returns where the terminal cursor belongs, if the caret is
    /// visible.
    pub fn render_with_caret(self, area: Rect, buf: &mut Buffer) -> Option<Position> {
        let [status, text, notice] = layout(area);
        self.status_line().render(status, buf);

        let (cells, caret) = self.cells();
        let graphemes = graphemes(&cells);
        let lines = visible_ranges(&graphemes, caret, text.width);
        let position = caret_position(&graphemes, &lines, caret, text);
        let lines: Vec<Line> = lines
            .into_iter()
            .map(|line| {
                cells[line]
                    .iter()
                    .map(|cell| Span::styled(cell.grapheme, cell.style))
                    .collect()
            })
            .collect();
        Paragraph::new(lines).render(text, buf);

        if let Some(text) = self.notice {
            Line::from(text).red().centered().render(notice, buf);
        }
        position
    }

    /// The next test until it starts, then progress and whichever live stats are enabled.
    fn status_line(&self) -> Line<'static> {
        let config = self.config;
        let session = self.session;
        if !session.is_started() {
            return next_test(config, self.lengths);
        }
        let mut spans = Vec::new();
        if config.live_progress {
            let progress = match session.remaining(self.now) {
                Some(remaining) => seconds(remaining).to_string(),
                None => format!("{}/{}", session.current_word(), session.words().len()),
            };
            spans.push(Span::from(progress).yellow());
        }
        if config.live_wpm || config.live_accuracy || config.live_errors {
            let live = Live::at(session, self.now);
            let mut stats = Vec::new();
            if config.live_wpm {
                stats.push(format!("{:.0} wpm", live.wpm));
            }
            if config.live_accuracy {
                stats.push(format!("{:.0}% acc", live.accuracy));
            }
            if config.live_errors {
                stats.push(format!("{} err", live.errors));
            }
            for stat in stats {
                if !spans.is_empty() {
                    spans.push(Span::from("  "));
                }
                spans.push(Span::from(stat).dark_gray());
            }
        }
        Line::from(spans)
    }

    /// Every word's target letters followed by its extra letters, separated by spaces, along
    /// with the index of the caret.
    fn cells(&self) -> (Vec<Cell<'_>>, usize) {
        let session = self.session;
        let blind = self.config.blind;
        let mut cells = Vec::new();
        let mut caret = 0;
        for (word, letters) in session.words().iter().enumerate() {
            if word > 0 {
                cells.push(Cell::new(" ", Style::new()));
            }
            if word == session.current_word() {
                caret = cells.len() + session.typed()[word].len();
            }
            for (letter, grapheme) in letters.iter().enumerate() {
                let style = letter_style(session.letter_state(word, letter), blind);
                cells.push(Cell::new(grapheme, style));
            }
            let extra = if blind {
                Style::new()
            } else {
                Style::new().red().dim()
            };
            for grapheme in session.extra(word) {
                cells.push(Cell::new(grapheme, extra));
            }
        }
        (cells, caret)
    }
}

/// The modes and the current mode's lengths, the selected ones highlighted.
fn next_test(config: &Config, lengths: &Lengths) -> Line<'static> {
    let modes = Mode::ALL
        .into_iter()
        .map(|mode| choice(mode.as_str(), mode == config.mode));
    let lengths = lengths
        .choices(config)
        .into_iter()
        .map(|length| choice(length.label, length.selected));
    let mut spans: Vec<Span> = Vec::new();
    for span in modes {
        spans.extend([span, Span::raw("  ")]);
    }
    spans.push("│".dark_gray());
    for span in lengths {
        spans.extend([Span::raw("  "), span]);
    }
    Line::from(spans)
}

fn choice(label: impl Into<Cow<'static, str>>, selected: bool) -> Span<'static> {
    if selected {
        Span::from(label).yellow().bold()
    } else {
        Span::from(label).dark_gray()
    }
}

/// One rendered grapheme of the typing text.
struct Cell<'a> {
    grapheme: &'a str,
    style: Style,
}

impl<'a> Cell<'a> {
    fn new(grapheme: &'a str, style: Style) -> Self {
        Self { grapheme, style }
    }
}

fn graphemes<'a>(cells: &[Cell<'a>]) -> Vec<&'a str> {
    cells.iter().map(|cell| cell.grapheme).collect()
}

/// In blind mode mistakes look like correct letters, and missed ones like untyped letters.
fn letter_style(state: LetterState, blind: bool) -> Style {
    match state {
        LetterState::Untyped => Style::new().dark_gray(),
        LetterState::Missed if blind => Style::new().dark_gray(),
        LetterState::Incorrect if blind => Style::new(),
        LetterState::Correct => Style::new(),
        LetterState::Incorrect => Style::new().red(),
        LetterState::Missed => Style::new().dark_gray().underlined(),
    }
}

/// Where the terminal cursor sits in `text` for the `caret` index, given the visible `lines`.
fn caret_position(
    graphemes: &[&str],
    lines: &[Range<usize>],
    caret: usize,
    text: Rect,
) -> Option<Position> {
    let last = lines.len().checked_sub(1)?;
    // past the final grapheme, the caret still belongs to the last line
    let (row, line) = lines
        .iter()
        .enumerate()
        .find(|&(row, line)| line.contains(&caret) || (row == last && line.end == caret))?;
    let offset: usize = graphemes[line.start..caret]
        .iter()
        .map(|g| Span::raw(*g).width())
        .sum();
    // on a space hanging past the edge, the caret sits where the next word starts
    let (row, offset) = if offset < usize::from(text.width) {
        (row, offset)
    } else {
        (row + 1, 0)
    };
    let row = u16::try_from(row).ok().filter(|&y| y < text.height)?;
    let offset = u16::try_from(offset).ok().filter(|&x| x < text.width)?;
    Some(Position::new(text.x + offset, text.y + row))
}

/// Splits the typing screen into status, text and notice rows.
fn layout(area: Rect) -> [Rect; 3] {
    let width = area.width.saturating_sub(4).min(MAX_WIDTH);
    let [column] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [status, _, text, _, notice] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(VISIBLE_LINES),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .flex(Flex::Center)
    .areas(column);
    [status, text, notice]
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
    use ratatui::{Terminal, backend::TestBackend, style::Modifier};

    use std::sync::LazyLock;

    use super::*;
    use crate::engine::{Input, Limit};

    static LENGTHS: LazyLock<Lengths> = LazyLock::new(|| Lengths::new(&Config::default()));

    const LIMIT: Limit = Limit::Time(Duration::from_secs(30));

    fn view<'a>(session: &'a Session, config: &'a Config, now: Instant) -> TypingView<'a> {
        TypingView {
            session,
            config,
            lengths: &LENGTHS,
            now,
            notice: None,
        }
    }

    fn draw(view: TypingView<'_>, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| frame.render_widget(view, frame.area()))
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    fn caret(view: TypingView<'_>, area: Rect) -> Option<Position> {
        view.render_with_caret(area, &mut Buffer::empty(area))
    }

    fn lines(buffer: &Buffer) -> Vec<String> {
        let area = buffer.area;
        (0..area.height)
            .map(|y| (0..area.width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    fn buffer(session: &Session, now: Instant, width: u16, height: u16) -> Buffer {
        draw(view(session, &Config::default(), now), width, height)
    }

    fn rendered(session: &Session, now: Instant, width: u16, height: u16) -> Vec<String> {
        lines(&buffer(session, now, width, height))
    }

    fn typed_session(target: &str, input: &str, now: Instant) -> Session {
        let mut session = Session::new(target.split_whitespace(), LIMIT);
        for c in input.chars() {
            session.handle(Input::Char(c), now);
        }
        session
    }

    #[test]
    fn shows_target_text_and_timer() {
        let now = Instant::now();
        let session = Session::new("hello world".split_whitespace(), LIMIT);
        let screen = rendered(&session, now, 40, 10).join("\n");

        assert!(screen.contains("hello world"), "{screen}");
        assert!(screen.contains("30"), "{screen}");
    }

    #[test]
    fn timer_counts_down_after_first_key() {
        let start = Instant::now();
        let mut session = Session::new("hello world".split_whitespace(), LIMIT);
        session.handle(Input::Char('h'), start);
        let screen = rendered(&session, start + Duration::from_millis(5500), 40, 10).join("\n");

        assert!(screen.contains("25"), "{screen}");
    }

    #[test]
    fn scrolls_to_keep_cursor_visible() {
        let now = Instant::now();
        let text = "aaaa bbbb cccc dddd eeee ffff";
        let mut session = Session::new(text.split_whitespace(), LIMIT);
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
        let mut session = Session::new("hello world".split_whitespace(), LIMIT);
        for c in "hello ".chars() {
            session.handle(Input::Char(c), now);
        }
        let screen = rendered(&session, now, 40, 10);
        let row = screen
            .iter()
            .position(|l| l.contains("hello world"))
            .expect("text row");
        let column = screen[row].find('w').expect("next grapheme");

        let caret = caret(
            view(&session, &Config::default(), now),
            Rect::new(0, 0, 40, 10),
        )
        .expect("caret visible");

        assert_eq!(
            caret,
            Position::new(u16::try_from(column).unwrap(), u16::try_from(row).unwrap())
        );
    }

    #[test]
    fn caret_follows_wrapped_line() {
        let now = Instant::now();
        let mut session = Session::new("aaaa bbbb cccc".split_whitespace(), LIMIT);
        for c in "aaaa b".chars() {
            session.handle(Input::Char(c), now);
        }
        let area = Rect::new(0, 0, 9, 10);
        let [_, text, _] = layout(area);

        let caret = caret(view(&session, &Config::default(), now), area).expect("caret visible");

        assert_eq!(caret, Position::new(text.x + 1, text.y + 1));
    }

    #[test]
    fn caret_shows_after_last_word_in_words_mode() {
        let now = Instant::now();
        let mut session = Session::new("ab cd".split_whitespace(), Limit::Words);
        for c in "ab cx".chars() {
            session.handle(Input::Char(c), now);
        }
        let area = Rect::new(0, 0, 40, 10);
        let [_, text, _] = layout(area);

        let caret = caret(view(&session, &Config::default(), now), area).expect("caret visible");

        assert_eq!(caret, Position::new(text.x + 5, text.y));
    }

    #[test]
    fn caret_wraps_when_word_fills_line() {
        let now = Instant::now();
        let session = typed_session("abcd efgh", "abcd", now);
        let area = Rect::new(0, 0, 8, 10);
        let [_, text, _] = layout(area);

        let caret = caret(view(&session, &Config::default(), now), area).expect("caret visible");

        assert_eq!(caret, Position::new(text.x, text.y + 1));
    }

    #[test]
    fn extra_letters_render_inline_and_move_caret() {
        let now = Instant::now();
        let session = typed_session("ab cd", "abx", now);
        let screen = rendered(&session, now, 40, 10);
        let row = screen
            .iter()
            .position(|l| l.contains("abx cd"))
            .expect("text row");
        let column = screen[row].find("abx").expect("word") + 3;

        let caret = caret(
            view(&session, &Config::default(), now),
            Rect::new(0, 0, 40, 10),
        )
        .expect("caret visible");

        assert_eq!(
            caret,
            Position::new(u16::try_from(column).unwrap(), u16::try_from(row).unwrap())
        );
    }

    #[test]
    fn missed_letters_are_underlined() {
        let now = Instant::now();
        let session = typed_session("abc de", "a ", now);
        let buffer = buffer(&session, now, 40, 10);
        let screen = rendered(&session, now, 40, 10);
        let row = screen
            .iter()
            .position(|l| l.contains("abc de"))
            .expect("text row");
        let column = screen[row].find("abc").expect("word");
        let style_at = |offset: usize| {
            buffer[(
                u16::try_from(column + offset).unwrap(),
                u16::try_from(row).unwrap(),
            )]
                .modifier
        };

        assert!(!style_at(0).contains(Modifier::UNDERLINED));
        assert!(style_at(1).contains(Modifier::UNDERLINED));
        assert!(style_at(2).contains(Modifier::UNDERLINED));
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

    #[test]
    fn words_mode_shows_word_progress() {
        let now = Instant::now();
        let mut session = Session::new("ab cd ef".split_whitespace(), Limit::Words);
        for c in "ab c".chars() {
            session.handle(Input::Char(c), now);
        }
        let screen = rendered(&session, now, 40, 10).join("\n");

        assert!(screen.contains("1/3"), "{screen}");
    }

    #[test]
    fn live_stats_follow_config() {
        let now = Instant::now();
        let session = typed_session("hello world", "hellx", now);
        let config = Config {
            live_progress: false,
            live_accuracy: true,
            live_errors: true,
            ..Config::default()
        };
        let screen = lines(&draw(view(&session, &config, now), 40, 10)).join("\n");

        assert!(screen.contains("80% acc  1 err"), "{screen}");
        assert!(!screen.contains("wpm"), "{screen}");
        assert!(!screen.contains("30"), "{screen}");
    }

    #[test]
    fn shows_notice() {
        let now = Instant::now();
        let session = Session::new("hello world".split_whitespace(), LIMIT);
        let config = Config::default();
        let mut view = view(&session, &config, now);
        view.notice = Some("config broken");
        let screen = lines(&draw(view, 40, 10)).join("\n");

        assert!(screen.contains("config broken"), "{screen}");
    }

    #[test]
    fn shows_next_test_before_start() {
        let now = Instant::now();
        let session = Session::new("hello world".split_whitespace(), LIMIT);
        let screen = rendered(&session, now, 50, 10).join("\n");

        assert!(
            screen.contains("time  words  │  15  30  60  120"),
            "{screen}"
        );
    }

    #[test]
    fn hides_next_test_once_started() {
        let now = Instant::now();
        let session = typed_session("hello world", "h", now);
        let screen = rendered(&session, now, 50, 10).join("\n");

        assert!(!screen.contains("words"), "{screen}");
    }

    #[test]
    fn blind_mode_hides_mistakes() {
        let now = Instant::now();
        let session = typed_session("abc", "axcz", now);
        let config = Config {
            blind: true,
            ..Config::default()
        };
        let buffer = draw(view(&session, &config, now), 40, 10);
        let screen = lines(&buffer);
        let row = screen.iter().position(|l| l.contains("abcz")).expect("row");
        let column = screen[row].find("abcz").expect("word");
        for offset in 0..4 {
            let cell = &buffer[(
                u16::try_from(column + offset).unwrap(),
                u16::try_from(row).unwrap(),
            )];
            assert_eq!(cell.fg, ratatui::style::Color::Reset, "offset {offset}");
        }
    }
}
