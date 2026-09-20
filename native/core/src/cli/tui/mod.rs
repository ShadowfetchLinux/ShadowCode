//! Interactive terminal frontend of the shared native engine.
mod model;
mod render;
mod worker;
use anyhow::{ensure, Context, Result};
use crossterm::{
    event::{
        DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyCode, KeyEvent,
        KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_util::StreamExt;
use model::Editor;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io::{self, Write},
    path::PathBuf,
};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use worker::{Action, ListKind, View};

struct Screen;
impl Screen {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let guard = Self;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            crossterm::cursor::Hide
        )?;
        Ok(guard)
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        let _ = disable_raw_mode();
        let _ = io::stdout().flush();
    }
}
#[derive(Default)]
enum Overlay {
    #[default]
    None,
    Help,
    Picker {
        kind: ListKind,
        query: Editor,
        selected: usize,
    },
    Approval {
        id: String,
        text: String,
        allow: bool,
        scroll: usize,
    },
    Trust,
}
struct App {
    view: View,
    editor: Editor,
    overlay: Overlay,
    mode: usize,
    scroll: usize,
    expanded: bool,
    notice: String,
    history: Vec<String>,
    history_pos: usize,
}
impl App {
    fn new(view: View) -> Self {
        Self {
            view,
            editor: Editor::default(),
            overlay: Overlay::None,
            mode: 0,
            scroll: 0,
            expanded: false,
            notice: String::new(),
            history: vec![],
            history_pos: 0,
        }
    }
    fn send(&mut self, tx: &mpsc::Sender<Action>, action: Action) {
        if tx.try_send(action).is_err() {
            self.notice = "Engine action queue is full; try again when it finishes.".into();
        }
    }
    fn picker(&mut self, tx: &mpsc::Sender<Action>, kind: ListKind) {
        self.send(tx, Action::Catalog(kind, String::new()));
        self.overlay = Overlay::Picker {
            kind,
            query: Editor::default(),
            selected: 0,
        };
    }
    fn choices(&self, kind: ListKind, query: &str) -> Vec<&worker::Choice> {
        if self.view.list_kind != Some(kind) {
            return vec![];
        }
        let query = query.to_lowercase();
        self.view
            .list
            .iter()
            .filter(|c| c.label.to_lowercase().contains(&query))
            .collect()
    }
    fn key(&mut self, key: KeyEvent, tx: &mpsc::Sender<Action>) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('q' | 'd')) {
            return true;
        }
        if !matches!(self.overlay, Overlay::None) {
            if key.code == KeyCode::Esc {
                self.overlay = Overlay::None;
                return false;
            }
            let overlay = std::mem::take(&mut self.overlay);
            self.overlay = match overlay {
                Overlay::Help => Overlay::Help,
                Overlay::Trust => {
                    if key.code == KeyCode::Enter {
                        self.send(tx, Action::Trust);
                        Overlay::None
                    } else {
                        Overlay::Trust
                    }
                }
                Overlay::Approval {
                    id,
                    text,
                    mut allow,
                    mut scroll,
                } => {
                    // Reviewing a captured ID never approves a newly arrived request.
                    let current = self.view.approvals.iter().any(|a| a["id"] == id);
                    if key.code == KeyCode::Tab && text.len() <= 128_000 {
                        allow = !allow;
                    }
                    if key.code == KeyCode::PageDown {
                        scroll = scroll.saturating_add(10);
                    }
                    if key.code == KeyCode::PageUp {
                        scroll = scroll.saturating_sub(10);
                    }
                    if key.code == KeyCode::Enter && current {
                        self.send(tx, Action::Decide(id, allow));
                        Overlay::None
                    } else {
                        Overlay::Approval {
                            id,
                            text,
                            allow,
                            scroll,
                        }
                    }
                }
                Overlay::Picker {
                    kind,
                    mut query,
                    mut selected,
                } => {
                    match key.code {
                        KeyCode::Down => selected = selected.saturating_add(1),
                        KeyCode::Up => selected = selected.saturating_sub(1),
                        KeyCode::Char(c) if !ctrl => {
                            query.insert(&c.to_string());
                            selected = 0;
                        }
                        KeyCode::Backspace => {
                            query.backspace();
                            selected = 0;
                        }
                        KeyCode::Enter => {
                            let choices = self.choices(kind, &query.text);
                            let choice = choices
                                .get(selected.min(choices.len().saturating_sub(1)))
                                .map(|c| c.id.clone());
                            if let Some(id) = choice {
                                match kind {
                                    ListKind::Sessions => self.send(tx, Action::Session(id)),
                                    ListKind::Models => self.send(tx, Action::Model(id)),
                                    ListKind::Projects => self.send(tx, Action::Project(id.into())),
                                    ListKind::Commands => self.editor.replace(format!("/{id} ")),
                                };
                                return false;
                            }
                        }
                        KeyCode::Char('r') if ctrl && kind == ListKind::Sessions => {
                            self.send(tx, Action::Catalog(kind, query.text.clone()));
                        }
                        _ => {}
                    }
                    let count = self.choices(kind, &query.text).len();
                    selected = selected.min(count.saturating_sub(1));
                    Overlay::Picker {
                        kind,
                        query,
                        selected,
                    }
                }
                Overlay::None => Overlay::None,
            };
            return false;
        }
        if ctrl {
            match key.code {
                KeyCode::Char('c') => self.send(tx, Action::Cancel),
                KeyCode::Char('n') => self.send(tx, Action::New),
                KeyCode::Char('p') => self.picker(tx, ListKind::Sessions),
                KeyCode::Char('g') => self.picker(tx, ListKind::Projects),
                KeyCode::Char('o') => self.expanded = !self.expanded,
                KeyCode::Char('b') => {
                    self.scroll = 0;
                    self.send(tx, Action::Older);
                }
                KeyCode::Char('f') => {
                    self.scroll = 0;
                    self.send(tx, Action::Latest);
                }
                KeyCode::Char('t') => self.overlay = Overlay::Trust,
                KeyCode::Char('j') => self.editor.insert("\n"),
                // Ctrl-M is indistinguishable from Enter in legacy terminals.
                KeyCode::Char('l') => self.mode = (self.mode + 1) % 4,
                KeyCode::Char('a') => self.editor.home(),
                KeyCode::Char('e') => self.editor.end(),
                _ => {}
            }
            return false;
        }
        match key.code {
            KeyCode::F(1) => self.overlay = Overlay::Help,
            KeyCode::F(2) => self.picker(tx, ListKind::Models),
            KeyCode::F(3) => self.mode = (self.mode + 1) % 4,
            KeyCode::F(4) => {
                if let Some(a) = self.view.approvals.first() {
                    self.overlay = Overlay::Approval {
                        id: a["id"].as_str().unwrap_or("").into(),
                        text: model::display(&serde_json::to_string_pretty(a).unwrap_or_default()),
                        allow: false,
                        scroll: 0,
                    };
                }
            }
            KeyCode::Tab => self.picker(tx, ListKind::Commands),
            KeyCode::Esc => self.send(tx, Action::Cancel),
            KeyCode::Enter
                if key
                    .modifiers
                    .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
            {
                self.editor.insert("\n")
            }
            KeyCode::Enter => {
                let input = self.editor.text.trim().to_string();
                if input.is_empty() {
                    return false;
                }
                match input.as_str() {
                    "/quit" | "/exit" => return true,
                    "/help" => {
                        self.overlay = Overlay::Help;
                        self.editor.take();
                    }
                    "/sessions" => {
                        self.picker(tx, ListKind::Sessions);
                        self.editor.take();
                    }
                    "/models" => {
                        self.picker(tx, ListKind::Models);
                        self.editor.take();
                    }
                    "/expand" => {
                        self.expanded = !self.expanded;
                        self.editor.take();
                    }
                    _ => {
                        let action = Action::Send(
                            input.clone(),
                            ["coder", "planner", "reviewer", "tester"][self.mode].into(),
                        );
                        if tx.try_send(action).is_ok() {
                            self.editor.take();
                            self.history.push(input);
                            if self.history.len() > 100 {
                                self.history.remove(0);
                            }
                            self.history_pos = self.history.len();
                            self.scroll = 0;
                            self.notice.clear();
                        } else {
                            self.notice =
                                "Engine action queue is full; your draft is preserved.".into();
                        }
                    }
                }
            }
            KeyCode::Char(c) => self.editor.insert(&c.to_string()),
            KeyCode::Backspace => self.editor.backspace(),
            KeyCode::Delete => self.editor.delete(),
            KeyCode::Left => self.editor.left(),
            KeyCode::Right => self.editor.right(),
            KeyCode::Home => self.editor.home(),
            KeyCode::End => self.editor.end(),
            KeyCode::Up => {
                self.history_pos = self.history_pos.saturating_sub(1);
                if let Some(text) = self.history.get(self.history_pos) {
                    self.editor.replace(text.clone());
                }
            }
            KeyCode::Down => {
                self.history_pos = (self.history_pos + 1).min(self.history.len());
                self.editor.replace(
                    self.history
                        .get(self.history_pos)
                        .cloned()
                        .unwrap_or_default(),
                );
            }
            KeyCode::PageUp => self.scroll = self.scroll.saturating_add(10),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(10),
            _ => {}
        };
        false
    }
}
pub(super) async fn run(
    paths: &crate::paths::AppPaths,
    workspace: PathBuf,
    session: Option<&str>,
    parent: Option<u32>,
) -> Result<()> {
    ensure!(
        std::env::var("TERM").unwrap_or_default() != "dumb",
        "The terminal interface requires a cursor-addressable terminal"
    );
    let _screen = Screen::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let (tx, rx) = mpsc::channel(32);
    let (updates, mut view) = watch::channel(View {
        workspace: workspace.clone(),
        ..Default::default()
    });
    let stop = CancellationToken::new();
    let mut worker = tokio::spawn(worker::run(
        paths.clone(),
        workspace,
        session.map(str::to_owned),
        rx,
        updates,
        stop.clone(),
    ));
    let mut app = App::new(view.borrow().clone());
    let mut events = EventStream::new();
    let signal = super::watch::interrupted(parent);
    tokio::pin!(signal);
    let mut ended = false;
    let result:Result<()>=async {
        loop {
            terminal.draw(|frame|render::draw(frame,&app))?;
            tokio::select! {
                event=events.next()=>match event.context("Terminal input closed")?? {
                    Event::Key(key) if app.key(key,&tx)=>break,
                    Event::Paste(text)=>match &mut app.overlay {Overlay::None=>app.editor.insert(&text),Overlay::Picker{query,..}=>query.insert(&text),_=>{}},
                    Event::Resize(_,_)=>{},_=>{}
                },
                update=view.changed()=>{update.context("Engine connection closed")?;let next=view.borrow_and_update().clone();if next.session!=app.view.session {app.scroll=0;}app.view=next;},
                result=&mut worker=>{ended=true;result.context("Terminal engine task stopped")??;break;},
                _=&mut signal=>break,
            }
        };Ok(())
    }.await;
    stop.cancel();
    drop(tx);
    let closed = if !ended {
        worker.await.context("Terminal engine task stopped")?
    } else {
        Ok(())
    };
    result?;
    closed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    #[test]
    fn typing_never_approves_and_review_defaults_to_deny() {
        let (tx, mut rx) = mpsc::channel(8);
        let mut app = App::new(View {
            approvals: vec![json!({"id":"a","command":"touch exact-file"})],
            ..Default::default()
        });
        app.key(key(KeyCode::Char('y')), &tx);
        assert!(rx.try_recv().is_err());
        assert_eq!(app.editor.text, "y");
        app.key(key(KeyCode::F(4)), &tx);
        app.key(key(KeyCode::Enter), &tx);
        assert!(matches!(rx.try_recv().unwrap(),Action::Decide(id,false) if id=="a"));
        app.key(key(KeyCode::F(4)), &tx);
        app.key(key(KeyCode::Tab), &tx);
        app.view.approvals = vec![json!({"id":"replacement"})];
        app.key(key(KeyCode::Enter), &tx);
        assert!(
            rx.try_recv().is_err(),
            "An unseen replacement request must not be approved"
        );
        app.key(key(KeyCode::Esc), &tx);
        app.key(key(KeyCode::F(4)), &tx);
        app.key(key(KeyCode::Tab), &tx);
        app.key(key(KeyCode::Enter), &tx);
        assert!(matches!(rx.try_recv().unwrap(),Action::Decide(id,true) if id=="replacement"));
    }
    #[test]
    fn full_queue_preserves_prompt_and_modes_use_engine_roles() {
        let (tx, mut rx) = mpsc::channel(1);
        tx.try_send(Action::New).unwrap();
        let mut app = App::new(View::default());
        app.editor.insert("repair the tests");
        app.key(key(KeyCode::Enter), &tx);
        assert_eq!(app.editor.text, "repair the tests");
        rx.try_recv().unwrap();
        for role in ["coder", "planner", "reviewer", "tester"] {
            app.key(key(KeyCode::Enter), &tx);
            assert!(matches!(rx.try_recv().unwrap(),Action::Send(_,purpose) if purpose==role));
            app.editor.insert("repair the tests");
            app.key(key(KeyCode::F(3)), &tx);
        }
    }
}
