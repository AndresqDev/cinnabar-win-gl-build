@echo off
setlocal
cd /d "%~dp0"
set WGPU_BACKEND=dx11
set CINNABAR_RENDER_DIAGNOSTICS=1
rem Keep the HD 4400 on the stable model route with direct terrain/entity draws.
set CINNABAR_HEAVY_TERRAIN=1
set CINNABAR_HEAVY_MODEL_TERRAIN=1
set CINNABAR_HEAVY_CUBE_TERRAIN=0
set CINNABAR_FORCE_DIRECT_DRAWS=1
set CINNABAR_CPU_CULLING=1
rem Indirect validation needs compute + dynamic storage offsets, which this backend does not provide yet.
set WGPU_VALIDATION_INDIRECT_CALL=0
rem Native Direct3D 11 backend. This build intentionally does not load
rem libEGL/libGLESv2; the graphics API is forced to dx11 for diagnostics.
cinnabar.exe %*
exit /b %errorlevel%
