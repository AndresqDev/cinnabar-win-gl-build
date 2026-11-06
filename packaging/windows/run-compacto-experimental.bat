@echo off
setlocal
title Cinnabar DX11 - compacto experimental
rem Opt-in unicamente: la ruta compacta sigue en validacion en la HD 4400.
set "CINNABAR_HEAVY_MODEL_TERRAIN=0"
set "CINNABAR_HEAVY_CUBE_TERRAIN=0"
call "%~dp0run-optimizado.bat" %*
exit /b %ERRORLEVEL%
