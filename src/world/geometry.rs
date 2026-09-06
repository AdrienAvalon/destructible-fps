//! Typed migration of World's storage to exact, sparse, locally refined cells.
//!
//! This specialization deliberately cannot enter the uniform-only gameplay consumers yet.
//! There is no representative material or implicit coarse fallback for a partial cell.
//!
//! ```compile_fail
//! use destructible_fps::{AuthoritativeServer, world::geometry::RefinedWorld};
//! let fine = RefinedWorld::default();
//! let _ = AuthoritativeServer::new(fine);
//! ```
//!
//! ```compile_fail
//! use destructible_fps::{IVec3, world::geometry::RefinedWorld};
//! let fine = RefinedWorld::default();
//! let _ = fine.voxel(IVec3::new(0, 0, 0)); // No uniform representative for a fine cell.
//! ```

pub mod wire;

use super::{
    Chunk, IVec3, World, WorldStorage, local_index, split_position, splitmix64, voxel_token,
};
use crate::{Voxel, volume::RefinedVolume};
use core::fmt;
use std::{collections::BTreeMap, sync::Arc};

pub const MAX_GEOMETRY_CHUNKS: usize = 512;
pub const MAX_GEOMETRY_CELLS: usize = 262_144;
pub const MAX_REFINED_PAGES: usize = 4_096;
pub const MAX_REFINED_LEAVES: usize = 131_072;
pub const MAX_GEOMETRY_CHANGES: usize = 256;
/// Counts BOTH before and after pages, including pages which do not change.
pub const MAX_TRANSACTION_LEAVES: usize = 32_768;

#[derive(Clone, Default)]
pub struct RefinedGeometry {
    // Dense slot is canonical AIR when an extension exists. Only the typed cell API reads it.
    pages: BTreeMap<usize, RefinedVolume>,
    leaves: usize,
}

pub type RefinedWorld = WorldStorage<RefinedGeometry>;

/// Canonical one-metre cell. Construction collapses uniform volumes, including all-air pages.
#[derive(Clone, Debug)]
pub struct GeometryCell(CellValue);

#[derive(Clone, Debug)]
enum CellValue {
    Uniform(Voxel),
    Refined(RefinedVolume),
}

impl PartialEq for GeometryCell {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (CellValue::Uniform(a), CellValue::Uniform(b)) => a == b,
            (CellValue::Refined(a), CellValue::Refined(b)) => a.leaves() == b.leaves(),
            _ => false,
        }
    }
}
impl Eq for GeometryCell {}

impl GeometryCell {
    pub const AIR: Self = Self(CellValue::Uniform(Voxel::AIR));

    #[must_use]
    pub const fn uniform(voxel: Voxel) -> Self {
        Self(CellValue::Uniform(if voxel.is_solid() {
            voxel
        } else {
            Voxel::AIR
        }))
    }

    #[must_use]
    pub fn refined(volume: RefinedVolume) -> Self {
        volume
            .uniform_voxel()
            .map_or_else(|| Self(CellValue::Refined(volume)), Self::uniform)
    }

    #[must_use]
    pub const fn uniform_voxel(&self) -> Option<Voxel> {
        match self.0 {
            CellValue::Uniform(voxel) => Some(voxel),
            CellValue::Refined(_) => None,
        }
    }

    #[must_use]
    pub const fn volume(&self) -> Option<&RefinedVolume> {
        match &self.0 {
            CellValue::Uniform(_) => None,
            CellValue::Refined(volume) => Some(volume),
        }
    }

    #[must_use]
    pub fn solid_units(&self) -> u32 {
        match &self.0 {
            CellValue::Uniform(voxel) => u32::from(voxel.is_solid()) * crate::volume::VOLUME_UNITS,
            CellValue::Refined(volume) => volume.solid_units(),
        }
    }

    fn leaves(&self) -> usize {
        self.volume().map_or(0, |volume| volume.leaves().len())
    }

    fn token(&self, position: IVec3) -> u128 {
        match &self.0 {
            CellValue::Uniform(voxel) => voxel_token(position, *voxel),
            CellValue::Refined(volume) => {
                let hash = volume.fingerprint();
                let low = u64::try_from(hash & u128::from(u64::MAX)).unwrap_or_default();
                let high = u64::try_from(hash >> 64).unwrap_or_default();
                let [x, y, z] = [position.x, position.y, position.z]
                    .map(|v| u64::from(u32::from_ne_bytes(v.to_ne_bytes())));
                let first = splitmix64(
                    low ^ x ^ y.rotate_left(21) ^ z.rotate_left(42) ^ 0xf529_b7d8_063a_c145,
                );
                let second = splitmix64(
                    high ^ z ^ x.rotate_left(17) ^ y.rotate_left(39) ^ 0x439c_785e_b102_6fad,
                );
                u128::from(first) << 64 | u128::from(second)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GeometryStats {
    pub chunks: usize,
    /// Occupied metre pages, NOT the number of fine solids or their volume.
    pub occupied_cells: usize,
    pub refined_pages: usize,
    pub refined_leaves: usize,
}

impl GeometryStats {
    const fn validate(self) -> Result<(), GeometryError> {
        if self.chunks > MAX_GEOMETRY_CHUNKS
            || self.occupied_cells > MAX_GEOMETRY_CELLS
            || self.refined_pages > MAX_REFINED_PAGES
            || self.refined_leaves > MAX_REFINED_LEAVES
            // Every accepted component state must remain checkpointable. Uniform records cost
            // 15B; replacing one by a page adds 11B plus 8B per canonical leaf, header 41B.
            || 41 + 15 * self.occupied_cells + 11 * self.refined_pages + 8 * self.refined_leaves
                > wire::MAX_GEOMETRY_CHECKPOINT_BYTES
        {
            return Err(GeometryError::WorldBudget);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeometryChange {
    pub position: IVec3,
    pub before: GeometryCell,
    pub after: GeometryCell,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GeometryError {
    WorldBudget,
    TransactionBudget,
    Order,
    BeforeState(IVec3),
    Fingerprint,
    Sequence,
    Tick,
    RequiresFineConsumers,
    Encoding,
    Allocation,
}
impl fmt::Display for GeometryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "world geometry refused: {self:?}")
    }
}
impl std::error::Error for GeometryError {}

impl RefinedWorld {
    /// Promote an immutable coarse world after checking aggregate resident bounds.
    /// # Errors
    /// Refuses an oversized source before cloning its dense cell payload.
    pub fn from_uniform(source: &World) -> Result<Self, GeometryError> {
        GeometryStats {
            chunks: source.chunks.len(),
            occupied_cells: source.solid_voxels,
            ..GeometryStats::default()
        }
        .validate()?;
        let mut promoted = Self {
            chunks: source
                .chunks
                .iter()
                .map(|(&position, chunk)| {
                    (
                        position,
                        Arc::new(Chunk {
                            voxels: chunk.voxels.clone(),
                            solid_voxels: chunk.solid_voxels,
                            revision: chunk.revision,
                            geometry: RefinedGeometry::default(),
                        }),
                    )
                })
                .collect(),
            chunk_capacity_high_water: 0,
            vacancy_epoch: Arc::default(),
            tick: source.tick,
            fingerprint: source.fingerprint,
            solid_voxels: source.solid_voxels,
        };
        promoted.chunk_capacity_high_water = promoted.chunks.capacity();
        Ok(promoted)
    }

    /// Explicit checked downgrade. No shape/material approximation is permitted.
    /// # Errors
    /// Any refined cell refuses the entire conversion, leaving the source intact.
    pub fn to_uniform(&self) -> Result<World, GeometryError> {
        if self
            .chunks
            .values()
            .any(|chunk| !chunk.geometry.pages.is_empty())
        {
            return Err(GeometryError::RequiresFineConsumers);
        }
        let mut uniform = World {
            chunks: self
                .chunks
                .iter()
                .map(|(&position, chunk)| {
                    (
                        position,
                        Arc::new(Chunk {
                            voxels: chunk.voxels.clone(),
                            solid_voxels: chunk.solid_voxels,
                            revision: chunk.revision,
                            geometry: super::UniformGeometry,
                        }),
                    )
                })
                .collect(),
            chunk_capacity_high_water: 0,
            vacancy_epoch: Arc::default(),
            tick: self.tick,
            fingerprint: self.fingerprint,
            solid_voxels: self.solid_voxels,
        };
        uniform.chunk_capacity_high_water = uniform.chunks.capacity();
        Ok(uniform)
    }

    #[must_use]
    pub fn cell(&self, position: IVec3) -> GeometryCell {
        let (chunk_position, local) = split_position(position);
        self.chunks
            .get(&chunk_position)
            .map_or(GeometryCell::AIR, |chunk| {
                let index = local_index(local);
                chunk.geometry.pages.get(&index).map_or_else(
                    || GeometryCell::uniform(chunk.voxels[index]),
                    |volume| GeometryCell::refined(volume.clone()),
                )
            })
    }

    #[must_use]
    pub fn geometry_stats(&self) -> GeometryStats {
        GeometryStats {
            chunks: self.chunks.len(),
            occupied_cells: self.solid_voxels,
            refined_pages: self
                .chunks
                .values()
                .map(|chunk| chunk.geometry.pages.len())
                .sum(),
            refined_leaves: self
                .chunks
                .values()
                .map(|chunk| chunk.geometry.leaves)
                .sum(),
        }
    }

    /// Full exact cells, in the same canonical XYZ order as coarse `occupied_voxels`.
    #[must_use]
    pub fn occupied_cells(&self) -> Vec<(IVec3, GeometryCell)> {
        self.occupied_positions()
            .into_iter()
            .map(|position| (position, self.cell(position)))
            .collect()
    }

    /// Sparse fine-page coordinates only; no dense scan or material approximation. Ordering is
    /// unspecified, and the iterator borrows the same immutable geometry snapshot.
    pub fn refined_positions(&self) -> impl Iterator<Item = IVec3> + '_ {
        self.chunks.iter().flat_map(|(&position, chunk)| {
            chunk.geometry.pages.keys().map(move |&index| {
                let edge = super::CHUNK_EDGE_USIZE;
                IVec3::new(
                    position.x * super::CHUNK_EDGE
                        + i32::try_from(index % edge).unwrap_or_default(),
                    position.y * super::CHUNK_EDGE
                        + i32::try_from(index / edge % edge).unwrap_or_default(),
                    position.z * super::CHUNK_EDGE
                        + i32::try_from(index / (edge * edge)).unwrap_or_default(),
                )
            })
        })
    }

    // The checkpoint sorts only 12-byte coordinates, not wide GeometryCell enums/Arc clones.
    fn occupied_positions(&self) -> Vec<IVec3> {
        let mut positions = Vec::with_capacity(self.solid_voxels);
        for (&position, chunk) in &self.chunks {
            for index in 0..super::VOXELS_PER_CHUNK {
                if chunk.voxels[index].is_solid() || chunk.geometry.pages.contains_key(&index) {
                    let edge = super::CHUNK_EDGE_USIZE;
                    let x = i32::try_from(index % edge).unwrap_or_default();
                    let y = i32::try_from(index / edge % edge).unwrap_or_default();
                    let z = i32::try_from(index / (edge * edge)).unwrap_or_default();
                    positions.push(IVec3::new(
                        position.x * super::CHUNK_EDGE + x,
                        position.y * super::CHUNK_EDGE + y,
                        position.z * super::CHUNK_EDGE + z,
                    ));
                }
            }
        }
        positions.sort_unstable();
        positions
    }

    #[must_use]
    pub fn recompute_fingerprint(&self) -> u128 {
        self.occupied_positions()
            .iter()
            .fold(0, |hash, pos| hash ^ self.cell(*pos).token(*pos))
    }

    // Infallible after complete semantic/resource validation. Standard allocator OOM behavior
    // still applies to HashMap/BTreeMap/Arc, as in the existing engine; no process OOM guarantee.
    fn set_cell(&mut self, position: IVec3, after: &GeometryCell) {
        let before = self.cell(position);
        if before == *after {
            return;
        }
        let (chunk_position, local) = split_position(position);
        let index = local_index(local);
        if !self.chunks.contains_key(&chunk_position) {
            self.vacancy_epoch = Arc::default();
        }
        let chunk = Arc::make_mut(self.chunks.entry(chunk_position).or_default());
        if let Some(previous) = chunk.geometry.pages.remove(&index) {
            chunk.geometry.leaves -= previous.leaves().len();
        }
        match &after.0 {
            CellValue::Uniform(voxel) => chunk.voxels[index] = *voxel,
            CellValue::Refined(volume) => {
                chunk.voxels[index] = Voxel::AIR;
                chunk.geometry.leaves += volume.leaves().len();
                chunk.geometry.pages.insert(index, volume.clone());
            }
        }
        if before.solid_units() != 0 {
            chunk.solid_voxels -= 1;
            self.solid_voxels -= 1;
        }
        if after.solid_units() != 0 {
            chunk.solid_voxels += 1;
            self.solid_voxels += 1;
        }
        chunk.revision = chunk.revision.wrapping_add(1);
        let empty = chunk.solid_voxels == 0;
        self.chunk_capacity_high_water = self.chunk_capacity_high_water.max(self.chunks.capacity());
        self.fingerprint ^= before.token(position) ^ after.token(position);
        if empty {
            self.chunks.remove(&chunk_position);
            self.vacancy_epoch = Arc::default();
        }
    }

    // Computes final residency before candidate writes; removal before insertion keeps candidate
    // peak residency <= max(original,final). Retained COW readers are a separate caller budget.
    fn validate_changes(&self, changes: &[GeometryChange]) -> Result<u128, GeometryError> {
        if changes.len() > MAX_GEOMETRY_CHANGES {
            return Err(GeometryError::TransactionBudget);
        }
        let mut stats = self.geometry_stats();
        let mut chunk_counts: BTreeMap<_, _> = self
            .chunks
            .iter()
            .map(|(&position, chunk)| (position, chunk.solid_voxels))
            .collect();
        let mut leaves = 0;
        let mut fingerprint = self.fingerprint;
        let mut previous = None;
        for change in changes {
            if previous.is_some_and(|p| p >= change.position) {
                return Err(GeometryError::Order);
            }
            previous = Some(change.position);
            leaves += change.before.leaves() + change.after.leaves();
            if leaves > MAX_TRANSACTION_LEAVES {
                return Err(GeometryError::TransactionBudget);
            }
            if self.cell(change.position) != change.before {
                return Err(GeometryError::BeforeState(change.position));
            }
            let occupied_before = usize::from(change.before.solid_units() != 0);
            let occupied_after = usize::from(change.after.solid_units() != 0);
            stats.occupied_cells = stats.occupied_cells - occupied_before + occupied_after;
            stats.refined_pages = stats.refined_pages
                - usize::from(change.before.volume().is_some())
                + usize::from(change.after.volume().is_some());
            stats.refined_leaves =
                stats.refined_leaves - change.before.leaves() + change.after.leaves();
            let count = chunk_counts
                .entry(super::chunk_position(change.position))
                .or_default();
            *count = *count - occupied_before + occupied_after;
            fingerprint ^=
                change.before.token(change.position) ^ change.after.token(change.position);
        }
        stats.chunks = chunk_counts.values().filter(|&&n| n != 0).count();
        stats.validate()?;
        Ok(fingerprint)
    }

    fn apply_changes(
        &mut self,
        changes: &[GeometryChange],
        expected: u128,
    ) -> Result<(), GeometryError> {
        if self.validate_changes(changes)? != expected {
            return Err(GeometryError::Fingerprint);
        }
        let mut candidate = self.clone();
        for change in changes.iter().filter(|c| c.before != c.after) {
            candidate.set_cell(change.position, &GeometryCell::AIR);
        }
        for change in changes.iter().filter(|c| c.before != c.after) {
            candidate.set_cell(change.position, &change.after);
        }
        if candidate.fingerprint != expected {
            return Err(GeometryError::Fingerprint);
        }
        *self = candidate;
        Ok(())
    }
}

/// Geometry component state, not a second network authority or a complete game snapshot.
#[derive(Clone)]
pub struct GeometryState {
    world: RefinedWorld,
    next_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeometryTransaction {
    sequence: u64,
    tick: u64,
    before: u128,
    after: u128,
    changes: Vec<GeometryChange>,
}

impl GeometryState {
    /// # Errors
    /// Refuses worlds beyond the geometry residency limits or zero sequence high-water marks.
    pub fn new(world: RefinedWorld, next_sequence: u64) -> Result<Self, GeometryError> {
        world.geometry_stats().validate()?;
        if next_sequence == 0 {
            return Err(GeometryError::Sequence);
        }
        Ok(Self {
            world,
            next_sequence,
        })
    }

    #[must_use]
    pub const fn world(&self) -> &RefinedWorld {
        &self.world
    }

    #[must_use]
    pub const fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    /// Prepares an exact state transition. This is internal authority output, NEVER client intent.
    /// # Errors
    /// Rejects stale before-states, ordering, bounds, regressing ticks or exhausted sequencing.
    pub fn prepare(
        &self,
        tick: u64,
        changes: Vec<GeometryChange>,
    ) -> Result<GeometryTransaction, GeometryError> {
        if tick < self.world.tick {
            return Err(GeometryError::Tick);
        }
        self.next_sequence
            .checked_add(1)
            .ok_or(GeometryError::Sequence)?;
        let after = self.world.validate_changes(&changes)?;
        Ok(GeometryTransaction {
            sequence: self.next_sequence,
            tick,
            before: self.world.fingerprint,
            after,
            changes,
        })
    }

    /// Publishes all changed cells, tick and sequence together, or leaves all state untouched.
    /// # Errors
    /// Rejects gaps/replay, stale state, malformed transactions, budgets or wrong fingerprints.
    pub fn apply(&mut self, transaction: &GeometryTransaction) -> Result<(), GeometryError> {
        if transaction.sequence != self.next_sequence {
            return Err(GeometryError::Sequence);
        }
        let next = self
            .next_sequence
            .checked_add(1)
            .ok_or(GeometryError::Sequence)?;
        if transaction.tick < self.world.tick {
            return Err(GeometryError::Tick);
        }
        if transaction.before != self.world.fingerprint {
            return Err(GeometryError::Fingerprint);
        }
        self.world
            .apply_changes(&transaction.changes, transaction.after)?;
        self.world.tick = transaction.tick;
        self.next_sequence = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
