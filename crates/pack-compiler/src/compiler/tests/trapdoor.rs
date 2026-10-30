use super::*;
use assets::{BlockFace, CompiledBiomeAssets, ModelFamily, NetworkIdMode, RuntimeAssets};

/// Compiles every admitted trapdoor state with six distinct directional sprites.
fn fixture(size: u32, malformed: Option<&str>) -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry_bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let registry = assets::read_registry_for_protocol(&registry_bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &registry_bytes,
        registry.len(),
        protocol,
    )
    .unwrap();
    let mut records = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            record.name.as_ref() == "minecraft:air" || record.model_family == ModelFamily::Trapdoor
        })
        .collect::<Vec<_>>();
    if let Some(state) = malformed {
        records
            .iter_mut()
            .find(|record| record.model_family == ModelFamily::Trapdoor)
            .unwrap()
            .canonical_state = state.into();
    }
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let faces = ["west", "east", "down", "up", "north", "south"];
    let textures: serde_json::Map<String, serde_json::Value> = faces
        .iter()
        .map(|face| (face.to_string(), serde_json::json!(face)))
        .collect();
    let blocks: serde_json::Map<String, serde_json::Value> = records
        .iter()
        .filter(|r| r.model_family == ModelFamily::Trapdoor)
        .map(|r| {
            (
                r.name.strip_prefix("minecraft:").unwrap().to_string(),
                serde_json::json!({"textures":textures}),
            )
        })
        .collect();
    write(
        directory.path().join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    );
    let texture_data: serde_json::Map<String, serde_json::Value> = faces
        .iter()
        .map(|face| {
            (
                face.to_string(),
                serde_json::json!({"textures":format!("textures/blocks/{face}")}),
            )
        })
        .collect();
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::to_vec(&serde_json::json!({"texture_data":texture_data})).unwrap(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    for (face, name) in faces.into_iter().enumerate() {
        let pixels = (0..size * size)
            .flat_map(|pixel| {
                [
                    40 + face as u8 * 25,
                    (pixel % size * 7) as u8,
                    (pixel / size * 7) as u8,
                    255,
                ]
            })
            .collect::<Vec<_>>();
        write_png(
            directory.path().join(format!("textures/blocks/{name}.png")),
            size,
            size,
            &pixels,
        );
    }
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, records)
}

/// Resolves compiler output through the encoded carrier consumed by world and item meshes.
fn runtime(compiled: &assets::CompiledAssets) -> RuntimeAssets {
    RuntimeAssets::decode(&assets::encode_blob(compiled).unwrap()).unwrap()
}

/// Reads the original face marker from the material's actual texture page and layer.
fn face_marker(compiled: &assets::CompiledAssets, material: u32) -> u8 {
    let texture = compiled.materials[material as usize].texture;
    let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
    mip.rgba8[(texture.layer() * mip.size * mip.size * 4) as usize]
}

#[test]
fn trapdoor_edges_use_the_top_three_sprite_rows_in_every_registered_pose() {
    for size in [16, 32] {
        let (compiled, records) = fixture(size, None);
        let runtime = runtime(&compiled);
        let mut states = BTreeMap::<&str, std::collections::BTreeSet<(u64, u64, u64)>>::new();
        for record in records
            .iter()
            .filter(|r| r.model_family == ModelFamily::Trapdoor)
        {
            let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
            let direction = state["direction"]["value"].as_u64().unwrap();
            let open = state["open_bit"]["value"].as_u64().unwrap();
            let top = state["upside_down_bit"]["value"].as_u64().unwrap();
            states
                .entry(&record.name)
                .or_default()
                .insert((direction, open, top));
            let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
            let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
            let quads = &runtime.model_quads()[template.quad_start as usize..]
                [..template.quad_count as usize];
            assert_eq!(quads.len(), 6);
            let thin_axis = if open == 0 {
                1
            } else if direction < 2 {
                0
            } else {
                2
            };
            for (face, quad) in BlockFace::ALL.into_iter().zip(quads) {
                let extent: [i16; 3] = std::array::from_fn(|axis| {
                    quad.positions.iter().map(|p| p[axis]).max().unwrap()
                        - quad.positions.iter().map(|p| p[axis]).min().unwrap()
                });
                let v = quad.uvs.map(|uv| uv[1]);
                let edge = extent[thin_axis] != 0;
                assert_eq!(
                    *v.iter().min().unwrap(),
                    0,
                    "{size}px {face:?} {}",
                    record.canonical_state
                );
                assert_eq!(
                    *v.iter().max().unwrap(),
                    if edge { 768 } else { 4096 },
                    "{size}px {face:?} {}",
                    record.canonical_state
                );
                assert_eq!(quad.uvs.iter().map(|uv| uv[0]).min(), Some(0));
                assert_eq!(quad.uvs.iter().map(|uv| uv[0]).max(), Some(4096));
                let source = if open == 1
                    && direction >= 2
                    && matches!(face, BlockFace::Up | BlockFace::Down)
                {
                    BlockFace::Up
                } else {
                    face
                };
                assert_eq!(
                    face_marker(&compiled, quad.material),
                    40 + source as u8 * 25
                );
                assert_eq!(quad.material, visual.face(face).material_id());
            }
        }
        assert!(!states.is_empty());
        for product in states.values() {
            assert_eq!(product.len(), 4 * 2 * 2);
        }
    }
}

#[test]
fn trapdoor_broad_artwork_follows_direction_and_top_half() {
    let (compiled, records) = fixture(16, None);
    let runtime = runtime(&compiled);
    for record in records
        .iter()
        .filter(|r| r.model_family == ModelFamily::Trapdoor)
    {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let direction = state["direction"]["value"].as_u64().unwrap() as usize;
        let open = state["open_bit"]["value"].as_u64().unwrap();
        let top = state["upside_down_bit"]["value"].as_u64().unwrap();
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        let (face, expected) = if open == 0 {
            (
                BlockFace::Up,
                [[4096, 0], [0, 4096], [0, 0], [4096, 4096]][direction],
            )
        } else if direction < 2 {
            (
                BlockFace::West,
                if top == 0 { [4096, 0] } else { [0, 4096] },
            )
        } else {
            (
                BlockFace::North,
                if top == 0 { [0, 0] } else { [4096, 4096] },
            )
        };
        assert_eq!(
            quads[face as usize].uvs[0], expected,
            "{}",
            record.canonical_state
        );
    }
}

#[test]
fn malformed_trapdoor_selectors_stay_diagnostic() {
    for state in [
        r#"{"direction":{"type":"int","value":4},"open_bit":{"type":"byte","value":0},"upside_down_bit":{"type":"byte","value":0}}"#,
        r#"{"direction":{"type":"byte","value":0},"open_bit":{"type":"byte","value":0},"upside_down_bit":{"type":"byte","value":0}}"#,
        r#"{"direction":{"type":"int","value":0},"open_bit":{"type":"byte","value":2},"upside_down_bit":{"type":"byte","value":0}}"#,
        r#"{"direction":{"type":"int","value":0},"open_bit":{"type":"byte","value":0},"upside_down_bit":{"type":"int","value":0}}"#,
        r#"{"direction":{"type":"int","value":0},"open_bit":{"type":"byte","value":0}}"#,
    ] {
        let (compiled, records) = fixture(16, Some(state));
        let record = records
            .iter()
            .find(|r| r.model_family == ModelFamily::Trapdoor)
            .unwrap();
        assert_eq!(
            runtime(&compiled)
                .resolve(NetworkIdMode::Sequential, record.sequential_id)
                .support(),
            VisualSupport::Diagnostic
        );
    }
}
