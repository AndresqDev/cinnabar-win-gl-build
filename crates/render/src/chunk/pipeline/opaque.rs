use crate::chunk::*;
use bevy::render::render_phase::{DrawFunctionId, DrawFunctionsInternal};
use render_scene::RuntimeStage;

pub(in crate::chunk) fn install_opaque_commands(render_app: &mut SubApp) {
    render_app
        .add_render_command::<Opaque3d, super::solid::DrawSolidChunkCommands>()
        .add_render_command::<Opaque3d, super::solid::DrawSolidChunkIndirectCommands>()
        .add_render_command::<Opaque3d, DrawChunkCommands>()
        .add_render_command::<Opaque3d, DrawChunkIndirectCommands>();
}

/// Timing stage of each opaque terrain stream, indexed like the stream-generic commands.
pub(in crate::chunk) const STREAM_STAGES: [RuntimeStage; 4] = [
    RuntimeStage::GpuTerrainSolid,
    RuntimeStage::GpuTerrainCutout,
    RuntimeStage::GpuTerrainModel,
    RuntimeStage::GpuTerrainDepthLiquid,
];

/// One stream's terrain draws, split by whether they draw entirely inside the opaque node.
struct StreamDraws {
    stage: RuntimeStage,
    /// Legacy direct and indirect, then retained units.
    in_pass: [Option<DrawFunctionId>; 3],
    /// GPU-cull early and late phases; the late phase runs outside the opaque node.
    gpu_culled: [Option<DrawFunctionId>; 2],
}

/// The single registry that both timing classification and layer-probe admission read.
fn terrain_draws(functions: &DrawFunctionsInternal<Opaque3d>) -> [StreamDraws; 4] {
    use super::units::commands::DrawTerrainUnits;
    use crate::chunk::gpu_cull::DrawGpuCulledCommands;
    [
        StreamDraws {
            stage: STREAM_STAGES[0],
            in_pass: [
                functions.get_id::<super::solid::DrawSolidChunkCommands>(),
                functions.get_id::<super::solid::DrawSolidChunkIndirectCommands>(),
                functions.get_id::<DrawTerrainUnits<0>>(),
            ],
            gpu_culled: [
                functions.get_id::<DrawGpuCulledCommands<0, false>>(),
                functions.get_id::<DrawGpuCulledCommands<0, true>>(),
            ],
        },
        StreamDraws {
            stage: STREAM_STAGES[1],
            in_pass: [
                functions.get_id::<DrawChunkCommands>(),
                functions.get_id::<DrawChunkIndirectCommands>(),
                functions.get_id::<DrawTerrainUnits<1>>(),
            ],
            gpu_culled: [
                functions.get_id::<DrawGpuCulledCommands<1, false>>(),
                functions.get_id::<DrawGpuCulledCommands<1, true>>(),
            ],
        },
        StreamDraws {
            stage: STREAM_STAGES[2],
            in_pass: [
                functions.get_id::<DrawModelCommands>(),
                functions.get_id::<DrawModelIndirectCommands>(),
                functions.get_id::<DrawTerrainUnits<2>>(),
            ],
            gpu_culled: [
                functions.get_id::<DrawGpuCulledCommands<2, false>>(),
                functions.get_id::<DrawGpuCulledCommands<2, true>>(),
            ],
        },
        StreamDraws {
            stage: STREAM_STAGES[3],
            in_pass: [
                functions.get_id::<DrawDepthLiquidCommands>(),
                functions.get_id::<DrawDepthLiquidIndirectCommands>(),
                functions.get_id::<DrawTerrainUnits<3>>(),
            ],
            gpu_culled: [
                functions.get_id::<DrawGpuCulledCommands<3, false>>(),
                functions.get_id::<DrawGpuCulledCommands<3, true>>(),
            ],
        },
    ]
}

/// Identifies terrain draw functions without depending on their registration order.
pub(crate) fn timing_category(
    functions: &DrawFunctionsInternal<Opaque3d>,
    draw: DrawFunctionId,
) -> RuntimeStage {
    terrain_draws(functions)
        .into_iter()
        .find(|stream| {
            stream.in_pass.contains(&Some(draw)) || stream.gpu_culled.contains(&Some(draw))
        })
        .map_or(RuntimeStage::GpuOpaqueOther, |stream| stream.stage)
}

/// Admits terrain draws except GPU-cull streams, whose late phase is outside the opaque node.
pub(crate) fn supports_layer_probe(
    functions: &DrawFunctionsInternal<Opaque3d>,
    draw: DrawFunctionId,
) -> bool {
    terrain_draws(functions)
        .iter()
        .any(|stream| stream.in_pass.contains(&Some(draw)))
}

#[cfg(test)]
mod timing_tests {
    use super::*;
    use bevy::render::render_phase::{Draw, DrawError, DrawFunctionId, DrawFunctionsInternal};

    struct NoDraw;

    impl Draw<Opaque3d> for NoDraw {
        fn draw<'w>(
            &mut self,
            _: &'w World,
            _: &mut TrackedRenderPass<'w>,
            _: Entity,
            _: &Opaque3d,
        ) -> Result<(), DrawError> {
            Ok(())
        }
    }

    /// Registers command identity without requiring the command's runtime resources.
    fn register<T: 'static>(functions: &mut DrawFunctionsInternal<Opaque3d>) -> DrawFunctionId {
        functions.add_with::<T, _>(NoDraw)
    }

    #[test]
    fn profiling_categories_and_layer_probe_admit_every_in_pass_terrain_command() {
        use crate::chunk::gpu_cull::DrawGpuCulledCommands;
        let functions = DrawFunctions::<Opaque3d>::default();
        let mut functions = functions.write();
        let solid = [
            register::<super::super::solid::DrawSolidChunkCommands>(&mut functions),
            register::<super::super::solid::DrawSolidChunkIndirectCommands>(&mut functions),
            register::<DrawGpuCulledCommands<0, false>>(&mut functions),
            register::<DrawGpuCulledCommands<0, true>>(&mut functions),
            register::<super::super::units::commands::DrawTerrainUnits<0>>(&mut functions),
        ];
        let cutout = [
            register::<DrawChunkCommands>(&mut functions),
            register::<DrawChunkIndirectCommands>(&mut functions),
            register::<DrawGpuCulledCommands<1, false>>(&mut functions),
            register::<DrawGpuCulledCommands<1, true>>(&mut functions),
            register::<super::super::units::commands::DrawTerrainUnits<1>>(&mut functions),
        ];
        let models = [
            register::<DrawModelCommands>(&mut functions),
            register::<DrawModelIndirectCommands>(&mut functions),
            register::<DrawGpuCulledCommands<2, false>>(&mut functions),
            register::<DrawGpuCulledCommands<2, true>>(&mut functions),
            register::<super::super::units::commands::DrawTerrainUnits<2>>(&mut functions),
        ];
        let liquids = [
            register::<DrawDepthLiquidCommands>(&mut functions),
            register::<DrawDepthLiquidIndirectCommands>(&mut functions),
            register::<DrawGpuCulledCommands<3, false>>(&mut functions),
            register::<DrawGpuCulledCommands<3, true>>(&mut functions),
            register::<super::super::units::commands::DrawTerrainUnits<3>>(&mut functions),
        ];
        for (draws, expected) in [
            (solid, RuntimeStage::GpuTerrainSolid),
            (cutout, RuntimeStage::GpuTerrainCutout),
            (models, RuntimeStage::GpuTerrainModel),
            (liquids, RuntimeStage::GpuTerrainDepthLiquid),
        ] {
            for (index, draw) in draws.into_iter().enumerate() {
                assert_eq!(timing_category(&functions, draw), expected);
                // Only the GPU-cull early and late phases leave the opaque node.
                assert_eq!(
                    supports_layer_probe(&functions, draw),
                    !matches!(index, 2 | 3)
                );
            }
        }
        let other = register::<NoDraw>(&mut functions);
        assert_eq!(
            timing_category(&functions, other),
            RuntimeStage::GpuOpaqueOther
        );
        assert!(!supports_layer_probe(&functions, other));
    }
}
