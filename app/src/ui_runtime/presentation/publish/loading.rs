//! Native dimension switching and destination terrain presentation are separate waits.

use crate::runtime::network::resource_packs::PackApplication;
use client_runtime::network::NetworkHandle;
use std::time::Duration;

use client_ui::ui_runtime::presentation::startup::{
    MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION, StartupReadinessInput,
};
use render_model::VisibilityDiagnosticSnapshot;
use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use client_session::PacketSendError;

/// Avoids repeating the fallback diagnostic every frame while a world is loading.
static DX11_LOADING_FALLBACK_LOGGED: AtomicBool = AtomicBool::new(false);

pub(super) struct LoadingObservation {
    pub restart: bool,
    pub menu_visible: bool,
    pub snapshot: VisibilityDiagnosticSnapshot,
    pub visible_rendered: usize,
    pub cohort: Option<chunk_pipeline::CohortProgress>,
    pub render_work_drained: bool,
    pub pipelines_ready: bool,
    pub now: Duration,
}

#[expect(
    clippy::print_stderr,
    reason = "the CINNABAR_LOADING_MILESTONE marker line is emitted on stderr"
)]
pub(super) fn prepare_loading(
    client_world: &mut ClientWorld,
    fatal_error: &mut Option<String>,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    diagnostics: &mut VisibilityDiagnosticsInput,
    network: &NetworkHandle<PackApplication>,
    observation: LoadingObservation,
) {
    if observation.restart {
        presentation.startup_mut().restart(
            diagnostics
                .frame_generation()
                .max(observation.snapshot.frame_generation),
        );
    }
    if observation.menu_visible && !client_world.dimension_transfer.active() {
        presentation.set_loading_stage(None);
        diagnostics.set_startup_probe_enabled(false);
        return;
    }
    let (connected, stream_work_drained) =
        client_world
            .stream
            .as_ref()
            .map_or((false, false), |stream| {
                let stats = stream.stats();
                let drained = stats.queued_decode_jobs == 0
                    && stats.in_flight_decode_jobs == 0
                    && stats.pending_light_jobs == 0
                    && stats.in_flight_light_jobs == 0
                    && stats.pending_mesh_jobs == 0
                    && stats.in_flight_mesh_jobs == 0
                    && stats.pending_retry_requests == 0
                    && stats.awaiting_sub_chunk_responses == 0
                    && stats.admitted_world_events == 0
                    && stats.admitted_heavy_events == 0
                    && stream.pending_request_work_count() == 0
                    && stream.outstanding_sub_chunk_count() == 0
                    && stream.pending_mesh_change_count() == 0
                    && stream.unacknowledged_mesh_count() == 0;
                (true, drained)
            });
    let loading = presentation.startup_mut().probe_enabled(connected);
    let transferring = client_world.dimension_transfer.active();
    let local_terrain_ready = loading
        && client_world.stream.as_ref().is_some_and(|stream| {
            if transferring {
                stream.dimension_transfer_presentable(stream.resolved_server_position().position)
            } else {
                stream.local_terrain_ready()
            }
        });
    // The Intel DX11 path can present real opaque terrain while the dense-view count is
    // still small (the startup radius is one 3x3 neighbourhood). GPU-completed opaque
    // submissions are stronger evidence than the fixed 1024-entity threshold, so let
    // that evidence release the loading overlay. The normal path remains unchanged for
    // larger views and for other backends.
    let dx11_completed_terrain = render::gl_compat::dx11()
        && observation
            .snapshot
            .gpu_completed_opaque
            .is_some_and(|digest| digest.count != 0)
        && observation
            .snapshot
            .submitted_opaque
            .is_some_and(|digest| digest.count != 0);
    let visible_rendered = if dx11_completed_terrain {
        observation
            .visible_rendered
            .max(MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION)
    } else {
        observation.visible_rendered
    };
    if dx11_completed_terrain && !DX11_LOADING_FALLBACK_LOGGED.swap(true, Ordering::AcqRel) {
        bevy::log::warn!(
            gpu_completed = ?observation.snapshot.gpu_completed_opaque,
            submitted = ?observation.snapshot.submitted_opaque,
            visible_rendered = observation.visible_rendered,
            local_terrain_ready,
            stream_work_drained,
            render_work_drained = observation.render_work_drained,
            pipelines_ready = observation.pipelines_ready,
            "DX11 loading fallback: completed opaque terrain will release the world"
        );
    }
    let experience_holds = {
        #[cfg(not(target_arch = "wasm32"))]
        {
            runtime.session.experiences.holds_world_entry()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    };
    // Release movement and loading End after the dimension handshake, presented
    // footing and a fresh GPU frame, without waiting for distant ticking columns.
    let (released, milestone) = presentation.startup_mut().observe_with_milestone(
        StartupReadinessInput {
            session_generation: runtime.session_id(),
            connected,
            diagnostics_frame_generation: diagnostics.frame_generation(),
            snapshot: observation.snapshot,
            visible_rendered: if transferring { 0 } else { visible_rendered },
            local_terrain_ready,
            // Dragonfly's initial spawn streams after initialization, so startup
            // can release an empty drained view. A transfer instead waits for
            // destination footing before releasing local movement/loading End.
            cohort_target_complete: !transferring
                && observation.cohort.map_or_else(
                    || {
                        loading
                            && client_world
                                .stream
                                .as_ref()
                                .is_some_and(chunk_pipeline::WorldStream::startup_view_complete)
                    },
                    |status| status.target_is_complete(),
                ),
            stream_work_drained,
            render_work_drained: observation.render_work_drained,
            world_entry_held: experience_holds
                || (!dx11_completed_terrain && !observation.pipelines_ready)
                || client_world.dimension_transfer.waiting_for_switch()
                || (client_world.dimension_transfer.active() && !local_terrain_ready),
        },
        u64::try_from(observation.now.as_millis()).unwrap_or(u64::MAX),
    );
    if let Some(milestone) = milestone {
        eprintln!("{milestone}");
    }
    // Spawn-first ordering outlives release only until the local columns have terrain.
    if released && let Some(stream) = client_world.stream.as_mut() {
        stream.finish_startup_priority();
    }
    if released && !presentation.startup_mut().completion_queued {
        let transfer_finished = if client_world.dimension_transfer.active() {
            match client_world.dimension_transfer.finish_presentation(network) {
                Ok(queued) => queued,
                Err(error) => {
                    record_transfer_send_error(fatal_error, error);
                    false
                }
            }
        } else {
            true
        };
        // An early transfer still owes the session its one-shot initialization.
        // Keep completion pending if either readiness command needs queue capacity.
        presentation.startup_mut().completion_queued =
            transfer_finished && network.finish_loading();
    }
    if released
        && presentation.startup_mut().completion_queued
        && !client_world.dimension_transfer.active()
    {
        bridge::join_profile::JoinProfile::current().ready_for_presentation();
    }
    diagnostics.set_startup_probe_enabled(presentation.startup_mut().probe_enabled(connected));
    presentation.set_loading_stage(if !connected {
        Some(LoadingStage::Connecting)
    } else if released && !client_world.dimension_transfer.active() {
        None
    } else if client_world.dimension_transfer.active() {
        Some(LoadingStage::ChangingDimension)
    } else {
        Some(LoadingStage::BuildingTerrain)
    });
}

fn record_transfer_send_error(fatal_error: &mut Option<String>, error: PacketSendError) {
    if error.is_closed() {
        record_fatal_error(fatal_error, format!("dimension transfer: {error}"));
    }
}

#[cfg(test)]
mod tests;
