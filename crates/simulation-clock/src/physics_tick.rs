/// Identifies a completed local physics tick or its server anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicsTick(u64);

impl PhysicsTick {
    /// Retains a tick from the simulator or the protocol's raw tick field.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw tick at the simulator, packet or diagnostic boundary.
    pub const fn get(self) -> u64 {
        self.0
    }
}
