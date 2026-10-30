//! Identities assigned by the semantic input router.

use core::num::NonZeroU64;

/// Identifies the authority that routed an action snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputAuthorityGeneration(NonZeroU64);

impl InputAuthorityGeneration {
    /// The router's initial authority, before the app supplies a replacement.
    pub const INITIAL: Self = Self(NonZeroU64::MIN);

    /// Retains a nonzero generation assigned by the input authority owner.
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// Returns the nonzero generation for the app's authority bookkeeping.
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// Orders finalized action snapshots independently of device activity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputSequence(u64);

impl InputSequence {
    /// Retains a finalized frame sequence; zero precedes the first frame.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric sequence for diagnostics.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advances the finalized frame sequence, rejecting exhaustion.
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalized_input_sequence_rejects_exhaustion() {
        assert_eq!(InputSequence::new(0).checked_next().unwrap().get(), 1);
        assert_eq!(InputSequence::new(u64::MAX).checked_next(), None);
    }
}
