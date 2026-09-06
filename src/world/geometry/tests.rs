use super::*;
use crate::{
    Material,
    volume::{LocalBox, VolumeLimits},
    world::chunk_position,
};

fn wood() -> GeometryCell {
    GeometryCell::uniform(Voxel::new(Material::Wood))
}

fn bore() -> GeometryCell {
    let (page, _) = RefinedVolume::uniform(Voxel::new(Material::Wood))
        .replace_box(
            LocalBox::new([127, 127, 0], [129, 129, 256]).unwrap(),
            Voxel::AIR,
            VolumeLimits::default(),
        )
        .unwrap();
    GeometryCell::refined(page)
}

fn empty() -> GeometryState {
    GeometryState::new(RefinedWorld::default(), 1).unwrap()
}

// Append a syntactically valid record which a bounded author could never emit. Keep the old
// hash deliberately: tests require a budget error BEFORE the eventual fingerprint check.
fn append_checkpoint(bytes: &mut Vec<u8>, position: IVec3, cell: &GeometryCell) {
    let count = u32::from_le_bytes(bytes[37..41].try_into().unwrap()) + 1;
    bytes[37..41].copy_from_slice(&count.to_le_bytes());
    for component in [position.x, position.y, position.z] {
        bytes.extend_from_slice(&component.to_le_bytes());
    }
    if let Some(voxel) = cell.uniform_voxel() {
        bytes.extend_from_slice(&[0, voxel.material as u8, voxel.integrity]);
    } else {
        let page = cell.volume().unwrap().encode().unwrap();
        bytes.push(1);
        bytes.extend_from_slice(&u32::try_from(page.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&page);
    }
}

fn change(state: &GeometryState, position: IVec3, after: GeometryCell) -> GeometryChange {
    GeometryChange {
        position,
        before: state.world.cell(position),
        after,
    }
}

fn commit(state: &mut GeometryState, mut cells: Vec<(IVec3, GeometryCell)>) -> GeometryTransaction {
    cells.sort_unstable_by_key(|(pos, _)| *pos);
    let changes = cells
        .into_iter()
        .map(|(pos, cell)| change(state, pos, cell))
        .collect();
    let transaction = state.prepare(state.world.tick() + 1, changes).unwrap();
    state.apply(&transaction).unwrap();
    assert_eq!(
        state.world.fingerprint(),
        state.world.recompute_fingerprint()
    );
    transaction
}

#[test]
fn typed_promotion_preserves_uniform_fingerprint_and_refuses_partial_downgrade() {
    let mut coarse = World::default();
    coarse.fill_box(
        IVec3::new(-17, -1, -2),
        IVec3::new(17, 0, 2),
        Voxel::new(Material::Brick),
    );
    coarse.set_tick(42);
    let fine = RefinedWorld::from_uniform(&coarse).unwrap();
    assert_eq!(coarse.stats(), fine.stats());
    assert_eq!(
        fine.to_uniform().unwrap().occupied_voxels(),
        coarse.occupied_voxels()
    );
    assert_eq!(fine.fingerprint(), fine.recompute_fingerprint());
    let mut state = GeometryState::new(fine, 12).unwrap();
    commit(&mut state, vec![(IVec3::new(-17, -1, -2), bore())]);
    assert!(matches!(
        state.world.to_uniform(),
        Err(GeometryError::RequiresFineConsumers)
    ));
    commit(
        &mut state,
        vec![(
            IVec3::new(-17, -1, -2),
            GeometryCell::refined(RefinedVolume::uniform(Voxel::new(Material::Brick))),
        )],
    );
    assert_eq!(
        state.world.to_uniform().unwrap().occupied_voxels(),
        coarse.occupied_voxels()
    );
    assert_eq!(state.world.fingerprint(), coarse.fingerprint());
}

#[test]
fn conversion_records_actual_hashmap_capacity_including_tiny_maps() {
    let mut coarse = World::default();
    coarse.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Wood));
    let fine = RefinedWorld::from_uniform(&coarse).unwrap();
    assert_eq!(fine.chunk_capacity_high_water, fine.chunks.capacity());
    let restored = fine.to_uniform().unwrap();
    assert_eq!(
        restored.chunk_capacity_high_water,
        restored.chunks.capacity()
    );
    let should_fit = fine.chunks.capacity() <= 2;
    assert_eq!(fine.bounded_snapshot(1).is_some(), should_fit);
    assert_eq!(restored.bounded_snapshot(1).is_some(), should_fit);
}

#[test]
fn refined_only_chunk_is_never_empty_and_cow_observations_reject_aba() {
    let position = IVec3::new(-17, -1, -16);
    let mut state = empty();
    let absent = state.world.observe_chunk(chunk_position(position));
    commit(&mut state, vec![(position, bore())]);
    assert!(!absent.matches(&state.world));
    assert_eq!(state.world.stats().solid_voxels, 1);
    assert_eq!(state.world.geometry_stats().refined_pages, 1);
    assert!(
        state
            .world
            .chunks
            .values()
            .all(|chunk| chunk.voxels.iter().all(|v| *v == Voxel::AIR))
    );
    let occupied = state.world.observe_chunk(chunk_position(position));
    let snapshot = state.world.clone();
    let noop = state
        .prepare(2, vec![change(&state, position, bore())])
        .unwrap();
    state.apply(&noop).unwrap();
    assert!(occupied.matches(&state.world));
    commit(&mut state, vec![(position, GeometryCell::AIR)]);
    assert_eq!(state.world.stats().chunks, 0);
    assert_eq!(state.world.fingerprint(), 0);
    assert!(!absent.matches(&state.world));
    commit(&mut state, vec![(position, bore())]);
    assert!(!occupied.matches(&state.world));
    assert_eq!(snapshot.cell(position), state.world.cell(position));
    assert_eq!(snapshot.stats().chunks, 1);
}

#[test]
fn multi_chunk_failure_keeps_all_bytes_tokens_ticks_and_sequences() {
    let a = IVec3::new(-1, 0, 0);
    let b = IVec3::new(16, 0, 0);
    let mut state = empty();
    commit(&mut state, vec![(a, wood()), (b, bore())]);
    let checkpoint = state.encode_checkpoint().unwrap();
    let observation = state.world.observe_chunk(chunk_position(a));
    let transaction = state
        .prepare(
            8,
            vec![change(&state, a, bore()), change(&state, b, wood())],
        )
        .unwrap();
    let mut bad = transaction.clone();
    bad.after ^= 1;
    assert_eq!(state.apply(&bad), Err(GeometryError::Fingerprint));
    bad = transaction.clone();
    bad.before ^= 1;
    assert_eq!(state.apply(&bad), Err(GeometryError::Fingerprint));
    bad = transaction.clone();
    bad.changes[1].before = wood();
    assert_eq!(state.apply(&bad), Err(GeometryError::BeforeState(b)));
    bad = transaction.clone();
    bad.changes.swap(0, 1);
    assert_eq!(state.apply(&bad), Err(GeometryError::Order));
    bad = transaction.clone();
    bad.changes[1] = bad.changes[0].clone();
    assert_eq!(state.apply(&bad), Err(GeometryError::Order));
    bad = transaction.clone();
    bad.tick = 0;
    assert_eq!(state.apply(&bad), Err(GeometryError::Tick));
    bad = transaction.clone();
    bad.sequence += 1;
    assert_eq!(state.apply(&bad), Err(GeometryError::Sequence));
    assert_eq!(state.encode_checkpoint().unwrap(), checkpoint);
    assert!(observation.matches(&state.world));
    state.apply(&transaction).unwrap();
    assert!(!observation.matches(&state.world));
    assert_ne!(state.world.fingerprint(), transaction.before);
    assert_eq!(state.apply(&transaction), Err(GeometryError::Sequence));
}

#[test]
fn unchanged_chunks_and_concurrent_immutable_reader_retain_complete_old_state() {
    let mut state = empty();
    let a = IVec3::new(-16, 0, 0);
    let b = IVec3::new(16, 0, 0);
    let untouched = IVec3::new(32, 0, 0);
    commit(
        &mut state,
        vec![(a, wood()), (b, wood()), (untouched, bore())],
    );
    let observation = state.world.observe_chunk(chunk_position(untouched));
    let snapshot = state.clone();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let reader_barrier = barrier.clone();
    let reader = std::thread::spawn(move || {
        reader_barrier.wait();
        for _ in 0..1000 {
            assert_eq!(snapshot.world.cell(a), wood());
            assert_eq!(snapshot.world.cell(b), wood());
            assert_eq!(snapshot.next_sequence, 2);
        }
    });
    barrier.wait();
    commit(&mut state, vec![(a, bore()), (b, bore())]);
    reader.join().unwrap();
    assert!(observation.matches(&state.world));
    assert_eq!(state.world.cell(a), bore());
    assert_eq!(state.world.cell(b), bore());
}

#[test]
fn component_transactions_and_checkpoint_repair_converge_two_exact_worlds() {
    let mut authority = empty();
    let mut replicas = [empty(), empty()];
    let initial = commit(&mut authority, vec![(IVec3::new(0, 0, 0), wood())]);
    for replica in &mut replicas {
        replica
            .apply(&GeometryTransaction::decode(&initial.encode().unwrap()).unwrap())
            .unwrap();
    }
    let shot = commit(&mut authority, vec![(IVec3::new(0, 0, 0), bore())]);
    replicas[0]
        .apply(&GeometryTransaction::decode(&shot.encode().unwrap()).unwrap())
        .unwrap();
    let more = commit(&mut authority, vec![(IVec3::new(-17, 4, 15), bore())]);
    let checkpoint_before_gap = replicas[1].encode_checkpoint().unwrap();
    assert_eq!(replicas[1].apply(&more), Err(GeometryError::Sequence));
    assert_eq!(
        replicas[1].encode_checkpoint().unwrap(),
        checkpoint_before_gap
    );
    replicas[0].apply(&more).unwrap();
    // This is component staging, not a live ClientReplica snapshot or a loss-impaired transport.
    let checkpoint = authority.encode_checkpoint().unwrap();
    replicas[1] = GeometryState::decode_checkpoint(&checkpoint).unwrap();
    for replica in &replicas {
        assert_eq!(replica.encode_checkpoint().unwrap(), checkpoint);
        assert_eq!(
            replica
                .world
                .cell(IVec3::new(0, 0, 0))
                .volume()
                .unwrap()
                .leaf_at([128, 128, 128])
                .unwrap()
                .voxel(),
            Voxel::AIR
        );
    }
    let collapse = commit(
        &mut authority,
        vec![(IVec3::new(0, 0, 0), GeometryCell::AIR)],
    );
    for replica in &mut replicas {
        replica.apply(&collapse).unwrap();
    }
    assert_eq!(
        replicas[0].encode_checkpoint().unwrap(),
        replicas[1].encode_checkpoint().unwrap()
    );
}

#[test]
fn coordinate_extrema_uniform_air_and_zero_integrity_keep_exact_semantics() {
    let mut state = empty();
    let positions = [
        IVec3::new(i32::MIN, i32::MIN, i32::MIN),
        IVec3::new(-17, -16, -1),
        IVec3::new(15, 16, 17),
        IVec3::new(i32::MAX, i32::MAX, i32::MAX),
    ];
    commit(
        &mut state,
        positions.into_iter().map(|p| (p, bore())).collect(),
    );
    assert_eq!(
        state
            .world
            .occupied_cells()
            .iter()
            .map(|(p, _)| *p)
            .collect::<Vec<_>>(),
        positions
    );
    assert_eq!(
        GeometryState::decode_checkpoint(&state.encode_checkpoint().unwrap())
            .unwrap()
            .world
            .occupied_cells(),
        state.world.occupied_cells()
    );
    let air = GeometryCell::uniform(Voxel {
        material: Material::Air,
        integrity: 255,
    });
    assert_eq!(air, GeometryCell::AIR);
    let zero_integrity = GeometryCell::uniform(Voxel {
        material: Material::Wood,
        integrity: 0,
    });
    assert_eq!(zero_integrity.solid_units(), crate::volume::VOLUME_UNITS);
    commit(&mut state, vec![(positions[0], zero_integrity.clone())]);
    assert_eq!(state.world.cell(positions[0]), zero_integrity);
}

#[test]
fn sparse_chunk_bound_is_checked_before_dense_allocation_and_full_budget_can_move() {
    let mut state = empty();
    for base in [0, 256] {
        commit(
            &mut state,
            (base..base + 256)
                .map(|x| (IVec3::new(x * 16, 0, 0), bore()))
                .collect(),
        );
    }
    assert_eq!(state.world.stats().chunks, MAX_GEOMETRY_CHUNKS);
    let before = state.encode_checkpoint().unwrap();
    let new = IVec3::new(-16, 0, 0);
    assert_eq!(
        state.prepare(3, vec![change(&state, new, wood())]),
        Err(GeometryError::WorldBudget)
    );
    assert_eq!(state.encode_checkpoint().unwrap(), before);
    assert!(GeometryState::decode_checkpoint(&before).is_ok());
    let mut oversized = before;
    append_checkpoint(&mut oversized, IVec3::new(8192, 0, 0), &wood());
    assert!(matches!(
        GeometryState::decode_checkpoint(&oversized),
        Err(GeometryError::WorldBudget)
    ));
    // Canonical order inserts the new cell first; staging removes old cells before ANY insert.
    commit(
        &mut state,
        vec![(new, bore()), (IVec3::new(0, 0, 0), GeometryCell::AIR)],
    );
    assert_eq!(state.world.stats().chunks, MAX_GEOMETRY_CHUNKS);
}

fn checker() -> GeometryCell {
    let mut bytes = b"DFVL\x02".to_vec();
    bytes.extend_from_slice(&8192_u32.to_le_bytes());
    for z in 0..16_u16 {
        for y in 0..16_u16 {
            for x in 0..32_u16 {
                for end in [(x + 1) * 8, (y + 1) * 16, (z + 1) * 16] {
                    bytes.extend_from_slice(&end.to_le_bytes());
                }
                let voxel = Voxel::new(if (x + y + z) % 2 == 0 {
                    Material::Wood
                } else {
                    Material::Steel
                });
                bytes.extend_from_slice(&[voxel.material as u8, voxel.integrity]);
            }
        }
    }
    GeometryCell::refined(RefinedVolume::decode(&bytes).unwrap())
}

#[test]
fn page_leaf_transaction_and_global_leaf_bounds_accept_exact_cap_and_refuse_excess() {
    let page = checker();
    let mut state = empty();
    for base in [0, 4, 8, 12] {
        let packet = commit(
            &mut state,
            (base..base + 4)
                .map(|x| (IVec3::new(x, 0, 0), page.clone()))
                .collect(),
        );
        assert_eq!(
            GeometryTransaction::decode(&packet.encode().unwrap()).unwrap(),
            packet
        );
    }
    assert_eq!(
        state.world.geometry_stats().refined_leaves,
        MAX_REFINED_LEAVES
    );
    let checkpoint = state.encode_checkpoint().unwrap();
    assert_eq!(
        GeometryState::decode_checkpoint(&checkpoint)
            .unwrap()
            .encode_checkpoint()
            .unwrap(),
        checkpoint
    );
    let mut oversized = checkpoint.clone();
    append_checkpoint(&mut oversized, IVec3::new(16, 0, 0), &page);
    assert!(matches!(
        GeometryState::decode_checkpoint(&oversized),
        Err(GeometryError::WorldBudget)
    ));
    assert_eq!(
        state.prepare(6, vec![change(&state, IVec3::new(17, 0, 0), page.clone())]),
        Err(GeometryError::WorldBudget)
    );
    let noops = (0..2)
        .map(|x| change(&state, IVec3::new(x, 0, 0), page.clone()))
        .collect();
    assert!(state.prepare(6, noops).is_ok()); // 2 * (8192 before + 8192 after).
    let oversized = (0..3)
        .map(|x| change(&state, IVec3::new(x, 0, 0), page.clone()))
        .collect();
    assert_eq!(
        state.prepare(6, oversized),
        Err(GeometryError::TransactionBudget)
    );
    assert_eq!(state.encode_checkpoint().unwrap(), checkpoint);
}

#[test]
fn refined_page_cap_in_one_dense_chunk_counts_occupancy_not_placeholder_air() {
    let (page, _) = RefinedVolume::uniform(Voxel::new(Material::Wood))
        .replace_box(
            LocalBox::new([0; 3], [128, 256, 256]).unwrap(),
            Voxel::AIR,
            VolumeLimits::default(),
        )
        .unwrap();
    assert_eq!(page.leaves().len(), 2);
    let page = GeometryCell::refined(page);
    let mut state = empty();
    for x in 0..16 {
        commit(
            &mut state,
            (0..16)
                .flat_map(|y| (0..16).map(move |z| IVec3::new(x, y, z)))
                .map(|p| (p, page.clone()))
                .collect(),
        );
    }
    assert_eq!(
        state.world.geometry_stats().refined_pages,
        MAX_REFINED_PAGES
    );
    assert_eq!(state.world.stats().chunks, 1);
    let checkpoint = state.encode_checkpoint().unwrap();
    assert_eq!(
        GeometryState::decode_checkpoint(&checkpoint)
            .unwrap()
            .world
            .geometry_stats(),
        state.world.geometry_stats()
    );
    let mut oversized = checkpoint;
    append_checkpoint(&mut oversized, IVec3::new(16, 0, 0), &page);
    assert!(matches!(
        GeometryState::decode_checkpoint(&oversized),
        Err(GeometryError::WorldBudget)
    ));
    assert_eq!(
        state.prepare(17, vec![change(&state, IVec3::new(16, 0, 0), page)]),
        Err(GeometryError::WorldBudget)
    );
}

#[test]
fn checkpointability_and_uniform_occupied_cell_caps_are_independent_residency_bounds() {
    let full = GeometryStats {
        chunks: 64,
        occupied_cells: MAX_GEOMETRY_CELLS,
        refined_pages: 0,
        refined_leaves: 0,
    };
    assert!(full.validate().is_ok());
    assert_eq!(
        GeometryStats {
            occupied_cells: MAX_GEOMETRY_CELLS + 1,
            ..full
        }
        .validate(),
        Err(GeometryError::WorldBudget)
    );
    assert_eq!(
        GeometryStats {
            refined_pages: 16,
            refined_leaves: MAX_REFINED_LEAVES,
            ..full
        }
        .validate(),
        Err(GeometryError::WorldBudget)
    );
    let mut coarse = World::default();
    coarse.fill_box(
        IVec3::new(0, 0, 0),
        IVec3::new(63, 63, 63),
        Voxel::new(Material::Wood),
    );
    let fine = RefinedWorld::from_uniform(&coarse).unwrap();
    let mut state = GeometryState::new(fine, 1).unwrap();
    let checkpoint = state.encode_checkpoint().unwrap();
    assert_eq!(checkpoint.len(), 41 + 15 * MAX_GEOMETRY_CELLS);
    assert_eq!(
        GeometryState::decode_checkpoint(&checkpoint)
            .unwrap()
            .world
            .fingerprint(),
        coarse.fingerprint()
    );
    // Four max pages still fit the record/leaf/chunk limits, but exceed 4MiB by 85 bytes.
    let page = checker();
    let changes = (0..4)
        .map(|x| change(&state, IVec3::new(x, 0, 0), page.clone()))
        .collect();
    assert_eq!(state.prepare(1, changes), Err(GeometryError::WorldBudget));
    assert_eq!(state.encode_checkpoint().unwrap(), checkpoint);
    commit(
        &mut state,
        (0..3)
            .map(|x| (IVec3::new(x, 0, 0), page.clone()))
            .collect(),
    );
    assert!(state.encode_checkpoint().unwrap().len() < wire::MAX_GEOMETRY_CHECKPOINT_BYTES);
    assert_eq!(
        state.prepare(2, vec![change(&state, IVec3::new(3, 0, 0), page)]),
        Err(GeometryError::WorldBudget)
    );
    coarse.set_voxel(IVec3::new(64, 0, 0), Voxel::new(Material::Wood));
    assert!(matches!(
        RefinedWorld::from_uniform(&coarse),
        Err(GeometryError::WorldBudget)
    ));
}

#[test]
fn sequence_overflow_and_empty_transactions_are_explicit() {
    assert!(matches!(
        GeometryState::new(RefinedWorld::default(), 0),
        Err(GeometryError::Sequence)
    ));
    let exhausted = GeometryState::new(RefinedWorld::default(), u64::MAX).unwrap();
    assert_eq!(exhausted.prepare(0, vec![]), Err(GeometryError::Sequence));
    let mut state = empty();
    let packet = state.prepare(50, vec![]).unwrap();
    state
        .apply(&GeometryTransaction::decode(&packet.encode().unwrap()).unwrap())
        .unwrap();
    assert_eq!(state.next_sequence(), 2);
    assert_eq!(state.world.tick(), 50);
    assert_eq!(state.world.fingerprint(), 0);
    assert_eq!(state.prepare(49, vec![]), Err(GeometryError::Tick));
    // The last representable transaction must remain checkpointable as a terminal state.
    let mut terminal = GeometryState::new(RefinedWorld::default(), u64::MAX - 1).unwrap();
    let last = terminal.prepare(1, vec![]).unwrap();
    terminal
        .apply(&GeometryTransaction::decode(&last.encode().unwrap()).unwrap())
        .unwrap();
    let terminal =
        GeometryState::decode_checkpoint(&terminal.encode_checkpoint().unwrap()).unwrap();
    assert_eq!(terminal.next_sequence(), u64::MAX);
    assert_eq!(terminal.prepare(2, vec![]), Err(GeometryError::Sequence));
}

#[test]
fn change_count_boundary_is_enforced_in_prepare_apply_encode_and_decode() {
    let mut state = empty();
    let changes: Vec<_> = (0..256)
        .map(|x| change(&state, IVec3::new(x, 0, 0), wood()))
        .collect();
    let accepted = state.prepare(1, changes).unwrap();
    let encoded = accepted.encode().unwrap();
    assert_eq!(GeometryTransaction::decode(&encoded).unwrap(), accepted);
    let mut excess = accepted.clone();
    excess
        .changes
        .push(change(&state, IVec3::new(256, 0, 0), wood()));
    assert_eq!(
        state.prepare(1, excess.changes.clone()),
        Err(GeometryError::TransactionBudget)
    );
    assert_eq!(excess.encode(), Err(GeometryError::TransactionBudget));
    assert_eq!(state.apply(&excess), Err(GeometryError::TransactionBudget));
    assert_eq!(state.next_sequence(), 1);
    let mut forged = encoded;
    forged[53..57].copy_from_slice(&257_u32.to_le_bytes());
    // Complete 257th minimal record; failure is the record cap, not truncation.
    for axis in [256_i32, 0, 0] {
        forged.extend_from_slice(&axis.to_le_bytes());
    }
    forged.extend_from_slice(&[0, 0, 0, 0, Material::Wood as u8, 255]);
    assert_eq!(
        GeometryTransaction::decode(&forged),
        Err(GeometryError::TransactionBudget)
    );
    state.apply(&accepted).unwrap();
    assert_eq!(
        state.world.geometry_stats().occupied_cells,
        MAX_GEOMETRY_CHANGES
    );
}

#[test]
fn all_air_refined_state_cannot_be_constructed_through_the_canonical_volume_boundary() {
    let mut stream = b"DFVL\x02".to_vec();
    stream.extend_from_slice(&2_u32.to_le_bytes());
    for end in [128_u16, 256] {
        for axis in [end, 256, 256] {
            stream.extend_from_slice(&axis.to_le_bytes());
        }
        stream.extend_from_slice(&[Material::Air as u8, 0]);
    }
    assert!(RefinedVolume::decode(&stream).is_err()); // Reducible adjacent air runs.
    stream[24] = 1;
    assert!(RefinedVolume::decode(&stream).is_err()); // Original noncanonical air integrity.
    let (air, _) = bore()
        .volume()
        .unwrap()
        .replace_box(LocalBox::FULL, Voxel::AIR, VolumeLimits::default())
        .unwrap();
    assert_eq!(air.leaves().len(), 1);
    assert_eq!(GeometryCell::refined(air), GeometryCell::AIR);
}

#[test]
fn component_codecs_refuse_truncation_trailing_bytes_bad_headers_and_noncanonical_cells() {
    let mut state = empty();
    let packet = commit(&mut state, vec![(IVec3::new(0, 0, 0), bore())]);
    let checkpoint = state.encode_checkpoint().unwrap();
    let bytes = packet.encode().unwrap();
    for end in 0..checkpoint.len() {
        assert!(GeometryState::decode_checkpoint(&checkpoint[..end]).is_err());
    }
    for end in 0..bytes.len() {
        assert!(GeometryTransaction::decode(&bytes[..end]).is_err());
    }
    let mut trailing = checkpoint.clone();
    trailing.push(0);
    assert!(GeometryState::decode_checkpoint(&trailing).is_err());
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(GeometryTransaction::decode(&trailing).is_err());
    for index in [0, 4, 21, 37, 53] {
        let mut altered = checkpoint.clone();
        altered[index] = 255;
        assert!(GeometryState::decode_checkpoint(&altered).is_err());
    }
    let mut bad_air = bytes;
    bad_air[71] = 99; // Before cell's original AIR integrity.
    assert!(GeometryTransaction::decode(&bad_air).is_err());
    let uniform = RefinedVolume::uniform(Voxel::new(Material::Wood))
        .encode()
        .unwrap();
    let mut bad_uniform = checkpoint[..53].to_vec(); // Keep header + position, replace refined payload.
    bad_uniform.push(1);
    bad_uniform.extend_from_slice(&u32::try_from(uniform.len()).unwrap().to_le_bytes());
    bad_uniform.extend_from_slice(&uniform);
    assert!(GeometryState::decode_checkpoint(&bad_uniform).is_err());
    assert!(
        GeometryState::decode_checkpoint(&vec![0; wire::MAX_GEOMETRY_CHECKPOINT_BYTES + 1])
            .is_err()
    );
    assert!(
        GeometryTransaction::decode(&vec![0; wire::MAX_GEOMETRY_TRANSACTION_BYTES + 1]).is_err()
    );
}

#[test]
fn malformed_decode_candidates_cannot_publish_and_accepted_mutations_reencode_identically() {
    let mut state = empty();
    let packet = commit(&mut state, vec![(IVec3::new(-16, 5, 3), bore())]);
    let checkpoint = state.encode_checkpoint().unwrap();
    let bytes = packet.encode().unwrap();
    let mut seed = 0xd457_018c_u64;
    for _ in 0..2000 {
        seed = splitmix64(seed);
        let mut altered = checkpoint.clone();
        let index = usize::try_from(seed % u64::try_from(altered.len()).unwrap()).unwrap();
        altered[index] ^= u8::try_from(seed >> 56).unwrap();
        if let Ok(decoded) = GeometryState::decode_checkpoint(&altered) {
            assert_eq!(decoded.encode_checkpoint().unwrap(), altered);
            assert_eq!(
                decoded.world.fingerprint(),
                decoded.world.recompute_fingerprint()
            );
        }
        let mut altered = bytes.clone();
        let index = usize::try_from(seed % u64::try_from(altered.len()).unwrap()).unwrap();
        altered[index] ^= u8::try_from(seed >> 56).unwrap();
        if let Ok(decoded) = GeometryTransaction::decode(&altered) {
            assert_eq!(decoded.encode().unwrap(), altered);
            let mut replica = empty();
            if replica.apply(&decoded).is_err() {
                assert_eq!(
                    replica.encode_checkpoint().unwrap(),
                    empty().encode_checkpoint().unwrap()
                );
            }
        }
    }
    assert_eq!(state.encode_checkpoint().unwrap(), checkpoint);
}
