use super::{ActorAnimationStore, ActorLifetimeId};

/// A controller state that completed authoritative animation evaluation for one actor lifetime.
#[derive(Clone, Copy, Debug)]
pub struct ActorParticleController<'a> {
    pub actor: ActorLifetimeId,
    pub entity: &'a str,
    pub controller: &'a str,
    pub state: &'a str,
    pub controller_index: u32,
    pub state_index: u32,
    pub entered_tick: u64,
    pub reset_generation: u64,
}

impl ActorAnimationStore {
    pub fn particle_controllers(&self) -> impl Iterator<Item = ActorParticleController<'_>> {
        self.rigs.iter().flat_map(move |(&actor, rig)| {
            let assets = if rig.pack {
                self.pack.as_ref().map(|pack| pack.assets.as_ref())
            } else {
                self.assets.as_deref()
            };
            rig.controllers.iter().filter_map(move |runtime| {
                if !rig.initialized || rig.reset_pending || !runtime.active {
                    return None;
                }
                let assets = assets?;
                let binding = assets.rig_bindings().get(rig.rig_binding)?;
                let entity = assets.symbols().get(binding.entity_symbol as usize)?;
                let controller = assets.controllers().get(runtime.controller)?;
                let symbol = assets.symbols().get(controller.symbol as usize)?;
                let state = assets
                    .controller_states()
                    .get(controller.first_state as usize + usize::from(runtime.state))?;
                let name = assets.molang_symbols().get(state.name as usize)?;
                Some(ActorParticleController {
                    actor,
                    entity: &entity.identifier,
                    controller: &symbol.identifier,
                    state: &name.identifier,
                    controller_index: runtime.controller as u32,
                    state_index: controller.first_state + u32::from(runtime.state),
                    entered_tick: runtime.entered_tick,
                    reset_generation: rig.reset_generation,
                })
            })
        })
    }
}

/// Retained numeric actor query inputs, with the emitter's frame and controller state time.
pub struct ActorParticleQueries<'a> {
    actor: &'a dyn crate::AnimatedActor,
    input: super::ActorTickInput,
    context: super::ActorTickContext,
    life_tick: u64,
    anim_tick: u64,
    bone_names: &'a [Box<str>],
    alpha: f32,
    state_time: f32,
}

impl<'a> super::ActorRigSnapshot<'a> {
    /// Retains actor query facts once without copying the owner's script variable map.
    /// `entered_tick` is the controller state's entry on the owner's animation clock.
    pub fn particle_queries(
        &self,
        actor: &'a dyn crate::AnimatedActor,
        alpha: f32,
        entered_tick: u64,
    ) -> ActorParticleQueries<'a> {
        let owner = self.animation_variables;
        let state_time = (owner.anim_tick().saturating_sub(entered_tick) as f32 + alpha)
            * simulation_clock::TICK_DURATION.as_secs_f32();
        let mut context = owner.owner_context().cloned().unwrap_or_default();
        context.frame_alpha = alpha;
        ActorParticleQueries {
            actor,
            input: owner.input().unwrap_or_default(),
            context,
            life_tick: owner.life_tick(),
            anim_tick: owner.anim_tick(),
            bone_names: self.bone_names,
            alpha,
            state_time,
        }
    }
}

impl ActorParticleQueries<'_> {
    /// Reads a numeric actor query; unknown facts retain the actor evaluator's idle value.
    pub fn query(&self, name: &str) -> f32 {
        let name = name.strip_prefix("query.").unwrap_or(name);
        if name == "frame_alpha" {
            return self.alpha;
        }
        let Some(binding) = assets::MOLANG_QUERIES
            .binary_search_by(|candidate| {
                candidate
                    .strip_prefix("query.")
                    .unwrap_or(candidate)
                    .cmp(name)
            })
            .ok()
            .and_then(|index| assets::MolangQuery::from_name(assets::MOLANG_QUERIES[index]))
        else {
            return 0.0;
        };
        let inputs = super::query::QueryInputs {
            actor: self.actor,
            input: &self.input,
            context: &self.context,
            anim_tick: self.anim_tick,
            anim_time: None,
            swell_amount: None,
            life_tick: self.life_tick,
            finished: (false, false),
            state_time: self.state_time,
            bones: &[],
            bone_names: self.bone_names,
        };
        super::query::query(&inputs, binding, &[]).number()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_query_names_keep_numeric_values_and_unknown_queries_idle() {
        let actor = crate::tests::actor_with_metadata(Default::default());
        let queries = ActorParticleQueries {
            actor: &actor,
            input: Default::default(),
            context: crate::ActorTickContext {
                frame_alpha: 0.5,
                ..Default::default()
            },
            life_tick: 40,
            anim_tick: 40,
            bone_names: &[],
            alpha: 0.5,
            state_time: 1.25,
        };
        for (name, expected) in [
            ("frame_alpha", queries.alpha),
            ("state_time", queries.state_time),
            (
                "life_time",
                (queries.life_tick as f32 + queries.alpha)
                    * simulation_clock::TICK_DURATION.as_secs_f32(),
            ),
            ("not_admitted", 0.0),
        ] {
            assert_eq!(queries.query(name), expected, "{name}");
            assert_eq!(queries.query(&format!("query.{name}")), expected, "{name}");
        }
    }

    #[test]
    fn particle_anim_time_uses_elapsed_ticks_after_spawn_and_reset() {
        use crate::{ActorTickContext, test_support::counting_random_assets};
        use std::collections::HashMap;

        let actor = crate::tests::actor_with_metadata(Default::default());
        let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
        let mut store = ActorAnimationStore::with_assets(counting_random_assets());
        store.completed_tick = 100;
        store.insert(1, 0, &actor);
        let rig = store.get(actor.runtime_id).unwrap();
        assert_eq!(rig.completed_tick, 100);
        assert_eq!(rig.particle_queries(&actor, 0.5, 0).query("anim_time"), 0.0);
        let advance = |store: &mut ActorAnimationStore| {
            store.advance_tick(&actors, None, None, true, false, |_| {
                ActorTickContext::default()
            });
        };
        advance(&mut store);
        let rig = store.get(actor.runtime_id).unwrap();
        assert_eq!(rig.completed_tick, 101);
        assert_eq!(
            rig.particle_queries(&actor, 0.5, 0).query("anim_time"),
            simulation_clock::TICK_DURATION.as_secs_f32()
        );
        // State time follows the actor's animation clock, not the store's global tick.
        assert_eq!(
            rig.particle_queries(&actor, 0.5, 0).query("state_time"),
            1.5 * simulation_clock::TICK_DURATION.as_secs_f32()
        );
        advance(&mut store);
        let rig = store.get(actor.runtime_id).unwrap();
        assert_eq!(
            rig.particle_queries(&actor, 0.5, 0).query("anim_time"),
            2.0 * simulation_clock::TICK_DURATION.as_secs_f32()
        );
        store.mark_reset(actor.runtime_id);
        advance(&mut store);
        let rig = store.get(actor.runtime_id).unwrap();
        assert_eq!(rig.completed_tick, 103);
        let queries = rig.particle_queries(&actor, 0.5, 0);
        assert_eq!(queries.query("query.anim_time"), 0.0);
        assert_eq!(
            queries.query("life_time"),
            3.5 * simulation_clock::TICK_DURATION.as_secs_f32()
        );
        advance(&mut store);
        let rig = store.get(actor.runtime_id).unwrap();
        assert_eq!(
            rig.particle_queries(&actor, 0.5, 0).query("anim_time"),
            simulation_clock::TICK_DURATION.as_secs_f32()
        );
    }
}
