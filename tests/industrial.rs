use destructible_fps::{
    AuthoritativeServer, ClientReplica, ExplosionCommand, FrameAssembler, IVec3, Material,
    SnapshotAssembler, Voxel, World, WorldPreset, decode_frame, encode_frames,
    encode_snapshot_frames,
    industrial::{BARRICADE, CANOPY_SUPPORTS, industrial_world},
    snapshot::{MAX_SNAPSHOT_PAYLOAD_BYTES, MAX_SNAPSHOT_STATIC_VOXELS},
};
use std::collections::{HashSet, VecDeque};

const NEIGHBORS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

fn unsupported(world: &World) -> HashSet<IVec3> {
    let mut remaining: HashSet<_> = world
        .occupied_voxels()
        .into_iter()
        .filter_map(|(p, _)| (p.y > 0).then_some(p))
        .collect();
    let mut queue: VecDeque<_> = remaining
        .iter()
        .copied()
        .filter(|p| p.y == 1 && world.voxel(IVec3::new(p.x, 0, p.z)).is_solid())
        .collect();
    for p in &queue {
        remaining.remove(p);
    }
    while let Some(p) = queue.pop_front() {
        for (x, y, z) in NEIGHBORS {
            let next = IVec3::new(p.x + x, p.y + y, p.z + z);
            if remaining.remove(&next) {
                queue.push_back(next);
            }
        }
    }
    remaining
}

fn snapshot_roundtrip(authority: &AuthoritativeServer, mtu: usize) {
    let frames = encode_snapshot_frames(1, authority, mtu).unwrap();
    let bytes: usize = frames.iter().map(Vec::len).sum();
    assert!(bytes < MAX_SNAPSHOT_PAYLOAD_BYTES);
    assert!(frames.iter().all(|frame| frame.len() <= mtu));
    let mut assembler = SnapshotAssembler::default();
    let mut installed = None;
    for frame in frames.iter().rev() {
        if let Some(snapshot) = assembler.push(frame).unwrap() {
            installed = Some(snapshot);
        }
    }
    let mut late = ClientReplica::new(WorldPreset::Range.build());
    installed.unwrap().install_into(&mut late).unwrap();
    assert_eq!(late.world().fingerprint(), authority.world().fingerprint());
    assert_eq!(
        late.world().occupied_voxels(),
        authority.world().occupied_voxels()
    );
    assert_eq!(late.bodies(), authority.bodies());
    assert_eq!(late.body_states(), authority.body_states());
    println!(
        "industrial snapshot MTU={mtu} frames={} bytes={bytes}",
        frames.len()
    );
}

#[test]
fn authored_world_is_bounded_deterministic_and_initially_supported() {
    let world = industrial_world();
    assert_eq!(
        world.occupied_voxels(),
        industrial_world().occupied_voxels()
    );
    assert_eq!(world.fingerprint(), world.recompute_fingerprint());
    assert!(world.stats().solid_voxels <= MAX_SNAPSHOT_STATIC_VOXELS);
    assert!(world.stats().chunks <= 512);
    assert_ne!(
        world.fingerprint(),
        WorldPreset::Range.build().fingerprint()
    );
    for (p, v) in world.occupied_voxels() {
        assert!((-64..64).contains(&p.x) && (-64..64).contains(&p.z) && (-3..=18).contains(&p.y));
        assert_eq!(
            v.integrity, 255,
            "intact content must not contain hidden pre-damage"
        );
        assert_ne!(
            v.material,
            Material::Glass,
            "open windows are actual apertures"
        );
    }
    assert!(
        unsupported(&world).is_empty(),
        "intact content contains floating material"
    );
    println!(
        "industrial intact solids={} chunks={} fingerprint={:032x}",
        world.stats().solid_voxels,
        world.stats().chunks,
        world.fingerprint()
    );
    let authority = AuthoritativeServer::new(world);
    for mtu in [1100, 1200] {
        snapshot_roundtrip(&authority, mtu);
    }
}

#[test]
fn canopy_has_exactly_four_load_paths_and_no_hidden_wall_or_terrain_anchor() {
    let mut world = industrial_world();
    let canopy: HashSet<_> = world
        .occupied_voxels()
        .into_iter()
        .filter_map(|(p, _)| {
            ((23..=37).contains(&p.x) && (11..=27).contains(&p.z) && p.y > 0 && p.y <= 6)
                .then_some(p)
        })
        .collect();
    assert!(!canopy.is_empty());
    for support in CANOPY_SUPPORTS.into_iter().take(3) {
        world.set_voxel(support, Voxel::AIR);
        assert!(
            unsupported(&world).is_empty(),
            "remaining supports must retain the slab"
        );
    }
    world.set_voxel(CANOPY_SUPPORTS[3], Voxel::AIR);
    let expected: HashSet<_> = canopy
        .difference(&CANOPY_SUPPORTS.into_iter().collect())
        .copied()
        .collect();
    assert_eq!(unsupported(&world), expected);
}

fn replicate(
    authority: &AuthoritativeServer,
    packet: &destructible_fps::DeltaPacket,
    replicas: &mut [ClientReplica; 2],
) {
    for (replica, mtu) in replicas.iter_mut().zip([1100, 1200]) {
        let mut assembler = FrameAssembler::default();
        let mut complete = None;
        for frame in encode_frames(packet, mtu).unwrap().into_iter().rev() {
            assert!(frame.len() <= mtu);
            if let Some(packet) = assembler.push(decode_frame(&frame).unwrap()).unwrap() {
                complete = Some(packet);
            }
        }
        replica.receive(&complete.unwrap()).unwrap();
        assert_eq!(
            replica.world().fingerprint(),
            authority.world().fingerprint()
        );
        assert_eq!(replica.bodies(), authority.bodies());
        assert_eq!(replica.body_states(), authority.body_states());
    }
}

#[test]
fn real_blasts_sever_canopy_conserve_undestroyed_cells_and_replicate_falling_bodies() {
    let world = industrial_world();
    let initial = world.stats().solid_voxels;
    let mut replicas = [
        ClientReplica::new(world.clone()),
        ClientReplica::new(world.clone()),
    ];
    let mut authority = AuthoritativeServer::new(world);
    let mut fractured = 0;
    for (index, center) in CANOPY_SUPPORTS.into_iter().enumerate() {
        let (packet, report) = authority
            .execute_explosion(
                1,
                ExplosionCommand {
                    command_id: index as u64 + 1,
                    center,
                    radius_voxels: 2,
                    peak_energy: 42_000,
                },
            )
            .unwrap();
        fractured += report.fractured_voxels;
        if index < 3 {
            assert_eq!(report.detached_voxels, 0);
        } else {
            assert!(report.detached_voxels >= 255 && !packet.body_assignments.is_empty());
        }
        let retained: usize = authority
            .bodies()
            .values()
            .map(|body| body.voxels.len())
            .sum();
        assert_eq!(
            authority.world().stats().solid_voxels + retained + fractured,
            initial
        );
        replicate(&authority, &packet, &mut replicas);
    }
    let spawned = authority.body_states().clone();
    for _ in 0..180 {
        if let (Some(packet), _) = authority.advance_physics() {
            replicate(&authority, &packet, &mut replicas);
        }
    }
    assert!(
        authority
            .body_states()
            .iter()
            .any(|(id, state)| state.translation_um.y < spawned[id].translation_um.y)
    );
    for mtu in [1100, 1200] {
        snapshot_roundtrip(&authority, mtu);
    }
}

#[test]
fn low_energy_hits_accumulate_on_authored_timber_without_changing_material_policy() {
    let mut authority = AuthoritativeServer::new(industrial_world());
    let mut prior = authority.world().voxel(BARRICADE).integrity;
    for command_id in 1..=4 {
        let (_, report) = authority
            .execute_explosion(
                1,
                ExplosionCommand {
                    command_id,
                    center: BARRICADE,
                    radius_voxels: 1,
                    peak_energy: 500,
                },
            )
            .unwrap();
        let voxel = authority.world().voxel(BARRICADE);
        assert!(voxel.integrity < prior);
        if command_id == 1 {
            assert!(voxel.is_solid() && report.damaged_voxels > 0);
        }
        prior = voxel.integrity;
    }
    assert!(!authority.world().voxel(BARRICADE).is_solid());
}
