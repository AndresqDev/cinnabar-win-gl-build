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
    and "compile_context=created" in egl
    and "lock_for_pipeline" in device
):
    print("wgpu GLES sources are already patched; nothing to do")
    raise SystemExit(0)

# The previous versions of this patch are not a suitable base for an in-place
# upgrade: they are already past the text anchors below and they have no shared
# compile context. Fail with an actionable message instead of a misleading
# missing-anchor error.
if "type WlEglWindow = core::ffi::c_void;" in egl and "compile_context=created" not in egl:
    raise SystemExit(
        "error: this wgpu checkout was patched by an older version of "
        "scripts/patch-wgpu-angle.sh and has no shared compile context; "
        "restore wgpu-hal/src/gles from the pinned revision and re-run"
    )

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

# Pipeline compilation needs an EGL context that can be made current without
# blocking presentation on the main context. A second context in the same share
# group can compile and link programs, while the program objects remain visible
# to the main context. EglContext::clone only copies the raw handle, so the
# compile context builds a distinct EGLContext and uses its own glow handle.
old_egl_context_impl = """impl EglContext {
    fn make_current(&self) {
        self.instance
            .make_current(self.display, self.pbuffer, self.pbuffer, Some(self.raw))
            .unwrap();
    }

    fn unmake_current(&self) {
        self.instance
            .make_current(self.display, None, None, None)
            .unwrap();
    }
}
"""
new_egl_context_impl = """impl EglContext {
    fn make_current_checked(&self) -> Result<(), khronos_egl::Error> {
        self.instance
            .make_current(self.display, self.pbuffer, self.pbuffer, Some(self.raw))
    }

    fn make_current(&self) {
        self.make_current_checked().unwrap();
    }

    fn unmake_current_checked(&self) -> Result<(), khronos_egl::Error> {
        self.instance
            .make_current(self.display, None, None, None)
    }

    fn unmake_current(&self) {
        self.unmake_current_checked().unwrap();
    }
}

/// A second EGL context in the main context's share group, used only for
/// pipeline compilation. It never owns a window surface.
#[derive(Debug)]
struct CompileContext {
    egl: EglContext,
    glow: ManuallyDrop<glow::Context>,
}

impl CompileContext {
    fn create(inner: &Inner) -> Result<Self, String> {
        let main = &inner.egl;
        main.instance
            .bind_api(inner.egl_context_api)
            .map_err(|e| format!("eglBindAPI failed: {e:?}"))?;
        let raw = main
            .instance
            .create_context(
                main.display,
                inner.config,
                Some(main.raw),
                &inner.egl_context_attributes,
            )
            .map_err(|e| format!("eglCreateContext(shared) failed: {e:?}"))?;
        let pbuffer = if main.pbuffer.is_some() {
            let attributes = [
                khronos_egl::WIDTH,
                1,
                khronos_egl::HEIGHT,
                1,
                khronos_egl::NONE,
            ];
            match main
                .instance
                .create_pbuffer_surface(main.display, inner.config, &attributes)
            {
                Ok(surface) => Some(surface),
                Err(e) => {
                    let _ = main.instance.destroy_context(main.display, raw);
                    return Err(format!("eglCreatePbufferSurface(shared) failed: {e:?}"));
                }
            }
        } else {
            None
        };

        let egl = EglContext {
            instance: main.instance.clone(),
            version: main.version,
            display: main.display,
            raw,
            pbuffer,
        };

        if let Err(e) = egl.make_current_checked() {
            let _ = egl.instance.destroy_context(egl.display, egl.raw);
            if let Some(pbuffer) = egl.pbuffer {
                let _ = egl.instance.destroy_surface(egl.display, pbuffer);
            }
            return Err(format!("eglMakeCurrent(shared) failed: {e:?}"));
        }

        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                egl.instance
                    .get_proc_address(name)
                    .map_or(ptr::null(), |p| p as *const _)
            })
        };
        if let Err(e) = egl.unmake_current_checked() {
            log::warn!("compile_context=unavailable reason=unmake_current {e:?}");
        }

        Ok(Self {
            egl,
            glow: ManuallyDrop::new(gl),
        })
    }
}

impl Drop for CompileContext {
    fn drop(&mut self) {
        let egl = &self.egl;
        if let Err(e) = egl.make_current_checked() {
            log::error!("compile_context=destroy make_current failed: {e:?}");
        } else {
            // SAFETY: The EGL context is current and the glow handle is not used
            // after this drop.
            unsafe { ManuallyDrop::drop(&mut self.glow) };
            if let Err(e) = egl.unmake_current_checked() {
                log::error!("compile_context=destroy unmake_current failed: {e:?}");
            }
        }
        if let Err(e) = egl.instance.destroy_context(egl.display, egl.raw) {
            log::warn!("compile_context=destroy destroy_context failed: {e:?}");
        }
        if let Some(pbuffer) = egl.pbuffer {
            if let Err(e) = egl.instance.destroy_surface(egl.display, pbuffer) {
                log::warn!("compile_context=destroy destroy_surface failed: {e:?}");
            }
        }
        log::debug!("compile_context=destroyed");
    }
}
"""
if old_egl_context_impl not in egl:
    raise SystemExit("error: expected EglContext implementation not found in egl.rs")
egl = egl.replace(old_egl_context_impl, new_egl_context_impl, 1)

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
    compile: Option<Mutex<CompileContext>>,
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

# Destroy the compile context before the main context. Dropping the Mutex runs
# CompileContext::drop, which makes the compile context current while dropping
# its glow handle. Inner::drop must only ever see the main EGL context.
old_adapter_drop = """impl Drop for AdapterContext {
    fn drop(&mut self) {
        struct CurrentGuard<'a>(&'a EglContext);
"""
new_adapter_drop = """impl Drop for AdapterContext {
    fn drop(&mut self) {
        if let Some(compile) = self.compile.take() {
            drop(compile);
        }

        struct CurrentGuard<'a>(&'a EglContext);
"""
if old_adapter_drop not in egl:
    raise SystemExit("error: expected AdapterContext drop not found in egl.rs")
egl = egl.replace(old_adapter_drop, new_adapter_drop, 1)

old_lock_drop = """impl<'a> Drop for AdapterContextLock<'a> {
    fn drop(&mut self) {
        if let Some(egl) = self.egl.take() {
            if let Err(err) = egl.instance.make_current(egl.display, None, None, None) {
                log::error!("Failed to make EGL context current: {err:?}");
            }
        }
    }
}
"""
new_lock_drop = """impl<'a> Drop for AdapterContextLock<'a> {
    fn drop(&mut self) {
        if let Some(egl) = self.egl.take() {
            if let Err(err) = egl.instance.make_current(egl.display, None, None, None) {
                log::error!("Failed to make EGL context current: {err:?}");
            }
        }
    }
}

enum PipelineContextLockInner<'a> {
    Compile(MutexGuard<'a, CompileContext>),
    Main(AdapterContextLock<'a>),
}

/// A guard that keeps the dedicated compile context, or the main context as a
/// fallback, current for pipeline creation. The EGL context is unbound on drop.
#[expect(missing_debug_implementations)]
pub struct PipelineContextLock<'a> {
    inner: PipelineContextLockInner<'a>,
}

impl<'a> core::ops::Deref for PipelineContextLock<'a> {
    type Target = glow::Context;

    fn deref(&self) -> &Self::Target {
        match &self.inner {
            PipelineContextLockInner::Compile(guard) => &guard.glow,
            PipelineContextLockInner::Main(guard) => guard,
        }
    }
}

impl<'a> Drop for PipelineContextLock<'a> {
    fn drop(&mut self) {
        if let PipelineContextLockInner::Compile(guard) = &self.inner {
            if let Err(err) = guard.egl.unmake_current_checked() {
                log::error!("Failed to release EGL compile context: {err:?}");
            }
        }
    }
}
"""
if old_lock_drop not in egl:
    raise SystemExit("error: expected AdapterContextLock drop not found in egl.rs")
egl = egl.replace(old_lock_drop, new_lock_drop, 1)

# Pipeline creation must be able to use the shared compile context without
# taking the main glow lock, so that the render thread can keep presenting.
old_adapter_lock_impl = """        AdapterContextLock { glow, egl }
    }
}

#[derive(Debug)]
struct Inner {
    /// Note: the context contains a dummy pbuffer (1x1).
    /// Required for `eglMakeCurrent` on platforms that doesn't supports `EGL_KHR_surfaceless_context`.
    egl: EglContext,
    version: (i32, i32),
    supports_native_window: bool,
    config: khronos_egl::Config,
    /// Method by which the framebuffer should support srgb
    srgb_kind: SrgbFrameBufferKind,
}
"""
new_adapter_lock_impl = """        AdapterContextLock { glow, egl }
    }

    /// Obtains a context for pipeline compilation.
    ///
    /// Uses the dedicated shared compile context when available and otherwise
    /// falls back to the main context. The returned guard unsets its EGL
    /// context when dropped.
    #[track_caller]
    pub fn lock_for_pipeline(&self) -> PipelineContextLock<'_> {
        if let Some(compile) = self.compile.as_ref() {
            let guard = compile
                .try_lock_for(Duration::from_secs(CONTEXT_LOCK_TIMEOUT_SECS))
                .expect("Could not lock compile context. This is most-likely a deadlock.");
            match guard.egl.make_current_checked() {
                Ok(()) => {
                    log::debug!("compile_context=using target=compile");
                    return PipelineContextLock {
                        inner: PipelineContextLockInner::Compile(guard),
                    };
                }
                Err(err) => {
                    log::error!("compile_context=unavailable reason=make_current {err:?}");
                    drop(guard);
                }
            }
        }

        log::debug!("compile_context=using target=main");
        PipelineContextLock {
            inner: PipelineContextLockInner::Main(self.lock()),
        }
    }
}

#[derive(Debug)]
struct Inner {
    /// Note: the context contains a dummy pbuffer (1x1).
    /// Required for `eglMakeCurrent` on platforms that doesn't supports `EGL_KHR_surfaceless_context`.
    egl: EglContext,
    version: (i32, i32),
    supports_native_window: bool,
    config: khronos_egl::Config,
    /// Method by which the framebuffer should support srgb
    srgb_kind: SrgbFrameBufferKind,
    egl_context_api: khronos_egl::Enum,
    egl_context_attributes: Vec<khronos_egl::Int>,
}
"""
if old_adapter_lock_impl not in egl:
    raise SystemExit("error: expected AdapterContext lock implementation not found in egl.rs")
egl = egl.replace(old_adapter_lock_impl, new_adapter_lock_impl, 1)

# Remember the API and the exact attribute list that successfully created the
# main context. The compile context must be created with the same values so the
# two contexts can share objects.
old_context_create = """        let context = {
            #[derive(Copy, Clone)]
            enum Robustness {
"""
new_context_create = """        let (context, context_api, context_attributes) = {
            #[derive(Copy, Clone)]
            enum Robustness {
"""
if old_context_create not in egl:
    raise SystemExit("error: expected EGL context creation block not found in egl.rs")
egl = egl.replace(old_context_create, new_context_create, 1)

old_context_return = """                            return Ok(context);
"""
new_context_return = """                            return Ok((context, api, context_attributes));
"""
if old_context_return not in egl:
    raise SystemExit("error: expected EGL context creation return not found in egl.rs")
egl = egl.replace(old_context_return, new_context_return, 1)

old_inner_init = """            version,
            supports_native_window,
            config,
            srgb_kind,
        })
"""
new_inner_init = """            version,
            supports_native_window,
            config,
            srgb_kind,
            egl_context_api: context_api,
            egl_context_attributes: context_attributes,
        })
"""
if old_inner_init not in egl:
    raise SystemExit("error: expected Inner initializer not found in egl.rs")
egl = egl.replace(old_inner_init, new_inner_init, 1)

# The compile context is opt-in. Without the environment flag every pipeline
# keeps using the main context exactly as before. Creation failures are logged
# and degrade to the main context rather than failing adapter enumeration.
old_window_kind = """#[derive(Clone, Copy, Debug, PartialEq)]
enum WindowKind {
"""
new_window_kind = """impl Inner {
    fn create_compile_context(&self) -> Option<CompileContext> {
        if !cfg!(windows) {
            log::info!("compile_context=disabled reason=platform");
            return None;
        }
        if !compile_context_requested() {
            log::info!("compile_context=disabled reason=env");
            return None;
        }
        match CompileContext::create(self) {
            Ok(context) => {
                log::info!("compile_context=created");
                Some(context)
            }
            Err(err) => {
                log::error!("compile_context=unavailable reason={err}");
                None
            }
        }
    }
}

fn compile_context_requested() -> bool {
    matches!(
        std::env::var("CINNABAR_ANGLE_COMPILE_CONTEXT").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum WindowKind {
"""
if old_window_kind not in egl:
    raise SystemExit("error: expected WindowKind definition not found in egl.rs")
egl = egl.replace(old_window_kind, new_window_kind, 1)

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
        let compile = inner.create_compile_context().map(Mutex::new);

        unsafe {
            super::Adapter::expose(
                AdapterContext {
                    glow: Mutex::new(gl),
                    // ERROR: Copying owned reference handles here, be careful to not drop them!
                    egl: Some(inner.egl.clone()),
                    base_vertex_ext,
                    compile,
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
                    compile: None,
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
pub use self::egl::{AdapterContext, AdapterContextLock, PipelineContextLock};
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

# Pipeline creation uses the dedicated compile context when the EGL backend
# provides one. WebGL keeps its existing lock because its AdapterContext has no
# separate context. destroy_* intentionally keeps using the main lock.
old_render_pipeline_lock = """        let gl = &self.shared.context.lock();
        let mut shaders = ArrayVec::new();
        shaders.push((naga::ShaderStage::Vertex, vertex_stage));
"""
new_render_pipeline_lock = """        #[cfg(not(webgl))]
        let gl = &self.shared.context.lock_for_pipeline();
        #[cfg(webgl)]
        let gl = &self.shared.context.lock();
        let mut shaders = ArrayVec::new();
        shaders.push((naga::ShaderStage::Vertex, vertex_stage));
"""
if old_render_pipeline_lock not in device:
    raise SystemExit("error: expected render pipeline context lock not found in device.rs")
device = device.replace(old_render_pipeline_lock, new_render_pipeline_lock, 1)

old_compute_pipeline_lock = """        let gl = &self.shared.context.lock();
        let mut shaders = ArrayVec::new();
        shaders.push((naga::ShaderStage::Compute, &desc.stage));
"""
new_compute_pipeline_lock = """        #[cfg(not(webgl))]
        let gl = &self.shared.context.lock_for_pipeline();
        #[cfg(webgl)]
        let gl = &self.shared.context.lock();
        let mut shaders = ArrayVec::new();
        shaders.push((naga::ShaderStage::Compute, &desc.stage));
"""
if old_compute_pipeline_lock not in device:
    raise SystemExit("error: expected compute pipeline context lock not found in device.rs")
device = device.replace(old_compute_pipeline_lock, new_compute_pipeline_lock, 1)

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
grep -nF 'compile_context=created' "$egl" | head -5
grep -nF 'lock_for_pipeline' "$device" | head -5
