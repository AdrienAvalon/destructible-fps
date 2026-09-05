//! Bounded lossy replication of authoritative player motion.

use crate::{AuthoritativePlayerState, FixedMicrometers3};
use core::fmt;
use std::collections::BTreeMap;

const PLAYER_STATE_MAGIC: [u8; 4] = *b"DFPL";
const PLAYER_STATE_VERSION: u8 = 1;
const PLAYER_STATE_HEADER_BYTES: usize = 4 + 1 + 8 + 1;
const PLAYER_STATE_ENTRY_BYTES: usize = 8 + 3 * 8 + 3 * 8 + 8 + 1;
const GROUNDED_FLAG: u8 = 1;
pub const MAX_REPLICATED_PLAYERS: usize = 16;
pub const PLAYER_STATE_BROADCAST_HZ: u64 = 20;
pub const PLAYER_STATE_BROADCAST_INTERVAL_TICKS: u64 = 3;
pub const MAX_PLAYER_STATE_DATAGRAM_BYTES: usize =
    PLAYER_STATE_HEADER_BYTES + MAX_REPLICATED_PLAYERS * PLAYER_STATE_ENTRY_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplicatedPlayerState {
    pub session_id: u64,
    pub position_um: FixedMicrometers3,
    pub velocity_um_per_second: FixedMicrometers3,
    pub grounded: bool,
    pub last_input_sequence: u64,
}

impl ReplicatedPlayerState {
    #[must_use]
    pub const fn from_authoritative(session_id: u64, state: AuthoritativePlayerState) -> Self {
        Self {
            session_id,
            position_um: state.position_um,
            velocity_um_per_second: state.velocity_um_per_second,
            grounded: state.grounded,
            last_input_sequence: state.last_input_sequence,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerStatePacket {
    pub server_tick: u64,
    pub players: Vec<ReplicatedPlayerState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerStateCodecError {
    Oversized(usize),
    InvalidLength { expected: usize, actual: usize },
    InvalidMagic,
    UnsupportedVersion(u8),
    InvalidServerTick,
    TooManyPlayers(usize),
    InvalidSession,
    UnorderedSession { previous: u64, current: u64 },
    InvalidFlags(u8),
}

impl fmt::Display for PlayerStateCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Oversized(bytes) => write!(formatter, "player-state datagram has {bytes} bytes"),
            Self::InvalidLength { expected, actual } => write!(
                formatter,
                "player-state datagram length mismatch: expected {expected}, received {actual}"
            ),
            Self::InvalidMagic => write!(formatter, "invalid player-state magic"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported player-state version {version}")
            }
            Self::InvalidServerTick => {
                write!(formatter, "player-state server tick must be non-zero")
            }
            Self::TooManyPlayers(players) => {
                write!(formatter, "player-state packet contains {players} players")
            }
            Self::InvalidSession => write!(formatter, "player-state session ID must be non-zero"),
            Self::UnorderedSession { previous, current } => write!(
                formatter,
                "player-state sessions are not strictly ordered: {previous} then {current}"
            ),
            Self::InvalidFlags(flags) => {
                write!(formatter, "invalid player-state flags {flags:#04x}")
            }
        }
    }
}

impl std::error::Error for PlayerStateCodecError {}

/// Encodes one complete, sorted authoritative player-state view.
///
/// # Errors
///
/// Rejects a zero tick, too many players, zero or unordered session IDs.
pub fn encode_player_state_packet(
    server_tick: u64,
    players: &[ReplicatedPlayerState],
) -> Result<Vec<u8>, PlayerStateCodecError> {
    validate_header(server_tick, players.len())?;
    validate_player_order(players)?;
    let mut bytes =
        Vec::with_capacity(PLAYER_STATE_HEADER_BYTES + players.len() * PLAYER_STATE_ENTRY_BYTES);
    bytes.extend_from_slice(&PLAYER_STATE_MAGIC);
    bytes.push(PLAYER_STATE_VERSION);
    push_u64(&mut bytes, server_tick);
    let player_count = u8::try_from(players.len())
        .map_err(|_error| PlayerStateCodecError::TooManyPlayers(players.len()))?;
    bytes.push(player_count);
    for player in players {
        push_u64(&mut bytes, player.session_id);
        push_fixed(&mut bytes, player.position_um);
        push_fixed(&mut bytes, player.velocity_um_per_second);
        push_u64(&mut bytes, player.last_input_sequence);
        bytes.push(u8::from(player.grounded));
    }
    Ok(bytes)
}

/// Decodes one complete player-state datagram with bounded allocation.
///
/// # Errors
///
/// Rejects oversized, malformed, unsupported, or non-canonical packets.
pub fn decode_player_state_packet(
    bytes: &[u8],
) -> Result<PlayerStatePacket, PlayerStateCodecError> {
    if bytes.len() > MAX_PLAYER_STATE_DATAGRAM_BYTES {
        return Err(PlayerStateCodecError::Oversized(bytes.len()));
    }
    if bytes.len() < PLAYER_STATE_HEADER_BYTES {
        return Err(PlayerStateCodecError::InvalidLength {
            expected: PLAYER_STATE_HEADER_BYTES,
            actual: bytes.len(),
        });
    }
    if bytes[..4] != PLAYER_STATE_MAGIC {
        return Err(PlayerStateCodecError::InvalidMagic);
    }
    if bytes[4] != PLAYER_STATE_VERSION {
        return Err(PlayerStateCodecError::UnsupportedVersion(bytes[4]));
    }
    let mut cursor = Cursor { bytes, offset: 5 };
    let server_tick = cursor.take_u64();
    let player_count = usize::from(cursor.take_u8());
    validate_header(server_tick, player_count)?;
    let expected = PLAYER_STATE_HEADER_BYTES + player_count * PLAYER_STATE_ENTRY_BYTES;
    if bytes.len() != expected {
        return Err(PlayerStateCodecError::InvalidLength {
            expected,
            actual: bytes.len(),
        });
    }
    let mut players = Vec::with_capacity(player_count);
    for _ in 0..player_count {
        let session_id = cursor.take_u64();
        let position_um = cursor.take_fixed();
        let velocity_um_per_second = cursor.take_fixed();
        let last_input_sequence = cursor.take_u64();
        let flags = cursor.take_u8();
        if flags & !GROUNDED_FLAG != 0 {
            return Err(PlayerStateCodecError::InvalidFlags(flags));
        }
        players.push(ReplicatedPlayerState {
            session_id,
            position_um,
            velocity_um_per_second,
            grounded: flags & GROUNDED_FLAG != 0,
            last_input_sequence,
        });
    }
    validate_player_order(&players)?;
    Ok(PlayerStatePacket {
        server_tick,
        players,
    })
}

#[must_use]
pub fn is_player_state_datagram(bytes: &[u8]) -> bool {
    bytes.starts_with(&PLAYER_STATE_MAGIC)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlayerStateApplyReport {
    pub server_tick: u64,
    pub joined_players: usize,
    pub updated_players: usize,
    pub removed_players: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerStateReceiveError {
    Codec(PlayerStateCodecError),
    StaleServerTick { received: u64, last_applied: u64 },
}

impl fmt::Display for PlayerStateReceiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => error.fmt(formatter),
            Self::StaleServerTick {
                received,
                last_applied,
            } => write!(
                formatter,
                "player-state tick {received} is not newer than {last_applied}"
            ),
        }
    }
}

impl std::error::Error for PlayerStateReceiveError {}

impl From<PlayerStateCodecError> for PlayerStateReceiveError {
    fn from(value: PlayerStateCodecError) -> Self {
        Self::Codec(value)
    }
}

/// Client-side latest-wins player state. Missing ticks never stall later motion.
#[derive(Debug, Default)]
pub struct PlayerStateInbox {
    last_server_tick: u64,
    players: BTreeMap<u64, ReplicatedPlayerState>,
}

impl PlayerStateInbox {
    /// Atomically installs a newer complete player set.
    ///
    /// # Errors
    ///
    /// Rejects malformed or stale/replayed datagrams without changing the installed view.
    pub fn receive(
        &mut self,
        bytes: &[u8],
    ) -> Result<PlayerStateApplyReport, PlayerStateReceiveError> {
        let packet = decode_player_state_packet(bytes)?;
        if packet.server_tick <= self.last_server_tick {
            return Err(PlayerStateReceiveError::StaleServerTick {
                received: packet.server_tick,
                last_applied: self.last_server_tick,
            });
        }
        let next = packet
            .players
            .into_iter()
            .map(|player| (player.session_id, player))
            .collect::<BTreeMap<_, _>>();
        let joined_players = next
            .keys()
            .filter(|session_id| !self.players.contains_key(session_id))
            .count();
        let updated_players = next
            .keys()
            .filter(|session_id| self.players.contains_key(session_id))
            .count();
        let removed_players = self
            .players
            .keys()
            .filter(|session_id| !next.contains_key(session_id))
            .count();
        self.last_server_tick = packet.server_tick;
        self.players = next;
        Ok(PlayerStateApplyReport {
            server_tick: packet.server_tick,
            joined_players,
            updated_players,
            removed_players,
        })
    }

    #[must_use]
    pub const fn last_server_tick(&self) -> u64 {
        self.last_server_tick
    }

    #[must_use]
    pub fn player(&self, session_id: u64) -> Option<ReplicatedPlayerState> {
        self.players.get(&session_id).copied()
    }

    #[must_use]
    pub fn players(&self) -> impl ExactSizeIterator<Item = ReplicatedPlayerState> + '_ {
        self.players.values().copied()
    }
}

const fn validate_header(
    server_tick: u64,
    player_count: usize,
) -> Result<(), PlayerStateCodecError> {
    if server_tick == 0 {
        return Err(PlayerStateCodecError::InvalidServerTick);
    }
    if player_count > MAX_REPLICATED_PLAYERS {
        return Err(PlayerStateCodecError::TooManyPlayers(player_count));
    }
    Ok(())
}

fn validate_player_order(players: &[ReplicatedPlayerState]) -> Result<(), PlayerStateCodecError> {
    let mut previous = 0_u64;
    for player in players {
        if player.session_id == 0 {
            return Err(PlayerStateCodecError::InvalidSession);
        }
        if player.session_id <= previous {
            return Err(PlayerStateCodecError::UnorderedSession {
                previous,
                current: player.session_id,
            });
        }
        previous = player.session_id;
    }
    Ok(())
}

fn push_fixed(bytes: &mut Vec<u8>, value: FixedMicrometers3) {
    push_i64(bytes, value.x);
    push_i64(bytes, value.y);
    push_i64(bytes, value.z);
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_i64(bytes: &mut Vec<u8>, value: i64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Cursor<'_> {
    fn take_u8(&mut self) -> u8 {
        self.take_array::<1>()[0]
    }

    fn take_u64(&mut self) -> u64 {
        u64::from_le_bytes(self.take_array())
    }

    fn take_i64(&mut self) -> i64 {
        i64::from_le_bytes(self.take_array())
    }

    fn take_fixed(&mut self) -> FixedMicrometers3 {
        FixedMicrometers3 {
            x: self.take_i64(),
            y: self.take_i64(),
            z: self.take_i64(),
        }
    }

    fn take_array<const N: usize>(&mut self) -> [u8; N] {
        let end = self.offset + N;
        let result = self.bytes[self.offset..end]
            .try_into()
            .expect("packet length and player count were validated before field decoding");
        self.offset = end;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(session_id: u64, offset: i64) -> ReplicatedPlayerState {
        ReplicatedPlayerState {
            session_id,
            position_um: FixedMicrometers3 {
                x: offset,
                y: offset + 1,
                z: offset + 2,
            },
            velocity_um_per_second: FixedMicrometers3 {
                x: offset + 3,
                y: offset + 4,
                z: offset + 5,
            },
            grounded: session_id.is_multiple_of(2),
            last_input_sequence: session_id + 20,
        }
    }

    #[test]
    fn maximum_packet_fits_one_secure_gameplay_datagram() {
        let players = (1..=MAX_REPLICATED_PLAYERS)
            .map(|session_id| player(u64::try_from(session_id).expect("small ID"), 7))
            .collect::<Vec<_>>();
        let encoded = encode_player_state_packet(91, &players).expect("maximum player packet");

        assert_eq!(encoded.len(), MAX_PLAYER_STATE_DATAGRAM_BYTES);
        assert!(encoded.len() <= crate::MAX_QUIC_DATAGRAM_PAYLOAD_BYTES);
        let simulation_hz = u64::try_from(crate::SERVER_PHYSICS_HZ).expect("positive server Hz");
        assert_eq!(
            simulation_hz / PLAYER_STATE_BROADCAST_INTERVAL_TICKS,
            PLAYER_STATE_BROADCAST_HZ
        );
        assert_eq!(simulation_hz % PLAYER_STATE_BROADCAST_INTERVAL_TICKS, 0);
        assert_eq!(
            decode_player_state_packet(&encoded),
            Ok(PlayerStatePacket {
                server_tick: 91,
                players,
            })
        );
    }

    #[test]
    fn malformed_or_non_canonical_packets_are_rejected() {
        let canonical = encode_player_state_packet(2, &[player(3, -9)]).expect("canonical packet");
        let mut malformed = canonical.clone();
        malformed[0] ^= 1;
        assert_eq!(
            decode_player_state_packet(&malformed),
            Err(PlayerStateCodecError::InvalidMagic)
        );
        malformed = canonical.clone();
        malformed[4] = 2;
        assert_eq!(
            decode_player_state_packet(&malformed),
            Err(PlayerStateCodecError::UnsupportedVersion(2))
        );
        malformed = canonical.clone();
        *malformed.last_mut().expect("flags byte") = 0b10;
        assert_eq!(
            decode_player_state_packet(&malformed),
            Err(PlayerStateCodecError::InvalidFlags(0b10))
        );
        malformed = canonical.clone();
        malformed.push(0);
        assert_eq!(
            decode_player_state_packet(&malformed),
            Err(PlayerStateCodecError::InvalidLength {
                expected: canonical.len(),
                actual: canonical.len() + 1,
            })
        );
        assert_eq!(
            encode_player_state_packet(3, &[player(2, 0), player(1, 0)]),
            Err(PlayerStateCodecError::UnorderedSession {
                previous: 2,
                current: 1,
            })
        );
        let too_many = (1..=MAX_REPLICATED_PLAYERS + 1)
            .map(|session_id| player(u64::try_from(session_id).expect("small ID"), 0))
            .collect::<Vec<_>>();
        assert_eq!(
            encode_player_state_packet(4, &too_many),
            Err(PlayerStateCodecError::TooManyPlayers(
                MAX_REPLICATED_PLAYERS + 1
            ))
        );
        assert_eq!(
            decode_player_state_packet(&vec![0; MAX_PLAYER_STATE_DATAGRAM_BYTES + 1]),
            Err(PlayerStateCodecError::Oversized(
                MAX_PLAYER_STATE_DATAGRAM_BYTES + 1
            ))
        );
    }

    #[test]
    fn inbox_replaces_the_complete_view_and_rejects_stale_state_atomically() {
        let mut inbox = PlayerStateInbox::default();
        let first =
            encode_player_state_packet(7, &[player(1, 10), player(2, 20)]).expect("first state");
        assert_eq!(
            inbox.receive(&first),
            Ok(PlayerStateApplyReport {
                server_tick: 7,
                joined_players: 2,
                updated_players: 0,
                removed_players: 0,
            })
        );
        let next_player = player(2, 30);
        let next = encode_player_state_packet(9, &[next_player]).expect("next state");
        assert_eq!(
            inbox.receive(&next),
            Ok(PlayerStateApplyReport {
                server_tick: 9,
                joined_players: 0,
                updated_players: 1,
                removed_players: 1,
            })
        );
        assert_eq!(inbox.player(1), None);
        assert_eq!(inbox.player(2), Some(next_player));
        assert_eq!(
            inbox.receive(&first),
            Err(PlayerStateReceiveError::StaleServerTick {
                received: 7,
                last_applied: 9,
            })
        );
        assert_eq!(inbox.last_server_tick(), 9);
        assert_eq!(inbox.player(2), Some(next_player));
    }
}
