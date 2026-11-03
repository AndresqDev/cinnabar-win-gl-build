//! Defers engine-owned world pipeline initializers while preserving their systems and resources.

use bevy::core_pipeline::deferred::copy_lighting_id::{
    CopyDeferredLightingIdPipeline, init_copy_deferred_lighting_id_pipeline,
};
use bevy::core_pipeline::mip_generation::experimental::depth::create_downsample_depth_pipelines;
use bevy::{
    ecs::schedule::{
        IntoSystemSet, ScheduleCleanupPolicy, ScheduleError, ScheduleLabel, common_conditions::not,
    },
    prelude::*,
    render::{
        Render, RenderStartup, RenderSystems,
        render_resource::{
            DownlevelFlags, SparseBufferUpdateBindGroups, init_sparse_buffer_update_bind_groups,
            update_sparse_buffers,
        },
        renderer::{RenderAdapter, RenderGraph, RenderGraphSystems},
    },
};
use render_scene::startup_work::{WorldWorkAdmission, world_work_allowed};

/// Moves world-only initializers after the menu; direct joins retain the normal engine schedule.
pub(super) fn install(app: &mut SubApp) {
    let admitted = app
        .world()
        .get_resource::<WorldWorkAdmission>()
        .is_none_or(WorldWorkAdmission::is_admitted);
    // Keep the stock startup schedule for backends that can create compute pipelines. A
    // direct-admitted DX11 launch would otherwise bypass the compute guard below and create
    // Bevy's sparse-buffer compute pipeline on a backend without COMPUTE_SHADERS.
    let adapter_compute_supported = app
        .world()
        .get_resource::<RenderAdapter>()
        .is_some_and(adapter_supports_compute);
    if admitted && adapter_compute_supported {
        return;
    }
    if !admitted {
        if take(app, RenderStartup, init_copy_deferred_lighting_id_pipeline) {
            app.add_systems(
                Render,
                init_copy_deferred_lighting_id_pipeline
                    .before(RenderSystems::PrepareViews)
                    .run_if(not(resource_exists::<CopyDeferredLightingIdPipeline>))
                    .run_if(world_work_allowed),
            );
        }
        if take(app, Render, create_downsample_depth_pipelines) {
            app.add_systems(
                Render,
                create_downsample_depth_pipelines
                    .in_set(RenderSystems::Prepare)
                    .run_if(world_work_allowed),
            );
        }
    }
    if take(app, RenderStartup, init_sparse_buffer_update_bind_groups) {
        app.add_systems(
            Render,
            init_sparse_buffer_update_bind_groups
                .before(RenderSystems::PrepareViews)
                .run_if(not(resource_exists::<SparseBufferUpdateBindGroups>))
                .run_if(world_work_allowed)
                .run_if(compute_supported),
        );
    }
    if take(app, RenderGraph, update_sparse_buffers) {
        app.add_systems(
            RenderGraph,
            update_sparse_buffers
                .in_set(RenderGraphSystems::Begin)
                .run_if(resource_exists::<SparseBufferUpdateBindGroups>)
                .run_if(compute_supported),
        );
    }
}

fn adapter_supports_compute(adapter: &RenderAdapter) -> bool {
    adapter
        .get_downlevel_capabilities()
        .flags
        .contains(DownlevelFlags::COMPUTE_SHADERS)
}

fn compute_supported(adapter: Option<Res<RenderAdapter>>) -> bool {
    adapter.is_some_and(|adapter| adapter_supports_compute(&adapter))
}

/// Removes a typed initializer once, without relying on its diagnostic name or schedule layout.
fn take<M>(app: &mut SubApp, label: impl ScheduleLabel, set: impl IntoSystemSet<M>) -> bool {
    match app
        .world_mut()
        .try_schedule_scope(label, |world, schedule| {
            schedule.remove_systems_in_set(set, world, ScheduleCleanupPolicy::RemoveSystemsOnly)
        }) {
        Ok(Ok(count)) => count != 0,
        Ok(Err(ScheduleError::SetNotFound)) | Err(_) => false,
        Ok(Err(error)) => panic!("could not defer world startup pipeline: {error}"),
    }
}
