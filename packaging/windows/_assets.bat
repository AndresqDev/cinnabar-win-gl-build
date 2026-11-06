@echo off
rem Edita esta línea si tu pack de assets compilado está en otra ruta.
if not defined CINNABAR_ASSETS set "CINNABAR_ASSETS=%LOCALAPPDATA%\Cinnabar\assets\compiled\vanilla-v2225.mcbea"
if not exist "%CINNABAR_ASSETS%" if exist "%~dp0assets\compiled\vanilla-v2225.mcbea" set "CINNABAR_ASSETS=%~dp0assets\compiled\vanilla-v2225.mcbea"
rem Cota de radio para el modo pesado. 0 la desactiva.
if not defined CINNABAR_HEAVY_MAX_RENDER_DISTANCE set "CINNABAR_HEAVY_MAX_RENDER_DISTANCE=4"
rem Ruta estable de modelos completa por defecto. 0 activa el compacto opt-in.
if not defined CINNABAR_HEAVY_MODEL_TERRAIN set "CINNABAR_HEAVY_MODEL_TERRAIN=1"
rem 1 activa el experimento de cubos+greedy; tiene prioridad sobre las otras dos rutas.
if not defined CINNABAR_HEAVY_CUBE_TERRAIN set "CINNABAR_HEAVY_CUBE_TERRAIN=0"
