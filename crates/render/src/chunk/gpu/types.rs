use crate::chunk::*;
use meshing::liquid::{TRANSPARENT_WATER_DRAW_FLAG, TRANSPARENT_WATER_RECORD_FLAG};
use render_scene::{RuntimeStageProfiler, VisibilityDiagnosticsInput};

pub(in crate::chunk) const MODEL_INDEX_COUNT: u32 = 6;

#[derive(Component, Clone)]
pub(in crate::chunk) struct GpuChunkAllocation {
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) generation: u64,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
    pub(in crate::chunk) quad_range: Range<u32>,
    pub(in crate::chunk) cube_layout: CubeQuadLayout, // validated against the GPU material table
    pub(in crate::chunk) cube_lighting_range: Option<Range<u32>>,
    pub(in crate::chunk) model_range: Option<Range<u32>>,
    pub(in crate::chunk) model_lighting_range: Option<Range<u32>>,
    pub(in crate::chunk) model_draw_range: Option<Range<u32>>,
    pub(in crate::chunk) transparent_model_draw_range: Option<Range<u32>>,
    pub(in crate::chunk) compat_faces_range: Option<Range<u32>>,
    pub(in crate::chunk) liquid_range: Option<Range<u32>>,
    pub(in crate::chunk) liquid_lighting_range: Option<Range<u32>>,
    pub(in crate::chunk) has_depth_liquid: bool,
    pub(in crate::chunk) has_transparent_liquid: bool,
    pub(in crate::chunk) depth_liquid_range: Option<Range<u32>>,
    /// Whether the transparent water blends the same in any face order and shares its
    /// sub-chunk with no transparent model; such water is drawn without a sort.
    pub(in crate::chunk) order_independent_liquid: bool,
    pub(in crate::chunk) metadata_index: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::chunk) struct StreamAddresses {
    pub(in crate::chunk) cube: Option<Range<u32>>,
    pub(in crate::chunk) cube_lighting: Option<Range<u32>>,
    pub(in crate::chunk) model: Option<Range<u32>>,
    pub(in crate::chunk) model_lighting: Option<Range<u32>>,
    pub(in crate::chunk) model_draw: Option<Range<u32>>,
    pub(in crate::chunk) transparent_model_draw: Option<Range<u32>>,
    pub(in crate::chunk) compat_faces: Option<Range<u32>>,
    pub(in crate::chunk) liquid: Option<Range<u32>>,
    pub(in crate::chunk) liquid_lighting: Option<Range<u32>>,
}

pub(in crate::chunk) fn direct_stream_addresses(
    allocation: &GpuChunkAllocation,
) -> StreamAddresses {
    StreamAddresses {
        cube: (!allocation.quad_range.is_empty()).then(|| allocation.quad_range.clone()),
        cube_lighting: allocation.cube_lighting_range.clone(),
        model: allocation.model_range.clone(),
        model_lighting: allocation.model_lighting_range.clone(),
        model_draw: allocation.model_draw_range.clone(),
        transparent_model_draw: allocation.transparent_model_draw_range.clone(),
        compat_faces: allocation.compat_faces_range.clone(),
        liquid: allocation.liquid_range.clone(),
        liquid_lighting: allocation.liquid_lighting_range.clone(),
    }
}

/// Points every liquid record at its sub-chunk's origin slot. Meshing stores each record's own
/// ordinal in this word as its lighting index; shaders derive lighting from the origin's liquid
/// bases instead, so every draw route reads the origin from the record rather than encoding it in
/// the draw.
pub(in crate::chunk) fn stamp_liquid_metadata_index(
    liquid_quads: &mut [[u32; 4]],
    metadata_index: u32,
) {
    for words in liquid_quads {
        words[3] = metadata_index;
    }
}

pub(in crate::chunk) fn mdi_stream_addresses(allocation: &GpuChunkAllocation) -> StreamAddresses {
    direct_stream_addresses(allocation)
}

pub(in crate::chunk) fn cube_lighting_record_address(
    addresses: &StreamAddresses,
    global_quad: u32,
) -> Option<u32> {
    if !cube_stream_addresses_valid(addresses) {
        return None;
    }
    let cube = addresses.cube.as_ref()?;
    let lighting = addresses.cube_lighting.as_ref()?;
    if global_quad < cube.start || global_quad >= cube.end || !lighting.start.is_multiple_of(2) {
        return None;
    }
    let local = global_quad.checked_sub(cube.start)?;
    let record = lighting.start.checked_div(2)?.checked_add(local)?;
    (record < lighting.end.checked_div(2)?).then_some(record)
}

pub(in crate::chunk) fn cube_stream_addresses_valid(addresses: &StreamAddresses) -> bool {
    match (addresses.cube.as_ref(), addresses.cube_lighting.as_ref()) {
        (None, None) => true,
        (Some(cube), Some(lighting)) => {
            !cube.is_empty()
                && !lighting.is_empty()
                && lighting.start.is_multiple_of(2)
                && lighting.end.is_multiple_of(2)
                && cube.end.checked_sub(cube.start)
                    == lighting
                        .end
                        .checked_sub(lighting.start)
                        .and_then(|words| words.checked_div(2))
        }
        _ => false,
    }
}

pub(in crate::chunk) fn shared_stream_ranges_disjoint(addresses: &StreamAddresses) -> bool {
    let ranges = [
        addresses.model.as_ref(),
        addresses.model_lighting.as_ref(),
        addresses.model_draw.as_ref(),
        addresses.transparent_model_draw.as_ref(),
        addresses.compat_faces.as_ref(),
        addresses.liquid.as_ref(),
        addresses.liquid_lighting.as_ref(),
        addresses.cube_lighting.as_ref(),
    ];
    ranges.iter().enumerate().all(|(index, left)| {
        left.is_none_or(|left| {
            !left.is_empty()
                && ranges[index + 1..]
                    .iter()
                    .flatten()
                    .all(|right| left.end <= right.start || right.end <= left.start)
        })
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) enum ChunkDrawMode {
    Direct,
    MultiDrawIndirect,
    Unsupported,
}

/// Qualcomm's PCI vendor ID, reported by Adreno adapters.
const QUALCOMM_VENDOR_ID: u32 = 0x5143;
/// The proprietary driver with empty indirect terrain draws on tested Adreno GPUs.
const QUALCOMM_VULKAN_DRIVER: &str = "Qualcomm Technologies Inc. Adreno Vulkan Driver";

/// Intel's PCI vendor ID.
const INTEL_VENDOR_ID: u32 = 0x8086;
/// The first Intel Windows driver branch (`24.x`, 2018) not known to fail indirect terrain on DX12.
/// HD Graphics 520 with `23.20.16.4905` faulted in the counted GPU-cull draws and drew no
/// opaque terrain with CPU-culled indirect draws.
const INTEL_DX12_INDIRECT_DRIVER_MAJOR: u32 = 24;
/// Forces direct terrain draws for diagnostics and for adapters whose indirect path is not
/// stable. Entity preprocessing is switched by the camera implementation.
const FORCE_DIRECT_DRAWS_ENV: &str = "CINNABAR_FORCE_DIRECT_DRAWS";
/// CPU culling is only meaningful when the matching draw path is direct, so requesting it
/// also opts out of GPU culling and indirect submission.
const CPU_CULLING_ENV: &str = "CINNABAR_CPU_CULLING";

/// Keeps Apple expansion costs, proprietary Adreno Vulkan failures, legacy Intel DX12
/// driver failures and Haswell Intel integrated GPUs on direct terrain draws.
pub(in crate::chunk) fn direct_draws_required(info: &wgpu::AdapterInfo) -> bool {
    force_direct_draws()
        || cpu_culling_requested()
        || apple_gpu_identity(info.vendor, &info.driver, &info.name)
        || (info.backend == wgpu::Backend::Vulkan
            && info.vendor == QUALCOMM_VENDOR_ID
            && info.driver == QUALCOMM_VULKAN_DRIVER)
        || legacy_intel_dx12_driver(info)
        || intel_haswell_integrated(info)
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        !matches!(
            value.to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "off" | "no"
        )
    })
}

/// Reads the explicit diagnostic escape hatch without making indirect/indirect-count
/// behaviour depend on an unknown driver feature.
fn force_direct_draws() -> bool {
    env_truthy(FORCE_DIRECT_DRAWS_ENV)
}

fn cpu_culling_requested() -> bool {
    env_truthy(CPU_CULLING_ENV)
}

/// Haswell integrated graphics have documented incomplete Vulkan support and a history of
/// indirect/MDI misrendering. The HD 4400 is the primary compatibility target; matching the
/// family also keeps the closely related Haswell parts on the same conservative route.
fn intel_haswell_integrated(info: &wgpu::AdapterInfo) -> bool {
    if info.vendor != INTEL_VENDOR_ID {
        return false;
    }
    // Haswell mobile/desktop GT1-GT3 PCI IDs. The HD 4400 is 0x0A16; keep its close relatives
    // on the same path because the driver and Vulkan limitations are shared.
    const HASWELL_DEVICE_IDS: &[u32] = &[
        0x0402, 0x0412, 0x0416, 0x041E, 0x0422, 0x0426, 0x042A, 0x042E, 0x0A02, 0x0A06, 0x0A0E,
        0x0A12, 0x0A16, 0x0A1E, 0x0A22, 0x0A26, 0x0A2A, 0x0A2E, 0x0A32, 0x0A36, 0x0D02, 0x0D06,
        0x0D0E, 0x0D12, 0x0D16, 0x0D1A, 0x0D1E, 0x0D22, 0x0D26,
    ];
    if HASWELL_DEVICE_IDS.contains(&info.device) {
        return true;
    }
    let name = info.name.to_ascii_lowercase();
    name.contains("hd graphics 4400")
        || name.contains("hd graphics 4600")
        || name.contains("hd graphics 5000")
        || name.contains("iris graphics 5100")
        || name.contains("iris pro graphics 5200")
        || name.contains("haswell")
}

/// Recognizes a DX12 Intel adapter whose dotted driver version predates the `24.x` branch.
/// An unparsable version, or the `0.0.0.0` wgpu reports when the query fails, keeps the
/// adapter's advertised indirect path.
fn legacy_intel_dx12_driver(info: &wgpu::AdapterInfo) -> bool {
    info.backend == wgpu::Backend::Dx12
        && info.vendor == INTEL_VENDOR_ID
        && info
            .driver
            .split('.')
            .next()
            .and_then(|major| major.trim().parse::<u32>().ok())
            .is_some_and(|major| (1..INTEL_DX12_INDIRECT_DRIVER_MAJOR).contains(&major))
}

/// Recognizes Apple hardware through its vendor ID or Vulkan driver and adapter names.
pub(in crate::chunk) fn apple_gpu_identity(vendor: u32, driver: &str, name: &str) -> bool {
    const APPLE_VENDOR_ID: u32 = 0x106B;
    vendor == APPLE_VENDOR_ID || driver == "Honeykrisp" || name.starts_with("Apple ")
}

pub(in crate::chunk) fn select_chunk_draw_mode(
    downlevel_flags: DownlevelFlags,
    features: WgpuFeatures,
    backend: Backends,
    direct_required: bool,
) -> ChunkDrawMode {
    if !downlevel_flags.contains(DownlevelFlags::BASE_VERTEX) {
        ChunkDrawMode::Unsupported
    } else if downlevel_flags.contains(DownlevelFlags::INDIRECT_EXECUTION)
        && features.contains(WgpuFeatures::INDIRECT_FIRST_INSTANCE)
        // Metal firmware lockups attributed to this client also stall
        // WindowServer. Keep the equivalent CPU-validated direct draws on
        // Metal until indirect submission has passed a native stability gate.
        && !backend.contains(Backends::METAL)
        && !direct_required
    {
        ChunkDrawMode::MultiDrawIndirect
    } else {
        ChunkDrawMode::Direct
    }
}

#[cfg(test)]
mod direct_draw_tests {
    use super::*;

    /// Supplies adapter-owned identity independently of which optional features it advertises.
    fn adapter(vendor: u32, backend: wgpu::Backend, driver: &str) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: "terrain fixture adapter".into(),
            vendor,
            driver: driver.into(),
            ..wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, backend)
        }
    }

    fn named_adapter(
        name: &str,
        vendor: u32,
        backend: wgpu::Backend,
        driver: &str,
    ) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: name.into(),
            ..adapter(vendor, backend, driver)
        }
    }

    #[test]
    fn proprietary_adreno_vulkan_preserves_terrain_with_direct_draws() {
        let flags = DownlevelFlags::BASE_VERTEX | DownlevelFlags::INDIRECT_EXECUTION;
        for features in [
            WgpuFeatures::INDIRECT_FIRST_INSTANCE,
            WgpuFeatures::INDIRECT_FIRST_INSTANCE | WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT,
        ] {
            let info = adapter(
                QUALCOMM_VENDOR_ID,
                wgpu::Backend::Vulkan,
                QUALCOMM_VULKAN_DRIVER,
            );
            assert_eq!(
                select_chunk_draw_mode(
                    flags,
                    features,
                    Backends::VULKAN,
                    direct_draws_required(&info)
                ),
                ChunkDrawMode::Direct,
            );
        }
    }

    #[test]
    fn legacy_intel_dx12_drivers_draw_terrain_directly() {
        let flags = DownlevelFlags::BASE_VERTEX | DownlevelFlags::INDIRECT_EXECUTION;
        let features =
            WgpuFeatures::INDIRECT_FIRST_INSTANCE | WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT;
        for driver in ["23.20.16.4905", "21.20.16.4590"] {
            let info = adapter(INTEL_VENDOR_ID, wgpu::Backend::Dx12, driver);
            assert_eq!(
                select_chunk_draw_mode(
                    flags,
                    features,
                    Backends::DX12,
                    direct_draws_required(&info)
                ),
                ChunkDrawMode::Direct,
                "{driver}",
            );
        }
    }

    #[test]
    fn haswell_hd_4400_draws_terrain_directly() {
        let flags = DownlevelFlags::BASE_VERTEX | DownlevelFlags::INDIRECT_EXECUTION;
        let features =
            WgpuFeatures::INDIRECT_FIRST_INSTANCE | WgpuFeatures::MULTI_DRAW_INDIRECT_COUNT;
        for name in [
            "Intel(R) HD Graphics 4400 (HSW GT2)",
            "Intel HD Graphics 4600",
            "Intel(R) Iris(TM) Pro Graphics 5200",
        ] {
            let info = named_adapter(name, INTEL_VENDOR_ID, wgpu::Backend::Dx11, "31.0.101.2135");
            assert!(direct_draws_required(&info), "{name}");
            assert_eq!(
                select_chunk_draw_mode(
                    flags,
                    features,
                    Backends::DX11,
                    direct_draws_required(&info)
                ),
                ChunkDrawMode::Direct,
                "{name}",
            );
        }
        let mut info = named_adapter(
            "Intel(R) HD Graphics Family",
            INTEL_VENDOR_ID,
            wgpu::Backend::Dx11,
            "31.0.101.2135",
        );
        info.device = 0x0A16;
        assert!(direct_draws_required(&info));
    }

    #[test]
    fn other_drivers_keep_their_supported_indirect_draw_path() {
        for info in [
            adapter(INTEL_VENDOR_ID, wgpu::Backend::Dx12, "31.0.101.2135"),
            adapter(INTEL_VENDOR_ID, wgpu::Backend::Dx12, "24.20.100.6025"),
            adapter(INTEL_VENDOR_ID, wgpu::Backend::Dx12, ""),
            adapter(INTEL_VENDOR_ID, wgpu::Backend::Dx12, "0.0.0.0"),
            adapter(INTEL_VENDOR_ID, wgpu::Backend::Vulkan, "23.20.16.4905"),
            adapter(0x1002, wgpu::Backend::Dx12, "23.20.16.4905"),
            adapter(QUALCOMM_VENDOR_ID, wgpu::Backend::Vulkan, "Mesa Turnip"),
            adapter(
                QUALCOMM_VENDOR_ID,
                wgpu::Backend::Dx12,
                QUALCOMM_VULKAN_DRIVER,
            ),
            adapter(0x1002, wgpu::Backend::Vulkan, QUALCOMM_VULKAN_DRIVER),
        ] {
            assert!(!direct_draws_required(&info));
            assert_eq!(
                select_chunk_draw_mode(
                    DownlevelFlags::BASE_VERTEX | DownlevelFlags::INDIRECT_EXECUTION,
                    WgpuFeatures::INDIRECT_FIRST_INSTANCE,
                    Backends::from(info.backend),
                    direct_draws_required(&info),
                ),
                ChunkDrawMode::MultiDrawIndirect,
            );
        }
    }
}

pub(in crate::chunk) const fn diagnostic_draw_mode(draw_mode: ChunkDrawMode) -> OpaqueDrawMode {
    match draw_mode {
        ChunkDrawMode::Direct => OpaqueDrawMode::Direct,
        ChunkDrawMode::MultiDrawIndirect => OpaqueDrawMode::MultiDrawIndirect,
        ChunkDrawMode::Unsupported => OpaqueDrawMode::Unsupported,
    }
}

pub(in crate::chunk) fn opaque_allocation_is_drawable(allocation: &GpuChunkAllocation) -> bool {
    cube_stream_drawable(allocation)
        || model_direct_draw_command(allocation).is_some()
        || depth_liquid_direct_draw_command(allocation).is_some()
}

pub(in crate::chunk) fn extracted_camera_identity(
    main_entity: &MainEntity,
    view: &ExtractedView,
) -> ExtractedCameraIdentity {
    let world_from_view = view.world_from_view.to_matrix();
    let clip_from_world = view
        .clip_from_world
        .unwrap_or(view.clip_from_view * world_from_view.inverse());
    ExtractedCameraIdentity {
        stable_id: main_entity.id().to_bits(),
        pose_hash: hash_f32_words(world_from_view.to_cols_array()),
        frustum_hash: hash_f32_words(clip_from_world.to_cols_array()),
    }
}

#[derive(SystemParam)]
pub(in crate::chunk) struct QueueFrameProbeParams<'w> {
    pub(in crate::chunk) frame_probe: Res<'w, ActiveFrameProbe>,
    pub(in crate::chunk) input: Res<'w, VisibilityDiagnosticsInput>,
    pub(in crate::chunk) profiler: Option<Res<'w, RuntimeStageProfiler>>,
    pub(in crate::chunk) visibility_probe: Res<'w, ActiveVisibilityFrameProbe>,
    pub(in crate::chunk) camera_identity_tracker: ResMut<'w, ExtractedCameraIdentityTracker>,
}

/// Validated cube-stream instance range, the layout it honours, and the origin base vertex.
pub(in crate::chunk) fn cube_draw_base(
    allocation: &GpuChunkAllocation,
) -> Option<(Range<u32>, CubeQuadLayout, i32)> {
    let addresses = mdi_stream_addresses(allocation);
    if !cube_stream_addresses_valid(&addresses) || !shared_stream_ranges_disjoint(&addresses) {
        return None;
    }
    let cube = addresses.cube.clone()?;
    cube_lighting_record_address(&addresses, cube.start)?;
    cube_lighting_record_address(&addresses, cube.end.checked_sub(1)?)?;
    let base_vertex = metadata_base_vertex(allocation.metadata_index)?;
    let layout = if allocation.cube_layout.solid_len() <= cube.end - cube.start {
        allocation.cube_layout
    } else {
        CubeQuadLayout::default()
    };
    Some((cube, layout, base_vertex))
}

fn cube_quad_command(base_vertex: i32, quads: Range<u32>) -> DrawIndexedIndirectArgs {
    DrawIndexedIndirectArgs {
        index_count: STATIC_QUAD_INDICES.len() as u32,
        instance_count: quads.end - quads.start,
        first_index: 0,
        base_vertex,
        first_instance: quads.start,
    }
}

pub(in crate::chunk) fn cube_stream_drawable(allocation: &GpuChunkAllocation) -> bool {
    cube_draw_base(allocation).is_some()
}

/// The cube quads that keep the alpha-tested two-sided pipeline.
pub(in crate::chunk) fn cutout_indirect_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    let (cube, layout, base_vertex) = cube_draw_base(allocation)?;
    let quads = cube.start + layout.solid_len()..cube.end;
    (!quads.is_empty()).then(|| cube_quad_command(base_vertex, quads))
}

/// Draws for the solid runs whose faces can face the camera; `None` for an invalid cube stream.
pub(in crate::chunk) fn solid_indirect_commands(
    allocation: &GpuChunkAllocation,
    camera: Option<[f64; 3]>,
) -> Option<impl Iterator<Item = DrawIndexedIndirectArgs>> {
    let (cube, layout, base_vertex) = cube_draw_base(allocation)?;
    let facing = camera.map_or(meshing::FaceMask::ALL, |camera| {
        meshing::sub_chunk_facing_faces(chunk_origin(allocation.key), camera)
    });
    Some(layout.solid_runs(facing).map(move |run| {
        cube_quad_command(base_vertex, cube.start + run.start..cube.start + run.end)
    }))
}

pub(in crate::chunk) fn model_draw_command(
    allocation: &GpuChunkAllocation,
    addresses: StreamAddresses,
) -> Option<DrawIndexedIndirectArgs> {
    let model = addresses.model?;
    let model_lighting = addresses.model_lighting?;
    let model_draw = addresses.model_draw?;
    let expected_draw_start =
        if allocation.transparent_model_draw_range.as_ref() == Some(&model_draw) {
            allocation
                .model_draw_range
                .as_ref()
                .map_or(model_lighting.end, |range| range.end)
        } else {
            model_lighting.end
        };
    if model.is_empty()
        || !model.start.is_multiple_of(4)
        || !model.end.is_multiple_of(4)
        || model_lighting.is_empty()
        || !model_lighting.start.is_multiple_of(2)
        || !model_lighting.end.is_multiple_of(2)
        || model_draw.is_empty()
        || !model_draw.start.is_multiple_of(2)
        || !model_draw.end.is_multiple_of(2)
        || model.end != model_lighting.start
        || expected_draw_start != model_draw.start
    {
        return None;
    }
    let first_instance = model_draw.start.checked_div(2)?;
    let end_instance = model_draw.end.checked_div(2)?;
    let instance_count = end_instance.checked_sub(first_instance)?;
    (instance_count != 0).then_some(DrawIndexedIndirectArgs {
        index_count: MODEL_INDEX_COUNT,
        instance_count,
        first_index: 0,
        base_vertex: allocation
            .metadata_index
            .checked_mul(4)
            .and_then(|value| i32::try_from(value).ok())?,
        first_instance,
    })
}

pub(in crate::chunk) fn compat_model_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    let faces = allocation.compat_faces_range.as_ref()?;
    let words_per_face =
        (std::mem::size_of::<meshing::CompatFace>() as u32) / GEOMETRY_STREAM_WORD_BYTES as u32;
    let word_count = faces.end.checked_sub(faces.start)?;
    if word_count == 0 || !word_count.is_multiple_of(words_per_face) {
        return None;
    }
    let instance_count = word_count.checked_div(words_per_face)?;
    (instance_count != 0).then_some(DrawIndexedIndirectArgs {
        index_count: MODEL_INDEX_COUNT,
        instance_count,
        first_index: 0,
        base_vertex: allocation
            .metadata_index
            .checked_mul(4)
            .and_then(|value| i32::try_from(value).ok())?,
        // Compact draws use chunk-local face ordinals; model.wesl takes the absolute word
        // base from ChunkOrigin.stream_bases.x.
        first_instance: 0,
    })
}

pub(in crate::chunk) fn model_direct_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    if allocation.cube_layout.is_compact_faces() {
        return compat_model_draw_command(allocation);
    }
    model_draw_command(allocation, direct_stream_addresses(allocation))
}

/// Direct draw commands for the model faces that can face `camera`.
///
/// This is the model equivalent of [`solid_indirect_commands`]: the camera-facing face runs
/// are selected on the CPU and each run is submitted as a normal direct draw. It deliberately
/// does not depend on GPU culling, multi-draw-indirect or `INDIRECT_FIRST_INSTANCE`. A missing
/// camera or a layout without a compatibility face partition keeps the previous full draw.
pub(in crate::chunk) fn model_direct_draw_commands(
    allocation: &GpuChunkAllocation,
    camera: Option<[f64; 3]>,
) -> Option<Vec<DrawIndexedIndirectArgs>> {
    let full = if allocation.cube_layout.is_compact_faces() {
        compat_model_draw_command(allocation)?
    } else {
        model_draw_command(allocation, direct_stream_addresses(allocation))?
    };
    let Some(camera) = camera else {
        return Some(vec![full]);
    };
    let layout = allocation.cube_layout;
    if !layout.is_compact_faces() && !layout.is_compat_model() {
        return Some(vec![full]);
    }
    let facing = meshing::sub_chunk_facing_faces(chunk_origin(allocation.key), camera);
    let solid_count = layout.solid_len().min(full.instance_count);
    let mut runs: Vec<Range<u32>> = layout
        .solid_runs(facing)
        .filter_map(|run| {
            let start = run.start.min(solid_count);
            let end = run.end.min(solid_count);
            (start < end).then_some(start..end)
        })
        .collect();
    // Alpha-tested and two-sided model faces are not part of the directional partition. They
    // remain visible from every side and must always be appended after the solid runs.
    if solid_count < full.instance_count {
        runs.push(solid_count..full.instance_count);
    }
    Some(
        runs.into_iter()
            .map(|run| DrawIndexedIndirectArgs {
                index_count: MODEL_INDEX_COUNT,
                instance_count: run.end - run.start,
                first_index: 0,
                base_vertex: full.base_vertex,
                first_instance: full.first_instance + run.start,
            })
            .collect(),
    )
}

pub(in crate::chunk) fn model_mdi_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    if allocation.cube_layout.is_compact_faces() {
        return compat_model_draw_command(allocation);
    }
    model_draw_command(allocation, mdi_stream_addresses(allocation))
}

pub(in crate::chunk) fn transparent_model_direct_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    let mut addresses = direct_stream_addresses(allocation);
    addresses.model_draw = addresses.transparent_model_draw.clone();
    model_draw_command(allocation, addresses)
}

pub(in crate::chunk) fn model_ref_count_for_witness(allocation: &GpuChunkAllocation) -> usize {
    allocation.model_range.as_ref().map_or(0, |range| {
        range.end.saturating_sub(range.start) as usize
            / (PACKED_MODEL_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as usize
    })
}

pub(in crate::chunk) const LEGACY_FIXED_MODEL_QUADS_PER_REF: usize = 32;

pub(in crate::chunk) fn summarize_model_workload<'a>(
    allocations: impl IntoIterator<Item = &'a GpuChunkAllocation>,
) -> ModelWorkloadCount {
    allocations
        .into_iter()
        // Both model commands need a model stream; testing it first skips building their
        // stream addresses for the many sub-chunks without models.
        .filter(|allocation| {
            allocation.model_range.is_some()
                && (model_direct_draw_command(allocation).is_some()
                    || transparent_model_direct_draw_command(allocation).is_some())
        })
        .fold(ModelWorkloadCount::default(), |mut total, allocation| {
            let model_ref_count = model_ref_count_for_witness(allocation);
            let model_draw_ref_count = allocation
                .model_draw_range
                .as_ref()
                .into_iter()
                .chain(allocation.transparent_model_draw_range.as_ref())
                .fold(0_usize, |total, range| {
                    total.saturating_add(
                        range.end.saturating_sub(range.start) as usize
                            / (PACKED_MODEL_DRAW_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as usize,
                    )
                });
            total.model_ref_count = total.model_ref_count.saturating_add(model_ref_count);
            total.model_draw_ref_count = total
                .model_draw_ref_count
                .saturating_add(model_draw_ref_count);
            total.legacy_fixed_slot_quad_invocations_avoided = total
                .legacy_fixed_slot_quad_invocations_avoided
                .saturating_add(
                    model_ref_count
                        .saturating_mul(LEGACY_FIXED_MODEL_QUADS_PER_REF)
                        .saturating_sub(model_draw_ref_count),
                );
            total
        })
}

pub(in crate::chunk) fn depth_liquid_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    if !allocation.has_depth_liquid {
        return None;
    }
    allocation.liquid_lighting_range.as_ref()?;
    let liquid = allocation.depth_liquid_range.as_ref()?;
    let instance_count = liquid.end.checked_sub(liquid.start)?;
    (instance_count != 0).then_some(DrawIndexedIndirectArgs {
        index_count: STATIC_QUAD_INDICES.len() as u32,
        instance_count,
        first_index: 0,
        base_vertex: metadata_base_vertex(allocation.metadata_index)?,
        first_instance: liquid.start,
    })
}

pub(in crate::chunk) fn depth_liquid_direct_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    depth_liquid_draw_command(allocation)
}

/// Draws water records directly through the transparent pipeline at base vertex zero:
/// first_instance selects a record and carries [`TRANSPARENT_WATER_DRAW_FLAG`] and
/// [`TRANSPARENT_WATER_RECORD_FLAG`]; each record names its own origin slot.
pub(in crate::chunk) fn transparent_liquid_direct_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    if !allocation.has_transparent_liquid {
        return None;
    }
    allocation.liquid_lighting_range.as_ref()?;
    let liquid = allocation.liquid_range.as_ref()?;
    if !liquid.start.is_multiple_of(4) || !liquid.end.is_multiple_of(4) {
        return None;
    }
    let first_instance = liquid.start / 4;
    let end = allocation
        .depth_liquid_range
        .as_ref()
        .map_or(liquid.end / 4, |depth| depth.start);
    let instance_count = end.checked_sub(first_instance)?;
    // Every flagged instance must still name its record below both flag bits.
    (instance_count != 0 && end <= TRANSPARENT_WATER_RECORD_FLAG).then_some(
        DrawIndexedIndirectArgs {
            index_count: STATIC_QUAD_INDICES.len() as u32,
            instance_count,
            first_index: 0,
            base_vertex: 0,
            first_instance: first_instance
                | TRANSPARENT_WATER_DRAW_FLAG
                | TRANSPARENT_WATER_RECORD_FLAG,
        },
    )
}

pub(in crate::chunk) fn depth_liquid_mdi_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    depth_liquid_draw_command(allocation)
}

pub(in crate::chunk) fn metadata_base_vertex(metadata_index: u32) -> Option<i32> {
    metadata_index
        .checked_mul(4)
        .and_then(|value| i32::try_from(value).ok())
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(in crate::chunk) struct GpuChunkOrigin {
    pub(in crate::chunk) value: [i32; 4],
    /// The sub-chunk's first cube quad, cube lighting record, liquid record and liquid lighting
    /// record, so shaders find a quad's lighting from its own index.
    pub(in crate::chunk) stream_bases: [u32; 4],
}

const _: () = assert!(std::mem::size_of::<GpuChunkOrigin>() == CHUNK_ORIGIN_BYTES as usize);

pub(in crate::chunk) fn gpu_chunk_origin(
    origin: [i32; 3],
    biome_start: u32,
    cube_quad_base: u32,
    cube_lighting_word_start: u32,
    liquid_word_start: u32,
    liquid_lighting_word_start: u32,
    compat_word_start: Option<u32>,
) -> Option<GpuChunkOrigin> {
    if !cube_lighting_word_start.is_multiple_of(2)
        || !liquid_word_start.is_multiple_of(4)
        || !liquid_lighting_word_start.is_multiple_of(2)
    {
        return None;
    }
    let cube_base = if let Some(compat) = compat_word_start {
        // The high bit tells model.wesl to decode compact compatibility faces from this
        // word base instead of reading a PackedModelDrawRef.
        0x8000_0000 | compat
    } else {
        cube_quad_base
    };
    Some(GpuChunkOrigin {
        value: [
            origin[0],
            origin[1],
            origin[2],
            i32::try_from(biome_start).ok()?,
        ],
        stream_bases: [
            cube_base,
            cube_lighting_word_start / 2,
            liquid_word_start / 4,
            liquid_lighting_word_start / 2,
        ],
    })
}

#[cfg(test)]
pub(in crate::chunk) fn build_indexed_indirect_commands<'a>(
    allocations: impl IntoIterator<Item = &'a GpuChunkAllocation>,
) -> Vec<DrawIndexedIndirectArgs> {
    allocations
        .into_iter()
        .filter_map(cutout_indirect_command)
        .collect()
}

pub(in crate::chunk) struct ChunkIndirectBatch {
    pub(in crate::chunk) visible_entities: Vec<Entity>,
    pub(in crate::chunk) drawn_allocations: Vec<(Entity, FrameAllocationIdentity)>,
    pub(in crate::chunk) indirect_offset: u64,
    pub(in crate::chunk) command_count: u32,
    /// Commands built from `visible_entities`, kept until `stale`.
    pub(in crate::chunk) commands: Vec<DrawIndexedIndirectArgs>,
    /// The list, its allocations or its face selection changed since `commands` was built.
    pub(in crate::chunk) stale: bool,
}

impl ChunkIndirectBatch {
    /// A batch over `visible_entities` whose commands the next prepare builds.
    pub(in crate::chunk) fn new(visible_entities: Vec<Entity>) -> Self {
        Self {
            visible_entities,
            drawn_allocations: Vec::new(),
            indirect_offset: 0,
            command_count: 0,
            commands: Vec::new(),
            stale: true,
        }
    }
}

#[derive(Resource, Default)]
pub(in crate::chunk) struct ChunkIndirectBatches(
    pub(in crate::chunk) HashMap<Entity, ChunkIndirectBatch>,
);

#[derive(Resource, Default)]
pub(in crate::chunk) struct ChunkModelIndirectBatches(
    pub(in crate::chunk) HashMap<Entity, ChunkIndirectBatch>,
);

#[derive(Resource, Default)]
pub(in crate::chunk) struct ChunkDepthLiquidIndirectBatches(
    pub(in crate::chunk) HashMap<Entity, ChunkIndirectBatch>,
);

#[derive(Clone)]
pub(in crate::chunk) struct ArenaAllocation {
    pub(in crate::chunk) generation: u64,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
    pub(in crate::chunk) cube_range: Option<Range<u32>>,
    pub(in crate::chunk) cube_lighting_range: Option<Range<u32>>,
    pub(in crate::chunk) model_range: Option<Range<u32>>,
    pub(in crate::chunk) model_lighting_range: Option<Range<u32>>,
    pub(in crate::chunk) model_draw_range: Option<Range<u32>>,
    pub(in crate::chunk) transparent_model_draw_range: Option<Range<u32>>,
    pub(in crate::chunk) compat_faces_range: Option<Range<u32>>,
    pub(in crate::chunk) liquid_range: Option<Range<u32>>,
    pub(in crate::chunk) liquid_lighting_range: Option<Range<u32>>,
    pub(in crate::chunk) quad_capacity: u32,
    pub(in crate::chunk) geometry_stream_range: Option<Range<u32>>,
    pub(in crate::chunk) geometry_stream_capacity: u32,
    pub(in crate::chunk) biome_range: Range<u32>,
    pub(in crate::chunk) biome_capacity: u32,
    /// The record written to `biome_range`, or `None` for the shared fallback.
    pub(in crate::chunk) biome_record: Option<meshing::PackedBiomeRecord>,
    pub(in crate::chunk) gpu: GpuChunkAllocation,
}

#[derive(Clone)]
pub(in crate::chunk) struct RetiredArenaAllocation {
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) identity: GpuChunkAllocation,
    pub(in crate::chunk) quad: Option<(Range<u32>, u32)>,
    pub(in crate::chunk) geometry: Option<(Range<u32>, u32)>,
    pub(in crate::chunk) biome: Option<(Range<u32>, u32)>,
    pub(in crate::chunk) origin: Option<u32>,
    pub(in crate::chunk) release_epoch: Option<u64>,
}

impl RetiredArenaAllocation {
    #[cfg(test)]
    pub(in crate::chunk) fn geometry_only(
        entity: Entity,
        allocation: &ArenaAllocation,
    ) -> Option<Self> {
        Some(Self {
            entity,
            identity: allocation.gpu.clone(),
            quad: None,
            geometry: Some((
                allocation.geometry_stream_range.clone()?,
                allocation.geometry_stream_capacity,
            )),
            biome: None,
            origin: None,
            release_epoch: None,
        })
    }

    pub(in crate::chunk) fn full(entity: Entity, allocation: ArenaAllocation) -> Self {
        let metadata_index = allocation.gpu.metadata_index;
        Self {
            entity,
            identity: allocation.gpu,
            quad: allocation
                .cube_range
                .map(|range| (range, allocation.quad_capacity)),
            geometry: allocation
                .geometry_stream_range
                .map(|range| (range, allocation.geometry_stream_capacity)),
            biome: (allocation.biome_capacity != 0)
                .then_some((allocation.biome_range, allocation.biome_capacity)),
            origin: Some(metadata_index),
            release_epoch: None,
        }
    }

    pub(in crate::chunk) fn owned_bytes(&self) -> u64 {
        let quad = self
            .quad
            .as_ref()
            .map_or(0, |(_, capacity)| u64::from(*capacity) * PACKED_QUAD_BYTES);
        let geometry = self.geometry.as_ref().map_or(0, |(_, capacity)| {
            u64::from(*capacity) * GEOMETRY_STREAM_WORD_BYTES
        });
        let biome = self
            .biome
            .as_ref()
            .map_or(0, |(_, capacity)| u64::from(*capacity) * BIOME_WORD_BYTES);
        let origin = self.origin.map_or(0, |_| CHUNK_ORIGIN_BYTES);
        quad.saturating_add(geometry)
            .saturating_add(biome)
            .saturating_add(origin)
    }

    pub(in crate::chunk) fn can_release(&self, completed_epoch: u64) -> bool {
        self.release_epoch
            .is_some_and(|epoch| epoch <= completed_epoch)
    }
}

impl ArenaAllocation {
    /// This allocation's biome range and capacity when it already holds exactly `record`, so a
    /// new generation can keep it without rewriting or reallocating it.
    pub(in crate::chunk) fn reusable_biome(
        &self,
        record: &meshing::PackedBiomeRecord,
    ) -> Option<(Range<u32>, u32)> {
        self.biome_record
            .as_ref()
            .filter(|resident| resident.words() == record.words())
            .map(|_| (self.biome_range.clone(), self.biome_capacity))
    }

    pub(in crate::chunk) fn expected_streams(&self) -> ChunkStreamMask {
        let mut mask = ChunkStreamMask::default();
        if self.cube_range.is_some() || self.cube_lighting_range.is_some() {
            mask = mask | ChunkStreamMask::CUBE;
        }
        if self.model_range.is_some()
            || self.model_lighting_range.is_some()
            || self.model_draw_range.is_some()
            || self.transparent_model_draw_range.is_some()
            || self.compat_faces_range.is_some()
        {
            mask = mask | ChunkStreamMask::MODEL;
        }
        if self.liquid_range.is_some() || self.liquid_lighting_range.is_some() {
            mask = mask | ChunkStreamMask::LIQUID;
        }
        mask
    }
}
