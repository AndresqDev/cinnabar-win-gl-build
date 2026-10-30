use super::*;
use crate::{
    SpawnRequest,
    world::{EmptyWorld, Fluid},
};
use std::cell::{Cell, RefCell};

#[derive(Default)]
struct CountingWorld {
    cells: RefCell<Vec<[i32; 3]>>,
    channels: Cell<(u8, u8)>,
}

impl ParticleWorld for CountingWorld {
    fn solid_boxes(&self, _: [f32; 3], _: [f32; 3], _: &mut Vec<[f32; 6]>) {}

    fn light(&self, cell: [i32; 3]) -> (u8, u8) {
        self.cells.borrow_mut().push(cell);
        self.channels.get()
    }

    fn fluid(&self, _: [i32; 3]) -> Fluid {
        Fluid::None
    }
}

/// Spawns two emitters so a shared cell must also reuse light across emitters.
fn system(lit: bool) -> ParticleSystem {
    let effect = super::tests::EFFECT.replace("\"num_particles\":1", "\"num_particles\":128");
    let effect = if lit {
        effect.replace(
            "\"minecraft:particle_appearance_tinting\"",
            "\"minecraft:particle_appearance_lighting\":{},\"minecraft:particle_appearance_tinting\"",
        )
    } else {
        effect
    };
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(effect.as_bytes()));
    for _ in 0..2 {
        system.spawn(&SpawnRequest {
            effect: "minecraft:quad".into(),
            position: [-0.25, 0.5, -5.25],
            ..SpawnRequest::default()
        });
    }
    system.tick(0.02, &EmptyWorld);
    system
}

#[test]
fn one_cell_queries_light_once_and_matches_uncached_instances_bit_for_bit() {
    let view = super::tests::view();
    for channels in [(0, 0), (0, 15), (10, 0), (7, 12), (255, 255)] {
        let world = CountingWorld {
            channels: Cell::new(channels),
            ..Default::default()
        };
        let mut baseline = system(true);
        let mut expected = Vec::new();
        let mut hidden = 0;
        for emitter in baseline.emitters_mut() {
            emit_emitter(
                emitter,
                &view,
                (view.half_diagonal + 0.15).min(3.0).cos(),
                &mut expected,
                &mut |_, _| false,
                &mut hidden,
            );
        }
        for (_, instance, _) in &mut expected {
            let [x, y, z, _] = instance.center_light;
            instance.center_light[3] = light_sample(&world, [x, y, z].map(|c| c.floor() as i32));
        }
        expected.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
        let expected: Vec<_> = expected
            .into_iter()
            .map(|(_, instance, _)| instance)
            .collect();
        assert_eq!(world.cells.borrow().len(), 256);
        world.cells.borrow_mut().clear();
        let lists = system(true).build_draw(&view, &world);
        assert_eq!(lists.opaque.len(), 256);
        assert_eq!((lists.light_lookups, lists.light_cache_hits), (1, 255));
        assert_eq!(*world.cells.borrow(), [[-1, 0, -6]]);
        assert_eq!(
            bytemuck::cast_slice::<_, u8>(&lists.opaque),
            bytemuck::cast_slice::<_, u8>(&expected)
        );
    }
}

#[test]
fn cells_are_distinct_and_light_is_refreshed_each_frame() {
    let view = super::tests::view();
    let world = CountingWorld::default();
    let mut system = system(true);
    system.emitters_mut()[0].particles[0].pos[0] = 0.25;
    let first = system.build_draw(&view, &world);
    assert_eq!((first.light_lookups, first.light_cache_hits), (2, 254));
    assert!(world.cells.borrow().contains(&[0, 0, -6]));
    assert!(world.cells.borrow().contains(&[-1, 0, -6]));
    world.cells.borrow_mut().clear();
    world.channels.set((9, 14));
    let second = system.build_draw(&view, &world);
    assert_eq!((second.light_lookups, second.light_cache_hits), (2, 254));
    assert_eq!(world.cells.borrow().len(), 2);
    for (a, b) in first.opaque.iter().zip(&second.opaque) {
        let mut expected = *a;
        expected.center_light[3] = f32::from(9u8 | (14u8 << 4));
        assert_eq!(*b, expected);
    }
}

#[test]
fn unlit_and_culled_particles_do_not_query_light() {
    let view = super::tests::view();
    let world = CountingWorld::default();
    let unlit = system(false).build_draw(&view, &world);
    assert_eq!((unlit.light_lookups, unlit.light_cache_hits), (0, 0));
    let hidden = system(true).build_draw_with_visibility(&view, &world, |_, _| true);
    assert_eq!(hidden.cave_hidden, 256);
    assert_eq!((hidden.light_lookups, hidden.light_cache_hits), (0, 0));
    assert!(world.cells.borrow().is_empty());
}
