@echo off
rem Edita esta línea si tu pack de assets compilado está en otra ruta.
if not defined CINNABAR_ASSETS set "CINNABAR_ASSETS=%LOCALAPPDATA%\Cinnabar\assets\compiled\vanilla-v2225.mcbea"
if not exist "%CINNABAR_ASSETS%" if exist "%~dp0assets\compiled\vanilla-v2225.mcbea" set "CINNABAR_ASSETS=%~dp0assets\compiled\vanilla-v2225.mcbea"
rem Cota de radio para el modo pesado. 0 la desactiva.
if not defined CINNABAR_HEAVY_MAX_RENDER_DISTANCE set "CINNABAR_HEAVY_MAX_RENDER_DISTANCE=4"
rem Fallback opcional al antiguo camino de referencias de modelo.
if not defined CINNABAR_HEAVY_MODEL_TERRAIN set "CINNABAR_HEAVY_MODEL_TERRAIN=0"
