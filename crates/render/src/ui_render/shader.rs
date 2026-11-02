use bevy::prelude::Shader;
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
pub(super) struct UiViewportUniform {
    pub(super) viewport_size: [f32; 2],
    /// Seconds since the UI renderer started; animates the item glint.
    pub(super) time_seconds: f32,
    pub(super) glint_strength: f32,
}

pub(crate) fn source(raw: &str) -> String {
    // Keep the GPU style bits owned by the renderer, with no UI-crate dependency or WGSL copy.
    let raw = raw.replace("UI_REPEAT_DECODER", render_api::ui_repeat::WGSL_DECODER);
    let raw = raw.replace("UI_SHADOW_DECODER", render_api::ui_shadow::WGSL);
    raw.replace(
        "UI_STYLE_MODEL_VERTEX",
        &format!("{}u", render_api::UI_STYLE_MODEL_VERTEX),
    )
    .replace(
        "UI_MODEL_LAYER_BITS",
        &format!("{}u", render_model::UI_MODEL_LAYER_BITS),
    )
    .replace(
        "// ACTOR_SHADE_CONSTANTS",
        &format!(
            "const ACTOR_SHADE: array<f32, 5> = array({});",
            render_api::ACTOR_SHADE_COEFFICIENTS
                .map(|value| format!("{value:?}"))
                .join(", ")
        ),
    )
    .replace(
        "GLINT_TINT_RGB",
        &crate::material_shader::glint_tint_source(),
    )
    .replace(
        "UI_STYLE_RGB_COVERAGE",
        &format!("{}u", render_api::UI_STYLE_RGB_COVERAGE),
    )
    .replace(
        "UI_STYLE_ALPHA_TEST",
        &format!("{}u", render_api::UI_STYLE_ALPHA_TEST),
    )
    .replace(
        "UI_STYLE_SHADOW",
        &format!("{}u", render_api::ui_shadow::UI_STYLE_SHADOW),
    )
    .replace(
        "UI_STYLE_COVERAGE_PAINT",
        &format!("{}u", render_api::UI_STYLE_COVERAGE_PAINT),
    )
    .replace(
        "UI_STYLE_GLINT",
        &format!("{}u", render_model::UI_STYLE_GLINT),
    )
    .replace(
        "UI_STYLE_SPRITE_COLOR_MASK",
        &format!("{}u", render_api::UI_STYLE_SPRITE_COLOR_MASK),
    )
    .replace(
        "UI_STYLE_COLOR_MASK",
        &format!("{}u", render_model::UI_STYLE_COLOR_MASK),
    )
    .replace(
        "UI_STYLE_ADDITIVE",
        &format!("{}u", render_model::UI_STYLE_ADDITIVE),
    )
    .replace(
        "UI_STYLE_PARTICLE",
        &format!("{}u", render_model::UI_STYLE_PARTICLE),
    )
    .replace(
        "UI_STYLE_BILINEAR",
        &format!("{}u", render_api::UI_STYLE_BILINEAR),
    )
    .replace(
        "UI_STYLE_REPEAT_X",
        &format!("{}u", render_api::ui_repeat::UI_STYLE_REPEAT_X),
    )
    .replace(
        "UI_STYLE_REPEAT_Y",
        &format!("{}u", render_api::ui_repeat::UI_STYLE_REPEAT_Y),
    )
    .replace(
        "UI_STYLE_RADIAL_GRADIENT",
        &format!("{}u", render_model::UI_STYLE_RADIAL_GRADIENT),
    )
    .replace(
        "FONT_STYLE_COVERAGE_GAMMA",
        &format!("{}u", assets::FONT_STYLE_COVERAGE_GAMMA),
    )
    .replace("FONT_STYLE_SDF", &format!("{}u", assets::FONT_STYLE_SDF))
    .replace(
        "MAX_UI_TEXTURE_LAYERS",
        &render_model::MAX_UI_TEXTURE_LAYERS.to_string(),
    )
    .replace(
        "UI_STYLE_REPEAT_PAGE",
        &format!("{}u", render_api::ui_repeat::UI_STYLE_REPEAT_PAGE),
    )
    .replace(
        "FONT_STYLE_OUTLINE_COVERAGE",
        &format!("{}u", assets::FONT_STYLE_OUTLINE_COVERAGE),
    )
    .replace(
        "OUTLINE_FILTER_MAX_SAMPLES",
        &format!(
            "{:?}",
            render_model::font_shading::OUTLINE_FILTER_MAX_SAMPLES as f32
        ),
    )
    .replace(
        "SDF_DISTANCE_SCALE",
        &format!("{:?}", render_model::font_shading::SDF_DISTANCE_SCALE),
    )
    .replace(
        "SDF_DISTANCE_BIAS",
        &format!("{:?}", render_model::font_shading::SDF_DISTANCE_BIAS),
    )
    .replace(
        "SDF_SMOOTHING_SCALE",
        &format!("{:?}", render_model::font_shading::SDF_SMOOTHING_SCALE),
    )
    .replace(
        "SDF_MIN_THRESHOLD",
        &format!("{:?}", render_model::font_shading::SDF_MIN_THRESHOLD),
    )
    .replace(
        "COVERAGE_GAMMA_BASE",
        &format!("{:?}", render_model::font_shading::COVERAGE_GAMMA_BASE),
    )
    .replace("COVERAGE_LUMINANCE", &{
        let [r, g, b] = render_model::font_shading::COVERAGE_LUMINANCE;
        format!("vec3<f32>({r:?}, {g:?}, {b:?})")
    })
}

pub(super) fn from_wgsl(raw: &str, path: impl Into<String>) -> Shader {
    let source = if crate::gl_compat::dx11() {
        source_dx11(raw)
    } else {
        source(raw)
    };
    crate::shader_safety::from_wgsl(source, path)
}

pub(super) fn from_wgsl_dx11(raw: &str, path: impl Into<String>) -> Shader {
    crate::shader_safety::from_wgsl(source_dx11(raw), path)
}

/// D3D11 has no storage buffers. The retained UI model palette is the only
/// storage binding in `ui.wgsl`; without it the menu and regular UI can still
/// render, while in-game model overlays are unavailable on this backend.
fn source_dx11(raw: &str) -> String {
    // Remove the storage-backed model palette while the source still uses the
    // symbolic style-bit names. `source` substitutes those names later; doing
    // the removal after that would leave the branch pointing at a declaration
    // that no longer exists.
    let mut text = raw.to_owned();

    if let (Some(struct_start), Some(binding_start)) = (
        text.find("struct UiModelBone {"),
        text.find("@group(0) @binding(7) var<storage, read> ui_model_bones"),
    ) {
        if let Some(newline) = text[binding_start..].find('\n') {
            text.replace_range(struct_start..binding_start + newline + 1, "");
        }
    }

    if let (Some(branch_start), Some(branch_end)) = (
        text.find("    if (style_flags & UI_STYLE_MODEL_VERTEX) != 0u {"),
        text.find("    let ndc = vec2<f32>("),
    ) {
        text.replace_range(branch_start..branch_end, "");
    }

    source(&text)
}
