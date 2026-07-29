use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use pretty_assertions::assert_eq;

use super::auth;
use super::wire;
use super::wire::CommandFrame;
use super::wire::Frame;
use super::wire::FrameKind;

const COMMAND: [&str; 3] = ["/usr/bin/printf", "", "%s"];

#[test]
fn command_frame_preserves_the_exact_approved_arguments() {
    let encoded = wire::encode_command(
        std::ffi::OsStr::new("/var/empty"),
        &COMMAND.map(str::to_string),
    )
    .expect("encode command");

    assert_eq!(
        wire::decode_command(&encoded).expect("decode command"),
        CommandFrame {
            cwd: PathBuf::from("/var/empty"),
            argv: COMMAND.map(Into::into).into(),
        }
    );
}

#[test]
fn peer_identity_cannot_be_rebound_to_a_different_controller_pid() {
    let (stream, _peer) = UnixStream::pair().expect("socket pair");
    let identity = auth::peer_identity(&stream).expect("peer identity");
    let wrong_pid = identity.pid.saturating_add(1);

    let error = auth::verify_server_peer(
        &identity,
        wrong_pid,
        identity.uid,
        auth::process_start_time(identity.pid).expect("peer start time"),
    )
    .expect_err("wrong controller PID must be rejected");

    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn credential_frame_rejects_payloads_larger_than_askpass_can_accept() {
    let frame = Frame {
        kind: FrameKind::Credential,
        payload: vec![0_u8; wire::MAX_CREDENTIAL_LEN + 1],
    };

    let error = wire::take_credential(frame).expect_err("oversized credential must be rejected");

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}
