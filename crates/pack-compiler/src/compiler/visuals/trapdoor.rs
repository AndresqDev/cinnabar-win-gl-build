//! Trapdoor faces retain their directional artwork and thin edge sprite regions.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::state::{exact_tagged_byte, exact_tagged_int};
use {super::super::*, assets::BlockFace};

const BLOCK: i16 = 256;
const THICKNESS: i16 = 3 * 16;
const UV_SCALE: u16 = 16;
const SPRITE: u16 = BLOCK as u16 * UV_SCALE;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::compiler) struct TemplateKey {
    materials: [u32; 6],
    direction: u8,
    open: u8,
    top: u8,
}

/// Reads the complete typed selector product before admitting a trapdoor pose.
fn state(record: &RegistryRecord) -> Option<(u8, u8, u8)> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 3 {
        return None;
    }
    let direction = exact_tagged_int(state.get("direction")?, 3)? as u8;
    let open = exact_tagged_byte(state.get("open_bit")?, 1)?;
    let top = exact_tagged_byte(state.get("upside_down_bit")?, 1)?;
    for (field, value) in [
        (ModelStateField::Orientation, direction),
        (ModelStateField::Open, open),
        (ModelStateField::Half, top),
    ] {
        if record.model_state.get(field)? != u32::from(value) {
            return None;
        }
    }
    Some((direction, open, top))
}

/// Compiles a state-owned template, including the opened cap texture override.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<TemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_trapdoor(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    let Some((direction, open, top)) = state(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let Some(mut materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    if open == 1 && direction >= 2 {
        materials[BlockFace::Down as usize] = materials[BlockFace::Up as usize];
    }
    let key = TemplateKey {
        materials,
        direction,
        open,
        top,
    };
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(quads(key), 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}

/// Selects the closed half or opened hinge side in block-coordinate units.
fn bounds(key: TemplateKey) -> ([i16; 3], [i16; 3]) {
    const HIGH: i16 = BLOCK - THICKNESS;
    match (key.open, key.direction, key.top) {
        (0, _, 0) => ([0, 0, 0], [BLOCK, THICKNESS, BLOCK]),
        (0, _, 1) => ([0, HIGH, 0], [BLOCK; 3]),
        (1, 0, _) => ([0; 3], [THICKNESS, BLOCK, BLOCK]),
        (1, 1, _) => ([HIGH, 0, 0], [BLOCK; 3]),
        (1, 2, _) => ([0; 3], [BLOCK, BLOCK, THICKNESS]),
        (1, 3, _) => ([0, 0, HIGH], [BLOCK; 3]),
        _ => unreachable!("trapdoor selectors are validated before compiling"),
    }
}

/// Selects per-face UV orientation without rotating the world-space geometry.
fn rotation(key: TemplateKey, face: BlockFace) -> u8 {
    if matches!(face, BlockFace::Up | BlockFace::Down) {
        return match key.direction {
            0 => 2,
            1 => 1,
            2 if key.open == 1 && face == BlockFace::Down => 3,
            2 => 0,
            3 if key.open == 1 && face == BlockFace::Down => 0,
            3 => 3,
            _ => unreachable!(),
        };
    }
    if key.open == 1 {
        match key.direction {
            0 | 1 if matches!(face, BlockFace::North | BlockFace::South) => {
                return if key.direction == 0 { 2 } else { 1 };
            }
            2 | 3 if matches!(face, BlockFace::West | BlockFace::East) => {
                return if key.direction == 2 { 2 } else { 1 };
            }
            _ => {}
        }
    }
    if key.top == 0 { 3 } else { 0 }
}

/// Maps a vertex to the selected sprite axes, cropping after orientation selection.
fn uv(face: BlockFace, rotation: u8, p: [u16; 3], min: [u16; 3], max: [u16; 3]) -> [u16; 2] {
    let [x, y, z] = p;
    if matches!(face, BlockFace::Up | BlockFace::Down) {
        let z = if face == BlockFace::Down {
            min[2] + max[2] - z
        } else {
            z
        };
        return match rotation {
            0 => [x, z],
            1 => [z, SPRITE - x],
            2 => [SPRITE - z, x],
            3 => [SPRITE - x, SPRITE - z],
            _ => unreachable!(),
        };
    }
    let axis = if matches!(face, BlockFace::West | BlockFace::East) {
        2
    } else {
        0
    };
    let a = p[axis];
    let reflected = matches!(face, BlockFace::North | BlockFace::East);
    match rotation {
        0 => [
            if reflected {
                min[axis] + max[axis] - a
            } else {
                a
            },
            SPRITE - y,
        ],
        1 => {
            let y = min[1] + max[1] - y;
            [if reflected { SPRITE - y } else { y }, SPRITE - a]
        }
        2 => [if reflected { y } else { SPRITE - y }, a],
        3 => [
            if reflected {
                SPRITE - (min[axis] + max[axis] - a)
            } else {
                SPRITE - a
            },
            y,
        ],
        _ => unreachable!(),
    }
}

/// Emits six face-owned quads with normalized UVs independent of texture resolution.
fn quads(key: TemplateKey) -> Vec<ModelQuad> {
    let (min, max) = bounds(key);
    let mut quads = super::geometry::cuboid_quads(key.materials, min, max);
    let min = min.map(|v| v as u16 * UV_SCALE);
    let max = max.map(|v| v as u16 * UV_SCALE);
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        let rotation = rotation(key, face);
        for (p, uv_out) in quad.positions.into_iter().zip(&mut quad.uvs) {
            let p = p.map(|v| v as u16 * UV_SCALE);
            *uv_out = if key.open == 1
                && key.direction >= 2
                && matches!(face, BlockFace::Down | BlockFace::Up)
            {
                // These caps use the full width and top edge of the Up sprite.
                let x = u16::from(p[0] == max[0]) * SPRITE;
                let z = u16::from(p[2] == max[2]) * SPRITE;
                let [u, v] = uv(face, rotation, [x, 0, z], [0; 3], [SPRITE; 3]);
                [u, v / SPRITE * THICKNESS as u16 * UV_SCALE]
            } else {
                uv(face, rotation, p, min, max)
            };
        }
    }
    quads.to_vec()
}
