@echo off
setlocal
cd /d "%~dp0"
set WGPU_BACKEND=gl
set WGPU_GLES_MINOR_VERSION=1
set ANGLE_DEFAULT_PLATFORM=d3d11
cinnabar.exe %*
endlocal
