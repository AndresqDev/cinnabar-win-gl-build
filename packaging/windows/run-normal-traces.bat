@echo off
setlocal
title Cinnabar DX11 - modo normal con trazas
cd /d "%~dp0"
call "%~dp0_assets.bat"

if not exist "%CINNABAR_ASSETS%" (
    echo No se encontro el pack de assets compilado:
    echo   %CINNABAR_ASSETS%
    echo.
    echo Edita _assets.bat y apunta CINNABAR_ASSETS al archivo .mcbea correcto.
    pause
    exit /b 1
)

set "WGPU_BACKEND=dx11"
set "CINNABAR_RENDER_DIAGNOSTICS=1"
set "WGPU_VALIDATION_INDIRECT_CALL=0"
set "CINNABAR_FORCE_DIRECT_DRAWS=1"
set "CINNABAR_CPU_CULLING=1"
set "CINNABAR_HEAVY_TERRAIN=0"
set "CINNABAR_STABLE_TERRAIN=0"
set "CINNABAR_GRAPHICS_TRACE=1"
set "CINNABAR_GRAPHICS_TRACE_DIR=%LOCALAPPDATA%\Cinnabar\traces\normal"
set "RUST_BACKTRACE=1"
if not exist "%CINNABAR_GRAPHICS_TRACE_DIR%" mkdir "%CINNABAR_GRAPHICS_TRACE_DIR%"

cinnabar.exe --assets "%CINNABAR_ASSETS%" %*
set "CINNABAR_EXIT=%ERRORLEVEL%"
if not "%CINNABAR_EXIT%"=="0" pause
exit /b %CINNABAR_EXIT%
