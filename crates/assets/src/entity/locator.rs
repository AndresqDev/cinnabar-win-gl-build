//! Bounded model-space attachment points retained by the geometry carrier.
use super::*;

pub const MAX_GEOMETRY_LOCATORS: usize = 256;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityGeometryLocator {
    pub name: Box<str>,
    pub offset: [EntityGeometryScalar; 3],
    pub rotation: [EntityGeometryScalar; 3],
    pub ignore_inherited_scale: bool,
}

/// Validates locator names, scalar values and the per-bone allocation bound.
pub(super) fn validate(bone: &EntityGeometryBone) -> Result<(), AssetError> {
    if bone.locators.len() > MAX_GEOMETRY_LOCATORS {
        return Err(invalid("entity geometry locator count exceeds bound"));
    }
    let mut names = std::collections::BTreeSet::new();
    for locator in &bone.locators {
        validate_geometry_name(&locator.name)?;
        if !names.insert(&locator.name) {
            return Err(invalid("duplicate entity geometry locator"));
        }
        validate_scalars(&locator.offset)?;
        validate_scalars(&locator.rotation)?;
    }
    Ok(())
}

/// A worker-resolved locator in the exact inherited skeleton order used by actor meshes.
#[derive(Clone, Debug)]
pub struct EntityGeometryLocatorBinding {
    pub bone_index: u32,
    pub pivot: [f32; 3],
    pub locator: EntityGeometryLocator,
}

/// Caps the total annotations before inheritance or cache allocation.
pub(super) fn validate_total(bones: &[EntityGeometryBone]) -> Result<(), AssetError> {
    if bones.iter().map(|bone| bone.locators.len()).sum::<usize>() > MAX_GEOMETRY_LOCATORS {
        return Err(invalid("entity geometry locator count exceeds bound"));
    }
    Ok(())
}

/// Resolves locator bone indices and pivots once when the immutable catalog is admitted.
pub(super) fn compile(
    geometries: &[EntityGeometry],
    parents: &[Option<usize>],
) -> Result<Vec<std::collections::BTreeMap<Box<str>, EntityGeometryLocatorBinding>>, AssetError> {
    let mut catalogs = Vec::with_capacity(geometries.len());
    for geometry in 0..geometries.len() {
        let mut chain = Vec::new();
        let mut current = Some(geometry);
        while let Some(index) = current {
            chain.push(index);
            current = parents[index];
        }
        if chain.iter().all(|index| {
            geometries[*index]
                .bones
                .iter()
                .all(|bone| bone.locators.is_empty())
        }) {
            catalogs.push(Default::default());
            continue;
        }
        let mut names = std::collections::BTreeMap::<String, usize>::new();
        let mut pivots = Vec::<[f32; 3]>::new();
        let mut locators =
            std::collections::BTreeMap::<Box<str>, (usize, EntityGeometryLocator)>::new();
        for index in chain.into_iter().rev() {
            for bone in &geometries[index].bones {
                let next = pivots.len();
                let index = *names.entry(bone.name.to_ascii_lowercase()).or_insert(next);
                if index == next {
                    pivots.push([0.0; 3]);
                }
                if let Some(pivot) = bone.pivot {
                    let [x, y, z] = pivot.map(EntityGeometryScalar::get);
                    pivots[index] = [-x / 16.0, y / 16.0, z / 16.0];
                }
                for locator in &bone.locators {
                    if locators.len() >= MAX_GEOMETRY_LOCATORS
                        && !locators.contains_key(locator.name.as_ref())
                    {
                        return Err(invalid("inherited geometry locator count exceeds bound"));
                    }
                    locators.insert(locator.name.clone(), (index, locator.clone()));
                }
            }
        }
        catalogs.push(
            locators
                .into_iter()
                .map(|(name, (index, locator))| {
                    (
                        name,
                        EntityGeometryLocatorBinding {
                            bone_index: index as u32,
                            pivot: pivots[index],
                            locator,
                        },
                    )
                })
                .collect(),
        );
    }
    Ok(catalogs)
}

impl RuntimeEntityAssets {
    /// Borrows the worker-resolved locator without scanning or cloning geometry during a frame.
    pub fn geometry_locator(
        &self,
        geometry: usize,
        name: &str,
    ) -> Option<&EntityGeometryLocatorBinding> {
        self.locator_catalog.get(geometry)?.get(name)
    }
}
