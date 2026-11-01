@echo off
setlocal
cd /d "%~dp0"
set WGPU_BACKEND=dx11
set CINNABAR_RENDER_DIAGNOSTICS=1
rem Indirect validation needs compute + dynamic storage offsets, which this backend does not provide yet.
set WGPU_VALIDATION_INDIRECT_CALL=0
rem Native Direct3D 11 backend. This build intentionally does not load
rem libEGL/libGLESv2; the graphics API is forced to dx11 for diagnostics.
cinnabar.exe %*
exit /b %errorlevel%
