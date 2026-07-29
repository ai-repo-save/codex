use std::fs;
use std::io;
use std::mem;
use std::os::fd::AsRawFd;
use std::os::fd::BorrowedFd;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;

pub(super) const SUDO_PATH: &str = "/usr/bin/sudo";

pub(super) struct ProcessIdentity {
    pub pid: u32,
    pub uid: u32,
    pub pidfd: OwnedFd,
}

pub(super) fn runtime_capability() -> io::Result<()> {
    if unsafe { libc::geteuid() } == 0 {
        return Err(io::Error::other("sudo_once is unavailable for root"));
    }
    let sudo = fs::metadata(SUDO_PATH)?;
    let sudo_mode = sudo.permissions().mode();
    if !sudo.is_file()
        || sudo.uid() != 0
        || sudo_mode & libc::S_ISUID == 0
        || sudo_mode & 0o022 != 0
        || sudo_mode & 0o111 == 0
    {
        return Err(io::Error::other(
            "sudo executable did not meet trust requirements",
        ));
    }
    fs::metadata("/proc/self/exe")?;
    let (left, right) = UnixStream::pair()?;
    let left_identity = peer_identity(&left)?;
    let right_identity = peer_identity(&right)?;
    let own_pid = std::process::id();
    if left_identity.pid != own_pid || right_identity.pid != own_pid {
        return Err(io::Error::other(
            "SO_PEERPIDFD did not identify this process",
        ));
    }
    Ok(())
}

pub(super) fn duplicate_pidfd(pidfd: BorrowedFd<'_>) -> io::Result<OwnedFd> {
    let duplicated = unsafe { libc::fcntl(pidfd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
    if duplicated == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
    }
}

pub(super) fn peer_identity(stream: &impl AsRawFd) -> io::Result<ProcessIdentity> {
    let socket = stream.as_raw_fd();
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut credentials_len = mem::size_of::<libc::ucred>() as libc::socklen_t;
    let credentials_result = unsafe {
        libc::getsockopt(
            socket,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            std::ptr::addr_of_mut!(credentials).cast(),
            &mut credentials_len,
        )
    };
    if credentials_result == -1 || credentials_len as usize != mem::size_of::<libc::ucred>() {
        return Err(io::Error::last_os_error());
    }

    let mut pidfd = -1;
    let mut pidfd_len = mem::size_of::<libc::c_int>() as libc::socklen_t;
    let pidfd_result = unsafe {
        libc::getsockopt(
            socket,
            libc::SOL_SOCKET,
            libc::SO_PEERPIDFD,
            std::ptr::addr_of_mut!(pidfd).cast(),
            &mut pidfd_len,
        )
    };
    if pidfd_result == -1 || pidfd_len as usize != mem::size_of::<libc::c_int>() || pidfd < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(ProcessIdentity {
        pid: u32::try_from(credentials.pid)
            .map_err(|_| io::Error::other("peer PID was not positive"))?,
        uid: credentials.uid,
        pidfd: unsafe { OwnedFd::from_raw_fd(pidfd) },
    })
}

pub(super) fn pidfd_is_alive(pidfd: &impl AsRawFd) -> io::Result<bool> {
    let mut descriptor = libc::pollfd {
        fd: pidfd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let result = unsafe { libc::poll(std::ptr::addr_of_mut!(descriptor), 1, 0) };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(result == 0)
}

pub(super) fn process_parent(pid: u32) -> io::Result<u32> {
    Ok(read_process_stat(pid)?.parent_pid)
}

pub(super) fn process_start_time(pid: u32) -> io::Result<u64> {
    Ok(read_process_stat(pid)?.start_time)
}

pub(super) fn process_has_ancestor(pid: u32, ancestor: u32) -> io::Result<bool> {
    let mut current = pid;
    for _ in 0..128 {
        if current == ancestor {
            return Ok(true);
        }
        let parent = process_parent(current)?;
        if parent == 0 || parent == current {
            return Ok(false);
        }
        current = parent;
    }
    Ok(false)
}

pub(super) fn process_is_traced(pid: u32) -> io::Result<bool> {
    let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
    let tracer = status
        .lines()
        .find_map(|line| line.strip_prefix("TracerPid:"))
        .ok_or_else(|| io::Error::other("process status omitted TracerPid"))?
        .trim()
        .parse::<u32>()
        .map_err(io::Error::other)?;
    Ok(tracer != 0)
}

pub(super) fn verify_server_peer(
    identity: &ProcessIdentity,
    expected_pid: u32,
    expected_uid: u32,
    expected_start_time: u64,
) -> io::Result<()> {
    if identity.pid != expected_pid
        || identity.uid != expected_uid
        || !pidfd_is_alive(&identity.pidfd)?
        || process_start_time(identity.pid)? != expected_start_time
        || process_is_traced(identity.pid)?
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "sudo_once controller identity did not match",
        ));
    }
    Ok(())
}

pub(super) fn set_nondumpable() -> io::Result<()> {
    let result = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

struct ProcessStat {
    parent_pid: u32,
    start_time: u64,
}

fn read_process_stat(pid: u32) -> io::Result<ProcessStat> {
    let stat = fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat"))?;
    let after_name = stat
        .rsplit_once(')')
        .map(|(_, fields)| fields.trim_start())
        .ok_or_else(|| io::Error::other("invalid process stat"))?;
    let fields = after_name.split_ascii_whitespace().collect::<Vec<_>>();
    let parent_pid = fields
        .get(1)
        .ok_or_else(|| io::Error::other("process stat omitted parent PID"))?
        .parse::<u32>()
        .map_err(io::Error::other)?;
    let start_time = fields
        .get(19)
        .ok_or_else(|| io::Error::other("process stat omitted start time"))?
        .parse::<u64>()
        .map_err(io::Error::other)?;
    Ok(ProcessStat {
        parent_pid,
        start_time,
    })
}
