use std::ffi::OsStr;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use zeroize::Zeroizing;

const MAGIC: [u8; 8] = *b"CDXSUDO1";
const HEADER_LEN: usize = 14;
const MAX_FRAME_LEN: usize = 1024 * 1024;
const MAX_ARGUMENTS: usize = 4096;
const MAX_ELEMENT_LEN: usize = 64 * 1024;
pub(super) const NONCE_LEN: usize = 32;
pub(super) const MAX_CREDENTIAL_LEN: usize = 1023;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(super) enum FrameKind {
    AskpassHello = 1,
    SupervisorHello = 2,
    Credential = 10,
    Cancel = 11,
    Start = 12,
    Started = 20,
    Exited = 21,
    Failed = 22,
    Stopped = 23,
}

impl TryFrom<u8> for FrameKind {
    type Error = io::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::AskpassHello),
            2 => Ok(Self::SupervisorHello),
            10 => Ok(Self::Credential),
            11 => Ok(Self::Cancel),
            12 => Ok(Self::Start),
            20 => Ok(Self::Started),
            21 => Ok(Self::Exited),
            22 => Ok(Self::Failed),
            23 => Ok(Self::Stopped),
            _ => Err(protocol_error("unknown sudo_once frame kind")),
        }
    }
}

pub(super) struct Frame {
    pub kind: FrameKind,
    pub payload: Vec<u8>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct CommandFrame {
    pub cwd: PathBuf,
    pub argv: Vec<OsString>,
}

pub(super) struct CredentialBytes(Zeroizing<Vec<u8>>);

impl CredentialBytes {
    pub fn expose_secret(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for CredentialBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CredentialBytes([REDACTED])")
    }
}

pub(super) async fn write_frame_async(
    stream: &mut (impl AsyncWrite + Unpin),
    kind: FrameKind,
    payload: &[u8],
) -> io::Result<()> {
    let header = encode_header(kind, payload.len())?;
    stream.write_all(&header).await?;
    stream.write_all(payload).await?;
    stream.flush().await
}

pub(super) async fn read_frame_async(
    stream: &mut (impl AsyncRead + Unpin),
) -> io::Result<Frame> {
    let mut header = [0_u8; HEADER_LEN];
    stream.read_exact(&mut header).await?;
    let (kind, payload_len) = decode_header(header)?;
    let mut payload = vec![0_u8; payload_len];
    stream.read_exact(&mut payload).await?;
    Ok(Frame { kind, payload })
}

pub(super) fn write_frame(
    stream: &mut impl Write,
    kind: FrameKind,
    payload: &[u8],
) -> io::Result<()> {
    let header = encode_header(kind, payload.len())?;
    stream.write_all(&header)?;
    stream.write_all(payload)?;
    stream.flush()
}

pub(super) fn read_frame(stream: &mut impl Read) -> io::Result<Frame> {
    let mut header = [0_u8; HEADER_LEN];
    stream.read_exact(&mut header)?;
    let (kind, payload_len) = decode_header(header)?;
    let mut payload = vec![0_u8; payload_len];
    stream.read_exact(&mut payload)?;
    Ok(Frame { kind, payload })
}

pub(super) async fn write_credential_async(
    stream: &mut (impl AsyncWrite + Unpin),
    credential: &[u8],
) -> io::Result<()> {
    if credential.len() > MAX_CREDENTIAL_LEN {
        return Err(protocol_error("sudo credential exceeded the maximum length"));
    }
    write_frame_async(stream, FrameKind::Credential, credential).await
}

pub(super) fn take_credential(frame: Frame) -> io::Result<CredentialBytes> {
    if frame.kind != FrameKind::Credential
        || frame.payload.len() > MAX_CREDENTIAL_LEN
        || frame
            .payload
            .iter()
            .any(|byte| matches!(*byte, b'\0' | b'\r' | b'\n'))
    {
        return Err(protocol_error("invalid sudo credential frame"));
    }
    Ok(CredentialBytes(Zeroizing::new(frame.payload)))
}

pub(super) fn encode_command(cwd: &OsStr, argv: &[String]) -> io::Result<Vec<u8>> {
    if argv.is_empty() || argv.len() > MAX_ARGUMENTS {
        return Err(protocol_error("invalid sudo command argument count"));
    }
    let cwd = cwd.as_bytes();
    validate_element(cwd, /*allow_empty*/ false)?;
    let mut payload = Vec::new();
    append_element(&mut payload, cwd, /*allow_empty*/ false)?;
    append_u32(&mut payload, argv.len())?;
    for (index, argument) in argv.iter().enumerate() {
        append_element(&mut payload, argument.as_bytes(), /*allow_empty*/ index != 0)?;
    }
    if payload.len() > MAX_FRAME_LEN {
        return Err(protocol_error("sudo command frame was too large"));
    }
    Ok(payload)
}

pub(super) fn decode_command(payload: &[u8]) -> io::Result<CommandFrame> {
    let mut cursor = Cursor::new(payload);
    let cwd = PathBuf::from(OsString::from_vec(cursor.take_element(/*allow_empty*/ false)?));
    let argument_count = cursor.take_u32()?;
    if argument_count == 0 || argument_count > MAX_ARGUMENTS {
        return Err(protocol_error("invalid sudo command argument count"));
    }
    let mut argv = Vec::with_capacity(argument_count);
    for index in 0..argument_count {
        argv.push(OsString::from_vec(
            cursor.take_element(/*allow_empty*/ index != 0)?,
        ));
    }
    if !cursor.is_empty() {
        return Err(protocol_error("sudo command frame had trailing bytes"));
    }
    Ok(CommandFrame { cwd, argv })
}

pub(super) fn encode_exit_status(exit_code: i32) -> [u8; 4] {
    exit_code.to_be_bytes()
}

pub(super) fn decode_exit_status(payload: &[u8]) -> io::Result<i32> {
    let bytes: [u8; 4] = payload
        .try_into()
        .map_err(|_| protocol_error("invalid sudo exit status frame"))?;
    Ok(i32::from_be_bytes(bytes))
}

fn encode_header(kind: FrameKind, payload_len: usize) -> io::Result<[u8; HEADER_LEN]> {
    if payload_len > MAX_FRAME_LEN {
        return Err(protocol_error("sudo_once frame was too large"));
    }
    let mut header = [0_u8; HEADER_LEN];
    header[..MAGIC.len()].copy_from_slice(&MAGIC);
    header[8] = 1;
    header[9] = kind as u8;
    header[10..].copy_from_slice(
        &u32::try_from(payload_len)
            .map_err(|_| protocol_error("sudo_once frame was too large"))?
            .to_be_bytes(),
    );
    Ok(header)
}

fn decode_header(header: [u8; HEADER_LEN]) -> io::Result<(FrameKind, usize)> {
    if header[..MAGIC.len()] != MAGIC || header[8] != 1 {
        return Err(protocol_error("invalid sudo_once frame header"));
    }
    let kind = FrameKind::try_from(header[9])?;
    let payload_len =
        u32::from_be_bytes(header[10..].try_into().expect("fixed header length")) as usize;
    if payload_len > MAX_FRAME_LEN {
        return Err(protocol_error("sudo_once frame was too large"));
    }
    Ok((kind, payload_len))
}

fn append_element(
    payload: &mut Vec<u8>,
    element: &[u8],
    allow_empty: bool,
) -> io::Result<()> {
    validate_element(element, allow_empty)?;
    append_u32(payload, element.len())?;
    payload.extend_from_slice(element);
    Ok(())
}

fn append_u32(payload: &mut Vec<u8>, value: usize) -> io::Result<()> {
    payload.extend_from_slice(
        &u32::try_from(value)
            .map_err(|_| protocol_error("sudo_once frame element was too large"))?
            .to_be_bytes(),
    );
    Ok(())
}

fn validate_element(element: &[u8], allow_empty: bool) -> io::Result<()> {
    if (!allow_empty && element.is_empty())
        || element.len() > MAX_ELEMENT_LEN
        || element.contains(&0)
    {
        return Err(protocol_error("invalid sudo command element"));
    }
    Ok(())
}

fn protocol_error(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

struct Cursor<'a> {
    payload: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(payload: &'a [u8]) -> Self {
        Self { payload, offset: 0 }
    }

    fn take_u32(&mut self) -> io::Result<usize> {
        let end = self
            .offset
            .checked_add(4)
            .ok_or_else(|| protocol_error("sudo command frame overflowed"))?;
        let bytes: [u8; 4] = self
            .payload
            .get(self.offset..end)
            .ok_or_else(|| protocol_error("sudo command frame was truncated"))?
            .try_into()
            .expect("four byte slice");
        self.offset = end;
        Ok(u32::from_be_bytes(bytes) as usize)
    }

    fn take_element(&mut self, allow_empty: bool) -> io::Result<Vec<u8>> {
        let length = self.take_u32()?;
        if (!allow_empty && length == 0) || length > MAX_ELEMENT_LEN {
            return Err(protocol_error("invalid sudo command element"));
        }
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| protocol_error("sudo command frame overflowed"))?;
        let element = self
            .payload
            .get(self.offset..end)
            .ok_or_else(|| protocol_error("sudo command frame was truncated"))?;
        if element.contains(&0) {
            return Err(protocol_error("invalid sudo command element"));
        }
        self.offset = end;
        Ok(element.to_vec())
    }

    fn is_empty(&self) -> bool {
        self.offset == self.payload.len()
    }
}
