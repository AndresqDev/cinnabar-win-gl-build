use super::*;

fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
}

fn load_square(stream: &mut WorldStream, radius: i32) {
    let center = stream.player_column();
    for x in -radius..=radius {
        for z in -radius..=radius {
            stream.loaded_columns.insert(ChunkKey::new(
                center.dimension,
                center.x.saturating_add(x),
                center.z.saturating_add(z),
            ));
        }
    }
}

#[test]
fn default_startup_readiness_still_requires_the_three_by_three_neighbourhood() {
    let mut stream = stream();
    stream.set_startup_priority(true);
    stream.loaded_columns.insert(ChunkKey::new(0, 0, 0));
    assert!(!stream.local_terrain_ready());

    load_square(&mut stream, 1);
    assert!(stream.local_terrain_ready());
}

#[test]
fn one_column_startup_accepts_the_player_column_then_returns_to_three_by_three() {
    let mut stream = stream();
    stream.set_startup_priority(true);
    stream.set_startup_radius(0);
    stream.loaded_columns.insert(ChunkKey::new(0, 0, 0));

    assert!(stream.local_terrain_ready());
    assert!(stream.local_terrain_ready_within(0));
    assert!(!stream.local_terrain_ready_within(1));

    assert!(stream.finish_startup_priority());
    assert!(
        !stream.local_terrain_ready(),
        "after startup the ordinary 3x3 gate returns"
    );
    assert!(!stream.local_terrain_ready_within(1));

    load_square(&mut stream, 1);
    assert!(stream.local_terrain_ready());
}

#[test]
fn startup_radius_never_exceeds_the_safe_neighbourhood() {
    let mut stream = stream();
    stream.set_startup_priority(true);
    stream.set_startup_radius(9);
    assert_eq!(stream.startup_radius, cohort::STARTUP_RADIUS);
    stream.set_startup_radius(-4);
    assert_eq!(stream.startup_radius, 0);
}
