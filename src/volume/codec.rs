//! Strict canonical fine-volume stream, independent of the not-yet-migrated world wire protocol.

use super::{
    MAX_VOLUME_LEAVES, RefinedVolume, VOLUME_DEPTH, VOLUME_UNITS, VolumeError, VolumeLeaf,
    coalesced_tail,
};
use crate::Voxel;
use std::sync::Arc;

const MAGIC: &[u8; 4] = b"DFVL";
const VERSION: u8 = 1;
const HEADER: usize = 9;
pub const MAX_VOLUME_BYTES: usize = HEADER + 3 * MAX_VOLUME_LEAVES;

impl RefinedVolume {
    #[must_use]
    pub fn encoded_bytes(&self) -> usize {
        HEADER + 3 * self.leaves.len()
    }

    /// # Errors
    /// Allocation refusal is explicit. The input volume is already bounded/canonical.
    pub fn encode(&self) -> Result<Vec<u8>, VolumeError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(self.encoded_bytes())
            .map_err(|_| VolumeError::Allocation)?;
        bytes.extend_from_slice(MAGIC);
        bytes.push(VERSION);
        bytes.extend_from_slice(
            &u32::try_from(self.leaves.len())
                .map_err(|_| VolumeError::LeafBudget)?
                .to_le_bytes(),
        );
        for leaf in &*self.leaves {
            bytes.extend_from_slice(&[leaf.depth, leaf.voxel.material as u8, leaf.voxel.integrity]);
        }
        Ok(bytes)
    }

    /// # Errors
    /// Rejects size/count/depth abuse, gaps, non-aligned partitions, invalid materials, noncanonical
    /// air, reducible siblings and trailing bytes before constructing an accepted volume.
    /// Vector reservation is fallible; the final Arc copy follows the process allocator OOM policy.
    pub fn decode(bytes: &[u8]) -> Result<Self, VolumeError> {
        if bytes.len() < HEADER
            || bytes.len() > MAX_VOLUME_BYTES
            || &bytes[..4] != MAGIC
            || bytes[4] != VERSION
        {
            return Err(VolumeError::InvalidEncoding);
        }
        let count = usize::try_from(u32::from_le_bytes(
            bytes[5..9]
                .try_into()
                .map_err(|_| VolumeError::InvalidEncoding)?,
        ))
        .map_err(|_| VolumeError::LeafBudget)?;
        if count == 0 || count > MAX_VOLUME_LEAVES || bytes.len() != HEADER + 3 * count {
            return Err(VolumeError::InvalidEncoding);
        }
        let mut leaves = Vec::new();
        leaves
            .try_reserve_exact(count)
            .map_err(|_| VolumeError::Allocation)?;
        let mut start = 0_u32;
        for encoded in bytes[HEADER..].chunks_exact(3) {
            let depth = encoded[0];
            if depth > VOLUME_DEPTH {
                return Err(VolumeError::InvalidEncoding);
            }
            let voxel = Voxel::from_wire(encoded[1], encoded[2])
                .map_err(|_| VolumeError::InvalidEncoding)?;
            // from_wire already canonicalizes air. Compare the *original byte*, not the returned
            // voxel to canonical_voxel(voxel), which would silently accept nonzero wire integrity.
            if !voxel.is_solid() && encoded[2] != 0 {
                return Err(VolumeError::NonCanonical);
            }
            let leaf = VolumeLeaf {
                start,
                depth,
                voxel,
            };
            if !start.is_multiple_of(leaf.units()) || start >= VOLUME_UNITS {
                return Err(VolumeError::NonCanonical);
            }
            start = start
                .checked_add(leaf.units())
                .ok_or(VolumeError::InvalidEncoding)?;
            leaves.push(leaf);
            if coalesced_tail(&leaves).is_some() {
                return Err(VolumeError::NonCanonical);
            }
        }
        if start != VOLUME_UNITS {
            return Err(VolumeError::NonCanonical);
        }
        Ok(Self::from_canonical(Arc::from(leaves)))
    }
}
