use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use pretty_assertions::assert_eq;

use super::REQUIRED_SEALS;
use super::SealedSudoExecutable;

const TRUE_PATH: &str = "/bin/true";
const SELF_FD_ZERO_PATH: &str = "/proc/self/fd/0";

#[test]
fn sealed_read_only_fd_zero_remains_executable() {
    let executable =
        SealedSudoExecutable::from_path(Path::new(TRUE_PATH)).expect("seal executable");
    let read_only = executable.open_read_only().expect("read-only executable");

    let status = Command::new(SELF_FD_ZERO_PATH)
        .stdin(Stdio::from(read_only))
        .status()
        .expect("execute sealed fd zero");

    assert_eq!(status.code(), Some(0));
}

#[test]
fn installed_seals_prevent_mutating_the_helper_image() {
    let executable =
        SealedSudoExecutable::from_path(Path::new(TRUE_PATH)).expect("seal executable");
    let installed = unsafe { libc::fcntl(executable.executable.as_raw_fd(), libc::F_GET_SEALS) };
    assert_eq!(installed & REQUIRED_SEALS, REQUIRED_SEALS);

    let mut read_only = executable.open_read_only().expect("read-only executable");
    let error = read_only
        .write_all(b"x")
        .expect_err("sealed read-only view must reject writes");
    assert_eq!(error.raw_os_error(), Some(libc::EBADF));
}
