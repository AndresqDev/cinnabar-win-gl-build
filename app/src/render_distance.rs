//! Live view-radius requests. The requested radius sets the render distance whatever the server
//! grants; the server's response only widens what residency keeps.
//!
//! Startup grows the requested radius one ring at a time through [`progressive`]. Bedrock has no
//! sector request, so camera direction is expressed only through the stream's work ranking
//! (`WorldStream::set_view_forward`), never through a fabricated packet.

pub(crate) mod progressive;

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

/// Suppresses the repeated `PacketSendError::Closed` warning a closing transport produces every
/// frame. Recovery clears it, so a later closure in the same session is still reported.
#[derive(Default)]
pub(crate) struct ClosedSendLog {
    session: Option<u64>,
}

impl ClosedSendLog {
    fn record(&mut self, session: u64) -> bool {
        if self.session == Some(session) {
            return false;
        }
        self.session = Some(session);
        true
    }

    fn reset(&mut self) {
        self.session = None;
    }
}

/// The heavy model-face path multiplies per-chunk mesh, upload and draw work. A desktop
/// render-distance setting that is harmless for the cube path can otherwise request thousands
/// of model-face chunks at once on legacy Intel hardware. Keep the server/UI setting intact but
/// cap only the radius this client actually requests. `0` disables the cap.
fn compatibility_render_radius(configured: i32) -> i32 {
    if !meshing::compatibility_terrain_enabled() {
        return configured;
    }
    static LIMIT: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    let limit = *LIMIT.get_or_init(|| {
        let configured_limit = std::env::var("CINNABAR_HEAVY_MAX_RENDER_DISTANCE")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(4);
        configured_limit.clamp(0, i32::from(render_api::MAX_VIEW_RADIUS_CHUNKS))
    });
    if limit == 0 {
        configured
    } else {
        configured.min(limit)
    }
}

/// Sends the selected radius after joining and whenever it changes, ramping through rings while
/// startup is still settling instead of requesting the whole target at once.
pub(crate) fn apply_render_distance(
    settings: Res<RuntimeSettings>,
    ui: Option<Res<UiRuntimeResource>>,
    world: Option<ResMut<ClientWorld>>,
    network: Option<Res<NetworkHandle<PackApplication>>>,
    time: Res<Time<Real>>,
    mut progressive: Local<ProgressiveLoad>,
    mut sent: Local<Option<(u64, u8)>>,
    mut closed_send_log: Local<ClosedSendLog>,
) {
    let (Some(ui), Some(mut world), Some(network)) = (ui, world, network) else {
        progressive.reset();
        *sent = None;
        closed_send_log.reset();
        return;
    };
    let session = ui.session_id();
    let configured_radius = i32::from(
        settings
            .user_settings_update()
            .1
            .video
            .render_distance_chunks,
    );
    let target_radius = compatibility_render_radius(configured_radius);
    // Menus have no stream; checking the session before `as_mut` keeps `ClientWorld` unchanged.
    if session == 0 {
        progressive.reset();
        *sent = None;
        closed_send_log.reset();
        return;
    }
    let transferring = world.dimension_transfer.active();
    let Some(stream) = world.stream.as_mut() else {
        progressive.reset();
        *sent = None;
        closed_send_log.reset();
        return;
    };
    // DX11 uses the original join order: request the configured radius immediately and let
    // the stream admit the publisher view without the progressive/ANGLE cap. The ANGLE ramp is
    // known to be broken and must not be reused for the heavy path; the heavy path is protected
    // only by the hard radius cap above.
    if render::gl_compat::dx11() {
        stream.set_progressive_view(false);
        stream.set_startup_priority(false);
        let radius = u8::try_from(target_radius).unwrap_or(u8::MAX);
        if stream.requested_radius() != Some(i32::from(radius)) {
            stream.set_requested_radius(i32::from(radius));
        }
        stream.set_strict_requested_radius(meshing::compatibility_terrain_enabled());
        let revision = (session, radius);
        if *sent != Some(revision) {
            let packet = protocol::request_chunk_radius_packet(
                radius,
                render_api::MAX_VIEW_RADIUS_CHUNKS as u8,
            );
            if network.send_settings_packet(session, packet).is_ok() {
                *sent = Some(revision);
            }
        }
        return;
    }
    // The server's publisher can describe a much larger view than this client requested. On the
    // GL compatibility path that would defeat the progressive ramp and recreate the join burst;
    // cap admission and retention to the requested radius while the ramp grows it.
    if render::gl_compat::gl() {
        stream.set_progressive_view(true);
    }

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
            // Startup priority is held until local terrain is presentable; the full render
            // distance only ramps after that entry gate opens. A dimension transfer instead
            // needs the fixed nine-by-nine client-ticking area.
            loading: stream.startup_priority() || transferring,
            loading_radius: if transferring { 4 } else { 0 },
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
                closed_send_log.reset();
            }
            Err(error) => {
                progressive.send_failed();
                if error.is_closed() {
                    if closed_send_log.record(session) {
                        warn!(
                            session,
                            radius,
                            error = %error,
                            "progressive chunk radius send closed"
                        );
                    }
                } else {
                    closed_send_log.reset();
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

#[cfg(test)]
mod tests {
    use super::ClosedSendLog;

    #[test]
    fn closed_sends_are_reported_once_per_session_until_recovery() {
        let mut log = ClosedSendLog::default();
        assert!(log.record(7));
        assert!(!log.record(7));
        assert!(!log.record(7));

        assert!(log.record(8), "a new session reports its own closure");

        log.reset();
        assert!(
            log.record(8),
            "recovery allows a later closure to be reported"
        );
    }
}
