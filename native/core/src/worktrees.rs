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
        let path = entry.path();
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .context("Invalid managed worktree record filename")?;
        let record = read_record_identity(paths, id)?;
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
        fs::read_dir(&records)?
            .collect::<std::io::Result<Vec<_>>>()?
            .iter()
            .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("json"))
            .count()
            < 64,
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

#[derive(Clone, Debug, Serialize)]
pub struct Inspection {
    pub record: Record,
    pub head: String,
    pub current_branch: String,
    pub status: String,
    pub can_remove: bool,
    pub reason: String,
    pub hash: String,
}
fn read_record_identity(paths: &AppPaths, id: &str) -> Result<Record> {
    ensure!(
        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "Use the full managed worktree ID"
    );
    let (records, checkouts) = roots(paths)?;
    let path = records.join(format!("{id}.json"));
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .context("Managed worktree record not found")?;
    ensure!(
        file.metadata()?.is_file(),
        "Worktree record must be a regular file"
    );
    let mut bytes = Vec::new();
    file.take(64_001).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 64_000, "Worktree record exceeds 64 KB");
    let record: Record = serde_json::from_slice(&bytes)?;
    ensure!(
        record.id == id
            && record.path == checkouts.join(id)
            && record.branch == format!("shadowcode/{id}"),
        "Managed worktree record identity changed"
    );
    Ok(record)
}
fn read_record(paths: &AppPaths, source: &Path, id: &str) -> Result<Record> {
    let record = read_record_identity(paths, id)?;
    ensure!(
        record.source == Workspace::open(source)?.path,
        "Worktree belongs to another source project"
    );
    Ok(record)
}
pub async fn inspect(
    paths: &AppPaths,
    source: &Path,
    id: &str,
    cancel: CancellationToken,
) -> Result<Inspection> {
    use sha2::{Digest, Sha256};
    let record = read_record(paths, source, id)?;
    let meta = fs::symlink_metadata(&record.path)
        .context("Managed checkout is missing; inspect its recovery record and Git registration")?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "Managed checkout must be a real directory"
    );
    ensure!(
        record.path.canonicalize()? == record.path,
        "Managed checkout path changed"
    );
    let actual = git(
        &record.path,
        &["rev-parse", "--show-toplevel"],
        cancel.clone(),
    )
    .await?;
    ensure!(
        Path::new(&actual).canonicalize()? == record.path,
        "Managed checkout repository root changed"
    );
    for path in [&record.source, &record.path] {
        let common = git(
            path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            cancel.clone(),
        )
        .await?;
        ensure!(
            Path::new(&common).canonicalize()? == record.common_directory,
            "Managed checkout repository identity changed"
        );
    }
    let registered = git(
        &record.source,
        &["worktree", "list", "--porcelain", "-z"],
        cancel.clone(),
    )
    .await?;
    let expected = format!(
        "worktree {}",
        record
            .path
            .to_str()
            .context("Worktree path must be UTF-8")?
    );
    let registration = registered
        .split("\0\0")
        .find(|block| block.split('\0').any(|field| field == expected))
        .context("Managed checkout is no longer registered with its source repository")?;
    let locked = registration
        .split('\0')
        .any(|field| field == "locked" || field.starts_with("locked "));
    let head = git(
        &record.path,
        &["rev-parse", "--verify", "HEAD"],
        cancel.clone(),
    )
    .await?;
    let current_branch = git(&record.path, &["branch", "--show-current"], cancel.clone()).await?;
    let status = git(
        &record.path,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignored=matching",
        ],
        cancel,
    )
    .await?;
    let reason = if locked {
        "Git worktree is locked; unlock it deliberately before removal"
    } else if !status.is_empty() {
        "Checkout contains local edits, untracked or ignored files; preserve them before removal"
    } else if current_branch.is_empty() {
        "Detached HEAD may contain unreferenced commits; attach a branch before removal"
    } else {
        "Clean checkout; its branch and commits will be preserved"
    }
    .to_string();
    let can_remove = !locked && status.is_empty() && !current_branch.is_empty();
    let hash = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(
            &serde_json::json!({"record":record,"head":head,"branch":current_branch,"status":status,"locked":locked})
        )?)
    );
    Ok(Inspection {
        record,
        head,
        current_branch,
        status,
        can_remove,
        reason,
        hash,
    })
}
pub async fn remove(
    paths: &AppPaths,
    source: &Path,
    id: &str,
    expected_hash: &str,
    cancel: CancellationToken,
) -> Result<Record> {
    let _guard = tokio::select! {guard=CREATION.lock()=>guard,_=cancel.cancelled()=>anyhow::bail!("Worktree removal cancelled")};
    let inspection = inspect(paths, source, id, cancel.clone()).await?;
    ensure!(
        inspection.hash == expected_hash,
        "Worktree changed; inspect it again before removal"
    );
    ensure!(inspection.can_remove, "{}", inspection.reason);
    let (records, _) = roots(paths)?;
    let mut record = inspection.record;
    record.state = "removing".into();
    record.detail = format!(
        "Removing clean checkout; branch {} is preserved",
        inspection.current_branch
    );
    save(&records, &record)?;
    // Git performs its own final cleanliness/registration checks. Never use
    // --force, branch deletion, or recursive filesystem deletion here.
    let result = git(
        &record.source,
        &[
            "worktree",
            "remove",
            "--",
            record
                .path
                .to_str()
                .context("Worktree path must be UTF-8")?,
        ],
        cancel,
    )
    .await;
    match result {
        Ok(_) => {
            record.state = "removed".into();
            record.detail = format!(
                "Checkout removed; branch {} and commit {} retained",
                inspection.current_branch, inspection.head
            );
            save(&records, &record)?;
            let archive = records.join("archive");
            paths::private_directory(&archive)?;
            fs::rename(
                records.join(format!("{}.json", record.id)),
                archive.join(format!("{}.json", record.id)),
            )?;
            fs::File::open(&records)?.sync_all()?;
            fs::File::open(&archive)?.sync_all()?;
            Ok(record)
        }
        Err(error) => {
            record.state = "needs_attention".into();
            record.detail = format!("{error:#}").chars().take(2000).collect();
            save(&records, &record)?;
            Err(error)
        }
    }
}
