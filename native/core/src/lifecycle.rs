//! Lifecycle of the temporary parent used by AppImage extraction mode.
//! Ordinary signals shut down owned work; parent loss also covers a killed
//! extraction wrapper that cannot forward a signal or clean up its child.
use std::{path::PathBuf, time::Duration};

pub async fn interrupted(parent: Option<u32>) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut terminate = signal(SignalKind::terminate()).ok();
        // Preserve nohup's inherited SIGHUP ignore. Registering a Tokio listener
        // unconditionally would silently turn nohup back into a shutdown signal.
        let mut previous = std::mem::MaybeUninit::<libc::sigaction>::uninit();
        // SAFETY: sigaction initializes the writable output on success. A null
        // action only reads the current disposition; it cannot change it.
        let ignore_hangup = unsafe {
            libc::sigaction(libc::SIGHUP, std::ptr::null(), previous.as_mut_ptr()) == 0
                && previous.assume_init().sa_sigaction == libc::SIG_IGN
        };
        let mut hangup = if ignore_hangup {
            None
        } else {
            signal(SignalKind::hangup()).ok()
        };
        async fn receive(stream: &mut Option<tokio::signal::unix::Signal>) {
            if let Some(stream) = stream {
                stream.recv().await;
            } else {
                std::future::pending::<()>().await;
            }
        }
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = receive(&mut terminate) => {},
            _ = receive(&mut hangup) => {},
            _ = parent_exited(parent) => {},
        }
    }
    #[cfg(not(unix))]
    tokio::select! {_=tokio::signal::ctrl_c()=>{},_=parent_exited(parent)=>{}}
}

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
