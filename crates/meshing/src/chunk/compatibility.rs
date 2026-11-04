//! Heavy compatibility conversion from the cube stream to synthetic model faces.

use crate::{
    CompatFace, PackedModelDrawRef, PackedModelRef, PackedQuad, PackedQuadLighting, SIDE,
    contributors::pack_model_transform,
};
use assets::{COMPAT_CUBE_TEMPLATE_FACES, Material, compat_cube_template_id};

/// Packs every one-block compatibility face into the compact 12-byte stream. The whole
/// conversion is validated before mutating `out`, so a failure leaves the cube fallback intact.
pub(crate) fn append_compact_faces(
    cube_quads: &[PackedQuad],
    cube_lighting: &[PackedQuadLighting],
    materials: &[Material],
    template_base: u32,
    out: &mut Vec<CompatFace>,
) -> bool {
    if cube_quads.len() != cube_lighting.len() {
        return false;
    }
    let Some(material_count) = u32::try_from(materials.len()).ok() else {
        return false;
    };
    let Some(template_end) = material_count
        .checked_mul(COMPAT_CUBE_TEMPLATE_FACES)
        .and_then(|count| template_base.checked_add(count))
    else {
        return false;
    };
    if template_end > u32::from(u16::MAX) + 1 {
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

    let mut next = Vec::with_capacity(out.len() + cube_quads.len());
    next.extend_from_slice(out);
    for (quad, lighting) in cube_quads.iter().zip(cube_lighting) {
        let template_id = compat_cube_template_id(
            template_base,
            quad.material_id(),
            quad.face().index() as u32,
        );
        let Ok(template_id) = u16::try_from(template_id) else {
            return false;
        };
        next.push(CompatFace::new(
            quad.origin(),
            template_id,
            lighting.samples(),
        ));
    }
    *out = next;
    true
}

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
    // Every fallible check has passed. Append in place so an ordinary terrain chunk does not
    // allocate and copy the carrier model stream three times just to keep the old vectors for a
    // failure that can no longer happen. Capacity is reserved once; the pushes below only index
    // records already proven to fit in `u32`.
    let added = cube_quads.len();
    let Some(model_ref_end) = model_refs.len().checked_add(added) else {
        return false;
    };
    let Some(lighting_end) = model_lighting.len().checked_add(added) else {
        return false;
    };
    let Some(draw_end) = opaque_draws.len().checked_add(added) else {
        return false;
    };
    if u32::try_from(model_ref_end).is_err()
        || u32::try_from(lighting_end).is_err()
        || u32::try_from(draw_end).is_err()
    {
        return false;
    }
    model_refs.reserve(added);
    model_lighting.reserve(added);
    opaque_draws.reserve(added);
    for (quad, lighting) in cube_quads.iter().zip(cube_lighting) {
        let model_ref_index = u32::try_from(model_refs.len())
            .expect("compatibility model-ref range was checked before appending");
        let lighting_base_index = u32::try_from(model_lighting.len())
            .expect("compatibility lighting range was checked before appending");
        let template_id = compat_cube_template_id(
            template_base,
            quad.material_id(),
            quad.face().index() as u32,
        );
        model_refs.push(PackedModelRef::new(
            pack_model_transform(quad.origin(), 0),
            template_id,
            lighting_base_index,
            1,
        ));
        model_lighting.push(*lighting);
        opaque_draws.push(PackedModelDrawRef::new(model_ref_index, 0));
    }
    true
}
