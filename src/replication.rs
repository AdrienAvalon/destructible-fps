use crate::destruction::{DestructionReport, Explosion};
use crate::material::{InvalidMaterial, Voxel};
use crate::world::{IVec3, VoxelChange, World, WorldError};
use core::fmt;
use std::collections::HashMap;

const MAGIC: [u8; 4] = *b"DFPS";
const PROTOCOL_VERSION: u8 = 1;
const DELTA_KIND: u8 = 1;
const HEADER_BYTES: usize = 60;
const CHANGE_BYTES: usize = 16;
const MAX_FRAGMENTS: u16 = 1_024;
const MAX_PENDING_PACKETS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExplosionCommand {
    pub command_id: u64,
    pub center: IVec3,
    pub radius_voxels: u16,
    pub peak_energy: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeltaPacket {
    pub sequence: u64,
    pub tick: u64,
    pub base_fingerprint: u128,
    pub final_fingerprint: u128,
    pub changes: Vec<VoxelChange>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeltaFrame {
    pub sequence: u64,
    pub tick: u64,
    pub base_fingerprint: u128,
    pub final_fingerprint: u128,
    pub fragment_index: u16,
    pub fragment_count: u16,
    pub changes: Vec<VoxelChange>,
}

#[derive(Clone)]
pub struct AuthoritativeServer {
    world: World,
    next_sequence: u64,
    last_command_id: HashMap<u64, u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandError {
    ZeroRadius,
    RadiusTooLarge(u16),
    EnergyTooLarge(u32),
    ReplayedCommand { client_id: u64, command_id: u64 },
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroRadius => write!(formatter, "explosion radius must be non-zero"),
            Self::RadiusTooLarge(radius) => {
                write!(formatter, "explosion radius {radius} exceeds 64")
            }
            Self::EnergyTooLarge(energy) => {
                write!(formatter, "explosion energy {energy} exceeds 1,000,000")
            }
            Self::ReplayedCommand {
                client_id,
                command_id,
            } => write!(
                formatter,
                "replayed command {command_id} from client {client_id}"
            ),
        }
    }
}

impl std::error::Error for CommandError {}

impl AuthoritativeServer {
    #[must_use]
    pub fn new(world: World) -> Self {
        Self {
            world,
            next_sequence: 1,
            last_command_id: HashMap::new(),
        }
    }

    /// Validates and applies one client request as an authoritative transaction.
    ///
    /// # Errors
    ///
    /// Rejects unsafe blast bounds and replayed or out-of-order client command identifiers.
    pub fn execute_explosion(
        &mut self,
        client_id: u64,
        command: ExplosionCommand,
    ) -> Result<(DeltaPacket, DestructionReport), CommandError> {
        validate_command(command)?;
        if self
            .last_command_id
            .get(&client_id)
            .is_some_and(|last| command.command_id <= *last)
        {
            return Err(CommandError::ReplayedCommand {
                client_id,
                command_id: command.command_id,
            });
        }

        let base_fingerprint = self.world.fingerprint();
        let report = self.world.apply_explosion(Explosion {
            center: command.center,
            radius_voxels: command.radius_voxels,
            peak_energy: command.peak_energy,
        });
        let tick = self.world.tick().wrapping_add(1);
        self.world.set_tick(tick);
        let packet = DeltaPacket {
            sequence: self.next_sequence,
            tick,
            base_fingerprint,
            final_fingerprint: self.world.fingerprint(),
            changes: report.changes.clone(),
        };
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.last_command_id.insert(client_id, command.command_id);
        Ok((packet, report))
    }

    #[must_use]
    pub const fn world(&self) -> &World {
        &self.world
    }
}

const fn validate_command(command: ExplosionCommand) -> Result<(), CommandError> {
    if command.radius_voxels == 0 {
        return Err(CommandError::ZeroRadius);
    }
    if command.radius_voxels > 64 {
        return Err(CommandError::RadiusTooLarge(command.radius_voxels));
    }
    if command.peak_energy > 1_000_000 {
        return Err(CommandError::EnergyTooLarge(command.peak_energy));
    }
    Ok(())
}

#[derive(Clone)]
pub struct ClientReplica {
    world: World,
    expected_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientStatus {
    Applied,
    DuplicateIgnored,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplicationError {
    SequenceGap { expected: u64, received: u64 },
    BaseFingerprintMismatch { expected: u128, received: u128 },
    World(WorldError),
}

impl fmt::Display for ReplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SequenceGap { expected, received } => {
                write!(
                    formatter,
                    "sequence gap: expected {expected}, received {received}"
                )
            }
            Self::BaseFingerprintMismatch { expected, received } => write!(
                formatter,
                "base fingerprint mismatch: local {expected:032x}, packet {received:032x}"
            ),
            Self::World(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ReplicationError {}

impl From<WorldError> for ReplicationError {
    fn from(value: WorldError) -> Self {
        Self::World(value)
    }
}

impl ClientReplica {
    #[must_use]
    pub const fn new(world: World) -> Self {
        Self {
            world,
            expected_sequence: 1,
        }
    }

    /// Applies the next complete authoritative delta.
    ///
    /// # Errors
    ///
    /// Rejects sequence gaps, a divergent base fingerprint, or an invalid world transaction.
    pub fn receive(&mut self, packet: &DeltaPacket) -> Result<ClientStatus, ReplicationError> {
        if packet.sequence < self.expected_sequence {
            return Ok(ClientStatus::DuplicateIgnored);
        }
        if packet.sequence > self.expected_sequence {
            return Err(ReplicationError::SequenceGap {
                expected: self.expected_sequence,
                received: packet.sequence,
            });
        }
        if self.world.fingerprint() != packet.base_fingerprint {
            return Err(ReplicationError::BaseFingerprintMismatch {
                expected: self.world.fingerprint(),
                received: packet.base_fingerprint,
            });
        }
        self.world
            .apply_checked(&packet.changes, packet.final_fingerprint)?;
        self.world.set_tick(packet.tick);
        self.expected_sequence = self.expected_sequence.wrapping_add(1);
        Ok(ClientStatus::Applied)
    }

    /// Installs a server snapshot after a detected gap. The next delta is explicit to avoid
    /// accepting a stale snapshot that would silently move the client backwards.
    pub fn install_snapshot(&mut self, world: World, next_sequence: u64) {
        self.world = world;
        self.expected_sequence = next_sequence;
    }

    #[must_use]
    pub const fn world(&self) -> &World {
        &self.world
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodecError {
    MtuTooSmall(usize),
    TooManyFragments(usize),
    Truncated,
    InvalidMagic,
    UnsupportedVersion(u8),
    InvalidKind(u8),
    InvalidFragmentLayout,
    InvalidLength { expected: usize, actual: usize },
    InvalidMaterial(InvalidMaterial),
    InconsistentFragment,
    TooManyPendingPackets,
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CodecError {}

impl From<InvalidMaterial> for CodecError {
    fn from(value: InvalidMaterial) -> Self {
        Self::InvalidMaterial(value)
    }
}

/// Fragments a complete delta into application frames no larger than `mtu`.
///
/// # Errors
///
/// Returns an error when the MTU cannot carry one change or the packet needs too many fragments.
pub fn encode_frames(packet: &DeltaPacket, mtu: usize) -> Result<Vec<Vec<u8>>, CodecError> {
    if mtu < HEADER_BYTES + CHANGE_BYTES {
        return Err(CodecError::MtuTooSmall(mtu));
    }
    let changes_per_frame = ((mtu - HEADER_BYTES) / CHANGE_BYTES).min(usize::from(u16::MAX));
    let fragment_count = packet.changes.len().max(1).div_ceil(changes_per_frame);
    if fragment_count > usize::from(MAX_FRAGMENTS) || fragment_count > usize::from(u16::MAX) {
        return Err(CodecError::TooManyFragments(fragment_count));
    }
    let fragment_count =
        u16::try_from(fragment_count).map_err(|_| CodecError::TooManyFragments(fragment_count))?;
    let mut frames = Vec::with_capacity(usize::from(fragment_count));
    for fragment_index in 0..fragment_count {
        let start = usize::from(fragment_index) * changes_per_frame;
        let end = (start + changes_per_frame).min(packet.changes.len());
        let changes = if start < packet.changes.len() {
            &packet.changes[start..end]
        } else {
            &[]
        };
        let mut bytes = Vec::with_capacity(HEADER_BYTES + changes.len() * CHANGE_BYTES);
        bytes.extend_from_slice(&MAGIC);
        bytes.push(PROTOCOL_VERSION);
        bytes.push(DELTA_KIND);
        push_u64(&mut bytes, packet.sequence);
        push_u64(&mut bytes, packet.tick);
        push_u128(&mut bytes, packet.base_fingerprint);
        push_u128(&mut bytes, packet.final_fingerprint);
        push_u16(&mut bytes, fragment_index);
        push_u16(&mut bytes, fragment_count);
        let change_count = u16::try_from(changes.len())
            .map_err(|_| CodecError::TooManyFragments(usize::from(fragment_count)))?;
        push_u16(&mut bytes, change_count);
        for change in changes {
            push_i32(&mut bytes, change.position.x);
            push_i32(&mut bytes, change.position.y);
            push_i32(&mut bytes, change.position.z);
            bytes.push(change.before.material as u8);
            bytes.push(change.before.integrity);
            bytes.push(change.after.material as u8);
            bytes.push(change.after.integrity);
        }
        debug_assert!(bytes.len() <= mtu);
        frames.push(bytes);
    }
    Ok(frames)
}

/// Validates and decodes one untrusted application frame.
///
/// # Errors
///
/// Rejects malformed headers, unsupported protocol values, inconsistent lengths, and materials.
pub fn decode_frame(bytes: &[u8]) -> Result<DeltaFrame, CodecError> {
    if bytes.len() < HEADER_BYTES {
        return Err(CodecError::Truncated);
    }
    let mut cursor = Cursor::new(bytes);
    if cursor.take_array::<4>()? != MAGIC {
        return Err(CodecError::InvalidMagic);
    }
    let version = cursor.take_u8()?;
    if version != PROTOCOL_VERSION {
        return Err(CodecError::UnsupportedVersion(version));
    }
    let kind = cursor.take_u8()?;
    if kind != DELTA_KIND {
        return Err(CodecError::InvalidKind(kind));
    }
    let sequence = cursor.take_u64()?;
    let tick = cursor.take_u64()?;
    let base_fingerprint = cursor.take_u128()?;
    let final_fingerprint = cursor.take_u128()?;
    let fragment_index = cursor.take_u16()?;
    let fragment_count = cursor.take_u16()?;
    let change_count = usize::from(cursor.take_u16()?);
    if fragment_count == 0 || fragment_count > MAX_FRAGMENTS || fragment_index >= fragment_count {
        return Err(CodecError::InvalidFragmentLayout);
    }
    let expected_length = HEADER_BYTES + change_count * CHANGE_BYTES;
    if bytes.len() != expected_length {
        return Err(CodecError::InvalidLength {
            expected: expected_length,
            actual: bytes.len(),
        });
    }

    let mut changes = Vec::with_capacity(change_count);
    for _ in 0..change_count {
        let position = IVec3::new(cursor.take_i32()?, cursor.take_i32()?, cursor.take_i32()?);
        let before = Voxel::from_wire(cursor.take_u8()?, cursor.take_u8()?)?;
        let after = Voxel::from_wire(cursor.take_u8()?, cursor.take_u8()?)?;
        changes.push(VoxelChange {
            position,
            before,
            after,
        });
    }
    Ok(DeltaFrame {
        sequence,
        tick,
        base_fingerprint,
        final_fingerprint,
        fragment_index,
        fragment_count,
        changes,
    })
}

#[derive(Default)]
pub struct FrameAssembler {
    pending: HashMap<u64, PendingPacket>,
}

struct PendingPacket {
    tick: u64,
    base_fingerprint: u128,
    final_fingerprint: u128,
    fragments: Vec<Option<Vec<VoxelChange>>>,
}

impl FrameAssembler {
    /// Adds a validated fragment and returns a packet only when every fragment is present.
    ///
    /// # Errors
    ///
    /// Rejects fragments that disagree with metadata already recorded for the same sequence.
    pub fn push(&mut self, frame: DeltaFrame) -> Result<Option<DeltaPacket>, CodecError> {
        if !self.pending.contains_key(&frame.sequence) && self.pending.len() >= MAX_PENDING_PACKETS
        {
            return Err(CodecError::TooManyPendingPackets);
        }
        let pending = self
            .pending
            .entry(frame.sequence)
            .or_insert_with(|| PendingPacket {
                tick: frame.tick,
                base_fingerprint: frame.base_fingerprint,
                final_fingerprint: frame.final_fingerprint,
                fragments: vec![None; usize::from(frame.fragment_count)],
            });
        if pending.tick != frame.tick
            || pending.base_fingerprint != frame.base_fingerprint
            || pending.final_fingerprint != frame.final_fingerprint
            || pending.fragments.len() != usize::from(frame.fragment_count)
        {
            self.pending.remove(&frame.sequence);
            return Err(CodecError::InconsistentFragment);
        }
        let slot = &mut pending.fragments[usize::from(frame.fragment_index)];
        if let Some(existing) = slot {
            if existing != &frame.changes {
                self.pending.remove(&frame.sequence);
                return Err(CodecError::InconsistentFragment);
            }
        } else {
            *slot = Some(frame.changes);
        }
        if pending.fragments.iter().any(Option::is_none) {
            return Ok(None);
        }

        let complete = self
            .pending
            .remove(&frame.sequence)
            .ok_or(CodecError::InconsistentFragment)?;
        let changes = complete.fragments.into_iter().flatten().flatten().collect();
        Ok(Some(DeltaPacket {
            sequence: frame.sequence,
            tick: complete.tick,
            base_fingerprint: complete.base_fingerprint,
            final_fingerprint: complete.final_fingerprint,
            changes,
        }))
    }

    #[must_use]
    pub fn pending_packets(&self) -> usize {
        self.pending.len()
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take_array<const N: usize>(&mut self) -> Result<[u8; N], CodecError> {
        let end = self.offset.checked_add(N).ok_or(CodecError::Truncated)?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or(CodecError::Truncated)?;
        self.offset = end;
        slice.try_into().map_err(|_| CodecError::Truncated)
    }

    fn take_u8(&mut self) -> Result<u8, CodecError> {
        Ok(self.take_array::<1>()?[0])
    }

    fn take_u16(&mut self) -> Result<u16, CodecError> {
        Ok(u16::from_le_bytes(self.take_array()?))
    }

    fn take_u64(&mut self) -> Result<u64, CodecError> {
        Ok(u64::from_le_bytes(self.take_array()?))
    }

    fn take_u128(&mut self) -> Result<u128, CodecError> {
        Ok(u128::from_le_bytes(self.take_array()?))
    }

    fn take_i32(&mut self) -> Result<i32, CodecError> {
        Ok(i32::from_le_bytes(self.take_array()?))
    }
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u128(bytes: &mut Vec<u8>, value: u128) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
