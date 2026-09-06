use crate::material::Voxel;
use core::fmt;
use std::{collections::HashMap, collections::hash_map::Entry, sync::Arc};

pub mod geometry;
pub mod query;

/// Uniform-only geometry keeps the existing dense two-byte cells and no extension allocation.
#[derive(Clone, Default)]
pub struct UniformGeometry;

pub const CHUNK_EDGE: i32 = 16;
const CHUNK_EDGE_USIZE: usize = CHUNK_EDGE as usize;
const VOXELS_PER_CHUNK: usize = CHUNK_EDGE_USIZE * CHUNK_EDGE_USIZE * CHUNK_EDGE_USIZE;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct IVec3 {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl IVec3 {
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    #[must_use]
    pub fn squared_distance(self, other: Self) -> u64 {
        let dx = i128::from(self.x) - i128::from(other.x);
        let dy = i128::from(self.y) - i128::from(other.y);
        let dz = i128::from(self.z) - i128::from(other.z);
        let squared = (dx * dx + dy * dy + dz * dz).cast_unsigned();
        u64::try_from(squared).unwrap_or(u64::MAX)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VoxelChange {
    pub position: IVec3,
    pub before: Voxel,
    pub after: Voxel,
}

#[derive(Clone)]
struct Chunk<G> {
    voxels: Box<[Voxel; VOXELS_PER_CHUNK]>,
    solid_voxels: usize,
    // Local mutation counter only: wraps and restarts after reclamation. Observations use the
    // retained Arc identity, never this counter as a globally monotonic cache/version key.
    revision: u64,
    geometry: G,
}

impl<G: Default> Default for Chunk<G> {
    fn default() -> Self {
        Self {
            voxels: Box::new([Voxel::AIR; VOXELS_PER_CHUNK]),
            solid_voxels: 0,
            revision: 0,
            geometry: G::default(),
        }
    }
}

#[derive(Clone, Default)]
pub struct WorldStorage<G> {
    chunks: HashMap<IVec3, Arc<Chunk<G>>>,
    chunk_capacity_high_water: usize,
    // Retained by absent-chunk observations. Allocation/reclamation replaces the token, so an
    // empty -> occupied -> empty ABA cannot make a previously observed absence look unchanged.
    vacancy_epoch: Arc<()>,
    tick: u64,
    fingerprint: u128,
    solid_voxels: usize,
}

/// Existing gameplay consumers accept only this uniform specialization.
pub type World = WorldStorage<UniformGeometry>;

pub(crate) struct ChunkObservation<G = UniformGeometry> {
    position: IVec3,
    state: ObservedChunk<G>,
}

enum ObservedChunk<G> {
    Occupied(Arc<Chunk<G>>),
    Absent(Arc<()>),
}

impl<G> ChunkObservation<G> {
    pub(crate) fn matches(&self, world: &WorldStorage<G>) -> bool {
        match &self.state {
            ObservedChunk::Occupied(expected) => world
                .chunks
                .get(&self.position)
                .is_some_and(|actual| Arc::ptr_eq(expected, actual)),
            ObservedChunk::Absent(expected) => {
                !world.chunks.contains_key(&self.position)
                    && Arc::ptr_eq(expected, &world.vacancy_epoch)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorldStats {
    pub chunks: usize,
    pub solid_voxels: usize,
    pub bytes_dense_payload: usize,
    pub tick: u64,
    pub fingerprint: u128,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorldError {
    DuplicateOrUnsortedChange(IVec3),
    BeforeStateMismatch {
        position: IVec3,
        expected: Voxel,
        actual: Voxel,
    },
    FinalFingerprintMismatch {
        expected: u128,
        actual: u128,
    },
}

impl fmt::Display for WorldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateOrUnsortedChange(position) => {
                write!(formatter, "duplicate or unsorted change at {position:?}")
            }
            Self::BeforeStateMismatch {
                position,
                expected,
                actual,
            } => write!(
                formatter,
                "voxel mismatch at {position:?}: expected {expected:?}, found {actual:?}"
            ),
            Self::FinalFingerprintMismatch { expected, actual } => write!(
                formatter,
                "final fingerprint mismatch: expected {expected:032x}, found {actual:032x}"
            ),
        }
    }
}

impl std::error::Error for WorldError {}

impl World {
    #[must_use]
    pub fn voxel(&self, position: IVec3) -> Voxel {
        let (chunk_position, local) = split_position(position);
        self.chunks
            .get(&chunk_position)
            .map_or(Voxel::AIR, |chunk| chunk.voxels[local_index(local)])
    }

    /// Returns the previous voxel. Empty chunks are reclaimed immediately.
    pub fn set_voxel(&mut self, position: IVec3, after: Voxel) -> Voxel {
        let before = self.voxel(position);
        if before == after {
            return before;
        }

        let (chunk_position, local) = split_position(position);
        let should_remove;
        let inserted;
        {
            let chunk = match self.chunks.entry(chunk_position) {
                Entry::Occupied(entry) => {
                    inserted = false;
                    entry.into_mut()
                }
                Entry::Vacant(entry) => {
                    inserted = true;
                    self.vacancy_epoch = Arc::new(());
                    entry.insert(Arc::default())
                }
            };
            let chunk = Arc::make_mut(chunk);
            let slot = &mut chunk.voxels[local_index(local)];
            if before.is_solid() && !after.is_solid() {
                chunk.solid_voxels -= 1;
                self.solid_voxels -= 1;
            } else if !before.is_solid() && after.is_solid() {
                chunk.solid_voxels += 1;
                self.solid_voxels += 1;
            }
            *slot = after;
            chunk.revision = chunk.revision.wrapping_add(1);
            should_remove = chunk.solid_voxels == 0;
        }

        if inserted {
            self.chunk_capacity_high_water =
                self.chunk_capacity_high_water.max(self.chunks.capacity());
        }

        self.fingerprint ^= voxel_token(position, before) ^ voxel_token(position, after);
        if should_remove {
            self.chunks.remove(&chunk_position);
            self.vacancy_epoch = Arc::new(());
        }
        before
    }

    /// Fills an inclusive axis-aligned region.
    ///
    /// # Panics
    ///
    /// Panics when any minimum coordinate is greater than its matching maximum coordinate.
    pub fn fill_box(&mut self, minimum: IVec3, maximum: IVec3, voxel: Voxel) {
        assert!(minimum.x <= maximum.x);
        assert!(minimum.y <= maximum.y);
        assert!(minimum.z <= maximum.z);
        for x in minimum.x..=maximum.x {
            for y in minimum.y..=maximum.y {
                for z in minimum.z..=maximum.z {
                    self.set_voxel(IVec3::new(x, y, z), voxel);
                }
            }
        }
    }

    /// Applies a replicated transaction atomically. A failed final hash rolls back all writes.
    ///
    /// # Errors
    ///
    /// Rejects unsorted or duplicate changes, stale before-states, and a wrong final fingerprint.
    pub fn apply_checked(
        &mut self,
        changes: &[VoxelChange],
        expected_fingerprint: u128,
    ) -> Result<(), WorldError> {
        for pair in changes.windows(2) {
            if pair[0].position >= pair[1].position {
                return Err(WorldError::DuplicateOrUnsortedChange(pair[1].position));
            }
        }
        for change in changes {
            let actual = self.voxel(change.position);
            if actual != change.before {
                return Err(WorldError::BeforeStateMismatch {
                    position: change.position,
                    expected: change.before,
                    actual,
                });
            }
        }
        for change in changes {
            self.set_voxel(change.position, change.after);
        }
        if self.fingerprint != expected_fingerprint {
            let actual = self.fingerprint;
            for change in changes.iter().rev() {
                self.set_voxel(change.position, change.before);
            }
            return Err(WorldError::FinalFingerprintMismatch {
                expected: expected_fingerprint,
                actual,
            });
        }
        Ok(())
    }

    /// Returns every occupied voxel in canonical world-coordinate order.
    #[must_use]
    pub fn occupied_voxels(&self) -> Vec<(IVec3, Voxel)> {
        let mut voxels = Vec::with_capacity(self.solid_voxels);
        for chunk_position in self.chunk_positions() {
            let Some(chunk) = self.chunks.get(&chunk_position) else {
                continue;
            };
            for local_z in 0..CHUNK_EDGE {
                for local_y in 0..CHUNK_EDGE {
                    for local_x in 0..CHUNK_EDGE {
                        let local = IVec3::new(local_x, local_y, local_z);
                        let voxel = chunk.voxels[local_index(local)];
                        if voxel.is_solid() {
                            voxels.push((
                                IVec3::new(
                                    chunk_position.x * CHUNK_EDGE + local_x,
                                    chunk_position.y * CHUNK_EDGE + local_y,
                                    chunk_position.z * CHUNK_EDGE + local_z,
                                ),
                                voxel,
                            ));
                        }
                    }
                }
            }
        }
        voxels.sort_unstable_by_key(|(position, _voxel)| *position);
        voxels
    }

    /// Slow reference implementation used by tests and periodic server audits.
    #[must_use]
    pub fn recompute_fingerprint(&self) -> u128 {
        let mut result = 0_u128;
        for (chunk_position, chunk) in &self.chunks {
            for local_z in 0..CHUNK_EDGE {
                for local_y in 0..CHUNK_EDGE {
                    for local_x in 0..CHUNK_EDGE {
                        let local = IVec3::new(local_x, local_y, local_z);
                        let voxel = chunk.voxels[local_index(local)];
                        if voxel.is_solid() {
                            let position = IVec3::new(
                                chunk_position.x * CHUNK_EDGE + local_x,
                                chunk_position.y * CHUNK_EDGE + local_y,
                                chunk_position.z * CHUNK_EDGE + local_z,
                            );
                            result ^= voxel_token(position, voxel);
                        }
                    }
                }
            }
        }
        result
    }
}

impl<G: Clone + Default> WorldStorage<G> {
    /// Bound `HashMap` storage as well as live entries: reclamation need not shrink its allocation.
    pub(crate) fn bounded_snapshot(&self, maximum_chunks: usize) -> Option<Self> {
        if self.chunks.len() > maximum_chunks
            || self.chunk_capacity_high_water > maximum_chunks.saturating_mul(2)
        {
            return None;
        }
        Some(self.clone())
    }

    pub(crate) fn observe_chunk(&self, position: IVec3) -> ChunkObservation<G> {
        ChunkObservation {
            position,
            state: self.chunks.get(&position).map_or_else(
                || ObservedChunk::Absent(Arc::clone(&self.vacancy_epoch)),
                |chunk| ObservedChunk::Occupied(Arc::clone(chunk)),
            ),
        }
    }

    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    pub(crate) const fn set_tick(&mut self, tick: u64) {
        self.tick = tick;
    }

    #[must_use]
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    /// Returns occupied chunk coordinates in a stable order for deterministic meshing.
    #[must_use]
    pub fn chunk_positions(&self) -> Vec<IVec3> {
        let mut positions: Vec<_> = self.chunks.keys().copied().collect();
        positions.sort_unstable();
        positions
    }

    #[must_use]
    pub fn stats(&self) -> WorldStats {
        WorldStats {
            chunks: self.chunks.len(),
            solid_voxels: self.solid_voxels,
            bytes_dense_payload: self.chunks.len() * VOXELS_PER_CHUNK * size_of::<Voxel>(),
            tick: self.tick,
            fingerprint: self.fingerprint,
        }
    }
}

/// Maps a world voxel coordinate to its chunk, including across negative axes.
#[must_use]
pub const fn chunk_position(position: IVec3) -> IVec3 {
    IVec3::new(
        position.x.div_euclid(CHUNK_EDGE),
        position.y.div_euclid(CHUNK_EDGE),
        position.z.div_euclid(CHUNK_EDGE),
    )
}

const fn split_position(position: IVec3) -> (IVec3, IVec3) {
    (
        chunk_position(position),
        IVec3::new(
            position.x.rem_euclid(CHUNK_EDGE),
            position.y.rem_euclid(CHUNK_EDGE),
            position.z.rem_euclid(CHUNK_EDGE),
        ),
    )
}

fn local_index(local: IVec3) -> usize {
    let x = usize::try_from(local.x).unwrap_or_default();
    let y = usize::try_from(local.y).unwrap_or_default();
    let z = usize::try_from(local.z).unwrap_or_default();
    x + y * CHUNK_EDGE_USIZE + z * CHUNK_EDGE_USIZE * CHUNK_EDGE_USIZE
}

fn voxel_token(position: IVec3, voxel: Voxel) -> u128 {
    if !voxel.is_solid() {
        return 0;
    }
    let x = u64::from(u32::from_ne_bytes(position.x.to_ne_bytes()));
    let y = u64::from(u32::from_ne_bytes(position.y.to_ne_bytes()));
    let z = u64::from(u32::from_ne_bytes(position.z.to_ne_bytes()));
    let state = u64::from(voxel.material as u8) << 8 | u64::from(voxel.integrity);
    let first = splitmix64(x ^ y.rotate_left(21) ^ z.rotate_left(42) ^ state.rotate_left(7));
    let second = splitmix64(
        z ^ x.rotate_left(17) ^ y.rotate_left(39) ^ state.rotate_left(51) ^ 0xd6e8_feb8_6659_fd93,
    );
    u128::from(first) << 64 | u128::from(second)
}

const fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::Material;

    #[test]
    fn copy_on_write_snapshots_share_only_unchanged_chunks() {
        let first = IVec3::new(0, 0, 0);
        let other = IVec3::new(32, 0, 0);
        let brick = Voxel::new(Material::Brick);
        let mut live = World::default();
        live.set_voxel(first, brick);
        live.set_voxel(other, brick);
        let snapshot = live.clone();
        let first_proof = snapshot.observe_chunk(chunk_position(first));
        let other_proof = snapshot.observe_chunk(chunk_position(other));
        live.set_voxel(first, brick);
        assert!(
            first_proof.matches(&live),
            "no-op write must keep the snapshot valid"
        );
        live.set_voxel(first, Voxel::new(Material::Steel));
        assert!(!first_proof.matches(&live));
        assert!(other_proof.matches(&live));
        assert_eq!(snapshot.voxel(first), brick);
        assert_eq!(snapshot.fingerprint(), snapshot.recompute_fingerprint());
        assert_eq!(live.fingerprint(), live.recompute_fingerprint());
        assert_ne!(snapshot.fingerprint(), live.fingerprint());
    }

    #[test]
    fn occupied_and_absent_chunk_observations_reject_aba() {
        let position = IVec3::new(-17, 4, -1);
        let brick = Voxel::new(Material::Brick);
        let mut world = World::default();
        let absent = world.observe_chunk(chunk_position(position));
        assert!(absent.matches(&world));
        world.set_voxel(position, brick);
        assert!(!absent.matches(&world));
        let occupied = world.observe_chunk(chunk_position(position));
        world.set_voxel(position, Voxel::new(Material::Wood));
        world.set_voxel(position, brick);
        assert!(
            !occupied.matches(&world),
            "same voxel bytes must not restore a retired token"
        );
        let occupied = world.observe_chunk(chunk_position(position));
        world.set_voxel(position, Voxel::AIR);
        assert!(
            !absent.matches(&world),
            "chunk reclamation must not restore observed absence"
        );
        world.set_voxel(position, brick);
        assert!(
            !occupied.matches(&world),
            "recreated chunk must have a new retained identity"
        );
    }

    #[test]
    fn rollback_restores_content_without_revalidating_an_old_chunk_observation() {
        let position = IVec3::new(1, 1, 1);
        let before = Voxel::new(Material::Brick);
        let mut world = World::default();
        world.set_voxel(position, before);
        let snapshot = world.clone();
        let observed = world.observe_chunk(chunk_position(position));
        let change = VoxelChange {
            position,
            before,
            after: Voxel::AIR,
        };
        assert!(world.apply_checked(&[change], 1).is_err());
        assert_eq!(world.fingerprint(), snapshot.fingerprint());
        assert_eq!(world.voxel(position), before);
        assert!(!observed.matches(&world));
    }

    #[test]
    fn negative_coordinates_round_trip() {
        let mut world = World::default();
        let position = IVec3::new(-17, -1, -16);
        world.set_voxel(position, Voxel::new(Material::Steel));
        assert_eq!(world.voxel(position), Voxel::new(Material::Steel));
        assert_eq!(world.stats().chunks, 1);
        assert_eq!(world.fingerprint(), world.recompute_fingerprint());
    }

    #[test]
    fn empty_chunks_are_reclaimed() {
        let mut world = World::default();
        let position = IVec3::new(40, 2, 3);
        world.set_voxel(position, Voxel::new(Material::Wood));
        world.set_voxel(position, Voxel::AIR);
        assert_eq!(world.stats().chunks, 0);
        assert_eq!(world.fingerprint(), 0);
    }

    #[test]
    fn occupied_voxels_are_canonical_across_chunk_boundaries() {
        let mut world = World::default();
        let positions = [
            IVec3::new(16, 0, 0),
            IVec3::new(-1, 0, 0),
            IVec3::new(0, 1, 0),
            IVec3::new(0, 0, 1),
        ];
        for position in positions {
            world.set_voxel(position, Voxel::new(Material::Stone));
        }

        let occupied = world.occupied_voxels();
        assert_eq!(
            occupied
                .iter()
                .map(|(position, _voxel)| *position)
                .collect::<Vec<_>>(),
            vec![
                IVec3::new(-1, 0, 0),
                IVec3::new(0, 0, 1),
                IVec3::new(0, 1, 0),
                IVec3::new(16, 0, 0),
            ]
        );
    }
}
