/// Identifies one published pose independently of physics and packet sequences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PoseGeneration(u64);

impl PoseGeneration {
    /// Retains the pose revision assigned by its publication owner.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric revision for diagnostics.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advances a pose publication, rejecting exhaustion.
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Invalidates a retained pose without wrapping its revision on exhaustion.
    pub const fn saturating_next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_rejects_exhaustion_and_invalidation_saturates() {
        assert_eq!(PoseGeneration::new(0).checked_next().unwrap().get(), 1);
        let exhausted = PoseGeneration::new(u64::MAX);
        assert_eq!(exhausted.checked_next(), None);
        assert_eq!(exhausted.saturating_next(), exhausted);
    }
}
