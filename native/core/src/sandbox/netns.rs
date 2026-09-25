//! A private network namespace for one command, with a proxy socket inside.
//!
//! Between fork and exec (so before bubblewrap starts) the child enters a new
//! user + network namespace, brings up loopback, binds a TCP listener on
//! `127.0.0.1:PROXY_PORT` there and passes that listener to this process over
//! a socketpair. A socket stays in the namespace it was created in, so this
//! process can accept the command's proxy connections and dial allowed hosts
//! from the normal network. bubblewrap then shares that namespace (it is run
//! without `--unshare-net`). No helper program runs inside the sandbox.
//!
//! Everything in [`Netns::enter`] runs in the forked child and only makes raw
//! system calls on memory prepared beforehand (no allocation, no locks).
use super::proxy::PROXY_PORT;
use anyhow::{ensure, Context, Result};
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
};

#[derive(Debug)]
pub struct Netns {
    child: OwnedFd,
    parent: OwnedFd,
    uid_map: Vec<u8>,
    gid_map: Vec<u8>,
}

fn check(result: libc::c_int) -> io::Result<libc::c_int> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

/// Write `data` to a NUL-terminated path. Async-signal-safe.
unsafe fn write_file(path: &[u8], data: &[u8]) -> io::Result<()> {
    let fd = check(libc::open(
        path.as_ptr().cast(),
        libc::O_WRONLY | libc::O_CLOEXEC,
    ))?;
    let written = libc::write(fd, data.as_ptr().cast(), data.len());
    let error = io::Error::last_os_error();
    libc::close(fd);
    if written != data.len() as isize {
        return Err(error);
    }
    Ok(())
}

impl Netns {
    pub fn new() -> Result<Self> {
        let mut fds = [0 as RawFd; 2];
        // SAFETY: fds is a valid two-element buffer.
        check(unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
                0,
                fds.as_mut_ptr(),
            )
        })
        .context("Could not create the proxy hand-off socket")?;
        // SAFETY: both descriptors were just returned by socketpair.
        let (child, parent) =
            unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        // SAFETY: getuid/getgid cannot fail.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        Ok(Self {
            child,
            parent,
            uid_map: format!("{uid} {uid} 1\n").into_bytes(),
            gid_map: format!("{gid} {gid} 1\n").into_bytes(),
        })
    }

    /// Runs in the forked child before exec.
    ///
    /// # Safety
    /// Call only between fork and exec in a single-threaded child.
    pub unsafe fn enter(&self) -> io::Result<()> {
        check(libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET))?;
        write_file(b"/proc/self/setgroups\0", b"deny")?;
        write_file(b"/proc/self/uid_map\0", &self.uid_map)?;
        write_file(b"/proc/self/gid_map\0", &self.gid_map)?;

        // Bring up loopback.
        let control = check(libc::socket(
            libc::AF_INET,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            0,
        ))?;
        let mut request: libc::ifreq = std::mem::zeroed();
        request.ifr_name[0] = b'l' as libc::c_char;
        request.ifr_name[1] = b'o' as libc::c_char;
        let result = (|| {
            check(libc::ioctl(control, libc::SIOCGIFFLAGS as _, &mut request))?;
            request.ifr_ifru.ifru_flags |= libc::IFF_UP as libc::c_short;
            check(libc::ioctl(control, libc::SIOCSIFFLAGS as _, &request))
        })();
        libc::close(control);
        result?;

        // The proxy listener, owned by the namespace.
        let listener = check(libc::socket(
            libc::AF_INET,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
            0,
        ))?;
        let mut address: libc::sockaddr_in = std::mem::zeroed();
        address.sin_family = libc::AF_INET as libc::sa_family_t;
        address.sin_port = PROXY_PORT.to_be();
        address.sin_addr.s_addr = u32::from_be_bytes([127, 0, 0, 1]).to_be();
        let result = (|| {
            check(libc::bind(
                listener,
                (&address as *const libc::sockaddr_in).cast(),
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            ))?;
            check(libc::listen(listener, 64))?;
            send_fd(self.child.as_raw_fd(), listener)
        })();
        libc::close(listener);
        result
    }

    /// In the parent after the child exec'd: take the namespace's listener.
    pub fn receive(&self) -> Result<std::net::TcpListener> {
        // SAFETY: parent is a valid socket; the buffers outlive the call.
        let fd = unsafe { receive_fd(self.parent.as_raw_fd()) }
            .context("The sandbox did not hand over its proxy socket")?;
        // SAFETY: fd is a freshly received, owned listening socket.
        Ok(unsafe { std::net::TcpListener::from_raw_fd(fd) })
    }
}

const CMSG_SPACE: usize = 64;

unsafe fn send_fd(socket: RawFd, fd: RawFd) -> io::Result<()> {
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    let mut control = [0u8; CMSG_SPACE];
    let mut message: libc::msghdr = std::mem::zeroed();
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) as _;
    let header = libc::CMSG_FIRSTHDR(&message);
    if header.is_null() {
        return Err(io::Error::from_raw_os_error(libc::EINVAL));
    }
    (*header).cmsg_level = libc::SOL_SOCKET;
    (*header).cmsg_type = libc::SCM_RIGHTS;
    (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as _;
    std::ptr::copy_nonoverlapping(
        (&fd as *const RawFd).cast::<u8>(),
        libc::CMSG_DATA(header),
        std::mem::size_of::<RawFd>(),
    );
    if libc::sendmsg(socket, &message, libc::MSG_NOSIGNAL) != 1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

unsafe fn receive_fd(socket: RawFd) -> Result<RawFd> {
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr().cast(),
        iov_len: 1,
    };
    let mut control = [0u8; CMSG_SPACE];
    let mut message: libc::msghdr = std::mem::zeroed();
    message.msg_iov = &mut iov;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = control.len() as _;
    let received = libc::recvmsg(
        socket,
        &mut message,
        libc::MSG_DONTWAIT | libc::MSG_CMSG_CLOEXEC,
    );
    ensure!(received == 1, "{}", io::Error::last_os_error());
    let header = libc::CMSG_FIRSTHDR(&message);
    ensure!(
        !header.is_null()
            && (*header).cmsg_level == libc::SOL_SOCKET
            && (*header).cmsg_type == libc::SCM_RIGHTS,
        "No socket was passed"
    );
    let mut fd: RawFd = -1;
    std::ptr::copy_nonoverlapping(
        libc::CMSG_DATA(header),
        (&mut fd as *mut RawFd).cast::<u8>(),
        std::mem::size_of::<RawFd>(),
    );
    ensure!(fd >= 0, "Invalid socket was passed");
    Ok(fd)
}

/// Whether this kernel lets an unprivileged process create the namespace.
/// Runs `/bin/true` through the same setup.
pub fn probe() -> Result<()> {
    use std::os::unix::process::CommandExt;
    let netns = std::sync::Arc::new(Netns::new()?);
    let inner = netns.clone();
    let mut command = std::process::Command::new("/bin/true");
    // SAFETY: enter only makes async-signal-safe system calls.
    unsafe {
        command.pre_exec(move || inner.enter());
    }
    let status = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Private network namespaces are unavailable (unprivileged user namespaces may be disabled)")?;
    ensure!(status.success(), "Namespace probe exited with {status}");
    netns.receive().map(|_| ())
}
