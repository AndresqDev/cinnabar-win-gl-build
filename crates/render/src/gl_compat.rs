//! Compatibility policy for the native OpenGL backend.
//!
//! wgpu's GL/GLES backend cannot create the alternate sRGB/linear texture views Cinnabar uses.
//! The GL fallback therefore renders the main scene in one sRGB attachment and flattens every
//! other alternate view to its base format. This module is the single place that decides which
//! compatibility adaptations are active.

use bevy::render::render_resource::TextureFormat;
use std::sync::OnceLock;

static GL_COMPAT: OnceLock<bool> = OnceLock::new();

/// Records the backend chosen for this process. Called once before renderer resources are built.
pub fn install(gl: bool) {
    let _ = GL_COMPAT.set(gl);
}

/// Whether the active renderer is the native OpenGL compatibility path.
pub fn enabled() -> bool {
    *GL_COMPAT.get().unwrap_or(&false)
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
}
