use super::*;
use crate::{AuthenticatedPrincipal, Material, PlayerInputCommand, Voxel};
use std::num::NonZeroU64;

fn floor() -> World {
    let mut world = World::default();
    world.fill_box(
        crate::IVec3::new(-8, 0, 30),
        crate::IVec3::new(8, 0, 50),
        Voxel::new(Material::Stone),
    );
    world
}

fn admitted(count: u8) -> AuthorityCore<u8> {
    let mut core = AuthorityCore::new(floor(), 1100).unwrap();
    for source in 1..=count {
        assert!(core.admit_authenticated(
            source,
            u64::from(source),
            u64::from(source),
            AuthenticatedPrincipal::new(NonZeroU64::new(u64::from(source)).unwrap()),
            &mut NetworkTickReport::default()
        ));
        core.players
            .get_mut(&u64::from(source))
            .unwrap()
            .accept_input(PlayerInputCommand {
                input_sequence: 7,
                movement_z_per_mille: -1000,
                ..PlayerInputCommand::default()
            })
            .unwrap();
    }
    core
}

#[test]
fn buried_character_recovers_only_to_a_free_validated_slot_without_reusing_inputs() {
    let mut core = admitted(2);
    let previous_slot = core.peers[&1].spawn_slot;
    let other = core.players[&2];
    let mut altered = floor();
    altered.set_voxel(crate::IVec3::new(0, 1, 40), Voxel::new(Material::Stone));
    // Inject a future enclosing-world replacement to exercise recovery, not a legal build command.
    core.authority = AuthoritativeServer::new(altered);
    let mut report = NetworkTickReport::default();
    core.simulate_players(&mut report);
    assert_eq!(report.player_geometry_failures, 1);
    assert_eq!(report.players_simulated, 1);
    assert_eq!(report.player_recoveries, 1);
    assert_eq!(report.player_recovery_deferred, 0);
    assert_ne!(core.peers[&1].spawn_slot, previous_slot);
    assert_ne!(core.peers[&1].spawn_slot, core.peers[&2].spawn_slot);
    let recovered = core.players.get_mut(&1).unwrap();
    assert!(
        recovered
            .is_clear_of_static_world(core.authority.world())
            .unwrap()
    );
    assert_eq!(recovered.state().last_input_sequence, 7);
    assert!(
        recovered
            .accept_input(PlayerInputCommand {
                input_sequence: 7,
                ..PlayerInputCommand::default()
            })
            .is_err()
    );
    recovered
        .accept_input(PlayerInputCommand {
            input_sequence: 8,
            ..PlayerInputCommand::default()
        })
        .unwrap();
    assert_ne!(
        core.players[&2].state().position_um,
        other.state().position_um
    );
}

#[test]
fn unavailable_recovery_is_reported_without_stalling_the_other_fifteen_players() {
    let mut core = admitted(16);
    let before = core.players[&1];
    let mut blocked = floor();
    blocked.set_voxel(crate::IVec3::new(0, 1, 40), Voxel::new(Material::Stone));
    core.authority = AuthoritativeServer::new(blocked);
    let mut report = NetworkTickReport::default();
    core.simulate_players(&mut report);
    assert_eq!(report.players_simulated, 15);
    assert_eq!(report.players_moved, 15);
    assert_eq!(report.player_geometry_failures, 1);
    assert_eq!(report.player_recoveries, 0);
    assert_eq!(report.player_recovery_deferred, 1);
    assert_eq!(core.players[&1], before);
    // A later valid world change removes the obstruction; no reconnect or input reset is needed.
    core.authority = AuthoritativeServer::new(floor());
    let mut report = NetworkTickReport::default();
    core.simulate_players(&mut report);
    assert_eq!(report.players_simulated, 16);
    assert_eq!(report.player_geometry_failures, 0);
    assert_ne!(
        core.players[&1].state().position_um,
        before.state().position_um
    );
}

#[test]
fn simultaneous_recoveries_reserve_distinct_slots_and_deferred_recovery_can_resume() {
    let mut core = admitted(2);
    let mut blocked = floor();
    // Enclose all declared spawn slots, so both recoveries must first defer.
    for slot in 0..u8::try_from(MAX_SERVER_PEERS).unwrap() {
        let position = player_for_spawn_slot(slot).state().position_um;
        blocked.set_voxel(
            crate::IVec3::new(
                i32::try_from(position.x.div_euclid(crate::MICROMETERS_PER_VOXEL)).unwrap(),
                1,
                i32::try_from(position.z.div_euclid(crate::MICROMETERS_PER_VOXEL)).unwrap(),
            ),
            Voxel::new(Material::Stone),
        );
    }
    core.authority = AuthoritativeServer::new(blocked);
    let mut report = NetworkTickReport::default();
    core.simulate_players(&mut report);
    assert_eq!(report.player_recovery_deferred, 2);
    assert_eq!(report.player_recoveries, 0);

    let mut reopened = floor();
    // Keep both players' current positions buried, but reopen the other declared slots.
    for player in core.players.values() {
        let position = player.state().position_um;
        reopened.set_voxel(
            crate::IVec3::new(
                i32::try_from(position.x.div_euclid(crate::MICROMETERS_PER_VOXEL)).unwrap(),
                1,
                i32::try_from(position.z.div_euclid(crate::MICROMETERS_PER_VOXEL)).unwrap(),
            ),
            Voxel::new(Material::Stone),
        );
    }
    core.authority = AuthoritativeServer::new(reopened);
    let mut report = NetworkTickReport::default();
    core.simulate_players(&mut report);
    assert_eq!(report.player_recoveries, 2);
    assert_eq!(report.player_recovery_deferred, 0);
    assert_ne!(core.peers[&1].spawn_slot, core.peers[&2].spawn_slot);
    for player in core.players.values() {
        assert!(
            player
                .is_clear_of_static_world(core.authority.world())
                .unwrap()
        );
        assert_eq!(player.state().last_input_sequence, 7);
    }
}
