//! Compatibility policy shared by the OpenGL and D3D11 backends.
//!
//! wgpu's GL/GLES backend cannot create the alternate sRGB/linear texture views Cinnabar uses.
//! The compatibility path therefore renders the main scene in one sRGB attachment and flattens
//! every other alternate view to its base format. D3D11 needs those shared adaptations too, but
//! not the GL/ANGLE-specific context lock, shader-def and array-layout workarounds. This module
//! keeps the two concepts apart: [`enabled`] selects the shared legacy policy, [`gl`] selects
//! GL-only hacks, and [`dx11`] identifies the D3D11 path.

use bevy::render::render_resource::TextureFormat;
use std::sync::OnceLock;

/// The renderer backend whose compatibility policy is installed for this process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompatBackend {
    /// Native backend with alternate views and native pipeline compilation.
    None,
    /// OpenGL or ANGLE/GLES.
    Gl,
    /// Direct3D 11.
    Dx11,
}

/// Explicit capability table for the legacy renderer profiles.
///
/// The plan names the D3D11 row `Dx11Legacy`; [`DX11_LEGACY`] is that row. Each row records
/// which capability group applies, instead of leaving the decisions spread across backend
/// checks:
///
/// - `legacy_common`: single encoded scene target, no HDR/Enhanced, MSAA or alternate views;
/// - `gl_only`: padded arrays, `GL_COMPAT`, the ANGLE compile context and GL fences;
/// - `dx11_only`: no depth pyramid, compute, storage textures, timestamps or pipeline cache;
/// - `native_only`: advanced features the legacy path must not request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dx11Legacy {
    /// Adaptations shared by GL/ANGLE and D3D11.
    pub legacy_common: bool,
    /// Adaptations that are valid only for GL/ANGLE.
    pub gl_only: bool,
    /// Restrictions that are valid only for D3D11.
    pub dx11_only: bool,
    /// Native-only capabilities.
    pub native_only: bool,
}

impl Dx11Legacy {
    /// The GL/ANGLE row: shared legacy adaptations plus GL-only workarounds.
    pub const GL: Self = Self {
        legacy_common: true,
        gl_only: true,
        dx11_only: false,
        native_only: false,
    };

    /// The D3D11 row: shared legacy adaptations plus D3D11-only restrictions.
    pub const DX11: Self = Self {
        legacy_common: true,
        gl_only: false,
        dx11_only: true,
        native_only: false,
    };

    /// The native row: no legacy adaptations and advanced capabilities enabled.
    pub const NATIVE: Self = Self {
        legacy_common: false,
        gl_only: false,
        dx11_only: false,
        native_only: true,
    };

    /// Whether the backend shares the legacy scene, view and gamma adaptations.
    pub const fn enabled(self) -> bool {
        self.legacy_common
    }

    /// Whether the backend is GL/ANGLE.
    pub const fn gl(self) -> bool {
        self.gl_only
    }

    /// Whether the backend is D3D11.
    pub const fn dx11(self) -> bool {
        self.dx11_only
    }

    /// Whether the backend has native-only capabilities enabled.
    pub const fn native(self) -> bool {
        self.native_only
    }
}

/// The explicit D3D11 legacy row.
pub const DX11_LEGACY: Dx11Legacy = Dx11Legacy::DX11;

impl CompatBackend {
    /// Selects the capability-table row for this backend.
    pub const fn profile(self) -> Dx11Legacy {
        match self {
            Self::None => Dx11Legacy::NATIVE,
            Self::Gl => Dx11Legacy::GL,
            Self::Dx11 => Dx11Legacy::DX11,
        }
    }

    /// Whether the backend shares the legacy scene, view and gamma adaptations.
    fn enabled(self) -> bool {
        self.profile().enabled()
    }

    /// Whether the backend is GL/ANGLE.
    fn gl(self) -> bool {
        self.profile().gl()
    }

    /// Whether the backend is D3D11.
    fn dx11(self) -> bool {
        self.profile().dx11()
    }
}

/// Opt-in switch for ANGLE's separate EGL compile context.
pub const COMPILE_CONTEXT_ENV: &str = "CINNABAR_ANGLE_COMPILE_CONTEXT";

/// Marker wgpu's GLES adapter appends to `AdapterInfo::driver_info` when the shared
/// compile context exists.
pub const COMPILE_CONTEXT_MARKER: &str = "angle-compile-context";

/// Marker wgpu's GLES adapter appends when no shared compile context was created.
pub const COMPILE_CONTEXT_UNAVAILABLE_MARKER: &str = "angle-compile-context-unavailable";

static BACKEND: OnceLock<CompatBackend> = OnceLock::new();
static COMPILE_CONTEXT: OnceLock<bool> = OnceLock::new();

/// Records the compatibility backend for this process. Called once before renderer resources
/// are built.
///
/// Pipeline compilation stays synchronous because this entry point runs before an adapter
/// exists and therefore cannot confirm that the separate compile context was created. Renderer
/// startup uses [`install_with_compile_context`] once the adapter reports its marker.
pub fn install_backend(backend: CompatBackend) {
    install_policy(backend, false);
}

/// Compatibility wrapper for callers that only know whether GL is selected.
///
/// `true` selects [`CompatBackend::Gl`]; `false` selects [`CompatBackend::None`].
pub fn install(gl: bool) {
    install_backend(from_gl(gl));
}

/// Records the backend and whether ANGLE may compile on its separate context.
///
/// The caller reads [`COMPILE_CONTEXT_ENV`] once so every later decision sees the same mode.
pub fn install_with_compile_context(gl: bool, compile_context: bool) {
    install_policy(from_gl(gl), compile_context);
}

fn from_gl(gl: bool) -> CompatBackend {
    if gl {
        CompatBackend::Gl
    } else {
        CompatBackend::None
    }
}

/// The public entry points converge here, so the backend is the single source of truth and
/// the compile context can only exist on GL.
fn install_policy(backend: CompatBackend, compile_context: bool) {
    let _ = BACKEND.set(backend);
    let _ = COMPILE_CONTEXT.set(backend.gl() && compile_context);
}

fn backend() -> CompatBackend {
    *BACKEND.get().unwrap_or(&CompatBackend::None)
}

/// The installed capability-table row.
///
/// Backend-specific code should ask this table instead of testing individual backends, so the
/// shared legacy adaptations cannot leak into a GL-only or D3D11-only decision.
pub fn profile() -> Dx11Legacy {
    backend().profile()
}

/// Whether the active backend needs the shared legacy compatibility adaptations.
///
/// True for GL and D3D11. Those backends flatten alternate texture views and keep the encoded
/// scene/gamma formats; backend-specific workarounds must use [`gl`] or [`dx11`] instead.
pub fn enabled() -> bool {
    profile().enabled()
}

/// Whether the active backend is GL/ANGLE.
///
/// GL-only workarounds include the single-context scheduling, the `GL_COMPAT` shader def and
/// the padded array layouts.
pub fn gl() -> bool {
    profile().gl()
}

/// Whether the active backend is D3D11.
pub fn dx11() -> bool {
    profile().dx11()
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
/// Bevy's worker compilation. The `gl` argument is the GL selection, not [`enabled`], which is
/// also true for D3D11.
pub fn synchronous_pipeline_compilation(gl: bool) -> bool {
    synchronous_pipeline_compilation_for(gl, compile_context_enabled())
}

fn synchronous_pipeline_compilation_for(gl: bool, compile_context: bool) -> bool {
    gl && !compile_context
}

/// Flattens alternate texture views to the base format on compatibility backends.
pub(crate) fn view_formats(formats: &'static [TextureFormat]) -> &'static [TextureFormat] {
    view_formats_for(formats, backend())
}

/// Flattens a requested view format to the base format on compatibility backends.
pub(crate) fn view_format(format: Option<TextureFormat>) -> Option<TextureFormat> {
    view_format_for(format, backend())
}

/// Storage format for the shared main-pass colour target.
pub(crate) fn scene_storage_format(format: TextureFormat) -> TextureFormat {
    scene_storage_format_for(format, backend())
}

/// Colour-target format for pipelines that would normally write the encoded scene view.
pub(crate) fn gamma_target_format(format: TextureFormat) -> TextureFormat {
    gamma_target_format_for(format, backend())
}

/// GL picks a texture target from the layer count, ignoring the requested view dimension.
/// Array textures therefore need at least two layers and must not look like a cube map.
pub fn array_storage_layers(square: bool, layers: u32) -> u32 {
    array_storage_layers_for(square, layers, backend())
}

fn array_storage_layers_for(square: bool, layers: u32, backend: CompatBackend) -> u32 {
    if !backend.gl() {
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
    array_pixels_for(square, layers, pixels, backend())
}

fn array_pixels_for<'a>(
    square: bool,
    layers: u32,
    pixels: &'a [u8],
    backend: CompatBackend,
) -> std::borrow::Cow<'a, [u8]> {
    if !backend.gl() || layers == 0 {
        return std::borrow::Cow::Borrowed(pixels);
    }
    let storage = array_storage_layers_for(square, layers, backend);
    if storage == layers {
        return std::borrow::Cow::Borrowed(pixels);
    }
    let layer_bytes = pixels.len() / layers as usize;
    let mut padded = Vec::with_capacity(layer_bytes * storage as usize);
    padded.extend_from_slice(pixels);
    padded.resize(layer_bytes * storage as usize, 0);
    std::borrow::Cow::Owned(padded)
}

fn view_formats_for(
    formats: &'static [TextureFormat],
    backend: CompatBackend,
) -> &'static [TextureFormat] {
    if backend.enabled() { &[] } else { formats }
}

fn view_format_for(format: Option<TextureFormat>, backend: CompatBackend) -> Option<TextureFormat> {
    if backend.enabled() { None } else { format }
}

fn scene_storage_format_for(format: TextureFormat, backend: CompatBackend) -> TextureFormat {
    if backend.enabled() {
        format
    } else {
        format.remove_srgb_suffix()
    }
}

fn gamma_target_format_for(format: TextureFormat, backend: CompatBackend) -> TextureFormat {
    if backend.enabled() {
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
    fn backends_separate_shared_compatibility_from_gl_only_policy() {
        assert!(!CompatBackend::None.enabled());
        assert!(!CompatBackend::None.gl());
        assert!(!CompatBackend::None.dx11());

        assert!(CompatBackend::Gl.enabled());
        assert!(CompatBackend::Gl.gl());
        assert!(!CompatBackend::Gl.dx11());

        assert!(CompatBackend::Dx11.enabled());
        assert!(!CompatBackend::Dx11.gl());
        assert!(CompatBackend::Dx11.dx11());
    }

    #[test]
    fn dx11_legacy_row_is_explicit_and_has_no_gl_or_native_capabilities() {
        assert_eq!(
            DX11_LEGACY,
            Dx11Legacy {
                legacy_common: true,
                gl_only: false,
                dx11_only: true,
                native_only: false,
            }
        );
        assert_eq!(CompatBackend::Dx11.profile(), DX11_LEGACY);
        assert!(DX11_LEGACY.enabled());
        assert!(!DX11_LEGACY.gl());
        assert!(DX11_LEGACY.dx11());
        assert!(!DX11_LEGACY.native());
    }

    #[test]
    fn gl_legacy_row_keeps_shared_and_gl_only_workarounds() {
        let profile = CompatBackend::Gl.profile();
        assert_eq!(profile, Dx11Legacy::GL);
        assert!(profile.enabled());
        assert!(profile.gl());
        assert!(!profile.dx11());
        assert!(!profile.native());
    }

    #[test]
    fn native_row_has_no_legacy_adaptations() {
        let profile = CompatBackend::None.profile();
        assert_eq!(profile, Dx11Legacy::NATIVE);
        assert!(!profile.enabled());
        assert!(!profile.gl());
        assert!(!profile.dx11());
        assert!(profile.native());
    }

    #[test]
    fn gl_bool_entry_points_map_to_compat_backends() {
        assert_eq!(from_gl(true), CompatBackend::Gl);
        assert_eq!(from_gl(false), CompatBackend::None);
    }

    #[test]
    fn compatibility_backends_flatten_view_formats_and_formats() {
        for backend in [CompatBackend::Gl, CompatBackend::Dx11] {
            assert_eq!(view_formats_for(&[RAW], backend), &[]);
            assert_eq!(view_format_for(Some(RAW), backend), None);
            assert_eq!(scene_storage_format_for(ENCODED, backend), ENCODED);
            assert_eq!(gamma_target_format_for(ENCODED, backend), ENCODED);
        }
    }

    #[test]
    fn native_backend_keeps_alternate_views_and_linear_storage() {
        let backend = CompatBackend::None;
        assert_eq!(view_formats_for(&[RAW], backend), &[RAW]);
        assert_eq!(view_format_for(Some(RAW), backend), Some(RAW));
        assert_eq!(scene_storage_format_for(ENCODED, backend), RAW);
        assert_eq!(gamma_target_format_for(ENCODED, backend), RAW);
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
        assert_eq!(array_storage_layers_for(true, 1, CompatBackend::Gl), 2);
        assert_eq!(array_storage_layers_for(false, 1, CompatBackend::Gl), 2);
        assert_eq!(array_storage_layers_for(true, 6, CompatBackend::Gl), 7);
        assert_eq!(array_storage_layers_for(true, 12, CompatBackend::Gl), 13);
        assert_eq!(array_storage_layers_for(false, 6, CompatBackend::Gl), 6);
        assert_eq!(array_storage_layers_for(true, 5, CompatBackend::Gl), 5);
    }

    #[test]
    fn dx11_and_native_array_storage_keeps_the_requested_layers() {
        for backend in [CompatBackend::None, CompatBackend::Dx11] {
            assert_eq!(array_storage_layers_for(true, 1, backend), 1);
            assert_eq!(array_storage_layers_for(false, 1, backend), 1);
            assert_eq!(array_storage_layers_for(true, 6, backend), 6);
            assert_eq!(array_storage_layers_for(false, 6, backend), 6);
        }
    }

    #[test]
    fn only_gl_pads_array_pixels() {
        let pixels = [1, 2, 3, 4];
        assert!(matches!(
            array_pixels_for(true, 1, &pixels, CompatBackend::Dx11),
            std::borrow::Cow::Borrowed(_)
        ));
        assert!(matches!(
            array_pixels_for(true, 1, &pixels, CompatBackend::None),
            std::borrow::Cow::Borrowed(_)
        ));
        assert_eq!(
            &*array_pixels_for(true, 1, &pixels, CompatBackend::Gl),
            [1u8, 2, 3, 4, 0, 0, 0, 0].as_slice()
        );
    }
}
