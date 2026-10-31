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

if [ -z "$egl" ] || [ -z "$mod" ] || [ -z "$adapter" ] || [ -z "$device" ] || [ -z "$fence" ]; then
    echo "error: pinned wgpu GLES sources not found under $checkout_root" >&2
    echo "run 'cargo fetch --locked' first and check the pinned wgpu revision" >&2
    exit 1
fi

python3 - "$egl" "$mod" "$adapter" "$device" "$fence" <<'PY'
import pathlib
import sys

egl_path = pathlib.Path(sys.argv[1])
mod_path = pathlib.Path(sys.argv[2])
adapter_path = pathlib.Path(sys.argv[3])
device_path = pathlib.Path(sys.argv[4])
fence_path = pathlib.Path(sys.argv[5])
egl = egl_path.read_text()
mod = mod_path.read_text()
adapter = adapter_path.read_text()
device = device_path.read_text()
fence = fence_path.read_text()

# EGL's Wayland window type is Unix-only. Keep the EGL swapchain code compiling
# on Windows by giving it an opaque placeholder type there. Replace the existing
# type references first; the alias itself must retain the real Wayland path.
egl = egl.replace(
    "wayland_sys::egl::wl_egl_window",
    "WlEglWindow",
)
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

# ANGLE advertises GLES 3.1 plus the base-vertex extensions, but wgpu only
# enables its BASE_VERTEX capability for core GLES 3.2. The draw entry points
# are present in ANGLE under the core names, so expose the capability when an
# extension is present.
old_base_vertex = (
    "        downlevel_flags.set("
    "wgt::DownlevelFlags::BASE_VERTEX, supported((3, 2), (3, 2)));"
)
new_base_vertex = """        let has_base_vertex = supported((3, 2), (3, 2))
            || extensions.contains("GL_OES_draw_elements_base_vertex")
            || extensions.contains("GL_EXT_draw_elements_base_vertex")
            || extensions.contains("GL_ANGLE_base_vertex_base_instance")
            || extensions.contains("GL_ARB_draw_elements_base_vertex");
        downlevel_flags.set(wgt::DownlevelFlags::BASE_VERTEX, has_base_vertex);"""
if old_base_vertex not in adapter:
    raise SystemExit("error: expected BASE_VERTEX capability assignment not found in adapter.rs")
adapter = adapter.replace(old_base_vertex, new_base_vertex, 1)

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

egl_path.write_text(egl)
mod_path.write_text(mod)
adapter_path.write_text(adapter)
device_path.write_text(device)
fence_path.write_text(fence)
PY

echo "Patched wgpu GLES to use EGL/ANGLE on Windows:"
grep -nF 'mod egl;' "$mod" | head -5
grep -nF 'type WlEglWindow' "$egl" | head -5
