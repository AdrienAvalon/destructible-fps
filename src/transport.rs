//! Fixed-size UDP control messages for dedicated-server discovery and commands.

use crate::{ExplosionCommand, IVec3};
use core::fmt;

const CONTROL_MAGIC: [u8; 4] = *b"DFCT";
const CONTROL_VERSION: u8 = 1;
const HELLO_KIND: u8 = 1;
const WELCOME_KIND: u8 = 2;
const EXPLOSION_KIND: u8 = 3;
const REPAIR_REQUEST_KIND: u8 = 4;
const SNAPSHOT_REQUEST_KIND: u8 = 5;
const HELLO_BYTES: usize = 14;
const WELCOME_BYTES: usize = 22;
const EXPLOSION_BYTES: usize = 40;
const REPAIR_REQUEST_BYTES: usize = 22;
const SNAPSHOT_REQUEST_BYTES: usize = 14;
pub const MAX_UDP_DATAGRAM_BYTES: usize = 1_200;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientControlMessage {
    Hello {
        nonce: u64,
    },
    Explosion {
        session_id: u64,
        command: ExplosionCommand,
    },
    RepairRequest {
        session_id: u64,
        missing_sequence: u64,
    },
    SnapshotRequest {
        session_id: u64,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerControlMessage {
    Welcome { nonce: u64, session_id: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlCodecError {
    Oversized(usize),
    InvalidLength { expected: usize, actual: usize },
    InvalidMagic,
    UnsupportedVersion(u8),
    InvalidKind(u8),
}

impl fmt::Display for ControlCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Oversized(size) => write!(formatter, "control datagram has {size} bytes"),
            Self::InvalidLength { expected, actual } => write!(
                formatter,
                "control datagram length mismatch: expected {expected}, received {actual}"
            ),
            Self::InvalidMagic => write!(formatter, "invalid control magic"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported control version {version}")
            }
            Self::InvalidKind(kind) => write!(formatter, "invalid control message kind {kind}"),
        }
    }
}

impl std::error::Error for ControlCodecError {}

#[must_use]
pub fn encode_client_hello(nonce: u64) -> Vec<u8> {
    let mut bytes = control_prefix(HELLO_KIND, HELLO_BYTES);
    push_u64(&mut bytes, nonce);
    bytes
}

#[must_use]
pub fn encode_explosion_request(session_id: u64, command: ExplosionCommand) -> Vec<u8> {
    let mut bytes = control_prefix(EXPLOSION_KIND, EXPLOSION_BYTES);
    push_u64(&mut bytes, session_id);
    push_u64(&mut bytes, command.command_id);
    push_i32(&mut bytes, command.center.x);
    push_i32(&mut bytes, command.center.y);
    push_i32(&mut bytes, command.center.z);
    push_u16(&mut bytes, command.radius_voxels);
    push_u32(&mut bytes, command.peak_energy);
    bytes
}

#[must_use]
pub fn encode_repair_request(session_id: u64, missing_sequence: u64) -> Vec<u8> {
    let mut bytes = control_prefix(REPAIR_REQUEST_KIND, REPAIR_REQUEST_BYTES);
    push_u64(&mut bytes, session_id);
    push_u64(&mut bytes, missing_sequence);
    bytes
}

#[must_use]
pub fn encode_snapshot_request(session_id: u64) -> Vec<u8> {
    let mut bytes = control_prefix(SNAPSHOT_REQUEST_KIND, SNAPSHOT_REQUEST_BYTES);
    push_u64(&mut bytes, session_id);
    bytes
}

#[must_use]
pub fn encode_server_welcome(nonce: u64, session_id: u64) -> Vec<u8> {
    let mut bytes = control_prefix(WELCOME_KIND, WELCOME_BYTES);
    push_u64(&mut bytes, nonce);
    push_u64(&mut bytes, session_id);
    bytes
}

/// Decodes one allocation-free client control datagram.
///
/// # Errors
///
/// Rejects oversized, malformed, unsupported, or unknown messages.
pub fn decode_client_control(bytes: &[u8]) -> Result<ClientControlMessage, ControlCodecError> {
    let mut cursor = decode_prefix(bytes)?;
    match cursor.kind {
        HELLO_KIND => {
            require_length(bytes, HELLO_BYTES)?;
            Ok(ClientControlMessage::Hello {
                nonce: cursor.take_u64(),
            })
        }
        EXPLOSION_KIND => {
            require_length(bytes, EXPLOSION_BYTES)?;
            Ok(ClientControlMessage::Explosion {
                session_id: cursor.take_u64(),
                command: ExplosionCommand {
                    command_id: cursor.take_u64(),
                    center: IVec3::new(cursor.take_i32(), cursor.take_i32(), cursor.take_i32()),
                    radius_voxels: cursor.take_u16(),
                    peak_energy: cursor.take_u32(),
                },
            })
        }
        REPAIR_REQUEST_KIND => {
            require_length(bytes, REPAIR_REQUEST_BYTES)?;
            Ok(ClientControlMessage::RepairRequest {
                session_id: cursor.take_u64(),
                missing_sequence: cursor.take_u64(),
            })
        }
        SNAPSHOT_REQUEST_KIND => {
            require_length(bytes, SNAPSHOT_REQUEST_BYTES)?;
            Ok(ClientControlMessage::SnapshotRequest {
                session_id: cursor.take_u64(),
            })
        }
        kind => Err(ControlCodecError::InvalidKind(kind)),
    }
}

/// Decodes one fixed-size server control response.
///
/// # Errors
///
/// Rejects oversized, malformed, unsupported, or unknown messages.
pub fn decode_server_control(bytes: &[u8]) -> Result<ServerControlMessage, ControlCodecError> {
    let mut cursor = decode_prefix(bytes)?;
    match cursor.kind {
        WELCOME_KIND => {
            require_length(bytes, WELCOME_BYTES)?;
            Ok(ServerControlMessage::Welcome {
                nonce: cursor.take_u64(),
                session_id: cursor.take_u64(),
            })
        }
        kind => Err(ControlCodecError::InvalidKind(kind)),
    }
}

#[must_use]
pub fn is_delta_datagram(bytes: &[u8]) -> bool {
    bytes.starts_with(b"DFPS")
}

fn control_prefix(kind: u8, capacity: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(&CONTROL_MAGIC);
    bytes.push(CONTROL_VERSION);
    bytes.push(kind);
    bytes
}

const fn require_length(bytes: &[u8], expected: usize) -> Result<(), ControlCodecError> {
    if bytes.len() != expected {
        return Err(ControlCodecError::InvalidLength {
            expected,
            actual: bytes.len(),
        });
    }
    Ok(())
}

fn decode_prefix(bytes: &[u8]) -> Result<ControlCursor<'_>, ControlCodecError> {
    if bytes.len() > MAX_UDP_DATAGRAM_BYTES {
        return Err(ControlCodecError::Oversized(bytes.len()));
    }
    if bytes.len() < 6 {
        return Err(ControlCodecError::InvalidLength {
            expected: 6,
            actual: bytes.len(),
        });
    }
    if bytes[..4] != CONTROL_MAGIC {
        return Err(ControlCodecError::InvalidMagic);
    }
    if bytes[4] != CONTROL_VERSION {
        return Err(ControlCodecError::UnsupportedVersion(bytes[4]));
    }
    Ok(ControlCursor {
        bytes,
        offset: 6,
        kind: bytes[5],
    })
}

struct ControlCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
    kind: u8,
}

impl ControlCursor<'_> {
    fn take_u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take_array())
    }

    fn take_u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take_array())
    }

    fn take_u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take_array())
    }

    fn take_i32(&mut self) -> i32 {
        i32::from_le_bytes(self.take_array())
    }

    fn take_array<const N: usize>(&mut self) -> [u8; N] {
        let end = self.offset + N;
        let result = self.bytes[self.offset..end]
            .try_into()
            .expect("message length was validated before field decoding");
        self.offset = end;
        result
    }
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_messages_round_trip_at_exact_sizes() {
        let hello = encode_client_hello(42);
        assert_eq!(hello.len(), HELLO_BYTES);
        assert_eq!(
            decode_client_control(&hello),
            Ok(ClientControlMessage::Hello { nonce: 42 })
        );

        let command = ExplosionCommand {
            command_id: 7,
            center: IVec3::new(-2, 3, 4),
            radius_voxels: 5,
            peak_energy: 6,
        };
        let request = encode_explosion_request(9, command);
        assert_eq!(request.len(), EXPLOSION_BYTES);
        assert_eq!(
            decode_client_control(&request),
            Ok(ClientControlMessage::Explosion {
                session_id: 9,
                command,
            })
        );

        let welcome = encode_server_welcome(42, 9);
        assert_eq!(welcome.len(), WELCOME_BYTES);
        assert_eq!(
            decode_server_control(&welcome),
            Ok(ServerControlMessage::Welcome {
                nonce: 42,
                session_id: 9,
            })
        );

        let repair = encode_repair_request(9, 11);
        assert_eq!(repair.len(), REPAIR_REQUEST_BYTES);
        assert_eq!(
            decode_client_control(&repair),
            Ok(ClientControlMessage::RepairRequest {
                session_id: 9,
                missing_sequence: 11,
            })
        );

        let snapshot = encode_snapshot_request(9);
        assert_eq!(snapshot.len(), SNAPSHOT_REQUEST_BYTES);
        assert_eq!(
            decode_client_control(&snapshot),
            Ok(ClientControlMessage::SnapshotRequest { session_id: 9 })
        );
    }

    #[test]
    fn malformed_control_messages_fail_before_field_reads() {
        assert!(matches!(
            decode_client_control(&[0; MAX_UDP_DATAGRAM_BYTES + 1]),
            Err(ControlCodecError::Oversized(_))
        ));
        let mut truncated = encode_explosion_request(
            1,
            ExplosionCommand {
                command_id: 1,
                center: IVec3::new(0, 0, 0),
                radius_voxels: 1,
                peak_energy: 1,
            },
        );
        truncated.pop();
        assert!(matches!(
            decode_client_control(&truncated),
            Err(ControlCodecError::InvalidLength { .. })
        ));
    }
}
