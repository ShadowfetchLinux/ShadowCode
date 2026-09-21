//! Preserve the directory capability in SQLite's filename. The bundled Unix
//! VFS otherwise canonicalizes /proc/self/fd back to an ambient path, losing
//! the held parent when a project directory is renamed. All locking, WAL and
//! I/O methods remain SQLite's own; this is not a second locking implementation.
use anyhow::{ensure, Context, Result};
use rusqlite::ffi;
use std::{
    ffi::{c_char, c_int, CStr},
    sync::OnceLock,
};

pub(super) const NAME: &str = "shadowcode-confined-readonly";

fn valid(name: &CStr) -> bool {
    let Some(rest) = name.to_bytes().strip_prefix(b"/proc/self/fd/") else {
        return false;
    };
    let Some(split) = rest.iter().position(|b| *b == b'/') else {
        return false;
    };
    let (fd, leaf) = rest.split_at(split);
    !fd.is_empty()
        && fd.iter().all(u8::is_ascii_digit)
        && leaf.len() > 1
        && !leaf[1..].contains(&b'/')
        && leaf != b"/."
        && leaf != b"/.."
}

unsafe extern "C" fn full_path(
    _: *mut ffi::sqlite3_vfs,
    input: *const c_char,
    size: c_int,
    out: *mut c_char,
) -> c_int {
    if input.is_null() || out.is_null() {
        return ffi::SQLITE_CANTOPEN;
    }
    // SQLite owns the NUL-terminated input and the output buffer of `size` bytes.
    let name = unsafe { CStr::from_ptr(input) };
    let bytes = name.to_bytes_with_nul();
    if !valid(name) || size < 0 || bytes.len() > size as usize {
        return ffi::SQLITE_CANTOPEN;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr().cast(), out, bytes.len());
    }
    ffi::SQLITE_OK
}

unsafe extern "C" fn open(
    vfs: *mut ffi::sqlite3_vfs,
    name: *const c_char,
    file: *mut ffi::sqlite3_file,
    flags: c_int,
    actual: *mut c_int,
) -> c_int {
    unsafe {
        (*file).pMethods = std::ptr::null();
    }
    if name.is_null()
        || flags & (ffi::SQLITE_OPEN_MAIN_DB | ffi::SQLITE_OPEN_MAIN_JOURNAL | ffi::SQLITE_OPEN_WAL)
            == 0
    {
        return ffi::SQLITE_READONLY;
    }
    let name_str = unsafe { CStr::from_ptr(name) };
    if !valid(name_str) {
        return ffi::SQLITE_CANTOPEN;
    }
    use std::os::unix::ffi::OsStrExt;
    let path = std::path::Path::new(std::ffi::OsStr::from_bytes(name_str.to_bytes()));
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => (),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && flags & ffi::SQLITE_OPEN_WAL != 0
                && flags & ffi::SQLITE_OPEN_CREATE != 0 => {}
        _ => return ffi::SQLITE_CANTOPEN,
    }
    // The main database and rollback journal are always read-only. Let SQLite
    // manage WAL coordination normally: even a read-only connection may need
    // to create a WAL or initialize shared memory after a clean/crashed writer.
    // SQL authorization separately prevents data/schema writes and checkpoints.
    let flags = if flags & ffi::SQLITE_OPEN_WAL != 0 {
        flags
    } else {
        (flags
            & !(ffi::SQLITE_OPEN_READWRITE
                | ffi::SQLITE_OPEN_CREATE
                | ffi::SQLITE_OPEN_DELETEONCLOSE))
            | ffi::SQLITE_OPEN_READONLY
    };
    let base = unsafe { ffi::sqlite3_vfs_find(c"unix".as_ptr()) };
    if base.is_null() {
        return ffi::SQLITE_CANTOPEN;
    }
    unsafe { (*base).xOpen.unwrap()(vfs, name, file, flags, actual) }
}

unsafe extern "C" fn delete(_: *mut ffi::sqlite3_vfs, _: *const c_char, _: c_int) -> c_int {
    ffi::SQLITE_READONLY
}

unsafe extern "C" fn access(
    _: *mut ffi::sqlite3_vfs,
    name: *const c_char,
    flags: c_int,
    result: *mut c_int,
) -> c_int {
    unsafe {
        *result = 0;
    }
    if name.is_null() {
        return ffi::SQLITE_CANTOPEN;
    }
    let name = unsafe { CStr::from_ptr(name) };
    if !valid(name) {
        return ffi::SQLITE_CANTOPEN;
    }
    use std::os::unix::ffi::OsStrExt;
    let path = std::path::Path::new(std::ffi::OsStr::from_bytes(name.to_bytes()));
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => {
            unsafe {
                *result = i32::from(flags != ffi::SQLITE_ACCESS_READWRITE);
            }
            ffi::SQLITE_OK
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ffi::SQLITE_OK,
        _ => ffi::SQLITE_CANTOPEN,
    }
}

pub(super) fn register() -> Result<()> {
    static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
    let result = REGISTERED.get_or_init(|| {
        let register = || -> Result<()> {
            ensure!(
                cfg!(target_os = "linux"),
                "Native SQLite inspection requires Linux /proc"
            );
            // SQLite's immutable callback table and pAppData have process lifetime.
            // This private copy is published once, never made default or mutated
            // after registration, and retained until process exit as required.
            let base = unsafe { ffi::sqlite3_vfs_find(c"unix".as_ptr()) };
            ensure!(!base.is_null(), "SQLite Unix VFS unavailable");
            let mut vfs = Box::new(unsafe { std::ptr::read(base) });
            vfs.zName = c"shadowcode-confined-readonly".as_ptr();
            vfs.pNext = std::ptr::null_mut();
            vfs.xFullPathname = Some(full_path);
            vfs.xOpen = Some(open);
            vfs.xDelete = Some(delete);
            vfs.xAccess = Some(access);
            vfs.xDlOpen = None;
            vfs.xDlSym = None;
            vfs.xDlClose = None;
            vfs.xSetSystemCall = None;
            let code = unsafe { ffi::sqlite3_vfs_register(Box::leak(vfs), 0) };
            ensure!(
                code == ffi::SQLITE_OK,
                "Could not register read-only SQLite VFS ({code})"
            );
            Ok(())
        };
        register().map_err(|e| format!("{e:#}"))
    });
    result
        .as_ref()
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!(e.clone()))
        .context("Native SQLite initialization failed")
}
