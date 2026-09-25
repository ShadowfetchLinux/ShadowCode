//! `/api/issues…`: "Start from an issue". Lists a repository's open issues
//! through `gh` (GitHub) or `glab` (GitLab) with the user's own sign-in, and
//! turns a picked issue into a task to review in the composer plus a
//! suggested branch name. The parsing and task text live in
//! `crate::issues`; the forge helpers are the Git panel's (`forge.rs`).
use super::forge::{clean, user_tool_env};
use super::*;
use crate::issues;

const ISSUE_TIMEOUT: Duration = Duration::from_secs(60);
/// Issue views include every comment; allow long threads before parsing.
const ISSUE_OUTPUT: usize = 4_000_000;

async fn forge_tool(
    workspace: &Path,
    program: &str,
    args: Vec<String>,
) -> Result<process::ProcessResult> {
    let mut spec = ProcessSpec::command(program, &[], workspace.to_owned());
    spec.args = args;
    spec.timeout = ISSUE_TIMEOUT;
    spec.output_limit = ISSUE_OUTPUT;
    spec.env = user_tool_env();
    let result = process::run(spec, CancellationToken::new(), None).await?;
    ensure!(
        result.ok,
        "{program} could not read the issues: {}",
        clean(if result.stderr.trim().is_empty() {
            &result.stdout
        } else {
            &result.stderr
        })
    );
    ensure!(
        !result.truncated,
        "{program} returned more text than expected"
    );
    Ok(result)
}

/// GitLab's API path for a project: `group%2Fsub%2Frepo`.
fn project_path(path: &str) -> String {
    path.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

impl Service {
    pub(super) async fn issue_routes(&self, call: &Arc<Call>) -> Result<Value> {
        match (call.method.as_str(), call.parts().as_slice()) {
            ("GET", ["api", "issues"]) => {
                self.issue_list(call.q("remote"), call.limit(30, 100)).await
            }
            ("GET", ["api", "issues", number]) => {
                let number = number
                    .parse::<u64>()
                    .ok()
                    .filter(|n| *n > 0)
                    .context("Invalid issue number")?;
                self.issue_detail(call.q("remote"), number).await
            }
            _ => Err(call.unavailable()),
        }
    }

    /// The forge of the chosen remote and whether its CLI is ready; `Err`
    /// carries an explanation for the window instead of failing the request.
    async fn issue_forge(
        &self,
        workspace: &Path,
        remote: &str,
    ) -> Result<std::result::Result<(String, super::forge::RemoteInfo, Value), Value>> {
        let (name, info) = match self.forge_remote(workspace, remote).await {
            Ok(found) => found,
            Err(error) => {
                return Ok(Err(json!({
                    "ready": false, "provider": null, "cli": {"name": null},
                    "reason": format!("{error:#}"), "issues": [],
                })))
            }
        };
        if !matches!(info.kind, "github" | "gitlab") {
            return Ok(Err(json!({
                "ready": false, "remote": name, "provider": info.kind, "cli": {"name": null},
                "reason": "Issues can be listed for GitHub and GitLab remotes.", "issues": [],
            })));
        }
        let cli = self.forge_cli(workspace, &info).await;
        if cli["installed"] != true || cli["authenticated"] != true {
            return Ok(Err(json!({
                "ready": false, "remote": name, "provider": info.kind, "cli": cli, "issues": [],
            })));
        }
        Ok(Ok((name, info, cli)))
    }

    /// GET /api/issues: open issues of the project's repository.
    async fn issue_list(&self, remote: &str, limit: usize) -> Result<Value> {
        let workspace = self.workspace()?;
        let (name, info, cli) = match self.issue_forge(&workspace, remote).await? {
            Ok(ready) => ready,
            Err(explained) => return Ok(explained),
        };
        let repo = Self::repo_arg(&info);
        let limit = limit.to_string();
        let found = if info.kind == "github" {
            let out = forge_tool(
                &workspace,
                "gh",
                vec![
                    "issue".into(),
                    "list".into(),
                    "--repo".into(),
                    repo,
                    "--state".into(),
                    "open".into(),
                    "--limit".into(),
                    limit,
                    "--json".into(),
                    "number,title,author,labels,updatedAt,url".into(),
                ],
            )
            .await?;
            issues::parse_gh_list(&out.stdout)?
        } else {
            let out = forge_tool(
                &workspace,
                "glab",
                vec![
                    "issue".into(),
                    "list".into(),
                    "--repo".into(),
                    repo,
                    "--per-page".into(),
                    limit,
                    "--output".into(),
                    "json".into(),
                ],
            )
            .await?;
            issues::parse_glab_list(&out.stdout)?
        };
        Ok(json!({
            "ready": true,
            "remote": name,
            "provider": info.kind,
            "remote_info": info,
            "cli": cli,
            "issues": found,
        }))
    }

    /// GET /api/issues/<n>: the issue with its newest comments, the task it
    /// pre-fills and a branch name.
    async fn issue_detail(&self, remote: &str, number: u64) -> Result<Value> {
        let workspace = self.workspace()?;
        let (_, info, _) = match self.issue_forge(&workspace, remote).await? {
            Ok(ready) => ready,
            Err(explained) => {
                bail!(
                    "{}",
                    explained["reason"]
                        .as_str()
                        .unwrap_or("Sign in to the forge's command-line tool to read issues")
                )
            }
        };
        let repo = Self::repo_arg(&info);
        let issue = if info.kind == "github" {
            let out = forge_tool(
                &workspace,
                "gh",
                vec![
                    "issue".into(),
                    "view".into(),
                    number.to_string(),
                    "--repo".into(),
                    repo,
                    "--json".into(),
                    "number,title,body,url,author,labels,state,comments,updatedAt".into(),
                ],
            )
            .await?;
            issues::parse_gh_issue(&out.stdout)?
        } else {
            let out = forge_tool(
                &workspace,
                "glab",
                vec![
                    "issue".into(),
                    "view".into(),
                    number.to_string(),
                    "--repo".into(),
                    repo,
                    "--output".into(),
                    "json".into(),
                ],
            )
            .await?;
            // Comments are optional: an older glab or a restricted token
            // still gives the issue itself.
            let notes = forge_tool(
                &workspace,
                "glab",
                vec![
                    "api".into(),
                    "--hostname".into(),
                    info.host.clone(),
                    format!(
                        "projects/{}/issues/{number}/notes?sort=desc&order_by=created_at&per_page=20",
                        project_path(&info.path)
                    ),
                ],
            )
            .await
            .ok();
            issues::parse_glab_issue(&out.stdout, notes.as_ref().map(|n| n.stdout.as_str()))?
        };
        Ok(json!({
            "provider": info.kind,
            "branch": issues::branch_name(issue.number, &issue.title),
            "marker": issues::marker(info.kind, issue.number),
            "task": issues::task_text(info.kind, &issue),
            "issue": issue,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitlab_project_paths_are_encoded() {
        assert_eq!(project_path("group/sub/repo"), "group%2Fsub%2Frepo");
        assert_eq!(project_path("a b"), "a%20b");
    }
}
