//! Fixed-size, integrity-checked, offline HDR lighting. No image parser or runtime convolution.

use miniz_oxide::inflate::decompress_to_vec_zlib_with_limit;
use ring::digest::{Context, SHA256};

pub const SKY_EDGE: u32 = 256;
pub const DIFFUSE_EDGE: u32 = 16;
pub const SPECULAR_EDGE: u32 = 64;
pub const SPECULAR_MIPS: u32 = 7;
pub const LUT_EDGE: u32 = 64;
pub const ENVIRONMENT_BYTES: usize = 3_452_912;
const SKY_END: usize = 6 * 256 * 256 * 8;
const DIFFUSE_END: usize = SKY_END + 6 * 16 * 16 * 8;
const SPECULAR_END: usize = ENVIRONMENT_BYTES - 64 * 64 * 8;
const HEADER_BYTES: usize = 32;
const PAYLOAD_OFFSET: usize = HEADER_BYTES + 32;
const MAX_PACK_BYTES: usize = ENVIRONMENT_BYTES + 65_536;
const EMBEDDED: &[u8] = include_bytes!("../assets/environment/overcast.iblz");

#[derive(Debug, Eq, PartialEq)]
pub enum EnvironmentError {
    InvalidHeader,
    InvalidCompression,
    InvalidDigest,
    InvalidTexel,
}

impl core::fmt::Display for EnvironmentError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidHeader => "unsupported environment package dimensions or size",
            Self::InvalidCompression => "environment decompression exceeded its fixed contract",
            Self::InvalidDigest => "environment package integrity check failed",
            Self::InvalidTexel => "environment contains invalid linear half-float texels",
        })
    }
}

impl std::error::Error for EnvironmentError {}

pub struct EnvironmentLibrary {
    texels: Vec<u8>,
}

impl EnvironmentLibrary {
    /// Validates and decodes the fixed, embedded environment at renderer startup.
    ///
    /// # Errors
    /// Rejects unsupported headers, corrupt/oversized payloads, negative or non-finite half floats.
    pub fn embedded() -> Result<Self, EnvironmentError> {
        Self::decode(EMBEDDED)
    }

    fn decode(bytes: &[u8]) -> Result<Self, EnvironmentError> {
        if !(PAYLOAD_OFFSET + 1..=MAX_PACK_BYTES).contains(&bytes.len())
            || &bytes[..8] != b"AVIBL001"
        {
            return Err(EnvironmentError::InvalidHeader);
        }
        for (field, expected) in bytes[8..HEADER_BYTES].chunks_exact(4).zip([
            SKY_EDGE,
            DIFFUSE_EDGE,
            SPECULAR_EDGE,
            SPECULAR_MIPS,
            LUT_EDGE,
            u32::try_from(ENVIRONMENT_BYTES).unwrap_or(u32::MAX),
        ]) {
            if u32::from_le_bytes([field[0], field[1], field[2], field[3]]) != expected {
                return Err(EnvironmentError::InvalidHeader);
            }
        }
        let texels = decompress_to_vec_zlib_with_limit(&bytes[PAYLOAD_OFFSET..], ENVIRONMENT_BYTES)
            .map_err(|_| EnvironmentError::InvalidCompression)?;
        if texels.len() != ENVIRONMENT_BYTES {
            return Err(EnvironmentError::InvalidCompression);
        }
        let mut digest = Context::new(&SHA256);
        digest.update(&bytes[..HEADER_BYTES]);
        digest.update(&texels);
        if digest.finish().as_ref() != &bytes[HEADER_BYTES..PAYLOAD_OFFSET] {
            return Err(EnvironmentError::InvalidDigest);
        }
        for texel in texels.chunks_exact(8) {
            if texel[6..] != [0x00, 0x3c]
                || texel[..6].chunks_exact(2).any(|half| {
                    // Sign bit and exponent all-ones are rejected, including negative zero/NaNs.
                    let bits = u16::from_le_bytes([half[0], half[1]]);
                    bits & 0x8000 != 0 || bits & 0x7c00 == 0x7c00
                })
            {
                return Err(EnvironmentError::InvalidTexel);
            }
        }
        Ok(Self { texels })
    }

    #[must_use]
    pub fn sky(&self) -> &[u8] {
        &self.texels[..SKY_END]
    }

    /// Cosine convolution already divided by pi: multiply by diffuse albedo, not albedo/pi.
    #[must_use]
    pub fn diffuse(&self) -> &[u8] {
        &self.texels[SKY_END..DIFFUSE_END]
    }

    /// Seven perceptual-roughness mips, mip-major then face-major, in WebGPU cube order.
    #[must_use]
    pub fn specular(&self) -> &[u8] {
        &self.texels[DIFFUSE_END..SPECULAR_END]
    }

    /// Red/green split-sum coefficients; x = `NdotV`, y = perceptual roughness, pixel centers.
    #[must_use]
    pub fn brdf(&self) -> &[u8] {
        &self.texels[SPECULAR_END..]
    }

    #[must_use]
    pub const fn packed_bytes() -> usize {
        EMBEDDED.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_pack_has_exact_linear_hdr_layout() {
        let library = EnvironmentLibrary::embedded().unwrap();
        assert_eq!(library.sky().len(), 6 * SKY_EDGE.pow(2) as usize * 8);
        assert_eq!(
            library.diffuse().len(),
            6 * DIFFUSE_EDGE.pow(2) as usize * 8
        );
        let specular: u32 = (0..SPECULAR_MIPS)
            .map(|mip| 6 * (SPECULAR_EDGE >> mip).pow(2) * 8)
            .sum();
        assert_eq!(library.specular().len(), specular as usize);
        assert_eq!(library.brdf().len(), LUT_EDGE.pow(2) as usize * 8);
        assert!(
            library
                .sky()
                .chunks_exact(2)
                .any(|v| u16::from_le_bytes([v[0], v[1]]) > 0x3c00)
        );
    }

    #[test]
    fn malformed_header_and_decompression_fail_closed() {
        for length in [0, 7, 31, 63, 64] {
            assert!(matches!(
                EnvironmentLibrary::decode(&EMBEDDED[..length]),
                Err(EnvironmentError::InvalidHeader)
            ));
        }
        for offset in [0, 8, 12, 16, 20, 24, 28] {
            let mut bytes = EMBEDDED[..65].to_vec();
            bytes[offset] ^= 1;
            assert!(matches!(
                EnvironmentLibrary::decode(&bytes),
                Err(EnvironmentError::InvalidHeader)
            ));
        }
        assert!(matches!(
            EnvironmentLibrary::decode(&EMBEDDED[..70]),
            Err(EnvironmentError::InvalidCompression)
        ));
        let mut bytes = EMBEDDED.to_vec();
        bytes[HEADER_BYTES] ^= 1;
        assert!(matches!(
            EnvironmentLibrary::decode(&bytes),
            Err(EnvironmentError::InvalidDigest)
        ));
        assert!(matches!(
            EnvironmentLibrary::decode(&vec![0; MAX_PACK_BYTES + 1]),
            Err(EnvironmentError::InvalidHeader)
        ));
    }

    fn pack(texels: &[u8]) -> Vec<u8> {
        let mut bytes = EMBEDDED[..HEADER_BYTES].to_vec();
        let mut digest = Context::new(&SHA256);
        digest.update(&bytes);
        digest.update(texels);
        bytes.extend_from_slice(digest.finish().as_ref());
        bytes.extend(miniz_oxide::deflate::compress_to_vec_zlib(texels, 1));
        bytes
    }

    #[test]
    fn digest_valid_nonfinite_negative_and_invalid_alpha_are_rejected() {
        let mut texels = EnvironmentLibrary::embedded().unwrap().texels;
        for bits in [0x7c00_u16, 0xfc00, 0x7e00, 0x8000, 0xbc00] {
            texels[..2].copy_from_slice(&bits.to_le_bytes());
            assert!(matches!(
                EnvironmentLibrary::decode(&pack(&texels)),
                Err(EnvironmentError::InvalidTexel)
            ));
        }
        texels[..2].copy_from_slice(&0_u16.to_le_bytes());
        texels[6] = 1;
        assert!(matches!(
            EnvironmentLibrary::decode(&pack(&texels)),
            Err(EnvironmentError::InvalidTexel)
        ));
    }

    #[test]
    fn compressed_bomb_cannot_exceed_exact_output_budget() {
        let texels = vec![0; ENVIRONMENT_BYTES + 1];
        assert!(matches!(
            EnvironmentLibrary::decode(&pack(&texels)),
            Err(EnvironmentError::InvalidCompression)
        ));
        assert!(matches!(
            EnvironmentLibrary::decode(&pack(&texels[..100])),
            Err(EnvironmentError::InvalidCompression)
        ));
    }
}
