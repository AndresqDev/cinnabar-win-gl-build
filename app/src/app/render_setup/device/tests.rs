use super::*;

#[test]
fn vulkan_minimum_storage_limit_is_accepted() {
    assert!(
        meets_requirements(capabilities(
            REQUIRED_DOWNLEVEL_FLAGS,
            4,
            wgpu::Backend::Vulkan
        ))
        .is_ok()
    );
}

/// Supplies native-independent capabilities to the production candidate filter.
fn capabilities(
    flags: wgpu::DownlevelFlags,
    storage_buffers: u32,
    backend: wgpu::Backend,
) -> AdapterCapabilities {
    AdapterCapabilities {
        backend,
        flags,
        limits: wgpu::Limits {
            max_storage_buffers_per_shader_stage: storage_buffers,
            ..Default::default()
        },
        angle: false,
    }
}

/// Supplies the capabilities wgpu reports for ANGLE's GLES 3.1/D3D11 adapter.
fn angle_capabilities(flags: wgpu::DownlevelFlags, storage_buffers: u32) -> AdapterCapabilities {
    AdapterCapabilities {
        backend: wgpu::Backend::Gl,
        flags,
        limits: wgpu::Limits {
            max_storage_buffers_per_shader_stage: storage_buffers,
            ..Default::default()
        },
        angle: true,
    }
}

/// Supplies the exact contract-row capabilities the D3D11 backend advertises.
fn dx11_capabilities(flags: wgpu::DownlevelFlags) -> AdapterCapabilities {
    AdapterCapabilities {
        backend: wgpu::Backend::Dx11,
        flags,
        limits: wgpu::Limits {
            max_storage_buffers_per_shader_stage: 0,
            max_uniform_buffers_per_shader_stage: 7,
            max_samplers_per_shader_stage: 8,
            ..Default::default()
        },
        angle: false,
    }
}

#[test]
fn adapters_must_support_every_renderer_capability_and_storage_limit() {
    let required = render::minimum_vertex_storage_buffers();
    assert!(
        meets_requirements(capabilities(
            REQUIRED_DOWNLEVEL_FLAGS,
            required,
            wgpu::Backend::Dx12
        ))
        .is_ok()
    );
    for flag in REQUIRED_DOWNLEVEL_FLAGS.iter() {
        assert!(
            meets_requirements(capabilities(
                REQUIRED_DOWNLEVEL_FLAGS - flag,
                required,
                wgpu::Backend::Dx12
            ))
            .is_err()
        );
    }
    assert!(
        meets_requirements(capabilities(
            REQUIRED_DOWNLEVEL_FLAGS,
            required - 1,
            wgpu::Backend::Dx12
        ))
        .is_err()
    );
}

#[test]
fn dx12_requires_alternate_texture_views_but_gl_flattens_them() {
    let view_formats = wgpu::DownlevelFlags::VIEW_FORMATS;
    let required = render::required_vertex_storage_buffers();
    assert!(REQUIRED_DOWNLEVEL_FLAGS.contains(view_formats));
    let flags = wgpu::DownlevelFlags::all() - view_formats;
    let error = meets_requirements(capabilities(flags, required, wgpu::Backend::Dx12)).unwrap_err();
    assert!(error.to_string().contains("VIEW_FORMATS"));
    assert!(meets_requirements(capabilities(flags, required, wgpu::Backend::Gl)).is_ok());
}

#[test]
fn gl_still_requires_every_other_renderer_capability() {
    let required = render::minimum_vertex_storage_buffers();
    let gl = capabilities(REQUIRED_DOWNLEVEL_FLAGS, required, wgpu::Backend::Gl);
    let flags = required_downlevel_flags(&gl);
    for flag in flags.iter() {
        assert!(
            meets_requirements(capabilities(flags - flag, required, wgpu::Backend::Gl)).is_err()
        );
    }
}

#[test]
fn angle_gl_does_not_require_sample_rate_shading() {
    let required = render::minimum_vertex_storage_buffers();
    let flags = REQUIRED_DOWNLEVEL_FLAGS
        - wgpu::DownlevelFlags::VIEW_FORMATS
        - wgpu::DownlevelFlags::MULTISAMPLED_SHADING;
    assert!(meets_requirements(angle_capabilities(flags, required)).is_ok());
}

#[test]
fn angle_gl_still_requires_sixteen_byte_binding_alignment() {
    let required = render::minimum_vertex_storage_buffers();
    let flags = REQUIRED_DOWNLEVEL_FLAGS
        - wgpu::DownlevelFlags::VIEW_FORMATS
        - wgpu::DownlevelFlags::MULTISAMPLED_SHADING
        - wgpu::DownlevelFlags::BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED;
    let error = meets_requirements(angle_capabilities(flags, required)).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED")
    );
}

#[test]
fn dx11_accepts_the_contract_flags_without_storage_views_or_msaa() {
    let required = DX11_REQUIRED_DOWNLEVEL_FLAGS;
    assert!(meets_requirements(dx11_capabilities(required)).is_ok());

    // The shared profile's storage, view-format, MSAA and 16-byte-alignment requirements do
    // not apply to the D3D11 contract.
    for absent in [
        wgpu::DownlevelFlags::VERTEX_STORAGE,
        wgpu::DownlevelFlags::FRAGMENT_STORAGE,
        wgpu::DownlevelFlags::VIEW_FORMATS,
        wgpu::DownlevelFlags::MULTISAMPLED_SHADING,
        wgpu::DownlevelFlags::BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED,
    ] {
        assert!(!required.contains(absent), "{absent:?}");
    }
    // Storage is zero in the D3D11 contract; the native terrain storage minimum must not
    // reject the only adapter the legacy profile can use.
    assert_eq!(
        dx11_capabilities(required)
            .limits
            .max_storage_buffers_per_shader_stage,
        0
    );
    assert!(meets_requirements(dx11_capabilities(required)).is_ok());
}

#[test]
fn dx11_requires_each_contract_flag() {
    let required = DX11_REQUIRED_DOWNLEVEL_FLAGS;
    for flag in required.iter() {
        let error = meets_requirements(dx11_capabilities(required - flag)).unwrap_err();
        assert!(error.to_string().contains(&format!("{flag:?}")), "{error}");
    }
}

#[test]
fn dx11_device_requirements_use_adapter_values_and_never_raise_storage() {
    let mut settings = WgpuSettings::default();
    settings.priority = WgpuSettingsPriority::WebGL2;
    settings.limits.max_storage_buffers_per_shader_stage = 4;
    settings.limits.max_uniform_buffers_per_shader_stage = 12;
    settings.limits.max_samplers_per_shader_stage = 16;
    settings.limits.max_texture_dimension_2d = 8192;

    let adapter_limits = wgpu::Limits {
        max_storage_buffers_per_shader_stage: 0,
        max_uniform_buffers_per_shader_stage: 7,
        max_samplers_per_shader_stage: 8,
        max_texture_dimension_2d: 16384,
        ..wgpu::Limits::default()
    };
    let adapter_features = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    let (features, limits) = device_requirements_for(
        wgpu::Backend::Dx11,
        adapter_features,
        &adapter_limits,
        wgpu::DeviceType::IntegratedGpu,
        &settings,
    );
    assert_eq!(features, adapter_features);
    assert_eq!(limits.max_storage_buffers_per_shader_stage, 0);
    assert_eq!(limits.max_uniform_buffers_per_shader_stage, 7);
    assert_eq!(limits.max_samplers_per_shader_stage, 8);
    assert_eq!(limits.max_texture_dimension_2d, 16384);
}

#[test]
fn configured_storage_limits_cover_the_renderer_without_lowering_existing_limits() {
    let required = render::required_vertex_storage_buffers();
    for initial in [0, required, required + 1] {
        let mut settings = WgpuSettings::default();
        settings.limits.max_storage_buffers_per_shader_stage = initial;
        render::device_settings::configure(&mut settings);
        assert_eq!(
            settings.limits.max_storage_buffers_per_shader_stage,
            initial.max(required)
        );
    }
}

/// A window wider than 8192 px must not fail target creation on an adapter that supports it.
#[test]
fn window_texture_limits_rise_to_the_adapter_and_never_fall() {
    let adapter = wgpu::Limits {
        max_texture_dimension_1d: 16384,
        max_texture_dimension_2d: 16384,
        ..wgpu::Limits::default()
    };
    let mut limits = wgpu::Limits::default();
    raise_window_texture_limits(&mut limits, &adapter);
    assert_eq!(limits.max_texture_dimension_1d, 16384);
    assert_eq!(limits.max_texture_dimension_2d, 16384);
    let requested = limits.clone();
    raise_window_texture_limits(&mut limits, &wgpu::Limits::downlevel_webgl2_defaults());
    assert_eq!(
        limits.max_texture_dimension_2d,
        requested.max_texture_dimension_2d
    );
}

/// wgpu drops the DX12 backend when its DXC cannot load; only that case retries with FXC.
#[cfg(windows)]
#[test]
fn dx12_retries_with_fxc_only_when_its_dxc_cannot_load() {
    let settings = WgpuSettings::default();
    let fxc = wgpu::Dx12Compiler::Fxc;
    let with_fxc = instance(Backend::Dx12, &settings, fxc.clone(), None);
    if !has_dx12(&with_fxc) {
        eprintln!("missing fixture: a DX12 runtime");
        return;
    }
    assert!(!dxc_failed(Backend::Dx12, &fxc, &with_fxc));
    let missing = wgpu::Dx12Compiler::DynamicDxc {
        dxc_path: std::env::temp_dir()
            .join("cinnabar-missing-dxc")
            .join("dxcompiler.dll")
            .to_string_lossy()
            .into_owned(),
    };
    let without_dxc = instance(Backend::Dx12, &settings, missing.clone(), None);
    assert!(dxc_failed(Backend::Dx12, &missing, &without_dxc));
    assert!(!dxc_failed(Backend::Vulkan, &missing, &without_dxc));
}
