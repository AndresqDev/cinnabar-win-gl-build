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

static GL_COMPAT: OnceLock<bool> = OnceLock::new();
static COMPILE_CONTEXT: OnceLock<bool> = OnceLock::new();

/// Records the backend chosen for this process. Called once before renderer resources are built.
pub fn install(gl: bool) {
    install_with_compile_context(gl, gl && compile_context_from_env());
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

/// Reads the opt-in exactly once; every value except `1` selects the synchronous fallback.
pub fn compile_context_from_env() -> bool {
    env_compat::var_os(COMPILE_CONTEXT_ENV).is_some_and(|value| value == "1")
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
