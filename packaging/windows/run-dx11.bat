@echo off
setlocal
cd /d "%~dp0"
set WGPU_BACKEND=dx11
set CINNABAR_RENDER_DIAGNOSTICS=1
rem Native Direct3D 11 backend. This build intentionally does not load
rem libEGL/libGLESv2; the graphics API is forced to dx11 for diagnostics.
cinnabar.exe %*
exit /b %errorlevel%
