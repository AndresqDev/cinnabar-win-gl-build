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

if [ -z "$egl" ] || [ -z "$mod" ]; then
    echo "error: pinned wgpu GLES sources not found under $checkout_root" >&2
    echo "run 'cargo fetch --locked' first and check the pinned wgpu revision" >&2
    exit 1
fi

python3 - "$egl" "$mod" <<'PY'
import pathlib
import sys

egl_path = pathlib.Path(sys.argv[1])
mod_path = pathlib.Path(sys.argv[2])
egl = egl_path.read_text()
mod = mod_path.read_text()

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

egl_path.write_text(egl)
mod_path.write_text(mod)
PY

echo "Patched wgpu GLES to use EGL/ANGLE on Windows:"
grep -nF 'mod egl;' "$mod" | head -5
grep -nF 'type WlEglWindow' "$egl" | head -5
