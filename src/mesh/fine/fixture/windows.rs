//! Thin open steel fenestration in the actual fine source, not an opaque texture or collider proxy.
use super::{
    Error, GeometryCell, GeometryChange, GeometryState, IVec3, LocalBox, Material, RefinedVolume,
    RefinedWorld, VolumeLimits, Voxel,
};
use crate::industrial::{HALL_FRONT_WINDOW_STARTS, HALL_SIDE_COLUMNS, HALL_SIDE_WINDOW_STARTS};

#[derive(Clone, Copy, Debug)]
struct Opening {
    origin: IVec3,
    side: bool,
    positive: bool,
    width: u16,
}

impl Opening {
    const fn cell(self, u: i32, y: i32) -> IVec3 {
        IVec3::new(
            self.origin.x + if self.side { 0 } else { u },
            self.origin.y + y,
            self.origin.z + if self.side { u } else { 0 },
        )
    }

    const fn axes(self, u: u16, y: u16, depth: u16) -> [u16; 3] {
        if self.side {
            [depth, y, u]
        } else {
            [u, y, depth]
        }
    }

    fn bars(self) -> Vec<([u16; 2], [u16; 2])> {
        let width = self.width * 256;
        let height = 5 * 256;
        let mut bars = vec![
            ([0, 0], [16, height]),
            ([width - 16, 0], [width, height]),
            ([0, 0], [width, 16]),
            ([0, height - 16], [width, height]),
            ([0, height / 2 - 8], [width, height / 2 + 8]),
        ];
        for quarter in 1..4 {
            let centre = width * quarter / 4;
            bars.push(([centre - 8, 0], [centre + 8, height]));
        }
        bars
    }

    fn page(self, u: u16, y: u16) -> Result<GeometryCell, Box<dyn Error>> {
        let mut volume = RefinedVolume::uniform(Voxel::AIR);
        let depth = if self.positive { 208 } else { 16 };
        for (low, high) in self.bars() {
            let minimum = [low[0].max(u * 256), low[1].max(y * 256)];
            let maximum = [high[0].min((u + 1) * 256), high[1].min((y + 1) * 256)];
            if minimum[0] >= maximum[0] || minimum[1] >= maximum[1] {
                continue;
            }
            volume = volume
                .replace_box(
                    LocalBox::new(
                        self.axes(minimum[0] - u * 256, minimum[1] - y * 256, depth),
                        self.axes(maximum[0] - u * 256, maximum[1] - y * 256, depth + 32),
                    )?,
                    Voxel::new(Material::Steel),
                    VolumeLimits::default(),
                )?
                .0;
        }
        Ok(GeometryCell::refined(volume))
    }

    fn validate(self, source: &RefinedWorld) -> Result<(), Box<dyn Error>> {
        for u in -1..=i32::from(self.width) {
            for y in -1..=5 {
                let perimeter = u == -1 || u == i32::from(self.width) || y == -1 || y == 5;
                let units = source.cell(self.cell(u, y)).solid_units();
                if units
                    != if perimeter {
                        crate::volume::VOLUME_UNITS
                    } else {
                        0
                    }
                {
                    return Err(format!(
                        "window needs a clear opening and full perimeter: {self:?} at {u},{y}"
                    )
                    .into());
                }
            }
        }
        Ok(())
    }
}

fn openings() -> Vec<Opening> {
    let mut openings = Vec::with_capacity(14);
    for z in [-15, 15] {
        for x in HALL_FRONT_WINDOW_STARTS {
            openings.push(Opening {
                origin: IVec3::new(x, 7, z),
                side: false,
                positive: z > 0,
                width: 6,
            });
        }
    }
    for x in [-20, 20] {
        for z in HALL_SIDE_WINDOW_STARTS {
            // The coarse authoring installs its bearing columns AFTER cutting windows.
            // Preserve a column that occupies the first cutout cell; never carve it for trim.
            let inset = i32::from(HALL_SIDE_COLUMNS.contains(&z));
            openings.push(Opening {
                origin: IVec3::new(x, 7, z + inset),
                side: true,
                positive: x > 0,
                width: if inset == 0 { 6 } else { 5 },
            });
        }
    }
    openings
}

pub(super) fn install(source: &RefinedWorld) -> Result<RefinedWorld, Box<dyn Error>> {
    let openings = openings();
    // Validate the complete original layout before preparing any derived candidate.
    for opening in &openings {
        opening.validate(source)?;
    }
    let mut state = GeometryState::new(source.clone(), 1)?;
    for opening in openings {
        // At most 6*5 distinct cells per window, eight bars unioned BEFORE the transaction.
        let mut changes = Vec::with_capacity(30);
        for u in 0..opening.width {
            for y in 0..5 {
                let after = opening.page(u, y)?;
                if after == GeometryCell::AIR {
                    continue;
                }
                changes.push(GeometryChange {
                    position: opening.cell(i32::from(u), i32::from(y)),
                    before: GeometryCell::AIR,
                    after,
                });
            }
        }
        changes.sort_by_key(|c| c.position);
        // One authored snapshot: transaction sequence advances, simulation time does not.
        let tx = state.prepare(source.tick(), changes)?;
        state.apply(&tx)?;
    }
    Ok(state.world().clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        FixedMicrometers3,
        ballistics::FixedRay,
        world::query::ray::{TraceLimits, trace_materials},
    };

    #[test]
    fn material_rays_pass_open_panes_and_hit_real_mullions_and_transoms_on_every_facade() {
        let source = RefinedWorld::from_uniform(&crate::WorldPreset::Industrial.build()).unwrap();
        let framed = install(&source).unwrap();
        for opening in openings() {
            let width = opening.width * 256;
            for (u, y, hit) in [
                (width / 8, 128, false),
                (width / 4, 128, true),
                (width / 8, 640, true),
            ] {
                let local = opening.axes(u, y, 0).map(i64::from);
                let mut origin = [opening.origin.x, opening.origin.y, opening.origin.z]
                    .map(|v| i64::from(v) * 1_000_000);
                for axis in 0..3 {
                    origin[axis] += local[axis] * 1_000_000 / 256;
                }
                let depth_axis = if opening.side { 0 } else { 2 };
                origin[depth_axis] -= 500_000;
                let mut direction = [0; 3];
                direction[depth_axis] = 1;
                let ray = FixedRay::new(
                    FixedMicrometers3 {
                        x: origin[0],
                        y: origin[1],
                        z: origin[2],
                    },
                    direction,
                    2_000_000,
                )
                .unwrap();
                let trace = trace_materials(&framed, &ray, TraceLimits::default()).unwrap();
                assert_eq!(!trace.chords.is_empty(), hit, "{opening:?} u={u} y={y}");
                assert!(
                    trace
                        .chords
                        .iter()
                        .all(|c| c.material.leaf.voxel().material == Material::Steel)
                );
                assert_eq!(trace.source_fingerprint, framed.fingerprint());
            }
        }
    }

    #[test]
    fn all_openings_are_clear_and_supported_without_erasing_bearing_columns() {
        let original = RefinedWorld::from_uniform(&crate::WorldPreset::Industrial.build()).unwrap();
        let fingerprint = original.fingerprint();
        let framed = install(&original).unwrap();
        assert_eq!(original.fingerprint(), fingerprint);
        let openings = openings();
        assert_eq!(openings.len(), 14);
        assert_eq!(openings.iter().filter(|o| o.width == 5).count(), 2);
        for (position, cell) in original.occupied_cells() {
            assert_eq!(
                framed.cell(position),
                cell,
                "authored solids must not be replaced"
            );
        }
        for opening in openings {
            opening.validate(&original).unwrap();
            let mut solid = 0_u64;
            for u in 0..opening.width {
                for y in 0..5 {
                    solid += u64::from(framed.cell(opening.cell(i32::from(u), y)).solid_units());
                }
            }
            let width = u64::from(opening.width) * 256;
            // Five vertical 16-unit strips, three horizontal strips; intersections counted once.
            assert_eq!(solid, (80 * 1280 + 48 * width - 80 * 48) * 32);
        }
        assert_eq!(
            framed.fingerprint(),
            install(&original).unwrap().fingerprint()
        );
        assert!(
            install(&framed).is_err(),
            "never overwrite an occupied opening"
        );
        assert!(
            install(&RefinedWorld::default()).is_err(),
            "never float frames without masonry"
        );
    }
}
