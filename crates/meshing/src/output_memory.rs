use assets::{NetworkIdMode, RuntimeAssets};
use world::{BLOCKS_PER_SUB_CHUNK, SubChunk};

use crate::{
    ChunkMesh, DiagnosticGeometryCount, Face, MAX_DIAGNOSTIC_IDENTITIES_PER_MESH,
    MAX_PACKED_BIOME_RECORD_WORDS, PackedBiomeRecord, PackedLiquidQuad, PackedModelDrawRef,
    PackedModelRef, PackedQuad, PackedQuadLighting,
    chunk::models::{
        MAX_SELECTED_MODEL_TEMPLATES, ModelTemplateSelection, model_template_selection,
    },
    types::{CubeStreams, ModelDrawRefs},
};

/// Conservative packed-output sizes for one immutable runtime asset set.
#[derive(Debug)]
pub struct MeshOutputBounds {
    model_templates: Box<[u64]>,
    fallback_model: u64,
    compatibility_cube: u64,
    compatibility_compact: u64,
}

impl MeshOutputBounds {
    /// Accounts for every cell, face, selected model and compound model part.
    #[must_use]
    pub fn new(assets: &RuntimeAssets) -> Self {
        let quads = assets
            .model_templates()
            .iter()
            .map(|t| u64::from(t.quad_count))
            .max()
            .unwrap_or(0);
        let offset_prefix = assets
            .model_templates()
            .iter()
            .enumerate()
            .any(|(id, _)| assets.model_random_offset(id as u32).is_some());
        let part = model_part_bound(quads, offset_prefix);
        let mut chain = 0_u64;
        let mut max_parts = 1_u64;
        for template in assets.model_templates() {
            chain += 1;
            max_parts = max_parts.max(chain);
            if template.flags & assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
                chain = 0;
            }
        }
        let fallback_model = max_parts * part;
        let mut model_templates = vec![0; assets.model_templates().len()];
        let mut chain_bytes = 0;
        for (index, template) in assets.model_templates().iter().enumerate().rev() {
            let continuation = if template.flags & assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT != 0 {
                chain_bytes
            } else {
                0
            };
            chain_bytes =
                model_part_bound(u64::from(template.quad_count), offset_prefix) + continuation;
            model_templates[index] = chain_bytes;
        }
        Self {
            model_templates: model_templates.into_boxed_slice(),
            fallback_model,
            compatibility_cube: if crate::compatibility_model_faces_enabled() {
                compatibility_cube_bound()
            } else {
                0
            },
            compatibility_compact: if crate::compatibility_compact_faces_enabled() {
                compatibility_compact_bound()
            } else {
                0
            },
        }
    }

    /// Reserves the largest model the source palette can emit, including selected alternatives.
    #[must_use]
    pub fn for_sub_chunk(
        &self,
        source: &SubChunk,
        assets: &RuntimeAssets,
        mode: NetworkIdMode,
    ) -> u64 {
        let model_bytes = source
            .storages()
            .iter()
            .flat_map(|storage| storage.palette().values())
            .filter_map(|&id| {
                let visual = assets.resolve(mode, id);
                let index = visual.model_template()? as usize;
                let selection = assets
                    .model_templates()
                    .get(index)
                    .map(|template| model_template_selection(visual.kind(), template.flags));
                Some(match (self.model_templates.get(index), selection) {
                    (Some(&bound), Some(ModelTemplateSelection::Fixed)) => bound,
                    _ => self.fallback_model,
                })
            })
            .max()
            .unwrap_or(0);
        // The compatibility conversion is transactional: a chunk emits either the ordinary
        // cube stream or the synthetic model stream, never both. Reserving both paths at once
        // overstates every heavy chunk and reduces mesh-worker concurrency. The replacement
        // bound below is the larger of the two and therefore still covers a conversion that
        // has to fall back to cubes.
        let terrain = if self.compatibility_compact != 0 {
            plain_bound() - plain_cube_bound() + self.compatibility_compact
        } else if self.compatibility_cube != 0 {
            plain_bound() - plain_cube_bound() + self.compatibility_cube
        } else {
            plain_bound()
        };
        terrain + BLOCKS_PER_SUB_CHUNK as u64 * MAX_SELECTED_MODEL_TEMPLATES as u64 * model_bytes
    }

    /// What `overlays` cell overlays can add to a sub-chunk's mesh: each draws one model.
    #[must_use]
    pub const fn for_overlays(&self, overlays: usize) -> u64 {
        overlays as u64 * self.fallback_model
    }
}

/// Counts owned packed streams and biome words, including empty mesh allocations.
#[must_use]
pub fn mesh_output_byte_len(mesh: &ChunkMesh, biome: &PackedBiomeRecord) -> u64 {
    bytes::<ChunkMesh>(1)
        + bytes::<CubeStreams>(1)
        + bytes::<ModelDrawRefs>(1)
        + bytes::<PackedQuad>(mesh.cube_quads().len() as u64)
        + bytes::<PackedQuadLighting>(mesh.cube_lighting().len() as u64)
        + bytes::<PackedModelRef>(mesh.model_refs().len() as u64)
        + bytes::<PackedQuadLighting>(mesh.model_lighting().len() as u64)
        + bytes::<PackedModelDrawRef>(
            (mesh.model_draw_refs().len() + mesh.transparent_model_draw_refs().len()) as u64,
        )
        + bytes::<crate::CompatFace>(mesh.compat_faces().len() as u64)
        + bytes::<PackedLiquidQuad>(mesh.liquid_quads().len() as u64)
        + bytes::<PackedQuadLighting>(mesh.liquid_lighting().len() as u64)
        + bytes::<DiagnosticGeometryCount>(mesh.diagnostic_geometry().entries().len() as u64)
        + biome.byte_len()
}

/// Charges one model reference, optional displacement and every quad's packed output.
fn model_part_bound(quads: u64, offset_prefix: bool) -> u64 {
    bytes::<PackedModelRef>(1)
        + if offset_prefix {
            bytes::<PackedQuadLighting>(2)
        } else {
            0
        }
        + quads * (bytes::<PackedQuadLighting>(1) + bytes::<PackedModelDrawRef>(1))
}

/// Reserves every exposed cube face rewritten as one model ref, one lighting
/// record and one draw record. The heavy path emits these instead of the cube
/// stream, so admission replaces the cube portion of [`plain_bound`] with this.
fn compatibility_cube_bound() -> u64 {
    BLOCKS_PER_SUB_CHUNK as u64
        * Face::ALL.len() as u64
        * (bytes::<PackedModelRef>(1)
            + bytes::<PackedQuadLighting>(1)
            + bytes::<PackedModelDrawRef>(1))
}

/// One compact compatibility face: header plus four inline light samples. Keep the
/// reservation at least as large as the cube fallback and the full model-ref fallback,
/// because mixed chunks and conversion failures can use either route.
fn compatibility_compact_bound() -> u64 {
    let compact = u64::from(
        u32::try_from(std::mem::size_of::<crate::CompatFace>()).unwrap_or(u32::MAX),
    );
    let cube = bytes::<PackedQuad>(1) + bytes::<PackedQuadLighting>(1);
    let model = bytes::<PackedModelRef>(1)
        + bytes::<PackedQuadLighting>(1)
        + bytes::<PackedModelDrawRef>(1);
    BLOCKS_PER_SUB_CHUNK as u64 * Face::ALL.len() as u64 * compact.max(cube).max(model)
}

/// The cube-quad and cube-lighting portion of [`plain_bound`]. A heavy chunk
/// drops exactly this part when the transactional conversion succeeds.
fn plain_cube_bound() -> u64 {
    BLOCKS_PER_SUB_CHUNK as u64
        * Face::ALL.len() as u64
        * (bytes::<PackedQuad>(1) + bytes::<PackedQuadLighting>(1))
}

/// Reserves all cube and liquid faces, plus the largest packed biome descriptor.
fn plain_bound() -> u64 {
    bytes::<ChunkMesh>(1)
        + bytes::<CubeStreams>(1)
        + bytes::<ModelDrawRefs>(1)
        + bytes::<DiagnosticGeometryCount>(MAX_DIAGNOSTIC_IDENTITIES_PER_MESH as u64)
        + bytes::<u32>(MAX_PACKED_BIOME_RECORD_WORDS as u64)
        + plain_cube_bound()
        + BLOCKS_PER_SUB_CHUNK as u64
            * Face::ALL.len() as u64
            * (bytes::<PackedLiquidQuad>(1) + bytes::<PackedQuadLighting>(1))
}

/// Converts an owned stream length to bytes without depending on wire layout literals.
const fn bytes<T>(count: u64) -> u64 {
    count * std::mem::size_of::<T>() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_payload_is_charged_even_without_geometry() {
        let mut mesh = ChunkMesh::default();
        let biome = PackedBiomeRecord::fallback();
        let empty_bytes = mesh_output_byte_len(&mesh, &biome);
        mesh.cube_streams.diagnostic_geometry = crate::DiagnosticGeometrySummary::from_counts(
            (0..MAX_DIAGNOSTIC_IDENTITIES_PER_MESH)
                .map(|id| DiagnosticGeometryCount::new(None, id as u32, 1)),
        );
        assert_eq!(
            mesh_output_byte_len(&mesh, &biome) - empty_bytes,
            bytes::<DiagnosticGeometryCount>(MAX_DIAGNOSTIC_IDENTITIES_PER_MESH as u64)
        );
        assert!(mesh_output_byte_len(&mesh, &biome) <= plain_bound());
    }
}
