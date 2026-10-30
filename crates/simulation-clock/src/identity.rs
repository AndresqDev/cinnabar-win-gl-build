//! Session and position scopes shared by simulation, animation and transport.

/// Identifies one local network session, including zero before a session starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionGeneration(u64);

impl SessionGeneration {
    /// Retains the generation assigned by the local session owner.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric generation for storage and diagnostics.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Revokes movement samples when their position authority is replaced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct PositionAuthorityGeneration(u64);

impl PositionAuthorityGeneration {
    /// Retains a position authority epoch from its publisher.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric epoch for diagnostics.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advances the position authority with the publisher's wrapping behavior.
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_authority_wraps_at_exhaustion() {
        assert_eq!(PositionAuthorityGeneration::new(0).next().get(), 1);
        assert_eq!(PositionAuthorityGeneration::new(u64::MAX).next().get(), 0);
    }
}
