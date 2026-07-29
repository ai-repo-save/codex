use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use pretty_assertions::assert_eq;

use super::REQUIRED_SEALS;
use super::SealedSudoExecutable;

const TRUE_PATH: &str = "/bin/true";
const SELF_FD_ZERO_PATH: &str = "/proc/self/fd/0";
const DUMPABILITY_PROBE_ENV: &str = "CODEX_SUDO_MEMFD_DUMPABILITY_PROBE";
const DUMPABILITY_PROBE_TEST: &str =
    "linux::sealed_executable::tests::execute_only_helper_is_not_user_dumpable_probe";

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
    let installed = unsafe { libc::fcntl(executable.read_only.as_raw_fd(), libc::F_GET_SEALS) };
    assert_eq!(installed & REQUIRED_SEALS, REQUIRED_SEALS);
    assert_eq!(
        executable
            .read_only
            .metadata()
            .expect("sealed metadata")
            .permissions()
            .mode()
            & 0o777,
        0o100
    );

    let mut read_only = executable.open_read_only().expect("read-only executable");
    let error = read_only
        .write_all(b"x")
        .expect_err("sealed read-only view must reject writes");
    assert_eq!(error.raw_os_error(), Some(libc::EBADF));
}

#[test]
fn execute_only_helper_is_not_user_dumpable() {
    let executable =
        SealedSudoExecutable::from_path(Path::new("/proc/self/exe")).expect("seal test executable");
    let read_only = executable.open_read_only().expect("read-only executable");

    let status = Command::new(SELF_FD_ZERO_PATH)
        .stdin(Stdio::from(read_only))
        .arg("--exact")
        .arg(DUMPABILITY_PROBE_TEST)
        .env(DUMPABILITY_PROBE_ENV, "1")
        .status()
        .expect("execute dumpability probe");

    assert_eq!(status.code(), Some(0));
}

#[test]
fn execute_only_helper_is_not_user_dumpable_probe() {
    if std::env::var_os(DUMPABILITY_PROBE_ENV).is_none() {
        return;
    }

    let dumpable = unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
    assert!(matches!(dumpable, 0 | 2));
}
