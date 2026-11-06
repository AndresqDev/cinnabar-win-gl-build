@echo off
setlocal
title Cinnabar DX11 - optimizado
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

rem Ruta estable de modelos completos, draws directos y cota estricta para DX11.
rem Un lanzador opt-in puede definir estas variables antes de llamar a este archivo.
set "WGPU_BACKEND=dx11"
set "WGPU_VALIDATION_INDIRECT_CALL=0"
set "CINNABAR_FORCE_DIRECT_DRAWS=1"
set "CINNABAR_CPU_CULLING=1"
set "CINNABAR_HEAVY_TERRAIN=1"
set "CINNABAR_STABLE_TERRAIN="
set "CINNABAR_HEAVY_MAX_RENDER_DISTANCE=4"
if not defined CINNABAR_HEAVY_MODEL_TERRAIN set "CINNABAR_HEAVY_MODEL_TERRAIN=1"
if not defined CINNABAR_HEAVY_CUBE_TERRAIN set "CINNABAR_HEAVY_CUBE_TERRAIN=0"

rem Sin diagnosticos ni trazas para no penalizar el rendimiento.
set "CINNABAR_RENDER_DIAGNOSTICS=0"
set "CINNABAR_GRAPHICS_TRACE=0"

cinnabar.exe --assets "%CINNABAR_ASSETS%" %*
set "CINNABAR_EXIT=%ERRORLEVEL%"
if not "%CINNABAR_EXIT%"=="0" pause
exit /b %CINNABAR_EXIT%
