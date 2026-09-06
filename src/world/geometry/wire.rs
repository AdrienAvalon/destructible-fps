//! Geometry component sections for future snapshot/delta integration, NOT gameplay datagrams.
//!
//! DFGC/DFGT v1 deliberately cannot be mistaken for existing DFSN/DFDP or standalone DFVL.
//! Bounds cover encoded bytes and accepted resident geometry; fingerprints are not authentication.

use super::{
    CellValue, GeometryCell, GeometryChange, GeometryError, GeometryState, GeometryTransaction,
    MAX_GEOMETRY_CELLS, MAX_GEOMETRY_CHANGES, MAX_GEOMETRY_CHUNKS, MAX_TRANSACTION_LEAVES,
    RefinedWorld,
};
use crate::{
    IVec3, Voxel,
    volume::{RefinedVolume, codec::MAX_VOLUME_BYTES},
    world::chunk_position,
};

const VERSION: u8 = 1;
const CHECKPOINT_HEADER: usize = 41;
const TRANSACTION_HEADER: usize = 57;
pub const MAX_GEOMETRY_CHECKPOINT_BYTES: usize = 4 * 1_024 * 1_024;
pub const MAX_GEOMETRY_TRANSACTION_BYTES: usize = 512 * 1_024;

impl GeometryState {
    /// Captures only static geometry and its ordering boundary, not bodies/inventory/players.
    /// # Errors
    /// Refuses oversized sections or vector allocation failure before encoding records.
    pub fn encode_checkpoint(&self) -> Result<Vec<u8>, GeometryError> {
        let stats = self.world.geometry_stats();
        stats.validate()?;
        let size = CHECKPOINT_HEADER
            + (stats.occupied_cells - stats.refined_pages) * 15
            + self
                .world
                .chunks
                .values()
                .flat_map(|chunk| chunk.geometry.pages.values())
                .map(|page| 17 + page.encoded_bytes())
                .sum::<usize>();
        let mut bytes = buffer(size, MAX_GEOMETRY_CHECKPOINT_BYTES)?;
        bytes.extend_from_slice(b"DFGC");
        bytes.push(VERSION);
        bytes.extend_from_slice(&self.world.tick.to_le_bytes());
        bytes.extend_from_slice(&self.next_sequence.to_le_bytes());
        bytes.extend_from_slice(&self.world.fingerprint.to_le_bytes());
        push_count(&mut bytes, stats.occupied_cells)?;
        for position in self.world.occupied_positions() {
            push_position(&mut bytes, position);
            push_cell(&mut bytes, &self.world.cell(position))?;
        }
        debug_assert_eq!(bytes.len(), size);
        Ok(bytes)
    }

    /// Decode into a bounded unpublished candidate. Caller owns authenticated freshness and the
    /// eventual atomic installation alongside bodies, inventory and other game components.
    /// # Errors
    /// Rejects bad framing, noncanonical cells/order, aggregate residency or fingerprint mismatch.
    pub fn decode_checkpoint(bytes: &[u8]) -> Result<Self, GeometryError> {
        let mut cursor = Cursor::new(
            bytes,
            *b"DFGC",
            CHECKPOINT_HEADER,
            MAX_GEOMETRY_CHECKPOINT_BYTES,
        )?;
        let tick = cursor.u64()?;
        let next_sequence = cursor.u64()?;
        if next_sequence == 0 {
            return Err(GeometryError::Sequence);
        }
        let expected = cursor.u128()?;
        let count = cursor.count()?;
        if count > MAX_GEOMETRY_CELLS || count > cursor.remaining() / 15 {
            return Err(GeometryError::WorldBudget);
        }
        let mut world = RefinedWorld::default();
        let mut stats = world.geometry_stats();
        let mut previous = None;
        for _ in 0..count {
            let position = cursor.position()?;
            ordered(&mut previous, position)?;
            let cell = cursor.cell()?;
            if cell.solid_units() == 0 {
                return Err(GeometryError::Encoding);
            }
            let new_chunk = !world.chunks.contains_key(&chunk_position(position));
            if new_chunk && world.chunks.len() == MAX_GEOMETRY_CHUNKS {
                return Err(GeometryError::WorldBudget);
            }
            stats.chunks += usize::from(new_chunk);
            stats.occupied_cells += 1;
            stats.refined_pages += usize::from(cell.volume().is_some());
            stats.refined_leaves += cell.leaves();
            stats.validate()?;
            world.set_cell(position, &cell);
        }
        cursor.finish()?;
        if world.fingerprint != expected {
            return Err(GeometryError::Fingerprint);
        }
        world.tick = tick;
        Self::new(world, next_sequence)
    }
}

impl GeometryTransaction {
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub fn changes(&self) -> &[GeometryChange] {
        &self.changes
    }

    /// Encodes a component transaction for a future enclosing authenticated/bounded transport.
    /// # Errors
    /// Refuses aggregate byte/record/leaf limits and vector allocation failure.
    pub fn encode(&self) -> Result<Vec<u8>, GeometryError> {
        validate_transaction(self)?;
        let size = TRANSACTION_HEADER
            + self
                .changes
                .iter()
                .map(|change| 12 + cell_size(&change.before) + cell_size(&change.after))
                .sum::<usize>();
        let mut bytes = buffer(size, MAX_GEOMETRY_TRANSACTION_BYTES)?;
        bytes.extend_from_slice(b"DFGT");
        bytes.push(VERSION);
        bytes.extend_from_slice(&self.sequence.to_le_bytes());
        bytes.extend_from_slice(&self.tick.to_le_bytes());
        bytes.extend_from_slice(&self.before.to_le_bytes());
        bytes.extend_from_slice(&self.after.to_le_bytes());
        push_count(&mut bytes, self.changes.len())?;
        for change in &self.changes {
            push_position(&mut bytes, change.position);
            push_cell(&mut bytes, &change.before)?;
            push_cell(&mut bytes, &change.after)?;
        }
        debug_assert_eq!(bytes.len(), size);
        Ok(bytes)
    }

    /// Decode bounded syntax only. `GeometryState::apply` still validates full before/after state.
    /// # Errors
    /// Rejects framing, canonicality, ordering, zero sequence and aggregate record/leaf/byte caps.
    pub fn decode(bytes: &[u8]) -> Result<Self, GeometryError> {
        let mut cursor = Cursor::new(
            bytes,
            *b"DFGT",
            TRANSACTION_HEADER,
            MAX_GEOMETRY_TRANSACTION_BYTES,
        )?;
        let sequence = cursor.u64()?;
        if sequence == 0 || sequence == u64::MAX {
            return Err(GeometryError::Sequence);
        }
        let tick = cursor.u64()?;
        let before = cursor.u128()?;
        let after = cursor.u128()?;
        let count = cursor.count()?;
        if count > MAX_GEOMETRY_CHANGES || count > cursor.remaining() / 18 {
            return Err(GeometryError::TransactionBudget);
        }
        let mut changes = Vec::new();
        changes
            .try_reserve_exact(count)
            .map_err(|_| GeometryError::Allocation)?;
        let mut previous = None;
        let mut leaves = 0;
        for _ in 0..count {
            let position = cursor.position()?;
            ordered(&mut previous, position)?;
            let before = cursor.cell()?;
            leaves += before.leaves();
            if leaves > MAX_TRANSACTION_LEAVES {
                return Err(GeometryError::TransactionBudget);
            }
            let after = cursor.cell()?;
            leaves += after.leaves();
            if leaves > MAX_TRANSACTION_LEAVES {
                return Err(GeometryError::TransactionBudget);
            }
            changes.push(GeometryChange {
                position,
                before,
                after,
            });
        }
        cursor.finish()?;
        Ok(Self {
            sequence,
            tick,
            before,
            after,
            changes,
        })
    }
}

fn validate_transaction(transaction: &GeometryTransaction) -> Result<(), GeometryError> {
    if transaction.sequence == 0 || transaction.sequence == u64::MAX {
        return Err(GeometryError::Sequence);
    }
    if transaction.changes.len() > MAX_GEOMETRY_CHANGES {
        return Err(GeometryError::TransactionBudget);
    }
    let mut previous = None;
    let mut leaves = 0;
    for change in &transaction.changes {
        ordered(&mut previous, change.position)?;
        leaves += change.before.leaves() + change.after.leaves();
        if leaves > MAX_TRANSACTION_LEAVES {
            return Err(GeometryError::TransactionBudget);
        }
    }
    Ok(())
}

fn ordered(previous: &mut Option<IVec3>, position: IVec3) -> Result<(), GeometryError> {
    if previous.is_some_and(|p| p >= position) {
        return Err(GeometryError::Order);
    }
    *previous = Some(position);
    Ok(())
}

fn buffer(size: usize, maximum: usize) -> Result<Vec<u8>, GeometryError> {
    if size > maximum {
        return Err(GeometryError::Encoding);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| GeometryError::Allocation)?;
    Ok(bytes)
}

fn cell_size(cell: &GeometryCell) -> usize {
    cell.volume().map_or(3, |volume| 5 + volume.encoded_bytes())
}

fn push_count(bytes: &mut Vec<u8>, count: usize) -> Result<(), GeometryError> {
    bytes.extend_from_slice(
        &u32::try_from(count)
            .map_err(|_| GeometryError::Encoding)?
            .to_le_bytes(),
    );
    Ok(())
}

fn push_position(bytes: &mut Vec<u8>, position: IVec3) {
    for component in [position.x, position.y, position.z] {
        bytes.extend_from_slice(&component.to_le_bytes());
    }
}

fn push_cell(bytes: &mut Vec<u8>, cell: &GeometryCell) -> Result<(), GeometryError> {
    match &cell.0 {
        CellValue::Uniform(voxel) => {
            bytes.extend_from_slice(&[0, voxel.material as u8, voxel.integrity]);
        }
        CellValue::Refined(volume) => {
            bytes.push(1);
            push_count(bytes, volume.encoded_bytes())?;
            bytes.extend_from_slice(&volume.encode().map_err(|_| GeometryError::Encoding)?);
        }
    }
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    fn new(
        bytes: &'a [u8],
        magic: [u8; 4],
        minimum: usize,
        maximum: usize,
    ) -> Result<Self, GeometryError> {
        if bytes.len() < minimum
            || bytes.len() > maximum
            || bytes[..4] != magic
            || bytes[4] != VERSION
        {
            return Err(GeometryError::Encoding);
        }
        Ok(Self { bytes, offset: 5 })
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], GeometryError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(GeometryError::Encoding)?;
        let result = self
            .bytes
            .get(self.offset..end)
            .ok_or(GeometryError::Encoding)?;
        self.offset = end;
        Ok(result)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], GeometryError> {
        self.take(N)?
            .try_into()
            .map_err(|_| GeometryError::Encoding)
    }
    fn count(&mut self) -> Result<usize, GeometryError> {
        usize::try_from(u32::from_le_bytes(self.array()?)).map_err(|_| GeometryError::Encoding)
    }
    fn u64(&mut self) -> Result<u64, GeometryError> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    fn u128(&mut self) -> Result<u128, GeometryError> {
        Ok(u128::from_le_bytes(self.array()?))
    }
    fn position(&mut self) -> Result<IVec3, GeometryError> {
        Ok(IVec3::new(
            i32::from_le_bytes(self.array()?),
            i32::from_le_bytes(self.array()?),
            i32::from_le_bytes(self.array()?),
        ))
    }
    fn cell(&mut self) -> Result<GeometryCell, GeometryError> {
        match self.array::<1>()?[0] {
            0 => {
                let [material, integrity] = self.array()?;
                let voxel =
                    Voxel::from_wire(material, integrity).map_err(|_| GeometryError::Encoding)?;
                if !voxel.is_solid() && integrity != 0 {
                    return Err(GeometryError::Encoding);
                }
                Ok(GeometryCell::uniform(voxel))
            }
            1 => {
                let count = self.count()?;
                if count > MAX_VOLUME_BYTES {
                    return Err(GeometryError::Encoding);
                }
                let volume = RefinedVolume::decode(self.take(count)?)
                    .map_err(|_| GeometryError::Encoding)?;
                if volume.uniform_voxel().is_some() {
                    return Err(GeometryError::Encoding);
                }
                Ok(GeometryCell::refined(volume))
            }
            _ => Err(GeometryError::Encoding),
        }
    }
    const fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }
    const fn finish(&self) -> Result<(), GeometryError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(GeometryError::Encoding)
        }
    }
}
