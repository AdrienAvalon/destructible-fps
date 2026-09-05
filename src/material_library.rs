//! Bounded, offline-cooked PBR inputs. No runtime image decoder or asset network access.

use core::fmt;
use miniz_oxide::inflate::decompress_to_vec_zlib_with_limit;
use ring::digest::{Context, SHA256};

pub const MATERIAL_TEXTURE_EDGE: u32 = 1_024;
pub const MATERIAL_TEXTURE_LAYERS: u32 = 5;
pub const MATERIAL_TEXTURE_MIPS: u32 = 11;
pub const MATERIAL_TEXTURE_ARRAY_BYTES: usize = 27_962_020;
pub const MATERIAL_TEXTURE_BYTES: usize = MATERIAL_TEXTURE_ARRAY_BYTES * 2;
const MAX_PACK_BYTES: usize = MATERIAL_TEXTURE_BYTES + 1_024 * 1_024;
const FIXED_HEADER_BYTES: usize = 24;
const DIGEST_END: usize = FIXED_HEADER_BYTES + 32;
const HEADER_BYTES: usize = DIGEST_END + MATERIAL_TEXTURE_LAYERS as usize * 8;
const EMBEDDED: &[u8] = include_bytes!("../assets/materials/industrial.pbrz");

#[derive(Debug, Eq, PartialEq)]
pub enum MaterialLibraryError {
    InvalidHeader,
    InvalidScale,
    InvalidCompression,
    InvalidDigest,
}

impl fmt::Display for MaterialLibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidHeader => "material package has an unsupported layout or size",
            Self::InvalidScale => "material package has an invalid physical scale",
            Self::InvalidCompression => {
                "material package decompression failed within its fixed budget"
            }
            Self::InvalidDigest => "material package integrity check failed",
        })
    }
}

impl std::error::Error for MaterialLibraryError {}

pub struct MaterialLibrary {
    texels: Vec<u8>,
    /// Reciprocal physical tile width/height; remaining components are reserved and zero.
    pub scales: [[f32; 4]; MATERIAL_TEXTURE_LAYERS as usize],
}

impl MaterialLibrary {
    /// Decompresses the embedded library once before rendering starts.
    ///
    /// # Errors
    /// Rejects any unsupported dimensions, invalid scales, corrupt or oversized compressed data.
    pub fn embedded() -> Result<Self, MaterialLibraryError> {
        Self::decode(EMBEDDED)
    }

    fn decode(bytes: &[u8]) -> Result<Self, MaterialLibraryError> {
        if !(HEADER_BYTES + 1..=MAX_PACK_BYTES).contains(&bytes.len()) || &bytes[..8] != b"AVPBR001"
        {
            return Err(MaterialLibraryError::InvalidHeader);
        }
        let expected = [
            MATERIAL_TEXTURE_EDGE,
            MATERIAL_TEXTURE_LAYERS,
            MATERIAL_TEXTURE_MIPS,
            u32::try_from(MATERIAL_TEXTURE_BYTES).unwrap_or(u32::MAX),
        ];
        for (field, expected) in bytes[8..FIXED_HEADER_BYTES].chunks_exact(4).zip(expected) {
            if u32::from_le_bytes([field[0], field[1], field[2], field[3]]) != expected {
                return Err(MaterialLibraryError::InvalidHeader);
            }
        }
        let mut scales = [[0.0; 4]; MATERIAL_TEXTURE_LAYERS as usize];
        for (record, scale) in bytes[DIGEST_END..HEADER_BYTES]
            .chunks_exact(8)
            .zip(&mut scales)
        {
            for (field, result) in record.chunks_exact(4).zip(&mut scale[..2]) {
                let meters = f32::from_le_bytes([field[0], field[1], field[2], field[3]]);
                if !meters.is_finite() || !(0.25..=16.0).contains(&meters) {
                    return Err(MaterialLibraryError::InvalidScale);
                }
                *result = meters.recip();
            }
        }
        let texels =
            decompress_to_vec_zlib_with_limit(&bytes[HEADER_BYTES..], MATERIAL_TEXTURE_BYTES)
                .map_err(|_| MaterialLibraryError::InvalidCompression)?;
        if texels.len() != MATERIAL_TEXTURE_BYTES {
            return Err(MaterialLibraryError::InvalidCompression);
        }
        let mut digest = Context::new(&SHA256);
        digest.update(&bytes[..FIXED_HEADER_BYTES]);
        digest.update(&bytes[DIGEST_END..HEADER_BYTES]);
        digest.update(&texels);
        if digest.finish().as_ref() != &bytes[FIXED_HEADER_BYTES..DIGEST_END] {
            return Err(MaterialLibraryError::InvalidDigest);
        }
        Ok(Self { texels, scales })
    }

    #[must_use]
    pub const fn packed_bytes() -> usize {
        EMBEDDED.len()
    }

    #[must_use]
    pub fn color_roughness(&self) -> &[u8] {
        &self.texels[..MATERIAL_TEXTURE_ARRAY_BYTES]
    }

    #[must_use]
    pub fn normal_metalness(&self) -> &[u8] {
        &self.texels[MATERIAL_TEXTURE_ARRAY_BYTES..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_library_covers_every_layer_and_complete_mip_chain() {
        let library =
            MaterialLibrary::embedded().expect("the packaged asset must validate offline");
        let mip_bytes: usize = (0..MATERIAL_TEXTURE_MIPS)
            .map(|level| ((MATERIAL_TEXTURE_EDGE >> level).pow(2) * 4) as usize)
            .sum();
        assert_eq!(
            mip_bytes * MATERIAL_TEXTURE_LAYERS as usize,
            MATERIAL_TEXTURE_ARRAY_BYTES
        );
        assert_eq!(
            library.color_roughness().len(),
            MATERIAL_TEXTURE_ARRAY_BYTES
        );
        assert_eq!(
            library.normal_metalness().len(),
            MATERIAL_TEXTURE_ARRAY_BYTES
        );
        for layer in library.normal_metalness().chunks_exact(mip_bytes) {
            assert!(layer.chunks_exact(4).all(|texel| texel[3] == 0));
            assert!(layer.chunks_exact(4).all(|texel| texel[2] >= 127));
        }
    }

    #[test]
    fn unsupported_dimensions_and_nonfinite_scales_fail_before_decompression() {
        for offset in [0, 8, 12, 16, 20] {
            let mut pack = EMBEDDED[..=HEADER_BYTES].to_vec();
            pack[offset] ^= 1;
            assert!(matches!(
                MaterialLibrary::decode(&pack),
                Err(MaterialLibraryError::InvalidHeader)
            ));
        }
        for value in [f32::NAN, f32::INFINITY, 0.0, -1.0, 17.0] {
            let mut pack = EMBEDDED[..=HEADER_BYTES].to_vec();
            pack[DIGEST_END..DIGEST_END + 4].copy_from_slice(&value.to_le_bytes());
            assert!(matches!(
                MaterialLibrary::decode(&pack),
                Err(MaterialLibraryError::InvalidScale)
            ));
        }
        for length in [0, 7, 23, HEADER_BYTES] {
            assert!(matches!(
                MaterialLibrary::decode(&EMBEDDED[..length]),
                Err(MaterialLibraryError::InvalidHeader)
            ));
        }
    }

    #[test]
    fn truncated_compression_and_modified_payload_digest_are_rejected() {
        assert!(matches!(
            MaterialLibrary::decode(&EMBEDDED[..HEADER_BYTES + 8]),
            Err(MaterialLibraryError::InvalidCompression)
        ));
        let mut pack = EMBEDDED.to_vec();
        pack[FIXED_HEADER_BYTES] ^= 1;
        assert!(matches!(
            MaterialLibrary::decode(&pack),
            Err(MaterialLibraryError::InvalidDigest)
        ));
    }

    #[test]
    fn compressed_data_cannot_exceed_the_decode_budget() {
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&[0_u8; 4096], 6);
        assert!(decompress_to_vec_zlib_with_limit(&compressed, 1024).is_err());
    }
}
