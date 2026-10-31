//! Progressive view-radius ramp for startup and movement.
//!
//! Bedrock only has a square [`protocol::request_chunk_radius_packet`] request, so this state
//! machine cannot ask for a sector. Directional preference stays in [`chunk_pipeline::WorldStream`]:
//! the app publishes the camera forward vector and the mesh/light schedulers rank work in that
//! direction. The ramp below only decides when the square may grow.
//!
//! The first request is radius zero, which is literally the player's own column. If that column
//! does not become ready within [`ZERO_RADIUS_PATIENCE`], the ramp falls back to radius one (the
//! safe 3x3 spawn neighbourhood) instead of leaving startup stuck on a server that will not serve
//! a one-column view.

use std::{fmt, time::Duration};

/// A server that cannot serve the player's own column must not hold startup forever.
const ZERO_RADIUS_PATIENCE: Duration = Duration::from_secs(3);
/// The first ring waits longest; each later ring adds less wait, so the outer view fills faster.
const BASE_RING_DWELL: Duration = Duration::from_millis(500);
const RING_DWELL_STEP: Duration = Duration::from_millis(100);
const MIN_RING_DWELL: Duration = Duration::from_millis(100);
/// A frame beyond this is too slow to grow the view.
const ADVANCE_MAX_FRAME: Duration = Duration::from_millis(40);
/// Consecutive frames beyond this trip the backoff.
const SLOW_FRAME: Duration = Duration::from_millis(50);
const SLOW_FRAMES_FOR_BACKOFF: u8 = 3;
const STRESSED_FRAMES_FOR_BACKOFF: u8 = 3;
const STABLE_FRAMES_TO_RESUME: u8 = 5;
const BACKOFF_BASE: Duration = Duration::from_millis(750);
const BACKOFF_MAX: Duration = Duration::from_secs(8);
const TELEMETRY_INTERVAL: Duration = Duration::from_secs(1);

/// Growing the view while these queues are this deep only moves work around.
const ADVANCE_MAX_MESH_BACKLOG: usize = 24;
const ADVANCE_MAX_LIGHT_BACKLOG: usize = 64;
const ADVANCE_MAX_DECODE_BACKLOG: usize = 16;
/// Well above the advance thresholds: backoff, not ordinary waiting, starts here.
const BACKOFF_MESH_BACKLOG: usize = 64;
const BACKOFF_LIGHT_BACKLOG: usize = 128;
const BACKOFF_DECODE_BACKLOG: usize = 32;
/// Resume only after the queues fall below the ordinary advance limits.
const RESUME_MAX_MESH_BACKLOG: usize = 12;
const RESUME_MAX_LIGHT_BACKLOG: usize = 32;
const RESUME_MAX_DECODE_BACKLOG: usize = 8;

/// Current bounded queues, copied from [`chunk_pipeline::WorldStreamStats`] once per frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct WorkCounts {
    pub(super) decode: usize,
    pub(super) pending_mesh: usize,
    pub(super) in_flight_mesh: usize,
    pub(super) pending_light: usize,
    pub(super) in_flight_light: usize,
}

impl WorkCounts {
    fn mesh(self) -> usize {
        self.pending_mesh.saturating_add(self.in_flight_mesh)
    }

    fn light(self) -> usize {
        self.pending_light.saturating_add(self.in_flight_light)
    }
}

/// One frame's inputs. `ring_ready` is supplied to [`ProgressiveLoad::update`] because it needs
/// the radius the state machine chooses after applying a session or target change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProgressInput {
    pub(super) session: u64,
    pub(super) target_radius: i32,
    pub(super) now: Duration,
    pub(super) frame_delta: Duration,
    pub(super) work: WorkCounts,
}

/// The visible state of the ramp, used for logs and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Phase {
    #[default]
    Initial,
    Ramp,
    Waiting,
    Backoff,
    AtTarget,
}

impl fmt::Display for Phase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Initial => "initial",
            Self::Ramp => "ramp",
            Self::Waiting => "waiting",
            Self::Backoff => "backoff",
            Self::AtTarget => "at_target",
        })
    }
}

/// Why the ramp did not grow this frame. `None` only while a request is being sent or at target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WaitReason {
    RingIncomplete,
    RingDwell,
    MeshBacklog,
    LightBacklog,
    DecodeBacklog,
    SlowFrames,
    SendBackpressure,
    Backlog,
}

impl fmt::Display for WaitReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RingIncomplete => "ring_incomplete",
            Self::RingDwell => "ring_dwell",
            Self::MeshBacklog => "mesh_backlog",
            Self::LightBacklog => "light_backlog",
            Self::DecodeBacklog => "decode_backlog",
            Self::SlowFrames => "slow_frames",
            Self::SendBackpressure => "send_backpressure",
            Self::Backlog => "backlog",
        })
    }
}

/// Identity of a log-worthy state change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProgressMilestone {
    pub(super) session: u64,
    pub(super) current_radius: i32,
    pub(super) target_radius: i32,
    pub(super) phase: Phase,
    pub(super) wait_reason: Option<WaitReason>,
    pub(super) sent_radius: Option<i32>,
}

/// One telemetry sample. `changed` distinguishes a transition from the periodic wait log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProgressTelemetry {
    pub(super) milestone: ProgressMilestone,
    pub(super) work: WorkCounts,
    pub(super) changed: bool,
}

/// Session-local progressive radius state. It never sends packets itself: the caller confirms a
/// successful send so a failed send is retried without duplicating a request the server accepted.
#[derive(Debug, Default)]
pub(super) struct ProgressiveLoad {
    session: Option<u64>,
    target_radius: i32,
    current_radius: i32,
    phase: Phase,
    wait_reason: Option<WaitReason>,
    sent_radius: Option<i32>,
    work: WorkCounts,
    zero_radius_started_at: Option<Duration>,
    last_advance_at: Duration,
    backoff_until: Option<Duration>,
    backoff_rounds: u8,
    slow_frames: u8,
    stable_frames: u8,
    stressed_frames: u8,
    last_logged: Option<ProgressMilestone>,
    last_log_at: Duration,
}

impl ProgressiveLoad {
    /// Drops all state, such as when no session or stream exists.
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Applies this frame and returns a radius when the caller must send it. The caller calls
    /// [`Self::confirm_sent`] only after the transport accepts the packet; otherwise
    /// [`Self::send_failed`] makes the same radius retry on the next frame.
    pub(super) fn update(
        &mut self,
        input: ProgressInput,
        mut ring_ready: impl FnMut(i32) -> bool,
    ) -> Option<i32> {
        self.work = input.work;
        self.observe_frame(input.frame_delta);
        self.observe_backlog();

        let target = input
            .target_radius
            .clamp(0, render_api::MAX_VIEW_RADIUS_CHUNKS);
        if self.session != Some(input.session) {
            self.begin(input.session, target, input.now);
        } else if self.target_radius != target {
            self.retarget(target, input.now);
        }
        if self.current_radius > self.target_radius {
            self.current_radius = self.target_radius;
        }

        // Radius zero is literal: the player's own column. If it never arrives, escalate to the
        // safe 3x3 neighbourhood rather than holding the join on an unsupported request.
        if self.current_radius == 0 && self.target_radius > 0 {
            if ring_ready(0) {
                self.zero_radius_started_at = None;
            } else {
                let started = *self.zero_radius_started_at.get_or_insert(input.now);
                if input.now.saturating_sub(started) >= ZERO_RADIUS_PATIENCE {
                    self.current_radius = 1;
                    self.zero_radius_started_at = None;
                    self.last_advance_at = input.now;
                    self.phase = Phase::Ramp;
                    self.wait_reason = None;
                    self.backoff_until = None;
                    self.backoff_rounds = 0;
                }
            }
        }

        if self.current_radius >= self.target_radius {
            self.current_radius = self.target_radius;
            self.backoff_until = None;
            self.zero_radius_started_at = None;
            if self.sent_radius != Some(self.current_radius) {
                self.phase = Phase::Waiting;
                self.wait_reason = Some(WaitReason::SendBackpressure);
            } else if ring_ready(self.current_radius) {
                self.phase = Phase::AtTarget;
                self.wait_reason = None;
            } else {
                self.phase = Phase::Waiting;
                self.wait_reason = Some(WaitReason::RingIncomplete);
            }
        } else if self.sent_radius != Some(self.current_radius) {
            self.phase = Phase::Waiting;
            self.wait_reason = Some(WaitReason::SendBackpressure);
        } else {
            self.step_ramp(&input, &mut ring_ready);
        }

        (self.sent_radius != Some(self.current_radius)).then_some(self.current_radius)
    }

    /// Records that the transport accepted `radius`.
    pub(super) fn confirm_sent(&mut self, radius: i32) {
        if radius != self.current_radius {
            return;
        }
        self.sent_radius = Some(radius);
        if self.phase == Phase::Waiting && self.wait_reason == Some(WaitReason::SendBackpressure) {
            self.phase = Phase::Ramp;
            self.wait_reason = None;
        }
    }

    /// Records a rejected or backpressured send; the next update returns the same radius.
    pub(super) fn send_failed(&mut self) {
        self.phase = Phase::Waiting;
        self.wait_reason = Some(WaitReason::SendBackpressure);
    }

    pub(super) const fn current_radius(&self) -> i32 {
        self.current_radius
    }

    pub(super) const fn target_radius(&self) -> i32 {
        self.target_radius
    }

    pub(super) const fn phase(&self) -> Phase {
        self.phase
    }

    pub(super) const fn wait_reason(&self) -> Option<WaitReason> {
        self.wait_reason
    }

    #[cfg(test)]
    pub(super) const fn sent_radius(&self) -> Option<i32> {
        self.sent_radius
    }

    /// Returns the current state when it changed or the periodic wait interval elapsed.
    pub(super) fn telemetry_due(&mut self, now: Duration) -> Option<ProgressTelemetry> {
        let milestone = ProgressMilestone {
            session: self.session.unwrap_or(0),
            current_radius: self.current_radius,
            target_radius: self.target_radius,
            phase: self.phase,
            wait_reason: self.wait_reason,
            sent_radius: self.sent_radius,
        };
        let changed = self.last_logged != Some(milestone);
        let periodic = now.saturating_sub(self.last_log_at) >= TELEMETRY_INTERVAL;
        if !changed && !periodic {
            return None;
        }
        self.last_logged = Some(milestone);
        self.last_log_at = now;
        Some(ProgressTelemetry {
            milestone,
            work: self.work,
            changed,
        })
    }

    fn begin(&mut self, session: u64, target: i32, now: Duration) {
        self.session = Some(session);
        self.target_radius = target;
        self.current_radius = 0;
        self.phase = Phase::Initial;
        self.wait_reason = None;
        self.sent_radius = None;
        self.zero_radius_started_at = Some(now);
        self.last_advance_at = now;
        self.backoff_until = None;
        self.backoff_rounds = 0;
        self.slow_frames = 0;
        self.stable_frames = 0;
        self.stressed_frames = 0;
        self.last_logged = None;
        self.last_log_at = now;
    }

    fn retarget(&mut self, target: i32, now: Duration) {
        self.target_radius = target;
        if self.current_radius > target {
            self.current_radius = target;
            self.last_advance_at = now;
            self.zero_radius_started_at = (target == 0).then_some(now);
            self.backoff_until = None;
            self.backoff_rounds = 0;
        } else if self.current_radius < target && self.phase == Phase::AtTarget {
            self.phase = Phase::Ramp;
        }
    }

    fn observe_frame(&mut self, frame_delta: Duration) {
        if frame_delta > SLOW_FRAME {
            self.slow_frames = self.slow_frames.saturating_add(1);
            self.stable_frames = 0;
        } else {
            self.slow_frames = 0;
            if frame_delta <= ADVANCE_MAX_FRAME {
                self.stable_frames = self.stable_frames.saturating_add(1);
            } else {
                self.stable_frames = 0;
            }
        }
    }

    fn observe_backlog(&mut self) {
        if self.work.mesh() >= BACKOFF_MESH_BACKLOG
            || self.work.light() >= BACKOFF_LIGHT_BACKLOG
            || self.work.decode >= BACKOFF_DECODE_BACKLOG
        {
            self.stressed_frames = self.stressed_frames.saturating_add(1);
        } else {
            self.stressed_frames = 0;
        }
    }

    fn step_ramp(&mut self, input: &ProgressInput, ring_ready: &mut impl FnMut(i32) -> bool) {
        if self.phase == Phase::Backoff {
            let until = self.backoff_until.unwrap_or(input.now);
            if input.now < until {
                return;
            }
            if !self.resume_ready() {
                let reason = self.wait_reason.unwrap_or(WaitReason::Backlog);
                self.begin_backoff(input.now, reason);
                return;
            }
            self.phase = Phase::Ramp;
            self.wait_reason = None;
            self.backoff_until = None;
            self.backoff_rounds = 0;
        }

        if self.degraded() {
            let reason = self.stress_reason();
            self.begin_backoff(input.now, reason);
            return;
        }

        let reason = if !ring_ready(self.current_radius) {
            Some(WaitReason::RingIncomplete)
        } else if self.work.mesh() > ADVANCE_MAX_MESH_BACKLOG {
            Some(WaitReason::MeshBacklog)
        } else if self.work.light() > ADVANCE_MAX_LIGHT_BACKLOG {
            Some(WaitReason::LightBacklog)
        } else if self.work.decode > ADVANCE_MAX_DECODE_BACKLOG {
            Some(WaitReason::DecodeBacklog)
        } else if self.slow_frames != 0 || input.frame_delta > ADVANCE_MAX_FRAME {
            Some(WaitReason::SlowFrames)
        } else if input.now.saturating_sub(self.last_advance_at) < ring_dwell(self.current_radius) {
            Some(WaitReason::RingDwell)
        } else {
            None
        };

        if let Some(reason) = reason {
            self.phase = Phase::Waiting;
            self.wait_reason = Some(reason);
        } else {
            self.current_radius = self.current_radius.saturating_add(1);
            self.last_advance_at = input.now;
            self.phase = Phase::Ramp;
            self.wait_reason = None;
            self.backoff_rounds = 0;
        }
    }

    fn degraded(&self) -> bool {
        self.slow_frames >= SLOW_FRAMES_FOR_BACKOFF
            || self.stressed_frames >= STRESSED_FRAMES_FOR_BACKOFF
    }

    fn stress_reason(&self) -> WaitReason {
        if self.work.mesh() >= BACKOFF_MESH_BACKLOG {
            WaitReason::MeshBacklog
        } else if self.work.light() >= BACKOFF_LIGHT_BACKLOG {
            WaitReason::LightBacklog
        } else if self.work.decode >= BACKOFF_DECODE_BACKLOG {
            WaitReason::DecodeBacklog
        } else {
            WaitReason::SlowFrames
        }
    }

    fn resume_ready(&self) -> bool {
        self.slow_frames == 0
            && self.stable_frames >= STABLE_FRAMES_TO_RESUME
            && self.work.mesh() <= RESUME_MAX_MESH_BACKLOG
            && self.work.light() <= RESUME_MAX_LIGHT_BACKLOG
            && self.work.decode <= RESUME_MAX_DECODE_BACKLOG
    }

    fn begin_backoff(&mut self, now: Duration, reason: WaitReason) {
        self.backoff_rounds = self.backoff_rounds.saturating_add(1).min(4);
        let delay = BACKOFF_BASE
            .saturating_mul(1_u32 << (self.backoff_rounds - 1))
            .min(BACKOFF_MAX);
        self.backoff_until = Some(now.saturating_add(delay));
        self.phase = Phase::Backoff;
        self.wait_reason = Some(reason);
    }
}

/// Waits shrink as the radius grows: 500 ms for the first ring, 400 ms for the second, then
/// 300/200/100 ms. The first column is requested immediately, before this dwell applies.
fn ring_dwell(radius: i32) -> Duration {
    let radius = u32::try_from(radius).unwrap_or(0);
    BASE_RING_DWELL
        .saturating_sub(RING_DWELL_STEP.saturating_mul(radius))
        .max(MIN_RING_DWELL)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(
        session: u64,
        target_radius: i32,
        now_millis: u64,
        frame_millis: u64,
    ) -> ProgressInput {
        ProgressInput {
            session,
            target_radius,
            now: Duration::from_millis(now_millis),
            frame_delta: Duration::from_millis(frame_millis),
            work: WorkCounts::default(),
        }
    }

    fn with_work(mut input: ProgressInput, work: WorkCounts) -> ProgressInput {
        input.work = work;
        input
    }

    fn mesh_backlog(mesh: usize) -> WorkCounts {
        WorkCounts {
            pending_mesh: mesh,
            ..WorkCounts::default()
        }
    }

    #[test]
    fn a_session_starts_at_one_column_and_sends_only_until_confirmed() {
        let mut load = ProgressiveLoad::default();
        assert_eq!(load.update(input(7, 5, 0, 16), |_| true), Some(0));
        assert_eq!(load.current_radius(), 0);
        assert_eq!(load.phase(), Phase::Waiting);
        assert_eq!(load.wait_reason(), Some(WaitReason::SendBackpressure));

        load.confirm_sent(0);
        assert_eq!(load.update(input(7, 5, 16, 16), |_| true), None);
        assert_eq!(load.sent_radius(), Some(0));
        assert_eq!(load.wait_reason(), Some(WaitReason::RingDwell));
    }

    #[test]
    fn rings_advance_one_at_a_time_with_shrinking_waits() {
        let mut load = ProgressiveLoad::default();
        assert_eq!(
            load.update(input(1, 3, 0, 16), |radius| radius == 0),
            Some(0)
        );
        load.confirm_sent(0);

        // The first ring waits its full dwell, then requests exactly one more radius.
        assert_eq!(
            load.update(input(1, 3, 500, 16), |radius| radius == 0),
            Some(1)
        );
        assert_eq!(load.current_radius(), 1);
        load.confirm_sent(1);

        // Ring two does not grow before its own (shorter) dwell and readiness.
        assert_eq!(
            load.update(input(1, 3, 516, 16), |radius| radius == 1),
            None
        );
        assert_eq!(load.current_radius(), 1);
        assert_eq!(
            load.update(input(1, 3, 900, 16), |radius| radius == 1),
            Some(2)
        );
        assert_eq!(load.current_radius(), 2);
        load.confirm_sent(2);

        assert_eq!(
            load.update(input(1, 3, 1_200, 16), |radius| radius == 2),
            Some(3)
        );
        load.confirm_sent(3);
        assert_eq!(
            load.update(input(1, 3, 1_216, 16), |radius| radius == 3),
            None
        );
        assert_eq!(load.phase(), Phase::AtTarget);
        assert!(ring_dwell(0) > ring_dwell(2));
        assert_eq!(ring_dwell(10), MIN_RING_DWELL);
    }

    #[test]
    fn an_unready_previous_ring_blocks_the_next_request() {
        let mut load = ProgressiveLoad::default();
        assert_eq!(load.update(input(1, 4, 0, 16), |_| false), Some(0));
        load.confirm_sent(0);

        for now in [500, 516, 532] {
            assert_eq!(load.update(input(1, 4, now, 16), |_| false), None);
        }
        assert_eq!(load.current_radius(), 0);
        assert_eq!(load.wait_reason(), Some(WaitReason::RingIncomplete));
    }

    #[test]
    fn deep_mesh_backlog_backs_off_and_resumes_after_stability() {
        let mut load = ProgressiveLoad::default();
        assert_eq!(load.update(input(1, 4, 0, 16), |_| true), Some(0));
        load.confirm_sent(0);

        for now in [500, 516, 532] {
            assert_eq!(
                load.update(with_work(input(1, 4, now, 16), mesh_backlog(100)), |_| true),
                None
            );
        }
        assert_eq!(load.current_radius(), 0);
        assert_eq!(load.phase(), Phase::Backoff);
        assert_eq!(load.wait_reason(), Some(WaitReason::MeshBacklog));

        // Stable frames accumulate while the backoff waits, but the radius does not move.
        for now in [600, 616, 632, 648, 664] {
            assert_eq!(load.update(input(1, 4, now, 16), |_| true), None);
        }
        assert_eq!(load.current_radius(), 0);
        assert_eq!(load.phase(), Phase::Backoff);

        // Once the backoff expires with a stable queue, the ramp resumes one ring.
        assert_eq!(load.update(input(1, 4, 1_300, 16), |_| true), Some(1));
        assert_eq!(load.current_radius(), 1);
        assert_eq!(load.phase(), Phase::Ramp);
    }

    #[test]
    fn slow_frames_back_off_without_waiting_for_a_still_player() {
        let mut load = ProgressiveLoad::default();
        assert_eq!(load.update(input(1, 4, 0, 16), |_| true), Some(0));
        load.confirm_sent(0);

        for now in [16, 32, 48] {
            assert_eq!(load.update(input(1, 4, now, 60), |_| true), None);
        }
        assert_eq!(load.current_radius(), 0);
        assert_eq!(load.phase(), Phase::Backoff);
        assert_eq!(load.wait_reason(), Some(WaitReason::SlowFrames));
    }

    #[test]
    fn shrinking_the_target_never_leaves_the_current_radius_above_it() {
        let mut load = ProgressiveLoad::default();
        assert_eq!(load.update(input(9, 8, 0, 16), |_| true), Some(0));
        load.confirm_sent(0);
        assert_eq!(load.update(input(9, 8, 500, 16), |_| true), Some(1));
        load.confirm_sent(1);
        assert_eq!(load.update(input(9, 8, 900, 16), |_| true), Some(2));
        load.confirm_sent(2);
        assert_eq!(load.current_radius(), 2);

        assert_eq!(load.update(input(9, 1, 1_000, 16), |_| true), Some(1));
        assert_eq!(load.current_radius(), 1);
        assert_eq!(load.target_radius(), 1);
        load.confirm_sent(1);
        assert_eq!(load.update(input(9, 1, 1_016, 16), |_| true), None);
        assert_eq!(load.phase(), Phase::AtTarget);
    }

    #[test]
    fn zero_radius_falls_back_to_the_safe_three_by_three_neighbourhood() {
        let mut load = ProgressiveLoad::default();
        assert_eq!(load.update(input(3, 6, 0, 16), |_| false), Some(0));
        load.confirm_sent(0);

        assert_eq!(load.update(input(3, 6, 2_999, 16), |_| false), None);
        assert_eq!(load.phase(), Phase::Waiting);
        assert_eq!(load.wait_reason(), Some(WaitReason::RingIncomplete));

        assert_eq!(load.update(input(3, 6, 3_000, 16), |_| false), Some(1));
        assert_eq!(load.current_radius(), 1);
        load.confirm_sent(1);
        assert_eq!(load.current_radius(), 1);
    }

    #[test]
    fn a_failed_send_retries_the_same_radius_without_advancing() {
        let mut load = ProgressiveLoad::default();
        assert_eq!(load.update(input(5, 4, 0, 16), |_| true), Some(0));
        load.send_failed();
        assert_eq!(load.update(input(5, 4, 500, 16), |_| true), Some(0));
        assert_eq!(load.current_radius(), 0);
        assert_eq!(load.wait_reason(), Some(WaitReason::SendBackpressure));

        load.confirm_sent(0);
        assert_eq!(load.update(input(5, 4, 516, 16), |_| true), None);
        assert_eq!(load.sent_radius(), Some(0));
    }

    #[test]
    fn a_new_session_resets_the_ramp_to_one_column() {
        let mut load = ProgressiveLoad::default();
        assert_eq!(load.update(input(1, 4, 0, 16), |_| true), Some(0));
        load.confirm_sent(0);
        assert_eq!(load.update(input(1, 4, 500, 16), |_| true), Some(1));
        load.confirm_sent(1);

        assert_eq!(load.update(input(2, 4, 520, 16), |_| true), Some(0));
        assert_eq!(load.current_radius(), 0);
        assert_eq!(load.sent_radius(), None);
        assert_eq!(load.target_radius(), 4);
    }
}
