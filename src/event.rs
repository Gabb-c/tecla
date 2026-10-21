use eyre::WrapErr;
use ratatui::crossterm::event::{self, Event as CrosstermEvent};
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const TICK_FPS: f64 = 30.0;

#[derive(Debug)]
pub enum Event {
    /// Emitted at a fixed rate so the timer advances without user input.
    Tick,
    Crossterm(CrosstermEvent),
    App(AppEvent),
    /// The event thread failed and stopped; no further terminal events will arrive.
    Error(eyre::Report),
}

#[derive(Clone, Debug)]
pub enum AppEvent {
    Quit,
}

/// Receives terminal and tick events from a background thread.
#[derive(Debug)]
pub struct EventHandler {
    sender: mpsc::Sender<Event>,
    receiver: mpsc::Receiver<Event>,
}

impl EventHandler {
    /// Spawns the event thread.
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let actor = EventThread::new(sender.clone());
        thread::spawn(|| actor.run());
        Self { sender, receiver }
    }

    /// A handler without the event thread, so tests never touch the terminal.
    #[cfg(test)]
    pub fn detached() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }

    /// Blocks until the next event arrives.
    ///
    /// # Errors
    ///
    /// Returns an error if the channel is disconnected, which cannot happen while `self` holds
    /// a sender.
    pub fn next(&self) -> eyre::Result<Event> {
        Ok(self.receiver.recv()?)
    }

    /// Queues `app_event` for the next iteration of the event loop.
    pub fn send(&self, app_event: AppEvent) {
        // cannot fail: `self` owns the receiver
        let _ = self.sender.send(Event::App(app_event));
    }
}

struct EventThread {
    sender: mpsc::Sender<Event>,
}

impl EventThread {
    fn new(sender: mpsc::Sender<Event>) -> Self {
        Self { sender }
    }

    fn run(self) {
        // the handler keeps its own sender alive, so a silent exit would leave it blocked forever
        if let Err(error) = self.event_loop() {
            self.send(Event::Error(error));
        }
    }

    /// Emits ticks at a fixed rate and forwards crossterm events in between.
    fn event_loop(&self) -> eyre::Result<()> {
        let tick_interval = Duration::from_secs_f64(1.0 / TICK_FPS);
        let mut last_tick = Instant::now();
        loop {
            let timeout = tick_interval.saturating_sub(last_tick.elapsed());
            if timeout == Duration::ZERO {
                last_tick = Instant::now();
                self.send(Event::Tick);
            }
            if event::poll(timeout).wrap_err("failed to poll for crossterm events")? {
                let event = event::read().wrap_err("failed to read crossterm event")?;
                self.send(Event::Crossterm(event));
            }
        }
    }

    fn send(&self, event: Event) {
        // the receiver is dropped on shutdown; a failed send then is expected
        let _ = self.sender.send(event);
    }
}
