#!/usr/bin/env bash
# Build wgpu's GLES backend on Windows over EGL/ANGLE instead of WGL.
#
# The pinned wgpu checkout contains an EGL implementation with a Win32 surface path,
# but wgpu-hal currently excludes it on Windows. This script enables that path for
# the experimental ANGLE build. It is intentionally applied only to the CI checkout:
# the resulting Cinnabar payload ships ANGLE's libEGL/libGLESv2 next to the executable.
set -euo pipefail

checkout_root="${CARGO_HOME:-$HOME/.cargo}/git/checkouts"
egl=$(find "$checkout_root" -type f \
    -path '*/wgpu-*/060bd0b/wgpu-hal/src/gles/egl.rs' \
    -print -quit)
mod=$(find "$checkout_root" -type f \
    -path '*/wgpu-*/060bd0b/wgpu-hal/src/gles/mod.rs' \
    -print -quit)
adapter=$(find "$checkout_root" -type f \
    -path '*/wgpu-*/060bd0b/wgpu-hal/src/gles/adapter.rs' \
    -print -quit)
device=$(find "$checkout_root" -type f \
    -path '*/wgpu-*/060bd0b/wgpu-hal/src/gles/device.rs' \
    -print -quit)
fence=$(find "$checkout_root" -type f \
    -path '*/wgpu-*/060bd0b/wgpu-hal/src/gles/fence.rs' \
    -print -quit)
queue=$(find "$checkout_root" -type f \
    -path '*/wgpu-*/060bd0b/wgpu-hal/src/gles/queue.rs' \
    -print -quit)

if [ -z "$egl" ] || [ -z "$mod" ] || [ -z "$adapter" ] || [ -z "$device" ] || [ -z "$fence" ] || [ -z "$queue" ]; then
    echo "error: pinned wgpu GLES sources not found under $checkout_root" >&2
    echo "run 'cargo fetch --locked' first and check the pinned wgpu revision" >&2
    exit 1
fi

python3 - "$egl" "$mod" "$adapter" "$device" "$fence" "$queue" <<'PY'
import pathlib
import sys

egl_path = pathlib.Path(sys.argv[1])
mod_path = pathlib.Path(sys.argv[2])
adapter_path = pathlib.Path(sys.argv[3])
device_path = pathlib.Path(sys.argv[4])
fence_path = pathlib.Path(sys.argv[5])
queue_path = pathlib.Path(sys.argv[6])
egl = egl_path.read_text()
mod = mod_path.read_text()
adapter = adapter_path.read_text()
device = device_path.read_text()
fence = fence_path.read_text()
queue = queue_path.read_text()

# A cached cargo checkout can already contain a previous run of this script.
# Detect the complete patched state and leave it untouched so re-running the
# CI patch step is safe; a partially patched tree still fails on the exact
# text checks below instead of being patched twice.
if (
    "type WlEglWindow = core::ffi::c_void;" in egl
    and "#[cfg(not(webgl))]\nmod egl;" in mod
    and "get_latest_without_gl" in device
    and "get_latest_without_gl" in fence
    and "draw_elements_instanced_base_vertex_ext" in queue
):
    print("wgpu GLES sources are already patched; nothing to do")
    raise SystemExit(0)

# ANGLE's Windows display needs an explicit D3D11 platform type. The constants
# are not in khronos-egl, so declare the two values used below next to the
# existing ANGLE platform constants.
old_angle_constants = """const EGL_PLATFORM_ANGLE_ANGLE: u32 = 0x3202;
const EGL_PLATFORM_ANGLE_NATIVE_PLATFORM_TYPE_ANGLE: u32 = 0x348F;
"""
new_angle_constants = """const EGL_PLATFORM_ANGLE_ANGLE: u32 = 0x3202;
const EGL_PLATFORM_ANGLE_TYPE_ANGLE: u32 = 0x3203;
const EGL_PLATFORM_ANGLE_TYPE_D3D11_ANGLE: u32 = 0x3208;
const EGL_PLATFORM_ANGLE_NATIVE_PLATFORM_TYPE_ANGLE: u32 = 0x348F;
"""
if old_angle_constants not in egl:
    raise SystemExit("error: expected ANGLE platform constants not found in egl.rs")
egl = egl.replace(old_angle_constants, new_angle_constants, 1)

# EGL's Wayland window type is Unix-only. Keep the EGL swapchain code compiling
# on Windows by giving it an opaque placeholder type there. Replace the existing
# type references first; the alias itself must retain the real Wayland path.
if "wayland_sys::egl::wl_egl_window" not in egl:
    raise SystemExit("error: expected Wayland window type not found in egl.rs")
egl = egl.replace(
    "wayland_sys::egl::wl_egl_window",
    "WlEglWindow",
)
if "use std::sync::LazyLock;\n" not in egl:
    raise SystemExit("error: expected LazyLock import not found in egl.rs")
egl = egl.replace(
    "use std::sync::LazyLock;\n",
    "use std::sync::LazyLock;\n\n"
    "#[cfg(unix)]\n"
    "type WlEglWindow = wayland_sys::egl::wl_egl_window;\n"
    "#[cfg(not(unix))]\n"
    "type WlEglWindow = core::ffi::c_void;\n",
    1,
)

# destroy_surface is shared by all platforms; only Unix has a Wayland window to
# destroy after the EGL surface. Windows keeps the handle opaque.
old_unconfigure = """            if let Some(window) = wl_window {
                wayland_sys::ffi_dispatch!(
                    wayland_sys::egl::wayland_egl_handle(),
                    wl_egl_window_destroy,
                    window,
                );
            }
"""
new_unconfigure = """            if let Some(window) = wl_window {
                #[cfg(unix)]
                wayland_sys::ffi_dispatch!(
                    wayland_sys::egl::wayland_egl_handle(),
                    wl_egl_window_destroy,
                    window,
                );
                #[cfg(not(unix))]
                let _ = window;
            }
"""
if old_unconfigure not in egl:
    raise SystemExit("error: expected Wayland unconfigure block not found in egl.rs")
egl = egl.replace(old_unconfigure, new_unconfigure, 1)

# configure() also resizes the Wayland EGL window when a swapchain already
# exists. That path is unreachable on Windows (wl_window is always None), but
# the crate must still compile there.
old_resize = """                if let Some(window) = wl_window {
                    wayland_sys::ffi_dispatch!(
                        wayland_sys::egl::wayland_egl_handle(),
                        wl_egl_window_resize,
                        window,
                        config.extent.width as i32,
                        config.extent.height as i32,
                        0,
                        0,
                    );
                }
"""
new_resize = """                if let Some(window) = wl_window {
                    #[cfg(unix)]
                    wayland_sys::ffi_dispatch!(
                        wayland_sys::egl::wayland_egl_handle(),
                        wl_egl_window_resize,
                        window,
                        config.extent.width as i32,
                        config.extent.height as i32,
                        0,
                        0,
                    );
                    #[cfg(not(unix))]
                    let _ = window;
                }
"""
if old_resize not in egl:
    raise SystemExit("error: expected Wayland resize block not found in egl.rs")
egl = egl.replace(old_resize, new_resize, 1)

# Select ANGLE's D3D11 display explicitly on Windows. The default display also
# picks D3D11 when ANGLE_DEFAULT_PLATFORM is unset, but relying on the
# environment would silently follow any user override. EGL_DEFAULT_DISPLAY is
# the native display value ANGLE documents for its Windows platform.
old_angle_display = """            x if client_ext_str.contains("EGL_MESA_platform_surfaceless") => {
"""
new_angle_display = """            (Some(Rdh::Windows(_)) | None, Some(egl))
                if cfg!(windows) && client_ext_str.contains("EGL_ANGLE_platform_angle") =>
            {
                log::debug!("Using ANGLE platform with D3D11");
                let display_attributes = [
                    EGL_PLATFORM_ANGLE_TYPE_ANGLE as khronos_egl::Attrib,
                    EGL_PLATFORM_ANGLE_TYPE_D3D11_ANGLE as khronos_egl::Attrib,
                    EGL_PLATFORM_ANGLE_DEBUG_LAYERS_ENABLED as khronos_egl::Attrib,
                    usize::from(desc.flags.contains(wgt::InstanceFlags::VALIDATION)),
                    khronos_egl::ATTRIB_NONE,
                ];
                let display = unsafe {
                    egl.get_platform_display(
                        EGL_PLATFORM_ANGLE_ANGLE,
                        khronos_egl::DEFAULT_DISPLAY,
                        &display_attributes,
                    )
                }
                .map_err(instance_err("failed to get ANGLE D3D11 display"))?;
                (display, WindowKind::Unknown)
            }
            x if client_ext_str.contains("EGL_MESA_platform_surfaceless") => {
"""
if old_angle_display not in egl:
    raise SystemExit("error: expected surfaceless display fallback not found in egl.rs")
egl = egl.replace(old_angle_display, new_angle_display, 1)

# ANGLE's D3D11 backend exposes GL_EXT_draw_elements_base_vertex but gates the
# core ES 3.2 entry points on a 3.2 client version. wgpu's draw path calls the
# core name, so a 3.1 context silently records GL_INVALID_OPERATION and drops
# every indexed draw with a non-zero base vertex. Keep a pointer to the EXT
# entry point and use it when the extension is advertised.
old_adapter_context = """/// A wrapper around a [`glow::Context`] and the required EGL context that uses locking to guarantee
/// exclusive access when shared with multiple threads.
#[derive(Debug)]
pub struct AdapterContext {
    glow: Mutex<ManuallyDrop<glow::Context>>,
    egl: Option<EglContext>,
}
"""
new_adapter_context = """type DrawElementsInstancedBaseVertexExt =
    unsafe extern "system" fn(u32, i32, u32, *const ffi::c_void, i32, i32);

/// A wrapper around a [`glow::Context`] and the required EGL context that uses locking to guarantee
/// exclusive access when shared with multiple threads.
#[derive(Debug)]
pub struct AdapterContext {
    glow: Mutex<ManuallyDrop<glow::Context>>,
    egl: Option<EglContext>,
    base_vertex_ext: Option<DrawElementsInstancedBaseVertexExt>,
}
"""
if old_adapter_context not in egl:
    raise SystemExit("error: expected AdapterContext definition not found in egl.rs")
egl = egl.replace(old_adapter_context, new_adapter_context, 1)

old_raw_context = """    pub fn raw_context(&self) -> *mut ffi::c_void {
        match self.egl {
            Some(ref egl) => egl.raw.as_ptr(),
            None => ptr::null_mut(),
        }
    }
}
"""
new_raw_context = """    pub fn raw_context(&self) -> *mut ffi::c_void {
        match self.egl {
            Some(ref egl) => egl.raw.as_ptr(),
            None => ptr::null_mut(),
        }
    }

    pub fn has_base_vertex_ext(&self) -> bool {
        self.base_vertex_ext.is_some()
    }

    /// Issues `glDrawElementsInstancedBaseVertexEXT`.
    ///
    /// Returns `false` when the entry point is unavailable, so the caller can
    /// fall back to the core ES 3.2 function.
    pub unsafe fn draw_elements_instanced_base_vertex_ext(
        &self,
        mode: u32,
        count: i32,
        element_type: u32,
        offset: i32,
        instance_count: i32,
        base_vertex: i32,
    ) -> bool {
        let Some(draw) = self.base_vertex_ext else {
            return false;
        };
        unsafe {
            draw(
                mode,
                count,
                element_type,
                offset as *const ffi::c_void,
                instance_count,
                base_vertex,
            )
        };
        true
    }
}
"""
if old_raw_context not in egl:
    raise SystemExit("error: expected AdapterContext raw_context not found in egl.rs")
egl = egl.replace(old_raw_context, new_raw_context, 1)

# Load the extension entry point while the freshly created context is current.
old_enumerate = """        let gl = ManuallyDrop::new(gl);
        inner.egl.unmake_current();

        unsafe {
            super::Adapter::expose(
                AdapterContext {
                    glow: Mutex::new(gl),
                    // ERROR: Copying owned reference handles here, be careful to not drop them!
                    egl: Some(inner.egl.clone()),
                },
                self.options.clone(),
            )
        }
"""
new_enumerate = """        let extensions = unsafe { gl.get_parameter_string(glow::EXTENSIONS) };
        let base_vertex_ext = if extensions.contains("GL_EXT_draw_elements_base_vertex") {
            inner
                .egl
                .instance
                .get_proc_address("glDrawElementsInstancedBaseVertexEXT")
                .map(|ptr| {
                    // SAFETY: `eglGetProcAddress` returns an entry point with the
                    // GL calling convention, or null.
                    let draw: DrawElementsInstancedBaseVertexExt =
                        unsafe { core::mem::transmute(ptr) };
                    draw
                })
        } else {
            None
        };

        let gl = ManuallyDrop::new(gl);
        inner.egl.unmake_current();

        unsafe {
            super::Adapter::expose(
                AdapterContext {
                    glow: Mutex::new(gl),
                    // ERROR: Copying owned reference handles here, be careful to not drop them!
                    egl: Some(inner.egl.clone()),
                    base_vertex_ext,
                },
                self.options.clone(),
            )
        }
"""
if old_enumerate not in egl:
    raise SystemExit("error: expected adapter enumeration block not found in egl.rs")
egl = egl.replace(old_enumerate, new_enumerate, 1)

old_external = """                AdapterContext {
                    glow: Mutex::new(ManuallyDrop::new(context)),
                    egl: None,
                },
"""
new_external = """                AdapterContext {
                    glow: Mutex::new(ManuallyDrop::new(context)),
                    egl: None,
                    base_vertex_ext: None,
                },
"""
if old_external not in egl:
    raise SystemExit("error: expected external adapter context not found in egl.rs")
egl = egl.replace(old_external, new_external, 1)

# Select EGL on Windows too. The old WGL module is no longer compiled in this
# experimental build. The other backends keep their existing selection.
old_mod_imports = """#[cfg(not(any(windows, webgl)))]
mod egl;
#[cfg(Emscripten)]
mod emscripten;
#[cfg(webgl)]
mod web;
#[cfg(windows)]
mod wgl;
"""
new_mod_imports = """#[cfg(not(webgl))]
mod egl;
#[cfg(Emscripten)]
mod emscripten;
#[cfg(webgl)]
mod web;
#[cfg(any())]
mod wgl;
"""
if old_mod_imports not in mod:
    raise SystemExit("error: expected GLES module selection not found in mod.rs")
mod = mod.replace(old_mod_imports, new_mod_imports, 1)

old_mod_exports = """#[cfg(not(any(windows, webgl)))]
pub use self::egl::{AdapterContext, AdapterContextLock};
#[cfg(not(any(windows, webgl)))]
pub use self::egl::{Instance, Surface};

#[cfg(webgl)]
pub use self::web::AdapterContext;
#[cfg(webgl)]
pub use self::web::{Instance, Surface};

#[cfg(windows)]
use self::wgl::AdapterContext;
#[cfg(windows)]
pub use self::wgl::{Instance, Surface};
"""
new_mod_exports = """#[cfg(not(webgl))]
pub use self::egl::{AdapterContext, AdapterContextLock};
#[cfg(not(webgl))]
pub use self::egl::{Instance, Surface};

#[cfg(webgl)]
pub use self::web::AdapterContext;
#[cfg(webgl)]
pub use self::web::{Instance, Surface};
"""
if old_mod_exports not in mod:
    raise SystemExit("error: expected GLES exports not found in mod.rs")
mod = mod.replace(old_mod_exports, new_mod_exports, 1)

# ANGLE advertises GL_EXT_draw_elements_base_vertex on its GLES 3.1 D3D11
# context, but its core ES 3.2 entry points reject calls below 3.2. Expose
# BASE_VERTEX only when the EXT entry point is loaded (queue.rs routes draws
# through it) or when the context really is ES 3.2.
old_base_vertex = (
    "        downlevel_flags.set("
    "wgt::DownlevelFlags::BASE_VERTEX, supported((3, 2), (3, 2)));"
)
new_base_vertex = """        let has_base_vertex = supported((3, 2), (3, 2))
            || (extensions.contains("GL_EXT_draw_elements_base_vertex")
                && context.has_base_vertex_ext());
        downlevel_flags.set(wgt::DownlevelFlags::BASE_VERTEX, has_base_vertex);"""
if old_base_vertex not in adapter:
    raise SystemExit("error: expected BASE_VERTEX capability assignment not found in adapter.rs")
adapter = adapter.replace(old_base_vertex, new_base_vertex, 1)

# Route non-zero base-vertex indexed draws through the EXT entry point when the
# context loaded it. The core ES 3.2 name is a version error on ANGLE 3.1.
old_queue_base_vertex = """                        // If we've gotten here, wgpu-core has already validated that this function exists via the DownlevelFlags::BASE_VERTEX feature.
                        unsafe {
                            gl.draw_elements_instanced_base_vertex(
                                topology,
                                index_count as _,
                                index_type,
                                index_offset as i32,
                                instance_count as i32,
                                base_vertex,
                            )
                        }
"""
new_queue_base_vertex = """                        // If we've gotten here, wgpu-core has already validated that this function exists via the DownlevelFlags::BASE_VERTEX feature.
                        //
                        // ANGLE exposes the ES 3.2 core entry point but records a
                        // version error on its GLES 3.1 D3D11 context. Use the
                        // EXT entry point when the context loaded it.
                        let drew_ext = unsafe {
                            self.shared.context.draw_elements_instanced_base_vertex_ext(
                                topology,
                                index_count as i32,
                                index_type,
                                index_offset as i32,
                                instance_count as i32,
                                base_vertex,
                            )
                        };
                        if !drew_ext {
                            unsafe {
                                gl.draw_elements_instanced_base_vertex(
                                    topology,
                                    index_count as _,
                                    index_type,
                                    index_offset as i32,
                                    instance_count as i32,
                                    base_vertex,
                                )
                            }
                        }
"""
if old_queue_base_vertex not in queue:
    raise SystemExit("error: expected base-vertex draw call not found in queue.rs")
queue = queue.replace(old_queue_base_vertex, new_queue_base_vertex, 1)

old_alignment = """        downlevel_flags.set(
            wgt::DownlevelFlags::BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED,
            !(cfg!(any(webgl, Emscripten)) || is_angle),
        );"""
new_alignment = """        // Windows ANGLE accepts the 4-byte uniform bindings Cinnabar uses.
        // Keep the conservative WebGL restriction, but do not reject the
        // native ANGLE/D3D11 path solely because it is ANGLE.
        downlevel_flags.set(
            wgt::DownlevelFlags::BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED,
            !(cfg!(any(webgl, Emscripten)) || (is_angle && !cfg!(windows))),
        );"""
if old_alignment not in adapter:
    raise SystemExit("error: expected buffer-alignment capability assignment not found in adapter.rs")
adapter = adapter.replace(old_alignment, new_alignment, 1)

# GL_FRAMEBUFFER_SRGB is a desktop-GL capability. ANGLE only accepts the
# GL_FRAMEBUFFER_SRGB_EXT alias when EXT_sRGB_write_control is advertised,
# which its D3D11 backend does not do. The calls were already ineffective
# no-ops there, but they recorded GL_INVALID_ENUM during init and on every
# present. Keep them for the desktop-GL EGL paths and skip them on Windows.
old_srgb_init = """        // In contrast to OpenGL ES, OpenGL requires explicitly enabling sRGB conversions,
        // as otherwise the user has to do the sRGB conversion.
        if !matches!(inner.srgb_kind, SrgbFrameBufferKind::None) {
            unsafe { gl.enable(glow::FRAMEBUFFER_SRGB) };
        }
"""
new_srgb_init = """        // In contrast to OpenGL ES, OpenGL requires explicitly enabling sRGB conversions,
        // as otherwise the user has to do the sRGB conversion. GLES has no
        // FRAMEBUFFER_SRGB capability; ANGLE's D3D11 backend rejects it.
        if cfg!(not(windows)) && !matches!(inner.srgb_kind, SrgbFrameBufferKind::None) {
            unsafe { gl.enable(glow::FRAMEBUFFER_SRGB) };
        }
"""
if old_srgb_init not in egl:
    raise SystemExit("error: expected sRGB enable block not found in egl.rs")
egl = egl.replace(old_srgb_init, new_srgb_init, 1)

old_srgb_disable = """        if !matches!(self.srgb_kind, SrgbFrameBufferKind::None) {
            // Disable sRGB conversions for `glBlitFramebuffer` as behavior does diverge between
            // drivers and formats otherwise and we want to ensure no sRGB conversions happen.
            unsafe { gl.disable(glow::FRAMEBUFFER_SRGB) };
        }
"""
new_srgb_disable = """        if cfg!(not(windows)) && !matches!(self.srgb_kind, SrgbFrameBufferKind::None) {
            // Disable sRGB conversions for `glBlitFramebuffer` as behavior does diverge between
            // drivers and formats otherwise and we want to ensure no sRGB conversions happen.
            unsafe { gl.disable(glow::FRAMEBUFFER_SRGB) };
        }
"""
if old_srgb_disable not in egl:
    raise SystemExit("error: expected sRGB disable block not found in egl.rs")
egl = egl.replace(old_srgb_disable, new_srgb_disable, 1)

old_srgb_enable = """        if !matches!(self.srgb_kind, SrgbFrameBufferKind::None) {
            unsafe { gl.enable(glow::FRAMEBUFFER_SRGB) };
        }
"""
new_srgb_enable = """        if cfg!(not(windows)) && !matches!(self.srgb_kind, SrgbFrameBufferKind::None) {
            unsafe { gl.enable(glow::FRAMEBUFFER_SRGB) };
        }
"""
if old_srgb_enable not in egl:
    raise SystemExit("error: expected sRGB restore block not found in egl.rs")
egl = egl.replace(old_srgb_enable, new_srgb_enable, 1)

# Surface capabilities advertise Immediate on Windows. EGL defaults to a swap
# interval of 1, so store the requested mode and apply it to the window surface
# before each swap; otherwise NoVsync/Immediate silently remains vsynced.
old_swapchain_field = """    /// Extent because the window lies
    extent: wgt::Extent3d,
    format: wgt::TextureFormat,
"""
new_swapchain_field = """    /// Extent because the window lies
    extent: wgt::Extent3d,
    present_mode: wgt::PresentMode,
    format: wgt::TextureFormat,
"""
if old_swapchain_field not in egl:
    raise SystemExit("error: expected Swapchain extent fields not found in egl.rs")
egl = egl.replace(old_swapchain_field, new_swapchain_field, 1)

old_swapchain_init = """            framebuffer,
            extent: config.extent,
            format: config.format,
"""
new_swapchain_init = """            framebuffer,
            extent: config.extent,
            present_mode: config.present_mode,
            format: config.format,
"""
if old_swapchain_init not in egl:
    raise SystemExit("error: expected Swapchain initializer not found in egl.rs")
egl = egl.replace(old_swapchain_init, new_swapchain_init, 1)

old_present_interval = """        unsafe { gl.disable(glow::SCISSOR_TEST) };
"""
new_present_interval = """        if let Err(e) = self.egl.instance.swap_interval(
            self.egl.display,
            if sc.present_mode == wgt::PresentMode::Immediate {
                0
            } else {
                1
            },
        ) {
            // Presentation still works with the previous interval; do not lose
            // the surface just because the driver rejected the request.
            log::warn!("swap_interval failed: {e}");
        }

        unsafe { gl.disable(glow::SCISSOR_TEST) };
"""
if old_present_interval not in egl:
    raise SystemExit("error: expected present state reset not found in egl.rs")
egl = egl.replace(old_present_interval, new_present_interval, 1)

# A failed eglSwapBuffers must still unbind the window surface. Otherwise the
# EGL context stays current on the presenting thread while it is no longer
# protected by the glow mutex, and a later lock() on another thread can make
# the same context current there, which EGL forbids.
old_present = """        self.egl
            .instance
            .swap_buffers(self.egl.display, sc.surface)
            .map_err(|e| {
                log::error!("swap_buffers failed: {e}");
                crate::SurfaceError::Lost
                // TODO: should we unset the current context here?
            })?;
        self.egl
            .instance
            .make_current(self.egl.display, None, None, None)
            .map_err(|e| {
                log::error!("make_current(null) failed: {e}");
                crate::SurfaceError::Lost
            })?;
"""
new_present = """        let swap_result = self.egl.instance.swap_buffers(self.egl.display, sc.surface);
        // Always unbind the window surface, even when presentation fails.
        // Leaving it current after the glow mutex is released lets a later
        // lock() put the same EGL context current on another thread, which
        // EGL forbids.
        let unset_result = self
            .egl
            .instance
            .make_current(self.egl.display, None, None, None);
        if let Err(e) = swap_result {
            log::error!("swap_buffers failed: {e}");
            if let Err(e) = unset_result {
                log::error!("make_current(null) failed: {e}");
            }
            return Err(crate::SurfaceError::Lost);
        }
        unset_result.map_err(|e| {
            log::error!("make_current(null) failed: {e}");
            crate::SurfaceError::Lost
        })?;
"""
if old_present not in egl:
    raise SystemExit("error: expected surface present block not found in egl.rs")
egl = egl.replace(old_present, new_present, 1)

# Under ANGLE the main thread can sit inside eglSwapBuffers while holding the
# EGL context lock. The render thread polls the completion fence during that
# window; AutoFinish fences only need the atomic counter, but wgpu still took
# the adapter lock just to read it. That produced a deterministic 6-second
# "Could not lock adapter context" panic after the menu presented.
old_fence_get = """    pub fn get_latest(&self, gl: &glow::Context) -> crate::FenceValue {
        let mut max_value = self.last_completed.load(Ordering::Acquire);
"""
new_fence_get = """    pub fn is_auto_finish(&self) -> bool {
        self.fence_behavior.is_auto_finish()
    }

    /// Read the last completed value without touching the GL context.
    ///
    /// This is only sound for AutoFinish fences, where signal() updates the
    /// atomic counter directly and get_latest() never queries a GL sync.
    pub fn get_latest_without_gl(&self) -> crate::FenceValue {
        self.last_completed.load(Ordering::Acquire)
    }

    pub fn get_latest(&self, gl: &glow::Context) -> crate::FenceValue {
        let mut max_value = self.last_completed.load(Ordering::Acquire);
"""
if old_fence_get not in fence:
    raise SystemExit("error: expected fence get_latest not found in fence.rs")
fence = fence.replace(old_fence_get, new_fence_get, 1)

old_device_get = """    unsafe fn get_fence_value(
        &self,
        fence: &super::Fence,
    ) -> Result<crate::FenceValue, crate::DeviceError> {
        #[cfg_attr(target_arch = "wasm32", allow(clippy::needless_borrow))]
        Ok(fence.get_latest(&self.shared.context.lock()))
    }
"""
new_device_get = """    unsafe fn get_fence_value(
        &self,
        fence: &super::Fence,
    ) -> Result<crate::FenceValue, crate::DeviceError> {
        if fence.is_auto_finish() {
            return Ok(fence.get_latest_without_gl());
        }
        #[cfg_attr(target_arch = "wasm32", allow(clippy::needless_borrow))]
        Ok(fence.get_latest(&self.shared.context.lock()))
    }
"""
if old_device_get not in device:
    raise SystemExit("error: expected get_fence_value implementation not found in device.rs")
device = device.replace(old_device_get, new_device_get, 1)

# AutoFinish fences have no GL sync objects to delete. Taking the adapter
# context lock here would reintroduce the same 6-second panic that polling
# avoids when the presenting thread is inside eglSwapBuffers.
old_destroy_fence = """    unsafe fn destroy_fence(&self, fence: super::Fence) {
        let gl = &self.shared.context.lock();
        fence.destroy(gl);
        self.counters.fences.sub(1);
    }
"""
new_destroy_fence = """    unsafe fn destroy_fence(&self, fence: super::Fence) {
        if !fence.is_auto_finish() {
            let gl = &self.shared.context.lock();
            fence.destroy(gl);
        }
        self.counters.fences.sub(1);
    }
"""
if old_destroy_fence not in device:
    raise SystemExit("error: expected destroy_fence implementation not found in device.rs")
device = device.replace(old_destroy_fence, new_destroy_fence, 1)

egl_path.write_text(egl)
mod_path.write_text(mod)
adapter_path.write_text(adapter)
device_path.write_text(device)
fence_path.write_text(fence)
queue_path.write_text(queue)
PY

echo "wgpu GLES EGL/ANGLE patch state:"
grep -nF 'mod egl;' "$mod" | head -5
grep -nF 'type WlEglWindow' "$egl" | head -5
