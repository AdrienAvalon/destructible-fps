use core::fmt;

/// Materials remain byte-sized on the wire and inside voxel chunks.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum Material {
    #[default]
    Air = 0,
    Soil = 1,
    Stone = 2,
    Wood = 3,
    Brick = 4,
    Concrete = 5,
    Steel = 6,
    Glass = 7,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaterialProperties {
    /// Approximate bulk density used for debris mass estimates.
    pub density_kg_m3: u16,
    /// Energy scale used by the first deterministic blast model.
    pub blast_resistance: u32,
    /// Relative ability to carry structural load in the future constraint solver.
    pub structural_strength: u16,
    /// Relative amount of energy preserved as fragment velocity.
    pub fragmentation: u8,
    /// Tangential contact response in thousandths for deterministic Coulomb friction.
    pub friction_per_mille: u16,
    /// Normal impact speed retained after a collision, in thousandths.
    pub restitution_per_mille: u16,
}

impl Material {
    #[must_use]
    pub const fn properties(self) -> MaterialProperties {
        match self {
            Self::Air => MaterialProperties {
                density_kg_m3: 0,
                blast_resistance: 1,
                structural_strength: 0,
                fragmentation: 0,
                friction_per_mille: 0,
                restitution_per_mille: 0,
            },
            Self::Soil => MaterialProperties {
                density_kg_m3: 1_600,
                blast_resistance: 1_200,
                structural_strength: 180,
                fragmentation: 20,
                friction_per_mille: 850,
                restitution_per_mille: 30,
            },
            Self::Stone => MaterialProperties {
                density_kg_m3: 2_650,
                blast_resistance: 8_500,
                structural_strength: 1_600,
                fragmentation: 75,
                friction_per_mille: 780,
                restitution_per_mille: 100,
            },
            Self::Wood => MaterialProperties {
                density_kg_m3: 650,
                blast_resistance: 1_800,
                structural_strength: 650,
                fragmentation: 55,
                friction_per_mille: 620,
                restitution_per_mille: 220,
            },
            Self::Brick => MaterialProperties {
                density_kg_m3: 1_900,
                blast_resistance: 3_500,
                structural_strength: 700,
                fragmentation: 90,
                friction_per_mille: 720,
                restitution_per_mille: 80,
            },
            Self::Concrete => MaterialProperties {
                density_kg_m3: 2_400,
                blast_resistance: 6_500,
                structural_strength: 2_000,
                fragmentation: 85,
                friction_per_mille: 820,
                restitution_per_mille: 60,
            },
            Self::Steel => MaterialProperties {
                density_kg_m3: 7_850,
                blast_resistance: 28_000,
                structural_strength: 8_000,
                fragmentation: 35,
                friction_per_mille: 420,
                restitution_per_mille: 180,
            },
            Self::Glass => MaterialProperties {
                density_kg_m3: 2_500,
                blast_resistance: 500,
                structural_strength: 120,
                fragmentation: 100,
                friction_per_mille: 280,
                restitution_per_mille: 320,
            },
        }
    }

    /// Decodes a stable protocol identifier.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidMaterial`] when the identifier is not part of this protocol version.
    pub const fn from_wire(value: u8) -> Result<Self, InvalidMaterial> {
        match value {
            0 => Ok(Self::Air),
            1 => Ok(Self::Soil),
            2 => Ok(Self::Stone),
            3 => Ok(Self::Wood),
            4 => Ok(Self::Brick),
            5 => Ok(Self::Concrete),
            6 => Ok(Self::Steel),
            7 => Ok(Self::Glass),
            _ => Err(InvalidMaterial(value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidMaterial(pub u8);

impl fmt::Display for InvalidMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid material id {}", self.0)
    }
}

impl std::error::Error for InvalidMaterial {}

/// Two bytes per voxel keeps a 16^3 chunk at 8 KiB before compression.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[repr(C)]
pub struct Voxel {
    pub material: Material,
    pub integrity: u8,
}

impl Voxel {
    pub const AIR: Self = Self {
        material: Material::Air,
        integrity: 0,
    };

    #[must_use]
    pub const fn new(material: Material) -> Self {
        if matches!(material, Material::Air) {
            Self::AIR
        } else {
            Self {
                material,
                integrity: u8::MAX,
            }
        }
    }

    #[must_use]
    pub const fn is_solid(self) -> bool {
        !matches!(self.material, Material::Air)
    }

    /// Decodes the compact wire representation and canonicalizes air integrity to zero.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidMaterial`] when `material` is unknown.
    pub const fn from_wire(material: u8, integrity: u8) -> Result<Self, InvalidMaterial> {
        let material = match Material::from_wire(material) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        if matches!(material, Material::Air) {
            Ok(Self::AIR)
        } else {
            Ok(Self {
                material,
                integrity,
            })
        }
    }
}

const _: () = assert!(size_of::<Voxel>() == 2);
