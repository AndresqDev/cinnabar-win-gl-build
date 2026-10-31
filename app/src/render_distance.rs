//! Live view-radius requests. The requested radius sets the render distance whatever the server
//! grants; the server's response only widens what residency keeps.
//!
//! Startup grows the requested radius one ring at a time through [`progressive`]. Bedrock has no
//! sector request, so camera direction is expressed only through the stream's work ranking
//! (`WorldStream::set_view_forward`), never through a fabricated packet.

mod progressive;

use crate::runtime::network::resource_packs::PackApplication;
use crate::runtime::world::ClientWorld;
use bevy::{
    log::{debug, info, warn},
    prelude::{Local, Res, ResMut, Time},
    time::Real,
};
use client_runtime::network::NetworkHandle;
use client_shell::ui_resources::UiRuntimeResource;
use client_state::RuntimeSettings;
use progressive::{ProgressInput, ProgressiveLoad, WorkCounts};

/// Sends the selected radius after joining and whenever it changes, ramping through rings while
/// startup is still settling instead of requesting the whole target at once.
pub(crate) fn apply_render_distance(
    settings: Res<RuntimeSettings>,
    ui: Option<Res<UiRuntimeResource>>,
    world: Option<ResMut<ClientWorld>>,
    network: Option<Res<NetworkHandle<PackApplication>>>,
    time: Res<Time<Real>>,
    mut progressive: Local<ProgressiveLoad>,
) {
    let (Some(ui), Some(mut world), Some(network)) = (ui, world, network) else {
        progressive.reset();
        return;
    };
    let session = ui.session_id();
    let target_radius = i32::from(
        settings
            .user_settings_update()
            .1
            .video
            .render_distance_chunks,
    );
    let Some(stream) = world.stream.as_mut().filter(|_| session != 0) else {
        progressive.reset();
        return;
    };

    let stats = stream.stats();
    let work = WorkCounts {
        decode: stats
            .queued_decode_jobs
            .saturating_add(stats.in_flight_decode_jobs),
        pending_mesh: stats.pending_mesh_jobs,
        in_flight_mesh: stats.in_flight_mesh_jobs,
        pending_light: stats.pending_light_jobs,
        in_flight_light: stats.in_flight_light_jobs,
    };
    let now = time.elapsed();
    let desired = progressive.update(
        ProgressInput {
            session,
            target_radius,
            now,
            frame_delta: time.delta(),
            work,
        },
        |radius| stream.local_terrain_ready_within(radius),
    );
    // Only radius zero changes startup readiness; the setter clamps to the safe 3x3 radius.
    stream.set_startup_radius(progressive.current_radius());

    if let Some(radius) = desired {
        let packet = protocol::request_chunk_radius_packet(
            u8::try_from(radius).unwrap_or(u8::MAX),
            render_api::MAX_VIEW_RADIUS_CHUNKS as u8,
        );
        match network.send_settings_packet(session, packet) {
            Ok(()) => {
                progressive.confirm_sent(radius);
                stream.set_requested_radius(radius);
            }
            Err(error) => {
                progressive.send_failed();
                if error.is_closed() {
                    warn!(
                        session,
                        radius,
                        error = %error,
                        "progressive chunk radius send closed"
                    );
                } else {
                    debug!(
                        session,
                        radius,
                        error = %error,
                        "progressive chunk radius send deferred"
                    );
                }
            }
        }
    }

    if let Some(telemetry) = progressive.telemetry_due(now) {
        let milestone = telemetry.milestone;
        if telemetry.changed {
            info!(
                session = milestone.session,
                current_radius = milestone.current_radius,
                target_radius = milestone.target_radius,
                phase = %milestone.phase,
                wait_reason = ?milestone.wait_reason,
                sent_radius = ?milestone.sent_radius,
                pending_decode = telemetry.work.decode,
                pending_mesh = telemetry
                    .work
                    .pending_mesh
                    .saturating_add(telemetry.work.in_flight_mesh),
                pending_light = telemetry
                    .work
                    .pending_light
                    .saturating_add(telemetry.work.in_flight_light),
                "progressive chunk radius"
            );
        } else {
            debug!(
                session = milestone.session,
                current_radius = milestone.current_radius,
                target_radius = milestone.target_radius,
                phase = %milestone.phase,
                wait_reason = ?milestone.wait_reason,
                sent_radius = ?milestone.sent_radius,
                pending_decode = telemetry.work.decode,
                pending_mesh = telemetry
                    .work
                    .pending_mesh
                    .saturating_add(telemetry.work.in_flight_mesh),
                pending_light = telemetry
                    .work
                    .pending_light
                    .saturating_add(telemetry.work.in_flight_light),
                "progressive chunk radius wait"
            );
        }
    }
}
