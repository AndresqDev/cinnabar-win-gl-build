//! Compiled beacon tint facts keyed by admitted block identity.

use std::collections::HashMap;

use crate::{BeaconTint, NetworkIdMode, RegistryRecord};

impl BeaconTint {
    /// Returns a palette colour only for a supported tinting material.
    #[must_use]
    pub const fn dye_colour(self) -> Option<i64> {
        match self {
            Self::Unresolved | Self::NoTinting => None,
            _ => Some(17 - self as i64),
        }
    }
}

/// Immutable numeric lookup built before frame submission from validated block records.
pub struct BeaconTintCatalog {
    sequential: Box<[BeaconTint]>,
    hashed: HashMap<u32, BeaconTint>,
}

impl BeaconTintCatalog {
    /// Builds the active catalog at startup; malformed pinned data is a startup error.
    #[must_use]
    pub fn pinned() -> Self {
        let records = crate::read_registry_for_protocol(
            crate::pinned_block_registry_bytes(),
            crate::active_content_registry_protocol(),
        )
        .expect("valid pinned beacon tint descriptors; rebuild with make assets");
        Self::from_records(&records)
    }

    /// Indexes validated carrier identities without retaining or interpreting block names.
    #[must_use]
    pub fn from_records(records: &[RegistryRecord]) -> Self {
        let count = records
            .iter()
            .map(|r| r.sequential_id as usize + 1)
            .max()
            .unwrap_or(0);
        let mut sequential = vec![BeaconTint::Unresolved; count];
        let mut hashed = HashMap::with_capacity(records.len());
        for record in records {
            sequential[record.sequential_id as usize] = record.beacon_tint;
            hashed.insert(record.network_hash, record.beacon_tint);
        }
        Self {
            sequential: sequential.into_boxed_slice(),
            hashed,
        }
    }

    /// Resolves an internal session identity; custom or absent entries remain unresolved.
    #[must_use]
    pub fn get(&self, mode: NetworkIdMode, id: u32) -> BeaconTint {
        match mode {
            NetworkIdMode::Sequential => self.sequential.get(id as usize),
            NetworkIdMode::Hashed => self.hashed.get(&id),
        }
        .copied()
        .unwrap_or(BeaconTint::Unresolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beacon_facts_cover_all_pinned_states_and_ignore_display_names() {
        let mut records = crate::read_registry_for_protocol(
            crate::pinned_block_registry_bytes(),
            crate::active_content_registry_protocol(),
        )
        .unwrap();
        assert!(
            records
                .iter()
                .all(|r| r.beacon_tint != BeaconTint::Unresolved)
        );
        let red = records
            .iter_mut()
            .find(|r| r.name.as_ref() == "minecraft:red_stained_glass")
            .unwrap();
        assert_eq!(red.beacon_tint, BeaconTint::Red);
        let (sequential, hashed) = (red.sequential_id, red.network_hash);
        red.name = "fixture:renamed".into();
        let catalog = BeaconTintCatalog::from_records(&records);
        for (mode, id) in [
            (NetworkIdMode::Sequential, sequential),
            (NetworkIdMode::Hashed, hashed),
        ] {
            assert_eq!(catalog.get(mode, id), BeaconTint::Red);
        }
        assert_eq!(
            catalog.get(NetworkIdMode::Hashed, u32::MAX),
            BeaconTint::Unresolved
        );
        assert_eq!(
            catalog.get(NetworkIdMode::Sequential, u32::MAX),
            BeaconTint::Unresolved
        );
        assert_eq!(BeaconTint::Red.dye_colour(), Some(1));
        assert_eq!(BeaconTint::NoTinting.dye_colour(), None);
        assert_eq!(BeaconTint::Unresolved.dye_colour(), None);
    }
}
