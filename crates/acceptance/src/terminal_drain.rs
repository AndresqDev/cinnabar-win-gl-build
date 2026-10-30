use std::time::Instant;

use super::{AcceptanceRun, TRANSPARENT_PRESENTATION_EXIT_GRACE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementTerminalDrainDecision {
    Drained,
    Wait,
    TimedOut,
}

impl AcceptanceRun {
    pub fn deadline_reached(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|deadline| now >= deadline)
    }

    pub fn movement_terminal_drain_decision(
        &self,
        now: Instant,
        candidate_physics: bool,
        pending_count: usize,
    ) -> MovementTerminalDrainDecision {
        if self.shutdown_requested {
            return MovementTerminalDrainDecision::Drained;
        }
        if !candidate_physics || pending_count == 0 {
            return MovementTerminalDrainDecision::Drained;
        }
        let Some(deadline) = self.deadline else {
            return MovementTerminalDrainDecision::Wait;
        };
        let drain_deadline = deadline
            .checked_add(TRANSPARENT_PRESENTATION_EXIT_GRACE)
            .expect("movement terminal drain deadline overflowed");
        if now < drain_deadline {
            MovementTerminalDrainDecision::Wait
        } else {
            MovementTerminalDrainDecision::TimedOut
        }
    }
}
