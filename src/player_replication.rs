//! Bounded lossy replication of authoritative player motion.

use crate::{
    AuthoritativePlayerState, FixedMicrometers3, MAX_BUILD_COORDINATE, MICROMETERS_PER_VOXEL,
};
use core::fmt;
use std::collections::{BTreeMap, VecDeque};

const PLAYER_STATE_MAGIC: [u8; 4] = *b"DFPL";
const PLAYER_STATE_VERSION: u8 = 1;
const PLAYER_STATE_HEADER_BYTES: usize = 4 + 1 + 8 + 1;
const PLAYER_STATE_ENTRY_BYTES: usize = 8 + 3 * 8 + 3 * 8 + 8 + 1;
const GROUNDED_FLAG: u8 = 1;
pub const MAX_REPLICATED_PLAYERS: usize = 16;
pub const PLAYER_STATE_BROADCAST_HZ: u64 = 20;
pub const PLAYER_STATE_BROADCAST_INTERVAL_TICKS: u64 = 3;
pub const PLAYER_INTERPOLATION_DELAY_TICKS: u64 = 6;
pub const MAX_PLAYER_INTERPOLATION_PACKETS: usize = 8;
pub const MAX_REPLICATED_PLAYER_POSITION_UM: i64 =
    (MAX_BUILD_COORDINATE as i64 + 1) * MICROMETERS_PER_VOXEL;
pub const MAX_REPLICATED_PLAYER_VELOCITY_UM_PER_SECOND: i64 = 64_000_000;
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
    PositionOutOfRange(u64),
    VelocityOutOfRange(u64),
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
            Self::PositionOutOfRange(session_id) => {
                write!(
                    formatter,
                    "player {session_id} position exceeds the world bound"
                )
            }
            Self::VelocityOutOfRange(session_id) => {
                write!(
                    formatter,
                    "player {session_id} velocity exceeds the motion bound"
                )
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerInterpolationSample {
    pub target_server_tick: u64,
    pub target_subtick_per_mille: u16,
    pub source_ticks: [u64; 2],
    pub players: Vec<ReplicatedPlayerState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerInterpolationError {
    Receive(PlayerStateReceiveError),
    InvalidSubtick(u16),
    NoSamples,
}

impl fmt::Display for PlayerInterpolationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Receive(error) => error.fmt(formatter),
            Self::InvalidSubtick(subtick) => {
                write!(
                    formatter,
                    "player interpolation subtick {subtick} exceeds 999"
                )
            }
            Self::NoSamples => write!(formatter, "player interpolation has no samples"),
        }
    }
}

impl std::error::Error for PlayerInterpolationError {}

impl From<PlayerStateReceiveError> for PlayerInterpolationError {
    fn from(value: PlayerStateReceiveError) -> Self {
        Self::Receive(value)
    }
}

/// Bounded history used to render remote players behind the latest authoritative tick.
#[derive(Debug, Default)]
pub struct PlayerInterpolationBuffer {
    packets: VecDeque<PlayerStatePacket>,
}

impl PlayerInterpolationBuffer {
    /// Appends one newer complete view and evicts the oldest view at the fixed history cap.
    ///
    /// # Errors
    ///
    /// Rejects malformed or stale/replayed datagrams without changing buffered history.
    pub fn push(
        &mut self,
        bytes: &[u8],
    ) -> Result<PlayerStateApplyReport, PlayerInterpolationError> {
        let packet = decode_player_state_packet(bytes)
            .map_err(PlayerStateReceiveError::Codec)
            .map_err(PlayerInterpolationError::Receive)?;
        let previous = self.packets.back();
        if let Some(previous) = previous
            && packet.server_tick <= previous.server_tick
        {
            return Err(PlayerStateReceiveError::StaleServerTick {
                received: packet.server_tick,
                last_applied: previous.server_tick,
            }
            .into());
        }
        let joined_players = packet
            .players
            .iter()
            .filter(|player| {
                previous.is_none_or(|previous| {
                    player_by_session(&previous.players, player.session_id).is_none()
                })
            })
            .count();
        let updated_players = packet.players.len().saturating_sub(joined_players);
        let removed_players = previous.map_or(0, |previous| {
            previous
                .players
                .iter()
                .filter(|player| player_by_session(&packet.players, player.session_id).is_none())
                .count()
        });
        let server_tick = packet.server_tick;
        if self.packets.len() == MAX_PLAYER_INTERPOLATION_PACKETS {
            self.packets.pop_front();
        }
        self.packets.push_back(packet);
        Ok(PlayerStateApplyReport {
            server_tick,
            joined_players,
            updated_players,
            removed_players,
        })
    }

    /// Samples a complete remote-player view at an explicit server tick and fractional subtick.
    ///
    /// Targets outside retained history clamp to the nearest view. Between two views, players that
    /// leave remain visible until the newer tick while new players appear at that newer tick.
    ///
    /// # Errors
    ///
    /// Rejects fractional values above 999 or sampling before any state has arrived.
    pub fn sample(
        &self,
        target_server_tick: u64,
        target_subtick_per_mille: u16,
    ) -> Result<PlayerInterpolationSample, PlayerInterpolationError> {
        if target_subtick_per_mille > 999 {
            return Err(PlayerInterpolationError::InvalidSubtick(
                target_subtick_per_mille,
            ));
        }
        let first = self
            .packets
            .front()
            .ok_or(PlayerInterpolationError::NoSamples)?;
        let last = self
            .packets
            .back()
            .ok_or(PlayerInterpolationError::NoSamples)?;
        if target_server_tick < first.server_tick
            || (target_server_tick == first.server_tick && target_subtick_per_mille == 0)
        {
            return Ok(clamped_sample(
                target_server_tick,
                target_subtick_per_mille,
                first,
            ));
        }
        if target_server_tick >= last.server_tick {
            return Ok(clamped_sample(
                target_server_tick,
                target_subtick_per_mille,
                last,
            ));
        }
        for (older, newer) in self.packets.iter().zip(self.packets.iter().skip(1)) {
            if target_server_tick > newer.server_tick
                || (target_server_tick == newer.server_tick && target_subtick_per_mille > 0)
            {
                continue;
            }
            if target_server_tick < older.server_tick {
                continue;
            }
            let numerator = i128::from(target_server_tick - older.server_tick)
                .saturating_mul(1_000)
                .saturating_add(i128::from(target_subtick_per_mille));
            let denominator = i128::from(newer.server_tick - older.server_tick) * 1_000;
            let players = if numerator >= denominator {
                newer.players.clone()
            } else {
                interpolate_players(&older.players, &newer.players, numerator, denominator)
            };
            return Ok(PlayerInterpolationSample {
                target_server_tick,
                target_subtick_per_mille,
                source_ticks: [older.server_tick, newer.server_tick],
                players,
            });
        }
        Ok(clamped_sample(
            target_server_tick,
            target_subtick_per_mille,
            last,
        ))
    }

    #[must_use]
    pub fn delayed_target_tick(&self) -> Option<u64> {
        self.packets.back().map(|packet| {
            packet
                .server_tick
                .saturating_sub(PLAYER_INTERPOLATION_DELAY_TICKS)
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.packets.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }
}

fn clamped_sample(
    target_server_tick: u64,
    target_subtick_per_mille: u16,
    packet: &PlayerStatePacket,
) -> PlayerInterpolationSample {
    PlayerInterpolationSample {
        target_server_tick,
        target_subtick_per_mille,
        source_ticks: [packet.server_tick; 2],
        players: packet.players.clone(),
    }
}

fn interpolate_players(
    older: &[ReplicatedPlayerState],
    newer: &[ReplicatedPlayerState],
    numerator: i128,
    denominator: i128,
) -> Vec<ReplicatedPlayerState> {
    older
        .iter()
        .map(|older_player| {
            let Some(newer_player) = player_by_session(newer, older_player.session_id) else {
                return *older_player;
            };
            ReplicatedPlayerState {
                session_id: older_player.session_id,
                position_um: interpolate_fixed(
                    older_player.position_um,
                    newer_player.position_um,
                    numerator,
                    denominator,
                ),
                velocity_um_per_second: interpolate_fixed(
                    older_player.velocity_um_per_second,
                    newer_player.velocity_um_per_second,
                    numerator,
                    denominator,
                ),
                grounded: older_player.grounded,
                last_input_sequence: older_player.last_input_sequence,
            }
        })
        .collect()
}

fn interpolate_fixed(
    older: FixedMicrometers3,
    newer: FixedMicrometers3,
    numerator: i128,
    denominator: i128,
) -> FixedMicrometers3 {
    FixedMicrometers3 {
        x: interpolate_i64(older.x, newer.x, numerator, denominator),
        y: interpolate_i64(older.y, newer.y, numerator, denominator),
        z: interpolate_i64(older.z, newer.z, numerator, denominator),
    }
}

fn interpolate_i64(older: i64, newer: i64, numerator: i128, denominator: i128) -> i64 {
    debug_assert!(denominator > 0 && numerator < denominator);
    let older = i128::from(older);
    let difference = i128::from(newer) - older;
    let interpolated = older + difference * numerator / denominator;
    i64::try_from(interpolated).unwrap_or_else(|_error| {
        if interpolated.is_negative() {
            i64::MIN
        } else {
            i64::MAX
        }
    })
}

fn player_by_session(
    players: &[ReplicatedPlayerState],
    session_id: u64,
) -> Option<&ReplicatedPlayerState> {
    players
        .binary_search_by_key(&session_id, |player| player.session_id)
        .ok()
        .map(|index| &players[index])
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
        if fixed_out_of_range(player.position_um, MAX_REPLICATED_PLAYER_POSITION_UM) {
            return Err(PlayerStateCodecError::PositionOutOfRange(player.session_id));
        }
        if fixed_out_of_range(
            player.velocity_um_per_second,
            MAX_REPLICATED_PLAYER_VELOCITY_UM_PER_SECOND,
        ) {
            return Err(PlayerStateCodecError::VelocityOutOfRange(player.session_id));
        }
        previous = player.session_id;
    }
    Ok(())
}

fn fixed_out_of_range(value: FixedMicrometers3, maximum: i64) -> bool {
    [value.x, value.y, value.z]
        .into_iter()
        .any(|component| component.unsigned_abs() > maximum.cast_unsigned())
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

    #[test]
    fn interpolation_is_deterministic_across_join_and_leave_boundaries() {
        let mut buffer = PlayerInterpolationBuffer::default();
        let first = encode_player_state_packet(3, &[player(1, 0), player(2, 100)])
            .expect("first interpolation view");
        let second = encode_player_state_packet(6, &[player(1, 300), player(3, 900)])
            .expect("second interpolation view");
        buffer.push(&first).expect("first buffered view");
        assert_eq!(
            buffer.push(&second),
            Ok(PlayerStateApplyReport {
                server_tick: 6,
                joined_players: 1,
                updated_players: 1,
                removed_players: 1,
            })
        );

        let middle = buffer.sample(4, 500).expect("midpoint view");
        assert_eq!(middle.source_ticks, [3, 6]);
        assert_eq!(middle.players.len(), 2);
        assert_eq!(middle.players[0].session_id, 1);
        assert_eq!(middle.players[0].position_um.x, 150);
        assert_eq!(middle.players[1].session_id, 2);
        assert_eq!(middle.players[1].position_um.x, 100);
        assert!(middle.players.iter().all(|state| state.session_id != 3));

        let boundary = buffer.sample(6, 0).expect("exact newer boundary");
        assert_eq!(boundary.players, vec![player(1, 300), player(3, 900)]);
        assert_eq!(boundary.source_ticks, [6; 2]);
    }

    #[test]
    fn interpolation_history_and_stale_rejection_remain_bounded() {
        let mut buffer = PlayerInterpolationBuffer::default();
        assert_eq!(
            buffer.sample(1, 0),
            Err(PlayerInterpolationError::NoSamples)
        );
        for server_tick in 1..=12 {
            let packet = encode_player_state_packet(
                server_tick,
                &[player(1, i64::try_from(server_tick).expect("small tick"))],
            )
            .expect("bounded interpolation packet");
            buffer.push(&packet).expect("newer interpolation packet");
        }
        assert_eq!(buffer.len(), MAX_PLAYER_INTERPOLATION_PACKETS);
        assert_eq!(buffer.delayed_target_tick(), Some(6));
        let retained = buffer.sample(1, 0).expect("clamped old sample");
        assert_eq!(retained.source_ticks, [5; 2]);
        assert_eq!(retained.players[0].position_um.x, 5);
        assert_eq!(
            buffer.sample(12, 1_000),
            Err(PlayerInterpolationError::InvalidSubtick(1_000))
        );
        let stale = encode_player_state_packet(11, &[player(1, 99)]).expect("stale packet");
        assert_eq!(
            buffer.push(&stale),
            Err(PlayerInterpolationError::Receive(
                PlayerStateReceiveError::StaleServerTick {
                    received: 11,
                    last_applied: 12,
                }
            ))
        );
        assert_eq!(buffer.len(), MAX_PLAYER_INTERPOLATION_PACKETS);
        assert_eq!(
            buffer.sample(12, 0).expect("latest view").players[0]
                .position_um
                .x,
            12
        );
    }

    #[test]
    fn interpolation_handles_opposite_world_bounds_without_overflow() {
        let bounded = |server_tick, x| {
            encode_player_state_packet(
                server_tick,
                &[ReplicatedPlayerState {
                    session_id: 1,
                    position_um: FixedMicrometers3 { x, y: 0, z: 0 },
                    velocity_um_per_second: FixedMicrometers3::default(),
                    grounded: true,
                    last_input_sequence: server_tick,
                }],
            )
            .expect("bounded extreme state")
        };
        let mut buffer = PlayerInterpolationBuffer::default();
        buffer
            .push(&bounded(1, -MAX_REPLICATED_PLAYER_POSITION_UM))
            .expect("lower extreme");
        buffer
            .push(&bounded(3, MAX_REPLICATED_PLAYER_POSITION_UM))
            .expect("upper extreme");

        let middle = buffer.sample(2, 0).expect("extreme midpoint");
        assert_eq!(middle.players[0].position_um.x, 0);
    }
}
