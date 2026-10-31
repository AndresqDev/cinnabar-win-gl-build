//! Compatibility policy for the OpenGL and ANGLE/GLES backends.
//!
//! wgpu's GL/GLES backend cannot create the alternate sRGB/linear texture views Cinnabar uses.
//! The GL fallback therefore renders the main scene in one sRGB attachment and flattens every
//! other alternate view to its base format. This module is the single place that decides which
//! compatibility adaptations are active.

use bevy::render::render_resource::TextureFormat;
use std::sync::OnceLock;

/// Opt-in switch for ANGLE's separate EGL compile context.
pub const COMPILE_CONTEXT_ENV: &str = "CINNABAR_ANGLE_COMPILE_CONTEXT";

/// Marker wgpu's GLES adapter appends to `AdapterInfo::driver_info` when the shared
/// compile context exists.
pub const COMPILE_CONTEXT_MARKER: &str = "angle-compile-context";

/// Marker wgpu's GLES adapter appends when no shared compile context was created.
pub const COMPILE_CONTEXT_UNAVAILABLE_MARKER: &str = "angle-compile-context-unavailable";

static GL_COMPAT: OnceLock<bool> = OnceLock::new();
static COMPILE_CONTEXT: OnceLock<bool> = OnceLock::new();

/// Records the backend chosen for this process. Called once before renderer resources are built.
///
/// Pipeline compilation stays synchronous because this entry point runs before an adapter
/// exists and therefore cannot confirm that the separate compile context was created. Renderer
/// startup uses [`install_with_compile_context`] once the adapter reports its marker.
pub fn install(gl: bool) {
    install_with_compile_context(gl, false);
}

/// Records the backend and whether ANGLE may compile on its separate context.
///
/// The caller reads [`COMPILE_CONTEXT_ENV`] once so every later decision sees the same mode.
pub fn install_with_compile_context(gl: bool, compile_context: bool) {
    let _ = GL_COMPAT.set(gl);
    let _ = COMPILE_CONTEXT.set(gl && compile_context);
}

/// Whether the active renderer is the native OpenGL compatibility path.
pub fn enabled() -> bool {
    *GL_COMPAT.get().unwrap_or(&false)
}

/// Whether GL runs pipeline compilation on ANGLE's separate EGL context.
///
/// Without it, worker compiles share the presentation context and can stall the render
/// thread, so callers must keep pipeline compilation synchronous.
pub fn compile_context_enabled() -> bool {
    *COMPILE_CONTEXT.get().unwrap_or(&false)
}

/// The stable `driver_info` marker for whether the adapter created a compile context.
pub fn compile_context_marker(has_compile_context: bool) -> &'static str {
    if has_compile_context {
        COMPILE_CONTEXT_MARKER
    } else {
        COMPILE_CONTEXT_UNAVAILABLE_MARKER
    }
}

/// Whether wgpu's GLES adapter reported a usable separate EGL compile context.
///
/// Markers are matched as whole tokens because `angle-compile-context` is a prefix
/// of `angle-compile-context-unavailable` and driver strings contain arbitrary text.
pub fn compile_context_available(driver_info: &str) -> bool {
    let mut available = false;
    for token in driver_info.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-') {
        if token == COMPILE_CONTEXT_UNAVAILABLE_MARKER {
            return false;
        }
        if token == COMPILE_CONTEXT_MARKER {
            available = true;
        }
    }
    available
}

/// Reads the opt-in exactly as wgpu's GLES patch does.
///
/// This deliberately uses `std::env::var` rather than `env_compat`: wgpu does not
/// understand the legacy prefix, so accepting it here would enable worker
/// compilation while wgpu keeps using the main context.
pub fn compile_context_from_env() -> bool {
    compile_context_requested(std::env::var(COMPILE_CONTEXT_ENV).ok().as_deref())
}

fn compile_context_requested(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true" | "TRUE"))
}

/// Whether worker compilation may use the separate context.
pub fn compile_context_active(gl: bool, requested: bool, adapter_available: bool) -> bool {
    gl && requested && adapter_available
}

/// Whether the selected backend must compile pipelines synchronously.
///
/// Only GL without its separate compile context needs serialization; other backends keep
/// Bevy's worker compilation.
pub fn synchronous_pipeline_compilation(gl: bool) -> bool {
    synchronous_pipeline_compilation_for(gl, compile_context_enabled())
}

fn synchronous_pipeline_compilation_for(gl: bool, compile_context: bool) -> bool {
    gl && !compile_context
}

/// Flattens alternate texture views to the base format on GL.
pub(crate) fn view_formats(formats: &'static [TextureFormat]) -> &'static [TextureFormat] {
    view_formats_for(formats, enabled())
}

/// Flattens a requested view format to the base format on GL.
pub(crate) fn view_format(format: Option<TextureFormat>) -> Option<TextureFormat> {
    view_format_for(format, enabled())
}

/// Storage format for the shared main-pass colour target.
pub(crate) fn scene_storage_format(format: TextureFormat) -> TextureFormat {
    scene_storage_format_for(format, enabled())
}

/// Colour-target format for pipelines that would normally write the encoded scene view.
pub(crate) fn gamma_target_format(format: TextureFormat) -> TextureFormat {
    gamma_target_format_for(format, enabled())
}

/// GL picks a texture target from the layer count, ignoring the requested view dimension.
/// Array textures therefore need at least two layers and must not look like a cube map.
pub fn array_storage_layers(square: bool, layers: u32) -> u32 {
    array_storage_layers_for(square, layers, enabled())
}

fn array_storage_layers_for(square: bool, layers: u32, gl: bool) -> u32 {
    if !gl {
        return layers;
    }
    let mut storage = layers.max(2);
    if square && storage % 6 == 0 {
        storage += 1;
    }
    storage
}

/// Pads layer-major upload data when the GL storage has extra blank layers.
pub fn array_pixels<'a>(square: bool, layers: u32, pixels: &'a [u8]) -> std::borrow::Cow<'a, [u8]> {
    if !enabled() || layers == 0 {
        return std::borrow::Cow::Borrowed(pixels);
    }
    let storage = array_storage_layers(square, layers);
    if storage == layers {
        return std::borrow::Cow::Borrowed(pixels);
    }
    let layer_bytes = pixels.len() / layers as usize;
    let mut padded = Vec::with_capacity(layer_bytes * storage as usize);
    padded.extend_from_slice(pixels);
    padded.resize(layer_bytes * storage as usize, 0);
    std::borrow::Cow::Owned(padded)
}

fn view_formats_for(formats: &'static [TextureFormat], gl: bool) -> &'static [TextureFormat] {
    if gl { &[] } else { formats }
}

fn view_format_for(format: Option<TextureFormat>, gl: bool) -> Option<TextureFormat> {
    if gl { None } else { format }
}

fn scene_storage_format_for(format: TextureFormat, gl: bool) -> TextureFormat {
    if gl {
        format
    } else {
        format.remove_srgb_suffix()
    }
}

fn gamma_target_format_for(format: TextureFormat, gl: bool) -> TextureFormat {
    if gl {
        format
    } else {
        format.remove_srgb_suffix()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: TextureFormat = TextureFormat::Rgba8Unorm;
    const ENCODED: TextureFormat = TextureFormat::Rgba8UnormSrgb;

    #[test]
    fn gl_flattens_view_formats_and_formats() {
        assert_eq!(view_formats_for(&[RAW], true), &[]);
        assert_eq!(view_format_for(Some(RAW), true), None);
        assert_eq!(scene_storage_format_for(ENCODED, true), ENCODED);
        assert_eq!(gamma_target_format_for(ENCODED, true), ENCODED);
    }

    #[test]
    fn native_backends_keep_alternate_views() {
        assert_eq!(view_formats_for(&[RAW], false), &[RAW]);
        assert_eq!(view_format_for(Some(RAW), false), Some(RAW));
        assert_eq!(scene_storage_format_for(ENCODED, false), RAW);
        assert_eq!(gamma_target_format_for(ENCODED, false), RAW);
    }

    #[test]
    fn gl_compiles_synchronously_unless_the_compile_context_is_enabled() {
        assert!(synchronous_pipeline_compilation_for(true, false));
        assert!(!synchronous_pipeline_compilation_for(true, true));
        assert!(!synchronous_pipeline_compilation_for(false, false));
        assert!(!synchronous_pipeline_compilation_for(false, true));
    }

    #[test]
    fn compile_context_marker_requires_the_whole_token() {
        assert!(compile_context_available(compile_context_marker(true)));
        assert!(!compile_context_available(compile_context_marker(false)));
        assert!(compile_context_available(
            "OpenGL ES 3.1; angle-compile-context; vendor"
        ));
        assert!(!compile_context_available(
            "angle-compile-context-unavailable"
        ));
        assert!(!compile_context_available(
            "angle-compile-context angle-compile-context-unavailable"
        ));
        assert!(!compile_context_available("OpenGL ES 3.1"));
        assert!(!compile_context_available("not-angle-compile-context"));
    }

    #[test]
    fn compile_context_env_matches_wgpu_exactly() {
        for enabled in ["1", "true", "TRUE"] {
            assert!(compile_context_requested(Some(enabled)), "{enabled}");
        }
        for disabled in ["", "0", "false", "FALSE", "True", "yes", "on", " 1"] {
            assert!(!compile_context_requested(Some(disabled)), "{disabled}");
        }
        assert!(!compile_context_requested(None));
    }

    #[test]
    fn compile_context_requires_backend_request_and_adapter_confirmation() {
        assert!(compile_context_active(true, true, true));
        assert!(!compile_context_active(false, true, true));
        assert!(!compile_context_active(true, false, true));
        assert!(!compile_context_active(true, true, false));
    }

    #[test]
    fn gl_array_storage_avoids_one_and_cube_layer_counts() {
        assert_eq!(array_storage_layers_for(true, 1, true), 2);
        assert_eq!(array_storage_layers_for(false, 1, true), 2);
        assert_eq!(array_storage_layers_for(true, 6, true), 7);
        assert_eq!(array_storage_layers_for(true, 12, true), 13);
        assert_eq!(array_storage_layers_for(false, 6, true), 6);
        assert_eq!(array_storage_layers_for(true, 5, true), 5);
        assert_eq!(array_storage_layers_for(true, 6, false), 6);
    }
}
