use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::path::Path;

const SUID_DUMPABLE_PATH: &str = "/proc/sys/fs/suid_dumpable";
const REQUIRED_SEALS: libc::c_int = libc::F_SEAL_WRITE
    | libc::F_SEAL_FUTURE_WRITE
    | libc::F_SEAL_GROW
    | libc::F_SEAL_SHRINK
    | libc::F_SEAL_EXEC
    | libc::F_SEAL_SEAL;

/// An immutable executable copy of the running Codex binary.
///
/// The read-only view can be installed as a sudo process's fd 0. Sudo's
/// askpass child preserves that descriptor while closing descriptors above
/// stderr, so `/proc/self/fd/0` remains a stable executable path after sudo
/// drops the child back to the original user.
pub struct SealedSudoExecutable {
    read_only: File,
}

impl SealedSudoExecutable {
    pub fn from_current_executable() -> io::Result<Self> {
        require_secure_exec_dumpability()?;
        Self::from_path(Path::new("/proc/self/exe"))
    }

    /// Opens a read-only descriptor suitable for installation as child fd 0.
    pub fn open_read_only(&self) -> io::Result<File> {
        self.read_only.try_clone()
    }

    fn from_path(path: &Path) -> io::Result<Self> {
        let mut source = File::open(path)?;
        let name = CString::new("codex-sudo-helper").expect("static memfd name");
        let descriptor = unsafe {
            libc::memfd_create(
                name.as_ptr(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING | libc::MFD_EXEC,
            )
        };
        if descriptor == -1 {
            return Err(io::Error::last_os_error());
        }
        let mut executable = unsafe { File::from_raw_fd(descriptor) };
        io::copy(&mut source, &mut executable)?;
        executable.sync_all()?;
        let read_only = File::open(format!("/proc/self/fd/{}", executable.as_raw_fd()))?;
        if unsafe { libc::fchmod(executable.as_raw_fd(), 0o100) } == -1 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::fcntl(executable.as_raw_fd(), libc::F_ADD_SEALS, REQUIRED_SEALS) } == -1 {
            return Err(io::Error::last_os_error());
        }
        let installed_seals = unsafe { libc::fcntl(executable.as_raw_fd(), libc::F_GET_SEALS) };
        if installed_seals == -1 {
            return Err(io::Error::last_os_error());
        }
        if installed_seals & REQUIRED_SEALS != REQUIRED_SEALS {
            return Err(io::Error::other(
                "kernel did not install every required executable seal",
            ));
        }
        drop(executable);
        Ok(Self { read_only })
    }
}

fn require_secure_exec_dumpability() -> io::Result<()> {
    match std::fs::read_to_string(SUID_DUMPABLE_PATH)?
        .trim()
        .parse::<u8>()
        .map_err(io::Error::other)?
    {
        0 | 2 => Ok(()),
        1 => Err(io::Error::other(
            "execute-only sudo helper would remain ptraceable",
        )),
        _ => Err(io::Error::other("invalid fs.suid_dumpable value")),
    }
}

#[cfg(test)]
#[path = "sealed_executable_tests.rs"]
mod tests;
