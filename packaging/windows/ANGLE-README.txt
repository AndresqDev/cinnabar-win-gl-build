Cinnabar Windows ANGLE test build
=================================

This is an experimental build. It does not use the Intel OpenGL driver
(ig75icd64.dll). Instead, wgpu's GLES backend talks to ANGLE, which translates
OpenGL ES 3.1 to Direct3D 11 through libEGL.dll and libGLESv2.dll.

Run:
  run-angle.bat

Do not delete the DLLs or run cinnabar.exe from another folder. The ANGLE DLLs
must stay next to cinnabar.exe.

What to report back:
  - the Renderer API line in client.log (it should mention ANGLE)
  - whether the menu and the world render
  - any shader/validation errors
  - whether the Intel ig75icd64 crash is gone

This build is only a spike. Color/sRGB presentation and shader compatibility are
the main risks. Keep the previous d9ab8de build available for comparison.
