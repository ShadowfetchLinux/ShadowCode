//! `/api/agents` and `/api/subagents…` routes. Definitions live in
//! `crate::agents`; runs in `crate::subagents`.
use super::*;
use crate::{agents, subagents};

impl Service {
    pub(super) fn agents(
        &self,
        method: &str,
        parts: &[&str],
        query: &HashMap<String, String>,
    ) -> Result<Value> {
        let q = |key: &str| query.get(key).map(String::as_str).unwrap_or("");
        let store = self.engine.store();
        match (method, &parts[1..]) {
            // Agent definitions for this project (project, user, built-in).
            ("GET", ["agents"]) => {
                let workspace = if q("workspace").is_empty() {
                    Workspace::open(&self.workspace()?)?
                } else {
                    Workspace::open(&expand_path(q("workspace"))?)?
                };
                let mut value =
                    agents::discover(&workspace, Some(&agents::user_dir(self.engine.paths())))
                        .to_json();
                let config = Config::load(self.engine.paths(), Some(&workspace.path))?;
                value["settings"] = json!(subagents::SubagentsConfig::from_config(&config));
                value["user_dir"] = json!(agents::user_dir(self.engine.paths()));
                value["workspace"] = json!(workspace.path);
                Ok(value)
            }
            // Runs started from one conversation, oldest first.
            ("GET", ["subagents"]) => {
                let session = q("session_id");
                ensure!(!session.is_empty(), "Give session_id");
                Ok(json!({"runs": subagents::list(&store, session)?}))
            }
            ("GET", ["subagents", id]) => Ok(json!(subagents::get(&store, id)?)),
            _ => bail!(
                "Application command is not available: {method} /{}",
                parts.join("/")
            ),
        }
    }
}
