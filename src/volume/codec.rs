//! Strict DFVL v2 rectangular-run stream. V1 is rejected, not silently reinterpreted.
//! Still independent of the coarse world delta/snapshot protocols and network framing.

use super::{
    LocalBox, MAX_EDIT_VISITS, MAX_VOLUME_LEAVES, RefinedVolume, VOLUME_EDGE, VolumeError,
    VolumeLeaf, WorkBudget, same_profile,
};
use crate::Voxel;
use std::{ops::Range, sync::Arc};

const MAGIC: &[u8; 4] = b"DFVL";
const VERSION: u8 = 2;
const HEADER: usize = 9;
const RECORD: usize = 8;
pub const MAX_VOLUME_BYTES: usize = HEADER + RECORD * MAX_VOLUME_LEAVES;

impl RefinedVolume {
    #[must_use]
    pub fn encoded_bytes(&self) -> usize {
        HEADER + RECORD * self.leaves.len()
    }

    /// # Errors
    /// Reports vector allocation refusal; the input already has a bounded canonical partition.
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
            for end in leaf.bounds.maximum {
                bytes.extend_from_slice(&end.to_le_bytes());
            }
            bytes.extend_from_slice(&[leaf.voxel.material as u8, leaf.voxel.integrity]);
        }
        Ok(bytes)
    }

    /// # Errors
    /// Rejects bad sizes/version/material, noncanonical air, inconsistent nested partitions,
    /// reducible profiles and trailing data. Vector reservation is fallible; final Arc allocation
    /// retains the process allocator OOM policy.
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
        .map_err(|_| VolumeError::InvalidEncoding)?;
        if count == 0 || count > MAX_VOLUME_LEAVES || bytes.len() != HEADER + RECORD * count {
            return Err(VolumeError::InvalidEncoding);
        }
        let mut parser = Parser::new();
        parser
            .leaves
            .try_reserve_exact(count)
            .map_err(|_| VolumeError::Allocation)?;
        for record in bytes[HEADER..].chunks_exact(RECORD) {
            parser.push(record)?;
        }
        if !parser.complete {
            return Err(VolumeError::NonCanonical);
        }
        Ok(Self::from_canonical(Arc::from(parser.leaves)))
    }
}

struct Parser {
    leaves: Vec<VolumeLeaf>,
    minimum: [u16; 3],
    band_end: Option<u16>,
    slab_end: Option<u16>,
    band_start: usize,
    slab_start: usize,
    previous_band: Option<Range<usize>>,
    previous_slab: Option<Range<usize>>,
    work: WorkBudget,
    complete: bool,
}
impl Parser {
    const fn new() -> Self {
        Self {
            leaves: Vec::new(),
            minimum: [0; 3],
            band_end: None,
            slab_end: None,
            band_start: 0,
            slab_start: 0,
            previous_band: None,
            previous_slab: None,
            work: WorkBudget::new(MAX_EDIT_VISITS),
            complete: false,
        }
    }
    fn push(&mut self, encoded: &[u8]) -> Result<(), VolumeError> {
        self.work.tick()?;
        if self.complete {
            return Err(VolumeError::NonCanonical);
        }
        let maximum = std::array::from_fn(|axis| {
            u16::from_le_bytes([encoded[axis * 2], encoded[axis * 2 + 1]])
        });
        let bounds = LocalBox::new(self.minimum, maximum).map_err(|_| VolumeError::NonCanonical)?;
        if self.band_end.is_some_and(|end| end != maximum[1])
            || self.slab_end.is_some_and(|end| end != maximum[2])
        {
            return Err(VolumeError::NonCanonical);
        }
        self.band_end = Some(maximum[1]);
        self.slab_end = Some(maximum[2]);
        let voxel =
            Voxel::from_wire(encoded[6], encoded[7]).map_err(|_| VolumeError::InvalidEncoding)?;
        // from_wire normalizes air; only the ORIGINAL integrity byte proves canonical wire data.
        if !voxel.is_solid() && encoded[7] != 0 {
            return Err(VolumeError::NonCanonical);
        }
        if self.minimum[0] != 0 && self.leaves.last().is_some_and(|leaf| leaf.voxel == voxel) {
            return Err(VolumeError::NonCanonical);
        }
        self.leaves.push(VolumeLeaf { bounds, voxel });
        self.minimum[0] = maximum[0];
        if maximum[0] != VOLUME_EDGE {
            return Ok(());
        }
        if let Some(previous) = &self.previous_band
            && same_profile(
                &self.leaves[previous.clone()],
                &self.leaves[self.band_start..],
                1,
                &mut self.work,
            )?
        {
            return Err(VolumeError::NonCanonical);
        }
        self.previous_band = Some(self.band_start..self.leaves.len());
        self.band_start = self.leaves.len();
        self.minimum[0] = 0;
        self.minimum[1] = maximum[1];
        self.band_end = None;
        if maximum[1] != VOLUME_EDGE {
            return Ok(());
        }
        if let Some(previous) = &self.previous_slab
            && same_profile(
                &self.leaves[previous.clone()],
                &self.leaves[self.slab_start..],
                2,
                &mut self.work,
            )?
        {
            return Err(VolumeError::NonCanonical);
        }
        self.previous_slab = Some(self.slab_start..self.leaves.len());
        self.slab_start = self.leaves.len();
        self.previous_band = None;
        self.minimum[1] = 0;
        self.minimum[2] = maximum[2];
        self.slab_end = None;
        self.complete = maximum[2] == VOLUME_EDGE;
        Ok(())
    }
}
