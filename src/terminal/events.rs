use std::sync::mpsc::{channel, Receiver, Sender};

use alacritty_terminal::event::{Event as AlacEvent, EventListener};

/// Events emitted by the terminal
#[derive(Debug, Clone)]
pub enum TerminalEvent {
    /// Terminal needs redraw
    Wakeup,
    /// Title changed
    TitleChanged(String),
    /// Bell rang
    Bell,
    /// Terminal exited with status code
    Exit(i32),
    /// Clipboard store request
    ClipboardStore(String),
}

impl From<AlacEvent> for TerminalEvent {
    fn from(event: AlacEvent) -> Self {
        match event {
            AlacEvent::Wakeup => TerminalEvent::Wakeup,
            AlacEvent::Title(t) => TerminalEvent::TitleChanged(t),
            AlacEvent::Bell => TerminalEvent::Bell,
            AlacEvent::Exit => TerminalEvent::Exit(0),
            AlacEvent::ClipboardStore(_, data) => TerminalEvent::ClipboardStore(data),
            _ => TerminalEvent::Wakeup,
        }
    }
}

/// Event sender that implements alacritty's EventListener
#[derive(Clone)]
pub struct TerminalEventSender(pub Sender<TerminalEvent>);

impl EventListener for TerminalEventSender {
    fn send_event(&self, event: AlacEvent) {
        let _ = self.0.send(TerminalEvent::from(event));
    }
}

/// Create a new event channel for terminal events
pub fn event_channel() -> (TerminalEventSender, Receiver<TerminalEvent>) {
    let (tx, rx) = channel();
    (TerminalEventSender(tx), rx)
}
