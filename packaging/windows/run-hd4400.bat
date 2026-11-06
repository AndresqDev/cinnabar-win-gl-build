@echo off
setlocal EnableExtensions
title Cinnabar DX11 - Intel HD 4400
cd /d "%~dp0"

rem Perfil unico de compatibilidad/rendimiento para HD 4400.
rem Usa la ruta de modelos completa y evita indirect/MDI y culling GPU.
set "WGPU_BACKEND=dx11"
set "WGPU_VALIDATION_INDIRECT_CALL=0"
set "CINNABAR_RENDER_MODE=vanilla"
set "CINNABAR_FORCE_DIRECT_DRAWS=1"
set "CINNABAR_CPU_CULLING=1"
set "CINNABAR_HEAVY_TERRAIN=1"
if not defined CINNABAR_HEAVY_MODEL_TERRAIN set "CINNABAR_HEAVY_MODEL_TERRAIN=1"
if not defined CINNABAR_HEAVY_CUBE_TERRAIN set "CINNABAR_HEAVY_CUBE_TERRAIN=0"
if not defined CINNABAR_HEAVY_MAX_RENDER_DISTANCE set "CINNABAR_HEAVY_MAX_RENDER_DISTANCE=4"
set "CINNABAR_RENDER_DIAGNOSTICS=1"
set "CINNABAR_GRAPHICS_TRACE=0"
if not defined CINNABAR_WINDOW_SIZE set "CINNABAR_WINDOW_SIZE=1280x720"
if not defined CINNABAR_RENDER_SCALE set "CINNABAR_RENDER_SCALE=0.75"

if not defined CINNABAR_ASSETS set "CINNABAR_ASSETS=%LOCALAPPDATA%\Cinnabar\assets\compiled\vanilla-v2225.mcbea"
if not exist "%CINNABAR_ASSETS%" if exist "%~dp0assets\compiled\vanilla-v2225.mcbea" set "CINNABAR_ASSETS=%~dp0assets\compiled\vanilla-v2225.mcbea"
if not exist "%CINNABAR_ASSETS%" (
    echo No se encontro el pack de assets compilado:
    echo   %CINNABAR_ASSETS%
    echo.
    echo Copia vanilla-v2225.mcbea a esa ruta o define CINNABAR_ASSETS.
    pause
    exit /b 1
)

echo Logs: %LOCALAPPDATA%\Cinnabar\logs\client.log
cinnabar.exe --vsync --frame-cap 60 --render-mode vanilla --assets "%CINNABAR_ASSETS%" %*
set "CINNABAR_EXIT=%ERRORLEVEL%"
if not "%CINNABAR_EXIT%"=="0" pause
exit /b %CINNABAR_EXIT%
