//! Dense, bounded variable slots shared by effect and controller expressions.
use super::BUILTINS;
use std::collections::HashMap;
/// Interned variable names; the builtin slots below are fixed for every effect.
#[derive(Clone, Debug, Default)]
pub struct Interner {
    names: Vec<Box<str>>,
    index: HashMap<Box<str>, u16>,
}

/// Bound on interned names so a hostile definition cannot grow per-particle state.
pub const MAX_VARIABLES: usize = 256;

impl Interner {
    /// Creates the fixed built-in slots before admitting authored variables.
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut interner = Self::default();
        for name in BUILTINS {
            interner.intern(name);
        }
        interner
    }

    /// Slot for a variable name (without the `variable.` prefix), or `None` once full.
    pub fn intern(&mut self, name: &str) -> Option<u16> {
        if let Some(&slot) = self.index.get(name) {
            return Some(slot);
        }
        if self.names.len() >= MAX_VARIABLES {
            return None;
        }
        let slot = self.names.len() as u16;
        let boxed: Box<str> = name.into();
        self.names.push(boxed.clone());
        self.index.insert(boxed, slot);
        Some(slot)
    }

    /// Looks up a compiled variable without allocating a new slot.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<u16> {
        self.index.get(name).copied()
    }

    /// Visits compiled names and their dense slots without exposing mutable indexing.
    pub(crate) fn named_slots(&self) -> impl Iterator<Item = (&str, u16)> {
        self.names
            .iter()
            .enumerate()
            .map(|(slot, name)| (name.as_ref(), slot as u16))
    }

    /// Returns the storage required for all admitted variable slots.
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }
}
