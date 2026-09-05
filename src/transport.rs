//! Fixed-size UDP control messages for dedicated-server discovery and commands.

use crate::{
    BuildCommand, ExplosionCommand, IVec3, Material, PlayerInputCommand, material::InvalidMaterial,
};
use core::fmt;

const CONTROL_MAGIC: [u8; 4] = *b"DFCT";
const CONTROL_VERSION: u8 = 3;
const HELLO_KIND: u8 = 1;
const WELCOME_KIND: u8 = 2;
const EXPLOSION_KIND: u8 = 3;
const REPAIR_REQUEST_KIND: u8 = 4;
const SNAPSHOT_REQUEST_KIND: u8 = 5;
const SNAPSHOT_FRAGMENTS_REQUEST_KIND: u8 = 6;
const SNAPSHOT_ACK_KIND: u8 = 7;
const BUILD_KIND: u8 = 8;
const PLAYER_INPUT_KIND: u8 = 9;
const HELLO_BYTES: usize = 14;
const WELCOME_BYTES: usize = 22;
const EXPLOSION_BYTES: usize = 40;
const REPAIR_REQUEST_BYTES: usize = 22;
const SNAPSHOT_REQUEST_BYTES: usize = 14;
const SNAPSHOT_FRAGMENTS_REQUEST_BYTES: usize = 32;
const SNAPSHOT_ACK_BYTES: usize = 22;
const BUILD_BYTES: usize = 35;
const PLAYER_INPUT_BYTES: usize = 27;
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
    Build {
        session_id: u64,
        command: BuildCommand,
    },
    PlayerInput {
        session_id: u64,
        input: PlayerInputCommand,
    },
    RepairRequest {
        session_id: u64,
        missing_sequence: u64,
    },
    SnapshotRequest {
        session_id: u64,
    },
    SnapshotFragmentsRequest {
        session_id: u64,
        snapshot_id: u64,
        base_fragment: u16,
        missing_mask: u64,
    },
    SnapshotAck {
        session_id: u64,
        snapshot_id: u64,
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
    InvalidMaterial(InvalidMaterial),
    InvalidPlayerInputFlags(u8),
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
            Self::InvalidMaterial(error) => error.fmt(formatter),
            Self::InvalidPlayerInputFlags(flags) => {
                write!(formatter, "invalid player input flags {flags:#04x}")
            }
        }
    }
}

impl std::error::Error for ControlCodecError {}

impl From<InvalidMaterial> for ControlCodecError {
    fn from(value: InvalidMaterial) -> Self {
        Self::InvalidMaterial(value)
    }
}

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
pub fn encode_build_request(session_id: u64, command: BuildCommand) -> Vec<u8> {
    let mut bytes = control_prefix(BUILD_KIND, BUILD_BYTES);
    push_u64(&mut bytes, session_id);
    push_u64(&mut bytes, command.command_id);
    push_i32(&mut bytes, command.position.x);
    push_i32(&mut bytes, command.position.y);
    push_i32(&mut bytes, command.position.z);
    bytes.push(command.material as u8);
    bytes
}

#[must_use]
pub fn encode_player_input(session_id: u64, input: PlayerInputCommand) -> Vec<u8> {
    let mut bytes = control_prefix(PLAYER_INPUT_KIND, PLAYER_INPUT_BYTES);
    push_u64(&mut bytes, session_id);
    push_u64(&mut bytes, input.input_sequence);
    push_i16(&mut bytes, input.movement_x_per_mille);
    push_i16(&mut bytes, input.movement_z_per_mille);
    bytes.push(u8::from(input.jump) | (u8::from(input.sprint) << 1));
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
pub fn encode_snapshot_fragments_request(
    session_id: u64,
    snapshot_id: u64,
    base_fragment: u16,
    missing_mask: u64,
) -> Vec<u8> {
    let mut bytes = control_prefix(
        SNAPSHOT_FRAGMENTS_REQUEST_KIND,
        SNAPSHOT_FRAGMENTS_REQUEST_BYTES,
    );
    push_u64(&mut bytes, session_id);
    push_u64(&mut bytes, snapshot_id);
    push_u16(&mut bytes, base_fragment);
    push_u64(&mut bytes, missing_mask);
    bytes
}

#[must_use]
pub fn encode_snapshot_ack(session_id: u64, snapshot_id: u64) -> Vec<u8> {
    let mut bytes = control_prefix(SNAPSHOT_ACK_KIND, SNAPSHOT_ACK_BYTES);
    push_u64(&mut bytes, session_id);
    push_u64(&mut bytes, snapshot_id);
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
        BUILD_KIND => {
            require_length(bytes, BUILD_BYTES)?;
            Ok(ClientControlMessage::Build {
                session_id: cursor.take_u64(),
                command: BuildCommand {
                    command_id: cursor.take_u64(),
                    position: IVec3::new(cursor.take_i32(), cursor.take_i32(), cursor.take_i32()),
                    material: Material::from_wire(cursor.take_u8())?,
                },
            })
        }
        PLAYER_INPUT_KIND => {
            require_length(bytes, PLAYER_INPUT_BYTES)?;
            let session_id = cursor.take_u64();
            let input_sequence = cursor.take_u64();
            let motion_x = cursor.take_i16();
            let motion_z = cursor.take_i16();
            let flags = cursor.take_u8();
            if flags & !0b11 != 0 {
                return Err(ControlCodecError::InvalidPlayerInputFlags(flags));
            }
            Ok(ClientControlMessage::PlayerInput {
                session_id,
                input: PlayerInputCommand {
                    input_sequence,
                    movement_x_per_mille: motion_x,
                    movement_z_per_mille: motion_z,
                    jump: flags & 1 != 0,
                    sprint: flags & 2 != 0,
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
        SNAPSHOT_FRAGMENTS_REQUEST_KIND => {
            require_length(bytes, SNAPSHOT_FRAGMENTS_REQUEST_BYTES)?;
            Ok(ClientControlMessage::SnapshotFragmentsRequest {
                session_id: cursor.take_u64(),
                snapshot_id: cursor.take_u64(),
                base_fragment: cursor.take_u16(),
                missing_mask: cursor.take_u64(),
            })
        }
        SNAPSHOT_ACK_KIND => {
            require_length(bytes, SNAPSHOT_ACK_BYTES)?;
            Ok(ClientControlMessage::SnapshotAck {
                session_id: cursor.take_u64(),
                snapshot_id: cursor.take_u64(),
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
    fn take_u8(&mut self) -> u8 {
        self.take_array::<1>()[0]
    }

    fn take_u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take_array())
    }

    fn take_i16(&mut self) -> i16 {
        i16::from_le_bytes(self.take_array())
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

fn push_i16(bytes: &mut Vec<u8>, value: i16) {
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

        let input = PlayerInputCommand {
            input_sequence: 13,
            movement_x_per_mille: 600,
            movement_z_per_mille: -800,
            jump: true,
            sprint: true,
        };
        let request = encode_player_input(9, input);
        assert_eq!(request.len(), PLAYER_INPUT_BYTES);
        assert_eq!(
            decode_client_control(&request),
            Ok(ClientControlMessage::PlayerInput {
                session_id: 9,
                input,
            })
        );

        let build = BuildCommand {
            command_id: 8,
            position: IVec3::new(2, 3, -4),
            material: Material::Wood,
        };
        let request = encode_build_request(9, build);
        assert_eq!(request.len(), BUILD_BYTES);
        assert_eq!(
            decode_client_control(&request),
            Ok(ClientControlMessage::Build {
                session_id: 9,
                command: build,
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

        let fragments = encode_snapshot_fragments_request(9, 12, 64, 0x21);
        assert_eq!(fragments.len(), SNAPSHOT_FRAGMENTS_REQUEST_BYTES);
        assert_eq!(
            decode_client_control(&fragments),
            Ok(ClientControlMessage::SnapshotFragmentsRequest {
                session_id: 9,
                snapshot_id: 12,
                base_fragment: 64,
                missing_mask: 0x21,
            })
        );

        let ack = encode_snapshot_ack(9, 12);
        assert_eq!(ack.len(), SNAPSHOT_ACK_BYTES);
        assert_eq!(
            decode_client_control(&ack),
            Ok(ClientControlMessage::SnapshotAck {
                session_id: 9,
                snapshot_id: 12,
            })
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
        let mut invalid_material = encode_build_request(
            1,
            BuildCommand {
                command_id: 2,
                position: IVec3::default(),
                material: Material::Wood,
            },
        );
        *invalid_material.last_mut().expect("material byte") = u8::MAX;
        assert_eq!(
            decode_client_control(&invalid_material),
            Err(ControlCodecError::InvalidMaterial(InvalidMaterial(u8::MAX)))
        );
        let mut invalid_flags = encode_player_input(
            1,
            PlayerInputCommand {
                input_sequence: 1,
                ..PlayerInputCommand::default()
            },
        );
        *invalid_flags.last_mut().expect("flags byte") = 0b100;
        assert_eq!(
            decode_client_control(&invalid_flags),
            Err(ControlCodecError::InvalidPlayerInputFlags(0b100))
        );
        let mut old_version = encode_client_hello(1);
        old_version[4] = 2;
        assert_eq!(
            decode_client_control(&old_version),
            Err(ControlCodecError::UnsupportedVersion(2))
        );
    }
}
