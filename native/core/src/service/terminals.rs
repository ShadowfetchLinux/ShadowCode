//! `/api/terminals…`: the user's own interactive terminals in the drawer
//! (see `crate::terminal`). They belong to this view's hub, run the user's
//! login shell outside the agent sandbox, and need no approval or trust: the
//! person at the window types every byte. They work while a task runs, since
//! they hold no workspace reservation. No route here is reachable by a model
//! and no output is stored.
use super::*;
use crate::terminal::{self, OpenOptions};

#[derive(Default, Deserialize)]
#[serde(default)]
struct TerminalBody {
    cols: Loose<u16>,
    rows: Loose<u16>,
    data: Text,
}

impl Service {
    pub(super) async fn terminal_routes(&self, call: &Arc<Call>) -> Result<Value> {
        match (call.method.as_str(), call.parts().as_slice()) {
            // Input may wait for the terminal to drain; keep it off the
            // async workers. Everything else is quick bookkeeping.
            ("POST", ["api", "terminals", _, "input"]) => {
                self.blocking(call, Self::terminal_input).await
            }
            _ => self.terminal_state(call),
        }
    }
    fn terminal_input(&self, call: &Call) -> Result<Value> {
        let body: TerminalBody = call.body()?;
        let id = terminal::valid_id(call.parts()[2])?;
        self.terminals.write(id, body.data.as_str().as_bytes())?;
        Ok(json!({"ok": true}))
    }
    fn terminal_state(&self, call: &Call) -> Result<Value> {
        let body: TerminalBody = call.body()?;
        match (call.method.as_str(), call.parts().as_slice()) {
            ("GET", ["api", "terminals"]) => {
                let workspace = self.workspace()?;
                Ok(json!({
                    "workspace": workspace,
                    "terminals": self.terminals.list(Some(&workspace))?,
                    "limits": {
                        "open": terminal::MAX_TERMINALS,
                        "scrollback_bytes": terminal::SCROLLBACK_BYTES,
                    },
                }))
            }
            ("POST", ["api", "terminals"]) => self.terminals.open(
                &self.workspace()?,
                OpenOptions {
                    cols: body.cols.0.unwrap_or(0),
                    rows: body.rows.0.unwrap_or(0),
                    shell: None,
                    login: true,
                },
            ),
            ("GET", ["api", "terminals", id, "output"]) => {
                let after = match call.q("after") {
                    "" => 0,
                    value => value.parse().context("Invalid output cursor")?,
                };
                self.terminals.read(terminal::valid_id(id)?, after)
            }
            ("POST", ["api", "terminals", id, "resize"]) => self.terminals.resize(
                terminal::valid_id(id)?,
                body.cols.0.unwrap_or(0),
                body.rows.0.unwrap_or(0),
            ),
            ("POST", ["api", "terminals", id, "close"]) => {
                self.terminals.close(terminal::valid_id(id)?)?;
                Ok(json!({"ok": true}))
            }
            _ => Err(call.unavailable()),
        }
    }
    /// The desktop is quitting: hang up this view's terminals.
    pub fn close_terminals(&self, wait: Duration) {
        self.terminals.close_all(wait);
    }
}
