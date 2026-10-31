Cinnabar Windows ANGLE test build
=================================

This is an experimental build. It does not use the Intel OpenGL driver
(ig75icd64.dll). Instead, wgpu's GLES backend talks to ANGLE, which translates
OpenGL ES 3.1 to Direct3D 11 through libEGL.dll and libGLESv2.dll.

Run:
  run-angle.bat

The launcher forces WGPU_BACKEND=gl, requests an OpenGL ES 3.1 context and
enables wgpu's AutoFinish GL fence behavior. The pinned wgpu patch selects
ANGLE's D3D11 platform explicitly; ANGLE_DEFAULT_PLATFORM is also set. The patch
skips the EGL context lock for these fence reads, avoiding the deadlock that can
otherwise appear while the render thread polls during presentation.

Do not delete libEGL.dll, libGLESv2.dll or d3dcompiler_47.dll, and do not run
cinnabar.exe from another folder. All three DLLs must stay next to cinnabar.exe;
do not mix DLLs from different ANGLE builds. The launcher stops with an error if
a runtime DLL is missing.

Capability report:
  set CINNABAR_RENDER_DIAGNOSTICS=1
  run-angle.bat

That adds one-time "Renderer capabilities:", "Renderer format support:" and
"Renderer GL settings:" lines to the log. Include them, and the "Renderer API:"
line, when reporting a result. The GL settings line must show
gles_minor=Version1 and fence=AutoFinish.

What to report back:
  - the Renderer API and Renderer capabilities lines (they should mention ANGLE)
  - whether the menu and the world render
  - any shader or wgpu validation errors, GPU hangs or device-loss messages
  - whether the Intel ig75icd64 crash is gone
  - the graphics driver version and Windows build

This build is only a spike. Color/sRGB presentation, shader compatibility and
the relaxed fence semantics are the main risks. Keep the previous d9ab8de build
available for comparison.
