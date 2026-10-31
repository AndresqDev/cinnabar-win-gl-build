@echo off
setlocal
cd /d "%~dp0"
if not exist "libEGL.dll" goto missing
if not exist "libGLESv2.dll" goto missing
if not exist "d3dcompiler_47.dll" goto missing
set WGPU_BACKEND=gl
set WGPU_GLES_MINOR_VERSION=1
set WGPU_GL_FENCE_BEHAVIOR=AutoFinish
set ANGLE_DEFAULT_PLATFORM=d3d11
rem Opt into wgpu/ANGLE's separate EGL compile context. The client keeps
rem synchronous pipeline compilation when this is unset or not exactly 1.
if not defined CINNABAR_ANGLE_COMPILE_CONTEXT set CINNABAR_ANGLE_COMPILE_CONTEXT=1
cinnabar.exe %*
exit /b %errorlevel%

:missing
echo Missing ANGLE runtime DLL next to "%~dp0". Re-extract the artifact. 1>&2
exit /b 1
