//! Deterministic, bounded topology analysis for server-authoritative structural destruction.

use crate::{IVec3, Voxel, VoxelChange, World};
use core::fmt;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

const MAX_EXPLICIT_ANCHORS: usize = 4_096;
const NEIGHBORS: [IVec3; 6] = [
    IVec3::new(0, -1, 0),
    IVec3::new(-1, 0, 0),
    IVec3::new(1, 0, 0),
    IVec3::new(0, 0, -1),
    IVec3::new(0, 0, 1),
    IVec3::new(0, 1, 0),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StructuralLimits {
    pub max_changes: usize,
    pub max_affected_seeds: usize,
    pub max_visited_voxels: usize,
    pub max_component_voxels: usize,
    pub max_detached_islands: usize,
}

impl Default for StructuralLimits {
    fn default() -> Self {
        Self {
            max_changes: 65_536,
            max_affected_seeds: 4_096,
            max_visited_voxels: 131_072,
            max_component_voxels: 65_536,
            max_detached_islands: 256,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct StructuralAnchors {
    foundation_max_y: Option<i32>,
    explicit: BTreeSet<IVec3>,
}

impl StructuralAnchors {
    #[must_use]
    pub const fn foundation_plane(maximum_y: i32) -> Self {
        Self {
            foundation_max_y: Some(maximum_y),
            explicit: BTreeSet::new(),
        }
    }

    /// Adds authored anchors while retaining deterministic ordering and bounded memory.
    ///
    /// # Errors
    ///
    /// Rejects more than 4,096 distinct explicit anchors.
    pub fn with_explicit(
        mut self,
        anchors: impl IntoIterator<Item = IVec3>,
    ) -> Result<Self, StructuralError> {
        for anchor in anchors {
            self.explicit.insert(anchor);
            if self.explicit.len() > MAX_EXPLICIT_ANCHORS {
                return Err(StructuralError::TooManyExplicitAnchors(self.explicit.len()));
            }
        }
        Ok(self)
    }

    fn contains(&self, position: IVec3) -> bool {
        self.foundation_max_y
            .is_some_and(|maximum_y| position.y <= maximum_y)
            || self.explicit.contains(&position)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetachedIsland {
    pub(crate) voxels: Vec<IVec3>,
    pub(crate) minimum: IVec3,
    pub(crate) maximum: IVec3,
    pub(crate) mass_kg: u64,
    pub(crate) fingerprint: u128,
}

impl DetachedIsland {
    #[must_use]
    pub fn voxels(&self) -> &[IVec3] {
        &self.voxels
    }

    #[must_use]
    pub const fn minimum(&self) -> IVec3 {
        self.minimum
    }

    #[must_use]
    pub const fn maximum(&self) -> IVec3 {
        self.maximum
    }

    #[must_use]
    pub const fn mass_kg(&self) -> u64 {
        self.mass_kg
    }

    #[must_use]
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StructuralReport {
    pub affected_seeds: usize,
    pub visited_voxels: usize,
    pub supported_components: usize,
    pub detached_islands: Vec<DetachedIsland>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StructuralError {
    TooManyExplicitAnchors(usize),
    TooManyChanges(usize),
    DuplicateOrUnsortedChange(IVec3),
    WorldAfterMismatch {
        position: IVec3,
        expected: Voxel,
        actual: Voxel,
    },
    TooManyAffectedSeeds(usize),
    VisitLimitExceeded(usize),
    ComponentLimitExceeded(usize),
    TooManyDetachedIslands(usize),
}

impl fmt::Display for StructuralError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyExplicitAnchors(count) => {
                write!(
                    formatter,
                    "{count} explicit anchors exceed {MAX_EXPLICIT_ANCHORS}"
                )
            }
            Self::TooManyChanges(count) => {
                write!(formatter, "structural analysis received {count} changes")
            }
            Self::DuplicateOrUnsortedChange(position) => {
                write!(
                    formatter,
                    "duplicate or unsorted structural change at {position:?}"
                )
            }
            Self::WorldAfterMismatch {
                position,
                expected,
                actual,
            } => write!(
                formatter,
                "structural world mismatch at {position:?}: expected {expected:?}, found {actual:?}"
            ),
            Self::TooManyAffectedSeeds(count) => {
                write!(
                    formatter,
                    "structural change produced {count} affected seeds"
                )
            }
            Self::VisitLimitExceeded(count) => {
                write!(
                    formatter,
                    "structural analysis visited more than {count} voxels"
                )
            }
            Self::ComponentLimitExceeded(count) => {
                write!(formatter, "structural component exceeds {count} voxels")
            }
            Self::TooManyDetachedIslands(count) => {
                write!(
                    formatter,
                    "structural analysis produced more than {count} islands"
                )
            }
        }
    }
}

impl std::error::Error for StructuralError {}

/// Classifies only solid components adjacent to topology-changing voxel edits.
///
/// Searches prefer downward neighbors so supported structures normally reach a foundation without
/// traversing the whole world. Unsupported components are exhaustively enumerated before they can
/// become authoritative rigid bodies.
///
/// # Errors
///
/// Rejects non-canonical changes, a world that does not contain their after-state, and every
/// configured work or result limit.
pub fn analyze_structural_changes(
    world: &World,
    changes: &[VoxelChange],
    anchors: &StructuralAnchors,
    limits: StructuralLimits,
) -> Result<StructuralReport, StructuralError> {
    if changes.len() > limits.max_changes {
        return Err(StructuralError::TooManyChanges(changes.len()));
    }
    validate_changes(world, changes)?;
    let seeds = affected_seeds(world, changes, limits.max_affected_seeds)?;
    let mut report = StructuralReport {
        affected_seeds: seeds.len(),
        ..StructuralReport::default()
    };
    let mut classified = HashMap::<IVec3, bool>::new();

    for seed in seeds {
        if classified.contains_key(&seed) {
            continue;
        }
        let (component, supported) = classify_component(
            world,
            seed,
            anchors,
            limits,
            &classified,
            report.visited_voxels,
        )?;
        report.visited_voxels = report.visited_voxels.saturating_add(component.len());
        for &position in &component {
            classified.insert(position, supported);
        }
        if supported {
            report.supported_components += 1;
            continue;
        }
        if report.detached_islands.len() == limits.max_detached_islands {
            return Err(StructuralError::TooManyDetachedIslands(
                limits.max_detached_islands,
            ));
        }
        report
            .detached_islands
            .push(describe_island(world, component));
    }
    report
        .detached_islands
        .sort_unstable_by_key(|island| (island.minimum, island.maximum, island.fingerprint));
    Ok(report)
}

fn validate_changes(world: &World, changes: &[VoxelChange]) -> Result<(), StructuralError> {
    for pair in changes.windows(2) {
        if pair[0].position >= pair[1].position {
            return Err(StructuralError::DuplicateOrUnsortedChange(pair[1].position));
        }
    }
    for change in changes {
        let actual = world.voxel(change.position);
        if actual != change.after {
            return Err(StructuralError::WorldAfterMismatch {
                position: change.position,
                expected: change.after,
                actual,
            });
        }
    }
    Ok(())
}

fn affected_seeds(
    world: &World,
    changes: &[VoxelChange],
    maximum: usize,
) -> Result<BTreeSet<IVec3>, StructuralError> {
    let mut seeds = BTreeSet::new();
    for change in changes {
        if is_structural(change.before) == is_structural(change.after) {
            continue;
        }
        if is_structural(change.after) {
            seeds.insert(change.position);
            if seeds.len() > maximum {
                return Err(StructuralError::TooManyAffectedSeeds(seeds.len()));
            }
        }
        for offset in NEIGHBORS {
            let neighbor = saturating_add(change.position, offset);
            if is_structural(world.voxel(neighbor)) {
                seeds.insert(neighbor);
                if seeds.len() > maximum {
                    return Err(StructuralError::TooManyAffectedSeeds(seeds.len()));
                }
            }
        }
    }
    Ok(seeds)
}

fn classify_component(
    world: &World,
    seed: IVec3,
    anchors: &StructuralAnchors,
    limits: StructuralLimits,
    classified: &HashMap<IVec3, bool>,
    already_visited: usize,
) -> Result<(Vec<IVec3>, bool), StructuralError> {
    let mut queue = VecDeque::from([seed]);
    let mut queued = HashSet::from([seed]);
    let mut component = Vec::new();
    let mut supported = false;

    'search: while let Some(position) = queue.pop_front() {
        component.push(position);
        if component.len() > limits.max_component_voxels {
            return Err(StructuralError::ComponentLimitExceeded(
                limits.max_component_voxels,
            ));
        }
        if already_visited.saturating_add(component.len()) > limits.max_visited_voxels {
            return Err(StructuralError::VisitLimitExceeded(
                limits.max_visited_voxels,
            ));
        }
        if anchors.contains(position) {
            supported = true;
            break;
        }
        for offset in NEIGHBORS {
            let neighbor = saturating_add(position, offset);
            if let Some(&neighbor_supported) = classified.get(&neighbor) {
                if neighbor_supported {
                    supported = true;
                    break 'search;
                }
                continue;
            }
            if is_structural(world.voxel(neighbor)) && queued.insert(neighbor) {
                if queued.len() > limits.max_component_voxels {
                    return Err(StructuralError::ComponentLimitExceeded(
                        limits.max_component_voxels,
                    ));
                }
                if already_visited.saturating_add(queued.len()) > limits.max_visited_voxels {
                    return Err(StructuralError::VisitLimitExceeded(
                        limits.max_visited_voxels,
                    ));
                }
                queue.push_back(neighbor);
            }
        }
    }
    component.sort_unstable();
    Ok((component, supported))
}

pub(crate) fn describe_island(world: &World, mut voxels: Vec<IVec3>) -> DetachedIsland {
    voxels.sort_unstable();
    let (minimum, maximum) = voxels.iter().copied().fold(
        (
            IVec3::new(i32::MAX, i32::MAX, i32::MAX),
            IVec3::new(i32::MIN, i32::MIN, i32::MIN),
        ),
        |(minimum, maximum), position| {
            (
                IVec3::new(
                    minimum.x.min(position.x),
                    minimum.y.min(position.y),
                    minimum.z.min(position.z),
                ),
                IVec3::new(
                    maximum.x.max(position.x),
                    maximum.y.max(position.y),
                    maximum.z.max(position.z),
                ),
            )
        },
    );
    let mut mass_kg = 0_u64;
    let mut fingerprint = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d_u128;
    for &position in &voxels {
        let voxel = world.voxel(position);
        mass_kg = mass_kg.saturating_add(u64::from(voxel.material.properties().density_kg_m3));
        fingerprint = mix_island_fingerprint(fingerprint, position, voxel);
    }
    DetachedIsland {
        voxels,
        minimum,
        maximum,
        mass_kg,
        fingerprint,
    }
}

const fn is_structural(voxel: Voxel) -> bool {
    voxel.is_solid() && voxel.material.properties().structural_strength > 0
}

const fn saturating_add(position: IVec3, offset: IVec3) -> IVec3 {
    IVec3::new(
        position.x.saturating_add(offset.x),
        position.y.saturating_add(offset.y),
        position.z.saturating_add(offset.z),
    )
}

const fn mix_island_fingerprint(state: u128, position: IVec3, voxel: Voxel) -> u128 {
    let coordinates = (position.x.cast_unsigned() as u128)
        | ((position.y.cast_unsigned() as u128) << 32)
        | ((position.z.cast_unsigned() as u128) << 64);
    let material = (voxel.material as u8 as u128) << 96;
    let integrity = (voxel.integrity as u128) << 104;
    state
        .rotate_left(29)
        .wrapping_add(coordinates ^ material ^ integrity)
        .wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b_u128)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Material;

    fn remove(world: &mut World, position: IVec3) -> VoxelChange {
        let before = world.set_voxel(position, Voxel::AIR);
        VoxelChange {
            position,
            before,
            after: Voxel::AIR,
        }
    }

    #[test]
    fn severed_column_becomes_one_canonical_island() {
        let mut world = World::default();
        world.fill_box(
            IVec3::new(0, 0, 0),
            IVec3::new(0, 3, 0),
            Voxel::new(Material::Wood),
        );
        let changes = [remove(&mut world, IVec3::new(0, 1, 0))];

        let report = analyze_structural_changes(
            &world,
            &changes,
            &StructuralAnchors::foundation_plane(0),
            StructuralLimits::default(),
        )
        .expect("small structural analysis");

        assert_eq!(report.detached_islands.len(), 1);
        assert_eq!(
            report.detached_islands[0].voxels,
            vec![IVec3::new(0, 2, 0), IVec3::new(0, 3, 0)]
        );
        assert_eq!(report.detached_islands[0].mass_kg, 1_300);
        assert_eq!(report.detached_islands[0].minimum, IVec3::new(0, 2, 0));
        assert_eq!(report.detached_islands[0].maximum, IVec3::new(0, 3, 0));
        let replay = analyze_structural_changes(
            &world,
            &changes,
            &StructuralAnchors::foundation_plane(0),
            StructuralLimits::default(),
        )
        .expect("replayed structural analysis");
        assert_eq!(replay, report);
    }

    #[test]
    fn alternate_support_prevents_detachment() {
        let mut world = World::default();
        world.fill_box(
            IVec3::new(0, 0, 0),
            IVec3::new(0, 2, 0),
            Voxel::new(Material::Steel),
        );
        world.fill_box(
            IVec3::new(2, 0, 0),
            IVec3::new(2, 2, 0),
            Voxel::new(Material::Steel),
        );
        world.fill_box(
            IVec3::new(0, 2, 0),
            IVec3::new(2, 2, 0),
            Voxel::new(Material::Steel),
        );
        let changes = [remove(&mut world, IVec3::new(0, 1, 0))];

        let report = analyze_structural_changes(
            &world,
            &changes,
            &StructuralAnchors::foundation_plane(0),
            StructuralLimits::default(),
        )
        .expect("alternate column is supported");

        assert!(report.detached_islands.is_empty());
        assert!(report.supported_components > 0);
    }

    #[test]
    fn explicit_anchor_supports_a_floating_authored_structure() {
        let mut world = World::default();
        world.fill_box(
            IVec3::new(4, 10, 4),
            IVec3::new(4, 12, 4),
            Voxel::new(Material::Concrete),
        );
        let changes = [remove(&mut world, IVec3::new(4, 12, 4))];
        let anchors = StructuralAnchors::default()
            .with_explicit([IVec3::new(4, 10, 4)])
            .expect("one authored anchor");

        let report =
            analyze_structural_changes(&world, &changes, &anchors, StructuralLimits::default())
                .expect("explicit anchor is valid");

        assert!(report.detached_islands.is_empty());
    }

    #[test]
    fn component_limit_fails_closed() {
        let mut world = World::default();
        world.fill_box(
            IVec3::new(0, 2, 0),
            IVec3::new(0, 4, 0),
            Voxel::new(Material::Brick),
        );
        let changes = [remove(&mut world, IVec3::new(0, 2, 0))];
        let limits = StructuralLimits {
            max_component_voxels: 1,
            ..StructuralLimits::default()
        };

        assert_eq!(
            analyze_structural_changes(&world, &changes, &StructuralAnchors::default(), limits),
            Err(StructuralError::ComponentLimitExceeded(1))
        );
    }

    #[test]
    fn island_bounds_are_component_wise_not_lexicographic() {
        let mut world = World::default();
        let positions = [IVec3::new(0, 10, 2), IVec3::new(1, 3, 8)];
        for position in positions {
            world.set_voxel(position, Voxel::new(Material::Stone));
        }

        let island = describe_island(&world, positions.into());

        assert_eq!(island.minimum, IVec3::new(0, 3, 2));
        assert_eq!(island.maximum, IVec3::new(1, 10, 8));
    }

    #[test]
    fn world_after_mismatch_fails_before_graph_work() {
        let mut world = World::default();
        let position = IVec3::new(0, 2, 0);
        let solid = Voxel::new(Material::Wood);
        world.set_voxel(position, solid);
        let changes = [VoxelChange {
            position,
            before: solid,
            after: Voxel::AIR,
        }];

        assert_eq!(
            analyze_structural_changes(
                &world,
                &changes,
                &StructuralAnchors::foundation_plane(0),
                StructuralLimits::default()
            ),
            Err(StructuralError::WorldAfterMismatch {
                position,
                expected: Voxel::AIR,
                actual: solid,
            })
        );
    }
}
