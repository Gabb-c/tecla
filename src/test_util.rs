use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use ratatui::{Terminal, backend::TestBackend, widgets::Widget};

use crate::engine::{Input, Limit, Session};
use crate::stats::Timeline;

/// A fresh directory under the system temp dir, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        // tests run in parallel and several modules reuse names like "missing"
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("tecla-test-{}-{n}-{name}", std::process::id()));
        // leftovers from an aborted run; usually nothing to remove
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // best effort; a leftover temp dir is harmless
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A time test of `secs` seconds with one correct word typed at the start of each second.
pub fn steady_timeline(secs: u64) -> Timeline {
    let start = Instant::now();
    let mut session = Session::new(["a"; 64], Limit::Time(Duration::from_secs(secs)));
    let mut timeline = Timeline::default();
    for s in 0..secs {
        let now = start + Duration::from_secs(s);
        timeline.observe(&session, now);
        session.handle(Input::Char('a'), now);
        session.handle(Input::Char(' '), now);
    }
    session.tick(start + Duration::from_secs(secs));
    timeline.finish(&session);
    timeline
}

/// The symbols `widget` draws in a `width` by `height` terminal, one line per row.
pub fn render(widget: impl Widget, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| frame.render_widget(widget, frame.area()))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
