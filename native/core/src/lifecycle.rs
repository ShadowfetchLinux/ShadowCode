//! Lifecycle of the temporary parent used by AppImage extraction mode.
//! The extraction runtime does not forward a signal sent only to its own PID.
//! Observe that parent separately so managed work is not left running orphaned.
use std::{path::PathBuf, time::Duration};

pub fn extraction_parent() -> Option<u32> {
    let directory = PathBuf::from(std::env::var_os("APPDIR")?);
    std::env::var_os("APPIMAGE")?;
    // FUSE-mounted images can directly replace the launcher process. Only the
    // observed extract-and-run layout has a temporary owning wrapper to follow.
    if !directory
        .file_name()?
        .to_str()?
        .starts_with("appimage_extracted_")
        || !std::env::current_exe().ok()?.starts_with(&directory)
    {
        return None;
    }
    // SAFETY: getppid reads process metadata and takes no pointer arguments.
    let parent = unsafe { libc::getppid() } as u32;
    // If the wrapper already exited, the native process must also stop before
    // accepting work. An impossible PID causes the first check to resolve.
    Some(if parent <= 1 { 0 } else { parent })
}

pub async fn parent_exited(parent: Option<u32>) {
    let Some(parent) = parent else {
        std::future::pending::<()>().await;
        return;
    };
    loop {
        // Reparenting, rather than PID existence, also handles PID reuse.
        if unsafe { libc::getppid() } as u32 != parent {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
