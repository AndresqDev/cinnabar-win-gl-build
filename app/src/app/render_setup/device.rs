//! Instance, adapter and device requirements for renderer startup.

use anyhow::{Context, Result};
use bevy::render::settings::{WgpuSettings, WgpuSettingsPriority};
use launcher::graphics_backend::{Adapter, Backend};

/// Converts the policy's backend to wgpu's flags without keeping a separate order table.
pub(super) fn flags(backend: Backend) -> wgpu::Backends {
    match backend {
        Backend::Vulkan => wgpu::Backends::VULKAN,
        Backend::Dx12 => wgpu::Backends::DX12,
        Backend::Dx11 => wgpu::Backends::DX11,
        Backend::Gl => wgpu::Backends::GL,
        Backend::Metal => wgpu::Backends::METAL,
    }
}

/// Filters the single policy order by the compiled or explicitly requested backend mask.
pub(super) fn backends(mask: wgpu::Backends) -> Vec<Backend> {
    Backend::AVAILABLE
        .iter()
        .copied()
        .filter(|backend| mask.contains(flags(*backend)))
        .collect()
}

/// Copies diagnostic identity from the selected adapter rather than inferring a GPU from its name.
pub(super) fn identity(adapter: &wgpu::Adapter) -> Adapter {
    let info = adapter.get_info();
    let driver_version = info.driver.clone();
    #[cfg(any(windows, target_os = "linux", target_os = "android"))]
    let driver_version = if info.backend == wgpu::Backend::Vulkan {
        // SAFETY: This read-only properties query borrows the live adapter and its instance.
        if let Some(native) = unsafe { adapter.as_hal::<wgpu::hal::api::Vulkan>() } {
            let properties = unsafe {
                native
                    .shared_instance()
                    .raw_instance()
                    .get_physical_device_properties(native.raw_physical_device())
            };
            if cfg!(windows) && info.vendor == launcher::graphics_backend::INTEL_PCI_VENDOR {
                format!(
                    "{}.{}",
                    properties.driver_version >> 14,
                    properties.driver_version & 0x3fff
                )
            } else {
                properties.driver_version.to_string()
            }
        } else {
            driver_version
        }
    } else {
        driver_version
    };
    Adapter {
        name: info.name.clone(),
        vendor: info.vendor,
        device: info.device,
        driver: info.driver.clone(),
        driver_info: info.driver_info.clone(),
        driver_version,
        integrated: info.device_type == wgpu::DeviceType::IntegratedGpu,
    }
}

/// Flushes each startup boundary to the file without waiting for terminal output.
pub(super) fn log(message: std::fmt::Arguments<'_>) {
    diagnostics::console::console().write_synced(
        diagnostics::console::Stream::Stderr,
        format!("{message}\n").into_bytes(),
    );
}

/// Applies the existing automatic feature selection and constrained device limits.
fn device_requirements(
    adapter: &wgpu::Adapter,
    settings: &WgpuSettings,
) -> (wgpu::Features, wgpu::Limits) {
    let info = adapter.get_info();
    device_requirements_for(
        info.backend,
        adapter.features(),
        &adapter.limits(),
        info.device_type,
        settings,
    )
}

/// Selects the requested features and limits for one adapter.
///
/// D3D11 always takes the adapter's own limits and features. Its contract advertises zero
/// storage buffers and exactly the uniform/sampler slots the HAL implements, so requesting
/// Cinnabar's native storage minimum would make `request_device` fail before the legacy
/// profile can run. The other backends keep Bevy's priority rules and storage minimum.
fn device_requirements_for(
    backend: wgpu::Backend,
    adapter_features: wgpu::Features,
    adapter_limits: &wgpu::Limits,
    device_type: wgpu::DeviceType,
    settings: &WgpuSettings,
) -> (wgpu::Features, wgpu::Limits) {
    let mut features = wgpu::Features::empty();
    let mut limits = settings.limits.clone();
    if backend == wgpu::Backend::Dx11
        || matches!(settings.priority, WgpuSettingsPriority::Functionality)
    {
        features = adapter_features;
        limits = adapter_limits.clone();
        if device_type == wgpu::DeviceType::DiscreteGpu {
            features.remove(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);
        }
    }
    raise_window_texture_limits(&mut limits, adapter_limits);
    limits.max_storage_buffers_per_shader_stage = limits
        .max_storage_buffers_per_shader_stage
        .min(adapter_limits.max_storage_buffers_per_shader_stage);
    if let Some(disabled) = settings.disabled_features {
        features.remove(disabled);
    }
    features |= settings.features;
    if let Some(constrained) = &settings.constrained_limits {
        limits = limits.or_worse_values_from(constrained);
    }
    (features, limits)
}

/// Window-sized targets follow the window, which can span monitors beyond the 8192 default, so
/// every priority takes the adapter's texture dimensions.
fn raise_window_texture_limits(limits: &mut wgpu::Limits, adapter: &wgpu::Limits) {
    limits.max_texture_dimension_1d = limits
        .max_texture_dimension_1d
        .max(adapter.max_texture_dimension_1d);
    limits.max_texture_dimension_2d = limits
        .max_texture_dimension_2d
        .max(adapter.max_texture_dimension_2d);
}

/// Builds exactly one backend with the given DX12 shader compiler and the shipping display policy.
pub(super) fn instance(
    backend: Backend,
    settings: &WgpuSettings,
    dx12_shader_compiler: wgpu::Dx12Compiler,
    display: Option<Box<dyn wgpu::wgt::WgpuHasDisplayHandle>>,
) -> wgpu::Instance {
    let mut descriptor = wgpu::InstanceDescriptor {
        backends: flags(backend),
        flags: settings.instance_flags,
        memory_budget_thresholds: settings.instance_memory_budget_thresholds,
        // wgpu does not apply backend environment overrides itself; this keeps
        // WGPU_GL_FENCE_BEHAVIOR (AutoFinish on the ANGLE launcher) effective.
        backend_options: wgpu::BackendOptions::from_env_or_default(),
        display,
    };
    #[cfg(target_os = "macos")]
    if env_compat::var("CINNABAR_ASYNC_PRESENT").as_deref() != Ok("0") {
        descriptor
            .flags
            .insert(wgpu::InstanceFlags::METAL_ASYNC_WINDOWED_PRESENTATION);
    }
    descriptor.backend_options.dx12.shader_compiler = dx12_shader_compiler;
    descriptor.backend_options.dx12.presentation_system =
        wgpu::wgt::Dx12SwapchainKind::from_env().unwrap_or_default();
    descriptor.backend_options.dx12.latency_waitable_object =
        wgpu::wgt::Dx12UseFrameLatencyWaitableObject::from_env().unwrap_or_default();
    descriptor.backend_options.gl.gles_minor_version = settings.gles3_minor_version;
    wgpu::Instance::new(descriptor)
}

/// Whether a DX12 instance needs FXC instead: wgpu leaves out the DX12 backend, logging only at
/// debug level, when the DXC it was given fails to load.
pub(super) fn dxc_failed(
    backend: Backend,
    dx12_shader_compiler: &wgpu::Dx12Compiler,
    instance: &wgpu::Instance,
) -> bool {
    backend == Backend::Dx12
        && !matches!(dx12_shader_compiler, wgpu::Dx12Compiler::Fxc)
        && !has_dx12(instance)
}

#[cfg(windows)]
fn has_dx12(instance: &wgpu::Instance) -> bool {
    // SAFETY: Only the backend's presence is read; nothing is used or destroyed through it.
    unsafe { instance.as_hal::<wgpu::hal::api::Dx12>() }.is_some()
}

#[cfg(not(windows))]
fn has_dx12(_: &wgpu::Instance) -> bool {
    false
}

/// Reads the adapter-name restriction shared by all backend probes.
pub(super) fn requested_adapter_name(settings: &WgpuSettings) -> Option<String> {
    std::env::var("WGPU_ADAPTER_NAME")
        .ok()
        .or_else(|| settings.adapter_name.clone())
}

/// Preserves the environment override for software-adapter selection.
pub(super) fn force_fallback(settings: &WgpuSettings) -> bool {
    std::env::var("WGPU_FORCE_FALLBACK_ADAPTER").map_or(settings.force_fallback_adapter, |value| {
        !(value.is_empty() || value == "0" || value == "false")
    })
}

/// Opt-in one-time capability dump for adapter compatibility reports.
fn diagnostics_requested() -> bool {
    env_compat::var_os("CINNABAR_RENDER_DIAGNOSTICS").is_some_and(|value| value == "1")
}

/// Logs the exact capability matrix the renderer selected, before device creation can fail.
fn log_adapter_capabilities(
    adapter: &wgpu::Adapter,
    features: &wgpu::Features,
    limits: &wgpu::Limits,
) {
    if !diagnostics_requested() {
        return;
    }
    let info = adapter.get_info();
    let angle = is_angle(&info);
    let downlevel = adapter.get_downlevel_capabilities();
    log(format_args!(
        "Renderer capabilities: name={:?} backend={:?} angle={} driver={:?} driver_info={:?} \
         shader_model={:?} downlevel={:?} adapter_features={:?} requested_features={:?} \
         limits={:?}",
        info.name,
        info.backend,
        angle,
        info.driver,
        info.driver_info,
        downlevel.shader_model,
        downlevel.flags,
        adapter.features(),
        features,
        limits,
    ));
    let formats = [
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba16Float,
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureFormat::Depth24PlusStencil8,
    ];
    let described: Vec<String> = formats
        .into_iter()
        .map(|format| {
            let format_features = adapter.get_texture_format_features(format);
            format!(
                "{format:?}: usages={:?} flags={:?}",
                format_features.allowed_usages, format_features.flags
            )
        })
        .collect();
    log(format_args!(
        "Renderer format support: {}",
        described.join("; ")
    ));
}

/// Logs the D3D11 backend, adapter and the HAL's real feature level.
///
/// `AdapterInfo` has no feature-level field, so the value comes from the native HAL adapter
/// instead of being synthesized from the shader model or the adapter name.
pub(super) fn log_dx11_identity(adapter: &wgpu::Adapter) {
    let info = adapter.get_info();
    if info.backend != wgpu::Backend::Dx11 {
        return;
    }
    #[cfg(windows)]
    let feature_level = {
        // SAFETY: This read-only query borrows the live adapter; the native adapter is not
        // retained or used to create resources.
        unsafe { adapter.as_hal::<wgpu::hal::api::Dx11>() }
            .map_or("unknown", |native| native.feature_level_name())
    };
    #[cfg(not(windows))]
    let feature_level = "unavailable";
    log(format_args!(
        "Renderer DX11 backend: adapter={:?} vendor={:#06x} device={:#06x} feature_level={feature_level}",
        info.name, info.vendor, info.device
    ));
}

/// Logs the GL environment controls that change device and fence behavior.
fn log_render_settings(settings: &WgpuSettings) {
    if !diagnostics_requested() {
        return;
    }
    let fence = wgpu::wgt::GlFenceBehavior::from_env().unwrap_or_default();
    log(format_args!(
        "Renderer GL settings: gles_minor={:?} fence={fence:?}",
        settings.gles3_minor_version
    ));
}

/// Opens a device for a candidate that already passed renderer capability checks.
pub(super) async fn request_device(
    adapter: &wgpu::Adapter,
    settings: &WgpuSettings,
) -> Result<(wgpu::Device, wgpu::Queue)> {
    let (features, limits) = device_requirements(adapter, settings);
    log_adapter_capabilities(adapter, &features, &limits);
    log_render_settings(settings);
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: settings.device_label.as_deref(),
            required_features: features,
            required_limits: limits,
            // SAFETY: These are the same experimental features enabled by Bevy's renderer.
            experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
            memory_hints: settings.memory_hints.clone(),
            trace: wgpu::Trace::Off,
        })
        .await
        .context("create renderer device")
}

/// Capabilities needed by terrain draws, texture views, material reads and MSAA shadows.
const REQUIRED_DOWNLEVEL_FLAGS: wgpu::DownlevelFlags = wgpu::DownlevelFlags::BASE_VERTEX
    .union(wgpu::DownlevelFlags::VERTEX_STORAGE)
    .union(wgpu::DownlevelFlags::FRAGMENT_STORAGE)
    .union(wgpu::DownlevelFlags::BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED)
    .union(wgpu::DownlevelFlags::VIEW_FORMATS)
    .union(wgpu::DownlevelFlags::NON_POWER_OF_TWO_MIPMAPPED_TEXTURES)
    .union(wgpu::DownlevelFlags::MULTISAMPLED_SHADING);

/// Capabilities the D3D11 contract actually advertises.
///
/// D3D11 has no vertex/fragment storage, alternate texture views or MSAA in this backend, and
/// its downlevel flags do not include the 16-byte binding-alignment workaround. The legacy
/// profile therefore requires only these real capabilities; the shared flags above stay in
/// force for native and GL/ANGLE adapters.
const DX11_REQUIRED_DOWNLEVEL_FLAGS: wgpu::DownlevelFlags = wgpu::DownlevelFlags::BASE_VERTEX
    .union(wgpu::DownlevelFlags::NON_POWER_OF_TWO_MIPMAPPED_TEXTURES)
    .union(wgpu::DownlevelFlags::CUBE_ARRAY_TEXTURES)
    .union(wgpu::DownlevelFlags::COMPARISON_SAMPLERS)
    .union(wgpu::DownlevelFlags::INDEPENDENT_BLEND)
    .union(wgpu::DownlevelFlags::FULL_DRAW_INDEX_UINT32)
    .union(wgpu::DownlevelFlags::DEPTH_BIAS_CLAMP);

/// Whether an adapter is ANGLE's GLES translator rather than a native OpenGL driver.
fn is_angle(info: &wgpu::AdapterInfo) -> bool {
    info.driver_info.contains("ANGLE") || info.name.starts_with("ANGLE")
}

/// A candidate's capability snapshot, independent of native device creation.
pub(super) struct AdapterCapabilities {
    backend: wgpu::Backend,
    flags: wgpu::DownlevelFlags,
    limits: wgpu::Limits,
    /// ANGLE exposes GLES rather than desktop GL. Its D3D11 translator reports
    /// GLES 3.1 capabilities, notably without sample-rate shading.
    angle: bool,
}

impl From<&wgpu::Adapter> for AdapterCapabilities {
    /// Reads the capabilities without opening a device.
    fn from(adapter: &wgpu::Adapter) -> Self {
        let info = adapter.get_info();
        Self {
            backend: info.backend,
            flags: adapter.get_downlevel_capabilities().flags,
            limits: adapter.limits(),
            angle: is_angle(&info),
        }
    }
}

/// The D3D11 profile uses only the capabilities the contract advertises; GL flattens alternate
/// views, so it does not require VIEW_FORMATS either.
fn required_downlevel_flags(adapter: &AdapterCapabilities) -> wgpu::DownlevelFlags {
    if adapter.backend == wgpu::Backend::Dx11 {
        return DX11_REQUIRED_DOWNLEVEL_FLAGS;
    }
    let required = if adapter.backend == wgpu::Backend::Gl {
        REQUIRED_DOWNLEVEL_FLAGS - wgpu::DownlevelFlags::VIEW_FORMATS
    } else {
        REQUIRED_DOWNLEVEL_FLAGS
    };
    let required = if adapter.backend == wgpu::Backend::Gl {
        // The legacy compatibility path forces single-sample rendering.
        required - wgpu::DownlevelFlags::MULTISAMPLED_SHADING
    } else {
        required
    };
    if adapter.angle {
        // The GL compatibility path forces single-sample rendering and skips
        // the two shaders that read sample_index. The 16-byte alignment flag is
        // still required: wgpu rejects small uniform bindings without it, and
        // the pinned ANGLE patch restores it for Windows.
        required - wgpu::DownlevelFlags::MULTISAMPLED_SHADING
    } else {
        required
    }
}

/// Rejects candidates lacking the capabilities or limits used by the renderer.
pub(super) fn meets_requirements(adapter: impl Into<AdapterCapabilities>) -> Result<()> {
    let adapter = adapter.into();
    let missing = required_downlevel_flags(&adapter) - adapter.flags;
    anyhow::ensure!(
        missing.is_empty(),
        "GPU lacks renderer capabilities: {missing:?}"
    );
    if adapter.backend == wgpu::Backend::Dx11 {
        return Ok(());
    }
    anyhow::ensure!(
        adapter.limits.max_storage_buffers_per_shader_stage
            >= render::minimum_vertex_storage_buffers(),
        "GPU supports {} storage buffers per shader stage; terrain requires at least {}",
        adapter.limits.max_storage_buffers_per_shader_stage,
        render::minimum_vertex_storage_buffers()
    );
    Ok(())
}

#[cfg(test)]
mod tests;
