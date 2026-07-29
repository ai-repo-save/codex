use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::path::Path;

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
    executable: File,
}

impl SealedSudoExecutable {
    pub fn from_current_executable() -> io::Result<Self> {
        Self::from_path(Path::new("/proc/self/exe"))
    }

    /// Opens a read-only descriptor suitable for installation as child fd 0.
    pub fn open_read_only(&self) -> io::Result<File> {
        File::open(format!("/proc/self/fd/{}", self.executable.as_raw_fd()))
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
        if unsafe { libc::fchmod(executable.as_raw_fd(), 0o500) } == -1 {
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
        Ok(Self { executable })
    }
}

#[cfg(test)]
#[path = "sealed_executable_tests.rs"]
mod tests;
