//! Bounded canonical snapshot framing for replica join and gap repair.

use crate::{
    AuthoritativeServer, BodyError, BodyId, BodyLimits, BodyVoxel, ClientReplica,
    FixedMicrometers3, FixedMilliradians3, FixedQuaternion, ReplicationError, RigidBodyDescriptor,
    RigidBodyState, Voxel, World, material::InvalidMaterial, valid_rigid_body_state,
};
use core::fmt;
use std::collections::BTreeMap;

const SNAPSHOT_MAGIC: [u8; 4] = *b"DFSN";
const SNAPSHOT_VERSION: u8 = 2;
const SNAPSHOT_KIND: u8 = 1;
const FRAME_HEADER_BYTES: usize = 38;
const PAYLOAD_HEADER_BYTES: usize = 48;
const VOXEL_BYTES: usize = 14;
const BODY_HEADER_AND_STATE_BYTES: usize = 109;
const BODY_STATE_BYTES: usize = 97;
const MAX_SNAPSHOT_FRAGMENTS: usize = 4_096;
pub const MAX_SNAPSHOT_PAYLOAD_BYTES: usize = 4 * 1_024 * 1_024;
pub const MAX_SNAPSHOT_STATIC_VOXELS: usize = 262_144;
pub const MAX_SNAPSHOT_BODY_VOXELS: usize = 262_144;
pub const MAX_SNAPSHOT_DATAGRAM_BYTES: usize = 1_200;

pub struct AuthoritativeSnapshot {
    snapshot_id: u64,
    world: World,
    bodies: BTreeMap<BodyId, RigidBodyDescriptor>,
    body_states: BTreeMap<BodyId, RigidBodyState>,
    next_body_id: BodyId,
    next_sequence: u64,
}

impl AuthoritativeSnapshot {
    #[must_use]
    pub const fn snapshot_id(&self) -> u64 {
        self.snapshot_id
    }

    /// Atomically validates and installs this snapshot into a client replica.
    ///
    /// # Errors
    ///
    /// Returns the existing fail-closed replication validation errors.
    pub fn install_into(self, replica: &mut ClientReplica) -> Result<(), ReplicationError> {
        replica.install_snapshot(
            self.world,
            self.bodies,
            &self.body_states,
            self.next_body_id,
            self.next_sequence,
        )
    }

    #[must_use]
    pub const fn world(&self) -> &World {
        &self.world
    }

    #[must_use]
    pub const fn next_sequence(&self) -> u64 {
        self.next_sequence
    }
}

#[derive(Debug)]
pub enum SnapshotCodecError {
    InvalidMtu(usize),
    InvalidSnapshotId(u64),
    SnapshotTooLarge(usize),
    TooManyStaticVoxels(usize),
    TooManyBodies(usize),
    TooManyBodyVoxels(usize),
    InvalidMagic,
    UnsupportedVersion(u8),
    InvalidKind(u8),
    Truncated,
    InvalidLength { expected: usize, actual: usize },
    InvalidFragmentLayout,
    InconsistentFragment,
    PayloadHashMismatch,
    InvalidPayload(&'static str),
    InvalidMaterial(InvalidMaterial),
    Body(BodyError),
}

impl fmt::Display for SnapshotCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMtu(mtu) => write!(formatter, "invalid snapshot MTU {mtu}"),
            Self::InvalidSnapshotId(id) => write!(formatter, "invalid snapshot ID {id}"),
            Self::SnapshotTooLarge(bytes) => {
                write!(formatter, "snapshot payload has {bytes} bytes")
            }
            Self::TooManyStaticVoxels(count) => {
                write!(formatter, "snapshot has {count} static voxels")
            }
            Self::TooManyBodies(count) => write!(formatter, "snapshot has {count} bodies"),
            Self::TooManyBodyVoxels(count) => {
                write!(formatter, "snapshot has {count} body voxels")
            }
            Self::InvalidMagic => write!(formatter, "invalid snapshot magic"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported snapshot version {version}")
            }
            Self::InvalidKind(kind) => write!(formatter, "invalid snapshot frame kind {kind}"),
            Self::Truncated => write!(formatter, "truncated snapshot data"),
            Self::InvalidLength { expected, actual } => write!(
                formatter,
                "snapshot length mismatch: expected {expected}, received {actual}"
            ),
            Self::InvalidFragmentLayout => write!(formatter, "invalid snapshot fragment layout"),
            Self::InconsistentFragment => write!(formatter, "inconsistent snapshot fragment"),
            Self::PayloadHashMismatch => write!(formatter, "snapshot payload hash mismatch"),
            Self::InvalidPayload(reason) => write!(formatter, "invalid snapshot payload: {reason}"),
            Self::InvalidMaterial(error) => error.fmt(formatter),
            Self::Body(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SnapshotCodecError {}

impl From<InvalidMaterial> for SnapshotCodecError {
    fn from(value: InvalidMaterial) -> Self {
        Self::InvalidMaterial(value)
    }
}

impl From<BodyError> for SnapshotCodecError {
    fn from(value: BodyError) -> Self {
        Self::Body(value)
    }
}

/// Encodes one current authoritative state into independently bounded UDP frames.
///
/// The payload fingerprint detects corruption and mixed fragments; it is not a cryptographic MAC.
///
/// # Errors
///
/// Rejects invalid MTUs, zero IDs, oversized worlds/body sets, inconsistent authoritative state,
/// or payloads beyond the fixed snapshot budget.
pub fn encode_snapshot_frames(
    snapshot_id: u64,
    authority: &AuthoritativeServer,
    mtu: usize,
) -> Result<Vec<Vec<u8>>, SnapshotCodecError> {
    if snapshot_id == 0 {
        return Err(SnapshotCodecError::InvalidSnapshotId(snapshot_id));
    }
    if !(FRAME_HEADER_BYTES + 1..=MAX_SNAPSHOT_DATAGRAM_BYTES).contains(&mtu) {
        return Err(SnapshotCodecError::InvalidMtu(mtu));
    }
    let payload = encode_payload(authority)?;
    let payload_per_frame = mtu - FRAME_HEADER_BYTES;
    let fragment_count = payload.len().div_ceil(payload_per_frame);
    if fragment_count == 0 || fragment_count > MAX_SNAPSHOT_FRAGMENTS {
        return Err(SnapshotCodecError::InvalidFragmentLayout);
    }
    let fragment_count_u16 =
        u16::try_from(fragment_count).map_err(|_| SnapshotCodecError::InvalidFragmentLayout)?;
    let total_payload_bytes = u32::try_from(payload.len())
        .map_err(|_| SnapshotCodecError::SnapshotTooLarge(payload.len()))?;
    let hash = snapshot_payload_hash(&payload);
    let mut frames = Vec::with_capacity(fragment_count);
    for (index, chunk) in payload.chunks(payload_per_frame).enumerate() {
        let fragment_index =
            u16::try_from(index).map_err(|_| SnapshotCodecError::InvalidFragmentLayout)?;
        let mut frame = Vec::with_capacity(FRAME_HEADER_BYTES + chunk.len());
        frame.extend_from_slice(&SNAPSHOT_MAGIC);
        frame.push(SNAPSHOT_VERSION);
        frame.push(SNAPSHOT_KIND);
        push_u64(&mut frame, snapshot_id);
        push_u16(&mut frame, fragment_index);
        push_u16(&mut frame, fragment_count_u16);
        push_u32(&mut frame, total_payload_bytes);
        push_u128(&mut frame, hash);
        frame.extend_from_slice(chunk);
        frames.push(frame);
    }
    Ok(frames)
}

#[must_use]
pub fn is_snapshot_datagram(bytes: &[u8]) -> bool {
    bytes.starts_with(&SNAPSHOT_MAGIC)
}

struct SnapshotFrame {
    snapshot_id: u64,
    fragment_index: u16,
    fragment_count: u16,
    total_payload_bytes: usize,
    payload_hash: u128,
    payload: Vec<u8>,
}

fn decode_snapshot_frame(bytes: &[u8]) -> Result<SnapshotFrame, SnapshotCodecError> {
    if bytes.len() > MAX_SNAPSHOT_DATAGRAM_BYTES {
        return Err(SnapshotCodecError::InvalidLength {
            expected: MAX_SNAPSHOT_DATAGRAM_BYTES,
            actual: bytes.len(),
        });
    }
    if bytes.len() <= FRAME_HEADER_BYTES {
        return Err(SnapshotCodecError::Truncated);
    }
    let mut cursor = Cursor::new(bytes);
    if cursor.take_array::<4>()? != SNAPSHOT_MAGIC {
        return Err(SnapshotCodecError::InvalidMagic);
    }
    let version = cursor.take_u8()?;
    if version != SNAPSHOT_VERSION {
        return Err(SnapshotCodecError::UnsupportedVersion(version));
    }
    let kind = cursor.take_u8()?;
    if kind != SNAPSHOT_KIND {
        return Err(SnapshotCodecError::InvalidKind(kind));
    }
    let snapshot_id = cursor.take_u64()?;
    if snapshot_id == 0 {
        return Err(SnapshotCodecError::InvalidSnapshotId(snapshot_id));
    }
    let fragment_index = cursor.take_u16()?;
    let fragment_count = cursor.take_u16()?;
    let total_payload_bytes = usize::try_from(cursor.take_u32()?)
        .map_err(|_| SnapshotCodecError::SnapshotTooLarge(usize::MAX))?;
    let payload_hash = cursor.take_u128()?;
    if fragment_count == 0
        || usize::from(fragment_count) > MAX_SNAPSHOT_FRAGMENTS
        || fragment_index >= fragment_count
        || total_payload_bytes == 0
        || total_payload_bytes > MAX_SNAPSHOT_PAYLOAD_BYTES
    {
        return Err(SnapshotCodecError::InvalidFragmentLayout);
    }
    Ok(SnapshotFrame {
        snapshot_id,
        fragment_index,
        fragment_count,
        total_payload_bytes,
        payload_hash,
        payload: cursor.remaining().to_vec(),
    })
}

struct PendingSnapshot {
    snapshot_id: u64,
    total_payload_bytes: usize,
    payload_hash: u128,
    fragments: Vec<Option<Vec<u8>>>,
    received_fragments: usize,
    received_bytes: usize,
}

#[derive(Default)]
pub struct SnapshotAssembler {
    active: Option<PendingSnapshot>,
    last_completed_snapshot_id: u64,
}

impl SnapshotAssembler {
    /// Accepts a snapshot datagram and returns only a fully hashed, decoded snapshot.
    ///
    /// Newer snapshot IDs replace an incomplete older transfer. Older or completed duplicates are
    /// ignored. At most one transfer and four MiB of payload are retained.
    ///
    /// # Errors
    ///
    /// Rejects malformed, inconsistent, oversized, corrupt, or semantically invalid snapshots.
    pub fn push(
        &mut self,
        bytes: &[u8],
    ) -> Result<Option<AuthoritativeSnapshot>, SnapshotCodecError> {
        let frame = decode_snapshot_frame(bytes)?;
        if frame.snapshot_id <= self.last_completed_snapshot_id {
            return Ok(None);
        }
        if self
            .active
            .as_ref()
            .is_some_and(|pending| frame.snapshot_id < pending.snapshot_id)
        {
            return Ok(None);
        }
        if self
            .active
            .as_ref()
            .is_none_or(|pending| frame.snapshot_id > pending.snapshot_id)
        {
            self.active = Some(PendingSnapshot {
                snapshot_id: frame.snapshot_id,
                total_payload_bytes: frame.total_payload_bytes,
                payload_hash: frame.payload_hash,
                fragments: vec![None; usize::from(frame.fragment_count)],
                received_fragments: 0,
                received_bytes: 0,
            });
        }
        let Some(pending) = self.active.as_mut() else {
            return Err(SnapshotCodecError::InconsistentFragment);
        };
        if pending.snapshot_id != frame.snapshot_id
            || pending.total_payload_bytes != frame.total_payload_bytes
            || pending.payload_hash != frame.payload_hash
            || pending.fragments.len() != usize::from(frame.fragment_count)
        {
            return Err(SnapshotCodecError::InconsistentFragment);
        }
        let slot = &mut pending.fragments[usize::from(frame.fragment_index)];
        if let Some(existing) = slot {
            return if existing == &frame.payload {
                Ok(None)
            } else {
                Err(SnapshotCodecError::InconsistentFragment)
            };
        }
        let received_bytes = pending
            .received_bytes
            .checked_add(frame.payload.len())
            .ok_or(SnapshotCodecError::SnapshotTooLarge(usize::MAX))?;
        if received_bytes > pending.total_payload_bytes
            || received_bytes > MAX_SNAPSHOT_PAYLOAD_BYTES
        {
            return Err(SnapshotCodecError::SnapshotTooLarge(received_bytes));
        }
        *slot = Some(frame.payload);
        pending.received_bytes = received_bytes;
        pending.received_fragments += 1;
        if pending.received_fragments != pending.fragments.len() {
            return Ok(None);
        }

        let Some(complete) = self.active.take() else {
            return Err(SnapshotCodecError::InconsistentFragment);
        };
        if complete.received_bytes != complete.total_payload_bytes {
            return Err(SnapshotCodecError::InvalidLength {
                expected: complete.total_payload_bytes,
                actual: complete.received_bytes,
            });
        }
        let mut payload = Vec::with_capacity(complete.total_payload_bytes);
        for fragment in complete.fragments {
            let Some(fragment) = fragment else {
                return Err(SnapshotCodecError::InconsistentFragment);
            };
            payload.extend(fragment);
        }
        if snapshot_payload_hash(&payload) != complete.payload_hash {
            return Err(SnapshotCodecError::PayloadHashMismatch);
        }
        let mut snapshot = decode_payload(&payload)?;
        snapshot.snapshot_id = complete.snapshot_id;
        self.last_completed_snapshot_id = complete.snapshot_id;
        Ok(Some(snapshot))
    }

    #[must_use]
    pub fn retained_payload_bytes(&self) -> usize {
        self.active
            .as_ref()
            .map_or(0, |pending| pending.received_bytes)
    }

    #[must_use]
    pub fn active_snapshot_id(&self) -> Option<u64> {
        self.active.as_ref().map(|pending| pending.snapshot_id)
    }

    /// Returns exact received and expected fragment counts for the active bounded transfer.
    #[must_use]
    pub fn active_fragment_progress(&self) -> Option<(usize, usize)> {
        self.active
            .as_ref()
            .map(|pending| (pending.received_fragments, pending.fragments.len()))
    }

    /// Returns at most 64 fixed bitmap windows describing every currently missing fragment.
    #[must_use]
    pub fn missing_fragment_windows(&self) -> Vec<(u16, u64)> {
        let Some(pending) = &self.active else {
            return Vec::new();
        };
        let mut windows = Vec::with_capacity(pending.fragments.len().div_ceil(64));
        for base in (0..pending.fragments.len()).step_by(64) {
            let mut missing = 0_u64;
            for (offset, fragment) in pending.fragments
                [base..pending.fragments.len().min(base + 64)]
                .iter()
                .enumerate()
            {
                if fragment.is_none() {
                    missing |= 1_u64 << offset;
                }
            }
            if missing != 0 {
                windows.push((u16::try_from(base).unwrap_or(u16::MAX), missing));
            }
        }
        windows
    }
}

fn encode_payload(authority: &AuthoritativeServer) -> Result<Vec<u8>, SnapshotCodecError> {
    if authority.next_body_id() == 0 || authority.next_sequence() == 0 {
        return Err(SnapshotCodecError::InvalidPayload(
            "zero authoritative high-water mark",
        ));
    }
    let static_count = authority.world().stats().solid_voxels;
    if static_count > MAX_SNAPSHOT_STATIC_VOXELS {
        return Err(SnapshotCodecError::TooManyStaticVoxels(static_count));
    }
    let body_count = authority.bodies().len();
    if body_count > crate::replication::MAX_ACTIVE_BODIES {
        return Err(SnapshotCodecError::TooManyBodies(body_count));
    }
    let body_voxel_count = authority
        .bodies()
        .values()
        .try_fold(0_usize, |total, body| total.checked_add(body.voxels.len()))
        .ok_or(SnapshotCodecError::TooManyBodyVoxels(usize::MAX))?;
    if body_voxel_count > MAX_SNAPSHOT_BODY_VOXELS {
        return Err(SnapshotCodecError::TooManyBodyVoxels(body_voxel_count));
    }
    let payload_bytes = PAYLOAD_HEADER_BYTES
        .checked_add(
            static_count
                .checked_mul(VOXEL_BYTES)
                .ok_or(SnapshotCodecError::SnapshotTooLarge(usize::MAX))?,
        )
        .and_then(|bytes| bytes.checked_add(body_count.checked_mul(BODY_HEADER_AND_STATE_BYTES)?))
        .and_then(|bytes| bytes.checked_add(body_voxel_count.checked_mul(VOXEL_BYTES)?))
        .ok_or(SnapshotCodecError::SnapshotTooLarge(usize::MAX))?;
    if payload_bytes > MAX_SNAPSHOT_PAYLOAD_BYTES {
        return Err(SnapshotCodecError::SnapshotTooLarge(payload_bytes));
    }

    let static_voxels = authority.world().occupied_voxels();
    if static_voxels.len() != static_count {
        return Err(SnapshotCodecError::InvalidPayload(
            "world solid count changed during capture",
        ));
    }
    let mut payload = Vec::with_capacity(payload_bytes);
    push_u64(&mut payload, authority.world().tick());
    push_u128(&mut payload, authority.world().fingerprint());
    push_u64(&mut payload, authority.next_body_id());
    push_u64(&mut payload, authority.next_sequence());
    push_u32(
        &mut payload,
        u32::try_from(static_count)
            .map_err(|_| SnapshotCodecError::TooManyStaticVoxels(static_count))?,
    );
    push_u32(
        &mut payload,
        u32::try_from(body_count).map_err(|_| SnapshotCodecError::TooManyBodies(body_count))?,
    );
    for (position, voxel) in static_voxels {
        encode_voxel(&mut payload, position, voxel);
    }
    for (&body_id, body) in authority.bodies() {
        let state = authority.body_states().get(&body_id).copied().ok_or(
            SnapshotCodecError::InvalidPayload("body descriptor has no dynamic state"),
        )?;
        push_u64(&mut payload, body_id);
        push_u32(
            &mut payload,
            u32::try_from(body.voxels.len())
                .map_err(|_| SnapshotCodecError::TooManyBodyVoxels(body.voxels.len()))?,
        );
        encode_body_state(&mut payload, state);
        for body_voxel in &body.voxels {
            encode_voxel(&mut payload, body_voxel.position, body_voxel.voxel);
        }
    }
    debug_assert_eq!(payload.len(), payload_bytes);
    Ok(payload)
}

fn decode_payload(payload: &[u8]) -> Result<AuthoritativeSnapshot, SnapshotCodecError> {
    if payload.len() > MAX_SNAPSHOT_PAYLOAD_BYTES {
        return Err(SnapshotCodecError::SnapshotTooLarge(payload.len()));
    }
    if payload.len() < PAYLOAD_HEADER_BYTES {
        return Err(SnapshotCodecError::Truncated);
    }
    let mut cursor = Cursor::new(payload);
    let world_tick = cursor.take_u64()?;
    let expected_world_fingerprint = cursor.take_u128()?;
    let next_body_id = cursor.take_u64()?;
    let next_sequence = cursor.take_u64()?;
    if next_body_id == 0 || next_sequence == 0 {
        return Err(SnapshotCodecError::InvalidPayload(
            "zero snapshot high-water mark",
        ));
    }
    let static_count = usize::try_from(cursor.take_u32()?)
        .map_err(|_| SnapshotCodecError::TooManyStaticVoxels(usize::MAX))?;
    let body_count = usize::try_from(cursor.take_u32()?)
        .map_err(|_| SnapshotCodecError::TooManyBodies(usize::MAX))?;
    if static_count > MAX_SNAPSHOT_STATIC_VOXELS {
        return Err(SnapshotCodecError::TooManyStaticVoxels(static_count));
    }
    if body_count > crate::replication::MAX_ACTIVE_BODIES {
        return Err(SnapshotCodecError::TooManyBodies(body_count));
    }
    let static_bytes = static_count
        .checked_mul(VOXEL_BYTES)
        .ok_or(SnapshotCodecError::SnapshotTooLarge(usize::MAX))?;
    if cursor.remaining().len() < static_bytes {
        return Err(SnapshotCodecError::Truncated);
    }
    let mut world = World::default();
    let mut previous_static = None;
    for _ in 0..static_count {
        let (position, voxel) = decode_voxel(&mut cursor)?;
        if !voxel.is_solid() || previous_static.is_some_and(|previous| previous >= position) {
            return Err(SnapshotCodecError::InvalidPayload(
                "static voxels are not solid and canonical",
            ));
        }
        world.set_voxel(position, voxel);
        previous_static = Some(position);
    }

    let mut bodies = BTreeMap::new();
    let mut body_states = BTreeMap::new();
    let mut total_body_voxels = 0_usize;
    let mut previous_body_id = None;
    for _ in 0..body_count {
        let body_id = cursor.take_u64()?;
        if body_id == 0 || previous_body_id.is_some_and(|previous| previous >= body_id) {
            return Err(SnapshotCodecError::InvalidPayload(
                "body IDs are not non-zero and canonical",
            ));
        }
        let voxel_count = usize::try_from(cursor.take_u32()?)
            .map_err(|_| SnapshotCodecError::TooManyBodyVoxels(usize::MAX))?;
        if voxel_count == 0 || voxel_count > BodyLimits::default().max_voxels {
            return Err(SnapshotCodecError::TooManyBodyVoxels(voxel_count));
        }
        total_body_voxels = total_body_voxels
            .checked_add(voxel_count)
            .ok_or(SnapshotCodecError::TooManyBodyVoxels(usize::MAX))?;
        if total_body_voxels > MAX_SNAPSHOT_BODY_VOXELS {
            return Err(SnapshotCodecError::TooManyBodyVoxels(total_body_voxels));
        }
        if cursor.remaining().len()
            < BODY_STATE_BYTES.saturating_add(voxel_count.saturating_mul(VOXEL_BYTES))
        {
            return Err(SnapshotCodecError::Truncated);
        }
        let state = decode_body_state(&mut cursor)?;
        let mut voxels = Vec::with_capacity(voxel_count);
        for _ in 0..voxel_count {
            let (position, voxel) = decode_voxel(&mut cursor)?;
            voxels.push(BodyVoxel { position, voxel });
        }
        let body =
            RigidBodyDescriptor::from_replicated_voxels(body_id, voxels, BodyLimits::default())?;
        bodies.insert(body_id, body);
        body_states.insert(body_id, state);
        previous_body_id = Some(body_id);
    }
    if !cursor.remaining().is_empty() {
        return Err(SnapshotCodecError::InvalidLength {
            expected: payload.len() - cursor.remaining().len(),
            actual: payload.len(),
        });
    }
    validate_world_fingerprint(&mut world, world_tick, expected_world_fingerprint)?;
    Ok(AuthoritativeSnapshot {
        snapshot_id: 0,
        world,
        bodies,
        body_states,
        next_body_id,
        next_sequence,
    })
}

const fn validate_world_fingerprint(
    world: &mut World,
    world_tick: u64,
    expected: u128,
) -> Result<(), SnapshotCodecError> {
    world.set_tick(world_tick);
    if world.fingerprint() != expected {
        return Err(SnapshotCodecError::PayloadHashMismatch);
    }
    Ok(())
}

fn encode_voxel(bytes: &mut Vec<u8>, position: crate::IVec3, voxel: Voxel) {
    push_i32(bytes, position.x);
    push_i32(bytes, position.y);
    push_i32(bytes, position.z);
    bytes.push(voxel.material as u8);
    bytes.push(voxel.integrity);
}

fn decode_voxel(cursor: &mut Cursor<'_>) -> Result<(crate::IVec3, Voxel), SnapshotCodecError> {
    let position = crate::IVec3::new(cursor.take_i32()?, cursor.take_i32()?, cursor.take_i32()?);
    let voxel = Voxel::from_wire(cursor.take_u8()?, cursor.take_u8()?)?;
    Ok((position, voxel))
}

fn encode_body_state(bytes: &mut Vec<u8>, state: RigidBodyState) {
    push_i64(bytes, state.translation_um.x);
    push_i64(bytes, state.translation_um.y);
    push_i64(bytes, state.translation_um.z);
    push_i64(bytes, state.linear_velocity_um_per_second.x);
    push_i64(bytes, state.linear_velocity_um_per_second.y);
    push_i64(bytes, state.linear_velocity_um_per_second.z);
    push_i32(bytes, state.orientation.x);
    push_i32(bytes, state.orientation.y);
    push_i32(bytes, state.orientation.z);
    push_i32(bytes, state.orientation.w);
    push_i64(bytes, state.angular_velocity_mrad_per_second.x);
    push_i64(bytes, state.angular_velocity_mrad_per_second.y);
    push_i64(bytes, state.angular_velocity_mrad_per_second.z);
    bytes.extend_from_slice(&state.integration_remainder);
    bytes.extend_from_slice(&state.angular_integration_remainder);
    push_u16(bytes, state.sleep_ticks);
    bytes.push(u8::from(state.sleeping));
}

fn decode_body_state(cursor: &mut Cursor<'_>) -> Result<RigidBodyState, SnapshotCodecError> {
    let state = RigidBodyState {
        translation_um: FixedMicrometers3 {
            x: cursor.take_i64()?,
            y: cursor.take_i64()?,
            z: cursor.take_i64()?,
        },
        linear_velocity_um_per_second: FixedMicrometers3 {
            x: cursor.take_i64()?,
            y: cursor.take_i64()?,
            z: cursor.take_i64()?,
        },
        orientation: FixedQuaternion {
            x: cursor.take_i32()?,
            y: cursor.take_i32()?,
            z: cursor.take_i32()?,
            w: cursor.take_i32()?,
        },
        angular_velocity_mrad_per_second: FixedMilliradians3 {
            x: cursor.take_i64()?,
            y: cursor.take_i64()?,
            z: cursor.take_i64()?,
        },
        integration_remainder: [cursor.take_u8()?, cursor.take_u8()?, cursor.take_u8()?],
        angular_integration_remainder: [cursor.take_u8()?, cursor.take_u8()?, cursor.take_u8()?],
        sleep_ticks: cursor.take_u16()?,
        sleeping: match cursor.take_u8()? {
            0 => false,
            1 => true,
            _ => {
                return Err(SnapshotCodecError::InvalidPayload(
                    "invalid body sleeping flag",
                ));
            }
        },
    };
    if !valid_rigid_body_state(state) {
        return Err(SnapshotCodecError::InvalidPayload(
            "invalid rigid-body state",
        ));
    }
    Ok(state)
}

fn snapshot_payload_hash(payload: &[u8]) -> u128 {
    let mut hash = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d_u128;
    for (index, byte) in payload.iter().copied().enumerate() {
        hash ^= u128::from(byte) | (u128::from(index as u64) << 8);
        hash = hash
            .rotate_left(17)
            .wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b_u128);
    }
    hash ^ (payload.len() as u128)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.offset..]
    }

    fn take_u8(&mut self) -> Result<u8, SnapshotCodecError> {
        Ok(self.take_array::<1>()?[0])
    }

    fn take_u16(&mut self) -> Result<u16, SnapshotCodecError> {
        Ok(u16::from_le_bytes(self.take_array()?))
    }

    fn take_u32(&mut self) -> Result<u32, SnapshotCodecError> {
        Ok(u32::from_le_bytes(self.take_array()?))
    }

    fn take_u64(&mut self) -> Result<u64, SnapshotCodecError> {
        Ok(u64::from_le_bytes(self.take_array()?))
    }

    fn take_u128(&mut self) -> Result<u128, SnapshotCodecError> {
        Ok(u128::from_le_bytes(self.take_array()?))
    }

    fn take_i32(&mut self) -> Result<i32, SnapshotCodecError> {
        Ok(i32::from_le_bytes(self.take_array()?))
    }

    fn take_i64(&mut self) -> Result<i64, SnapshotCodecError> {
        Ok(i64::from_le_bytes(self.take_array()?))
    }

    fn take_array<const N: usize>(&mut self) -> Result<[u8; N], SnapshotCodecError> {
        let end = self
            .offset
            .checked_add(N)
            .ok_or(SnapshotCodecError::Truncated)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(SnapshotCodecError::Truncated)?
            .try_into()
            .map_err(|_| SnapshotCodecError::Truncated)?;
        self.offset = end;
        Ok(value)
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

fn push_u128(bytes: &mut Vec<u8>, value: u128) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_i64(bytes: &mut Vec<u8>, value: i64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExplosionCommand, IVec3, demo_world};
    use core::mem::size_of;

    #[test]
    fn fragmented_snapshot_round_trips_out_of_order_and_installs() {
        let mut authority = AuthoritativeServer::new(demo_world());
        authority
            .execute_explosion(
                1,
                ExplosionCommand {
                    command_id: 1,
                    center: IVec3::new(-20, 6, 0),
                    radius_voxels: 8,
                    peak_energy: 30_000,
                },
            )
            .expect("authoritative damage");
        let _ = authority.advance_physics();
        assert!(
            !authority.bodies().is_empty(),
            "round trip must exercise actual body metadata"
        );
        let mut frames = encode_snapshot_frames(1, &authority, MAX_SNAPSHOT_DATAGRAM_BYTES)
            .expect("snapshot frames");
        assert!(frames.len() > 1);
        frames.reverse();

        let mut assembler = SnapshotAssembler::default();
        let mut snapshot = None;
        for frame in frames {
            snapshot = assembler.push(&frame).expect("valid snapshot").or(snapshot);
        }
        let snapshot = snapshot.expect("complete snapshot");
        assert_eq!(
            snapshot.world().fingerprint(),
            authority.world().fingerprint()
        );
        assert_eq!(snapshot.next_sequence(), authority.next_sequence());

        let mut replica = ClientReplica::new(World::default());
        snapshot
            .install_into(&mut replica)
            .expect("install snapshot");
        assert_eq!(
            replica.world().fingerprint(),
            authority.world().fingerprint()
        );
        assert_eq!(replica.bodies(), authority.bodies());
        for (id, body) in authority.bodies() {
            let restored = &replica.bodies()[id];
            assert_eq!(restored.mass_properties, body.mass_properties);
            assert_eq!(
                restored
                    .mass_properties
                    .inertia_about_rounded_center()
                    .unwrap(),
                body.mass_properties.inertia_about_rounded_center().unwrap()
            );
        }
        assert_eq!(replica.body_states(), authority.body_states());
        assert_eq!(replica.next_body_id(), authority.next_body_id());
    }

    #[test]
    fn missing_fragment_windows_are_bounded_and_exact() {
        let authority = AuthoritativeServer::new(demo_world());
        let frames = encode_snapshot_frames(7, &authority, MAX_SNAPSHOT_DATAGRAM_BYTES)
            .expect("snapshot frames");
        assert!(frames.len() > 65);
        let mut assembler = SnapshotAssembler::default();
        for (index, frame) in frames.iter().enumerate() {
            if index != 0 && index != 65 {
                assert!(
                    assembler
                        .push(frame)
                        .expect("valid incomplete snapshot")
                        .is_none()
                );
            }
        }

        assert_eq!(assembler.active_snapshot_id(), Some(7));
        assert_eq!(
            assembler.active_fragment_progress(),
            Some((frames.len() - 2, frames.len()))
        );
        assert_eq!(assembler.missing_fragment_windows(), vec![(0, 1), (64, 2)]);
    }

    #[test]
    fn corrupt_complete_snapshot_fails_hash_validation_and_releases_memory() {
        let authority = AuthoritativeServer::new(demo_world());
        let mut frames = encode_snapshot_frames(1, &authority, MAX_SNAPSHOT_DATAGRAM_BYTES)
            .expect("snapshot frames");
        let last = frames.last_mut().expect("snapshot has frames");
        *last.last_mut().expect("frame payload") ^= 1;
        let mut assembler = SnapshotAssembler::default();
        let mut error = None;
        for frame in frames {
            match assembler.push(&frame) {
                Ok(None) => {}
                Ok(Some(_snapshot)) => panic!("corrupt snapshot completed"),
                Err(found) => {
                    error = Some(found);
                    break;
                }
            }
        }
        assert!(matches!(
            error,
            Some(SnapshotCodecError::PayloadHashMismatch)
        ));
        assert_eq!(assembler.retained_payload_bytes(), 0);
    }

    #[test]
    fn malformed_snapshot_frames_fail_before_unbounded_allocation() {
        let authority = AuthoritativeServer::new(World::default());
        assert!(matches!(
            encode_snapshot_frames(0, &authority, MAX_SNAPSHOT_DATAGRAM_BYTES),
            Err(SnapshotCodecError::InvalidSnapshotId(0))
        ));
        assert!(matches!(
            encode_snapshot_frames(1, &authority, FRAME_HEADER_BYTES),
            Err(SnapshotCodecError::InvalidMtu(_))
        ));
        let mut frame = encode_snapshot_frames(1, &authority, MAX_SNAPSHOT_DATAGRAM_BYTES)
            .expect("empty-world snapshot")
            .remove(0);
        let mut old_version = frame.clone();
        old_version[4] = 1;
        assert!(matches!(
            SnapshotAssembler::default().push(&old_version),
            Err(SnapshotCodecError::UnsupportedVersion(1))
        ));
        frame.truncate(FRAME_HEADER_BYTES);
        assert!(matches!(
            SnapshotAssembler::default().push(&frame),
            Err(SnapshotCodecError::Truncated)
        ));
        assert!(matches!(
            SnapshotAssembler::default().push(&vec![0; MAX_SNAPSHOT_DATAGRAM_BYTES + 1]),
            Err(SnapshotCodecError::InvalidLength { .. })
        ));
    }

    #[test]
    fn snapshot_payload_rejects_a_noncanonical_orientation() {
        let mut authority = AuthoritativeServer::new(demo_world());
        authority
            .execute_explosion(
                1,
                ExplosionCommand {
                    command_id: 1,
                    center: IVec3::new(-20, 6, 0),
                    radius_voxels: 8,
                    peak_energy: 30_000,
                },
            )
            .expect("body-producing snapshot fixture");
        assert_eq!(authority.bodies().len(), 1);
        let mut payload = encode_payload(&authority).expect("canonical snapshot payload");
        let body_state_start = PAYLOAD_HEADER_BYTES
            + authority.world().stats().solid_voxels * VOXEL_BYTES
            + size_of::<u64>()
            + size_of::<u32>();
        let orientation_w_start = body_state_start + 6 * size_of::<i64>() + 3 * size_of::<i32>();
        payload[orientation_w_start..orientation_w_start + size_of::<i32>()].fill(0);

        assert!(matches!(
            decode_payload(&payload),
            Err(SnapshotCodecError::InvalidPayload(
                "invalid rigid-body state"
            ))
        ));
    }
}
