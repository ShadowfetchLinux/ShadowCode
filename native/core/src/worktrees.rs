//! Managed Git worktree creation. The source checkout is never reset or stashed.
use crate::{
    paths::{self, AppPaths},
    process::{self, ProcessSpec},
    workspace::Workspace,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
static CREATION: Mutex<()> = Mutex::const_new(());
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub source: PathBuf,
    pub path: PathBuf,
    pub common_directory: PathBuf,
    pub base_commit: String,
    pub branch: String,
    pub state: String,
    pub created_at: f64,
    pub detail: String,
}
fn roots(paths: &AppPaths) -> Result<(PathBuf, PathBuf)> {
    let root = paths.data.join("managed-worktrees");
    paths::private_directory(&root)?;
    let records = root.join("records");
    paths::private_directory(&records)?;
    let checkouts = root.join("checkouts");
    paths::private_directory(&checkouts)?;
    Ok((records, checkouts))
}
fn save(root: &Path, record: &Record) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(record)?;
    ensure!(
        bytes.len() <= 64_000,
        "Worktree metadata exceeds its recovery-record limit"
    );
    paths::atomic_write(&root.join(format!("{}.json", record.id)), &bytes, true)
}
pub fn list(paths: &AppPaths, source: &Path) -> Result<Vec<Record>> {
    let source = Workspace::open(source)?.path;
    let (root, _) = roots(paths)?;
    let mut records = vec![];
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.path().extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        ensure!(
            records.len() < 64,
            "Managed worktree inventory exceeds its limit"
        );
        let meta = entry.file_type()?;
        ensure!(meta.is_file(), "Worktree records must be regular files");
        let mut data = Vec::new();
        fs::File::open(entry.path())?
            .take(64_001)
            .read_to_end(&mut data)?;
        ensure!(data.len() <= 64_000, "Worktree record exceeds 64 KB");
        let record: Record = serde_json::from_slice(&data)
            .context("Cannot read managed worktree recovery record")?;
        records.push(record);
    }
    records.retain(|r| r.source == source);
    records.sort_by(|a, b| b.created_at.total_cmp(&a.created_at));
    Ok(records)
}
async fn git(source: &Path, args: &[&str], cancel: CancellationToken) -> Result<String> {
    let mut flags = vec![
        "--no-pager",
        "--literal-pathspecs",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "color.ui=false",
    ];
    flags.extend_from_slice(args);
    let mut spec = ProcessSpec::command("git", &flags, source.into());
    spec.timeout = Duration::from_secs(120);
    spec.output_limit = 64_000;
    let result = process::run(spec, cancel, None).await?;
    ensure!(
        result.ok,
        "Git worktree operation failed: {}{}",
        result.stderr,
        result.stdout
    );
    ensure!(!result.truncated, "Git worktree output exceeded its limit");
    Ok(result.stdout.trim_end_matches('\n').into())
}
pub async fn create(
    paths: &AppPaths,
    source: &Path,
    reference: &str,
    cancel: CancellationToken,
) -> Result<Record> {
    let _guard = tokio::select! {guard=CREATION.lock()=>guard,_=cancel.cancelled()=>anyhow::bail!("Worktree creation cancelled")};
    let source = Workspace::open(source)?.path;
    ensure!(
        !reference.is_empty() && reference.len() <= 256 && !reference.chars().any(char::is_control),
        "Choose a local commit or branch reference"
    );
    let top = git(&source, &["rev-parse", "--show-toplevel"], cancel.clone()).await?;
    ensure!(
        Path::new(&top).canonicalize()? == source,
        "Open the repository root before creating an isolated worktree"
    );
    let commit = git(
        &source,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
        cancel.clone(),
    )
    .await?;
    ensure!(
        matches!(commit.len(), 40 | 64) && commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "Git did not return one commit ID"
    );
    let common = PathBuf::from(
        git(
            &source,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            cancel.clone(),
        )
        .await?,
    )
    .canonicalize()?;
    let (records, checkouts) = roots(paths)?;
    ensure!(
        fs::read_dir(&records)?.filter_map(Result::ok).count() < 64,
        "At most 64 managed worktree records are allowed"
    );
    let id = crate::id();
    let path = checkouts.join(&id);
    let branch = format!("shadowcode/{id}");
    let mut record=Record{id,source:source.clone(),path:path.clone(),common_directory:common,base_commit:commit.clone(),branch:branch.clone(),state:"creating".into(),created_at:crate::now(),detail:"Checkout starts from the selected commit; source uncommitted changes remain in the source checkout.".into()};
    save(&records, &record)?;
    // Write the recovery record before Git can create metadata or files. Never
    // recursively remove a partial checkout after failure or cancellation.
    let result = git(
        &source,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            "--",
            path.to_str().context("Worktree path must be UTF-8")?,
            &commit,
        ],
        cancel,
    )
    .await;
    match result {
        Ok(_) => {
            record.state = "ready".into();
            save(&records, &record)?;
            Ok(record)
        }
        Err(error) => {
            record.state = "needs_attention".into();
            record.detail = format!("{error:#}").chars().take(2000).collect();
            save(&records, &record)?;
            Err(error.context(format!(
                "Recovery record {} preserves any partial worktree at {}",
                record.id,
                path.display()
            )))
        }
    }
}
