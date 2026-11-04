//! Heavy compatibility conversion from the cube stream to synthetic model faces.

use crate::{
    PackedModelDrawRef, PackedModelRef, PackedQuad, PackedQuadLighting, SIDE,
    contributors::pack_model_transform,
};
use assets::{COMPAT_CUBE_TEMPLATE_FACES, Material, compat_cube_template_id};

/// Appends one synthetic model reference per one-block cube face.
///
/// The caller keeps the original cube streams when this returns `false`; that
/// matters because an un-converted cube remains visible even if the adapter
/// rejects the heavy model path. The conversion therefore never leaves a face
/// represented by neither stream.
pub(crate) fn append_model_faces(
    cube_quads: &[PackedQuad],
    cube_lighting: &[PackedQuadLighting],
    materials: &[Material],
    template_base: u32,
    model_refs: &mut Vec<PackedModelRef>,
    model_lighting: &mut Vec<PackedQuadLighting>,
    opaque_draws: &mut Vec<PackedModelDrawRef>,
) -> bool {
    if cube_quads.len() != cube_lighting.len() {
        return false;
    }
    if u32::try_from(materials.len())
        .ok()
        .and_then(|count| count.checked_mul(COMPAT_CUBE_TEMPLATE_FACES))
        .and_then(|count| template_base.checked_add(count))
        .is_none()
    {
        return false;
    }
    if cube_quads.iter().any(|quad| {
        quad.width() != 1
            || quad.height() != 1
            || quad
                .origin()
                .iter()
                .any(|coordinate| *coordinate >= SIDE as u8)
            || materials.get(quad.material_id() as usize).is_none()
    }) {
        return false;
    }
    let mut next_refs = Vec::with_capacity(model_refs.len() + cube_quads.len());
    next_refs.extend_from_slice(model_refs);
    let mut next_lighting = Vec::with_capacity(model_lighting.len() + cube_quads.len());
    next_lighting.extend_from_slice(model_lighting);
    let mut next_draws = Vec::with_capacity(opaque_draws.len() + cube_quads.len());
    next_draws.extend_from_slice(opaque_draws);
    for (quad, lighting) in cube_quads.iter().zip(cube_lighting) {
        let Ok(model_ref_index) = u32::try_from(next_refs.len()) else {
            return false;
        };
        let Ok(lighting_base_index) = u32::try_from(next_lighting.len()) else {
            return false;
        };
        let template_id = compat_cube_template_id(
            template_base,
            quad.material_id(),
            quad.face().index() as u32,
        );
        next_refs.push(PackedModelRef::new(
            pack_model_transform(quad.origin(), 0),
            template_id,
            lighting_base_index,
            1,
        ));
        next_lighting.push(*lighting);
        next_draws.push(PackedModelDrawRef::new(model_ref_index, 0));
    }
    *model_refs = next_refs;
    *model_lighting = next_lighting;
    *opaque_draws = next_draws;
    true
}
