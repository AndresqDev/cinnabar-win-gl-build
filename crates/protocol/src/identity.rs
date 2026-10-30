//! Local ordering assigned by the inbound session transport.

/// Orders inbound events in a session's FIFO, including the zero bootstrap fence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FifoSequence(u64);

impl FifoSequence {
    /// Retains an inbound event's sequence or a committed sequence fence.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric sequence at a storage or diagnostic boundary.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advances the inbound FIFO, retaining its final sequence on exhaustion.
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_exhaustion_keeps_the_last_sequence() {
        assert_eq!(FifoSequence::new(0).next().get(), 1);
        assert_eq!(FifoSequence::new(u64::MAX).next().get(), u64::MAX);
    }
}
