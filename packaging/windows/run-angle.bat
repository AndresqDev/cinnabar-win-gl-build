@echo off
setlocal
cd /d "%~dp0"
set WGPU_BACKEND=gl
set WGPU_GLES_MINOR_VERSION=1
set WGPU_GL_FENCE_BEHAVIOR=AutoFinish
set ANGLE_DEFAULT_PLATFORM=d3d11
cinnabar.exe %*
endlocal
