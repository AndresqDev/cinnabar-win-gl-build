use assets::{
    CompiledEntityAssets, EntityGeometryLocator, EntityGeometryScalar, MAX_GEOMETRY_LOCATORS,
    RuntimeEntityAssets, encode_entity_blob,
};

use super::suite::inherited_geometry_fixture;

/// Builds a catalog with distinct base locators and one locator on a derived bone.
fn locator_fixture(base_count: usize, derived_name: &str) -> CompiledEntityAssets {
    let mut compiled = inherited_geometry_fixture();
    let locator = EntityGeometryLocator {
        name: derived_name.into(),
        offset: [EntityGeometryScalar::ZERO; 3],
        rotation: [EntityGeometryScalar::ZERO; 3],
        ignore_inherited_scale: false,
    };
    compiled.geometries[0].bones[0].locators = (0..base_count)
        .map(|index| EntityGeometryLocator {
            name: format!("locator_{index}").into(),
            ..locator.clone()
        })
        .collect();
    let mut derived_bone = compiled.geometries[0].bones[0].clone();
    derived_bone.name = "derived_wing".into();
    derived_bone.parent = Some("wing".into());
    derived_bone.cubes = Box::default();
    derived_bone.locators = vec![locator].into();
    compiled.geometries[1].bones = vec![derived_bone].into();
    compiled
}

#[test]
fn shared_validation_rejects_inherited_locator_overflow() {
    let compiled = locator_fixture(MAX_GEOMETRY_LOCATORS, "derived_tip");
    assert!(compiled.validate().is_err());
    assert!(RuntimeEntityAssets::from_compiled(compiled).is_err());
}

#[test]
fn encoder_rejects_inherited_locator_overflow() {
    let compiled = locator_fixture(MAX_GEOMETRY_LOCATORS, "derived_tip");
    assert!(encode_entity_blob(&compiled).is_err());
}

#[test]
fn inherited_locators_at_the_limit_round_trip() {
    let compiled = locator_fixture(MAX_GEOMETRY_LOCATORS - 1, "derived_tip");
    compiled.validate().unwrap();
    let encoded = encode_entity_blob(&compiled).unwrap();
    let runtime = RuntimeEntityAssets::decode(&encoded).unwrap();
    assert!(runtime.geometry_locator(1, "locator_0").is_some());
    assert_eq!(
        runtime
            .geometry_locator(1, "derived_tip")
            .unwrap()
            .bone_index,
        1
    );
}

#[test]
fn inherited_locator_override_does_not_consume_another_slot() {
    let compiled = locator_fixture(MAX_GEOMETRY_LOCATORS, "locator_0");
    compiled.validate().unwrap();
    let encoded = encode_entity_blob(&compiled).unwrap();
    let runtime = RuntimeEntityAssets::decode(&encoded).unwrap();
    assert_eq!(
        runtime.geometry_locator(0, "locator_0").unwrap().bone_index,
        0
    );
    assert_eq!(
        runtime.geometry_locator(1, "locator_0").unwrap().bone_index,
        1
    );
    let last = format!("locator_{}", MAX_GEOMETRY_LOCATORS - 1);
    assert!(runtime.geometry_locator(1, &last).is_some());
}

#[test]
fn inherited_locator_cache_preserves_skeleton_index_and_derived_rest_pivot() {
    let scalar = |value| EntityGeometryScalar::new(value).unwrap();
    let mut compiled = inherited_geometry_fixture();
    let locator = assets::EntityGeometryLocator {
        name: "wing_tip".into(),
        offset: [scalar(16.0), scalar(32.0), scalar(0.0)],
        rotation: [scalar(0.0); 3],
        ignore_inherited_scale: true,
    };
    compiled.geometries[0].bones[0].locators = vec![locator.clone()].into();
    let mut derived_bone = compiled.geometries[0].bones[0].clone();
    derived_bone.locators = Box::default();
    derived_bone.cubes = Box::default();
    derived_bone.pivot = Some([scalar(8.0), scalar(16.0), scalar(0.0)]);
    compiled.geometries[1].bones = vec![derived_bone].into();
    let runtime = RuntimeEntityAssets::decode(&encode_entity_blob(&compiled).unwrap()).unwrap();
    let base = runtime.geometry_locator(0, "wing_tip").unwrap();
    let derived = runtime.geometry_locator(1, "wing_tip").unwrap();
    assert_eq!(base.bone_index, 0);
    assert_eq!(derived.bone_index, 0);
    assert_eq!(derived.pivot, [-0.5, 1.0, 0.0]);
    assert_eq!(derived.locator, locator);
    assert!(runtime.geometry_locator(1, "missing").is_none());
}
