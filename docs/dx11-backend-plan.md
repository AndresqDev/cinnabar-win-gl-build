# Plan de estabilización del backend D3D11

## Objetivo

Llevar Cinnabar a una experiencia jugable estable en la Intel HD Graphics 4400 usando un backend D3D11 nativo en `wgpu-hal`, sin ANGLE, sin D3D12 y sin depender de parches ad hoc que desmiembren Bevy o el renderer.

La prioridad no es "hacer que arranque una build". Es construir un camino con capacidades declaradas honestamente, pruebas de conformidad y una integración vertical que llegue a menú, mundo y gameplay sin regresiones ocultas.

## Reglas del proyecto

1. El backend declara únicamente lo que implementa. No se anuncian features, downlevel flags, formatos ni límites que todavía no funcionan.
2. Un camino alcanzable no usa `unimplemented!()` ni panics como fallback. Devuelve un error explícito y la app decide.
3. Los parches de compatibilidad de Bevy se hacen en un único lugar, documentados y con una prueba que demuestre que los sistemas retirados no dejan recursos o schedules rotos.
4. No se mezclan ejecutables, DLLs o perfiles de builds distintas.
5. Toda compilación Rust se ejecuta en GitHub Actions.
6. La rama de referencia estable y las ramas de implementación están separadas.
7. No se publica una build para el usuario hasta pasar la puerta correspondiente.

## Modelo de agentes

Se usa una jerarquía de como máximo tres capas:

### Capa 1 — Dirección de programa

Un agente responsable de:

- mantener el contrato de capacidades;
- decidir qué se mergea y en qué orden;
- proteger la rama de integración;
- revisar que cada cambio tenga prueba;
- rechazar parches que desactiven sistemas sin análisis de consecuencias.

Entregables: `dx11-contract.md`, matriz de puertas, resolución de conflictos, merges.

### Capa 2 — Líderes de dominio

Cada líder investiga primero y después coordina implementadores de capa 3:

1. Núcleo HAL: instancia, adaptador, dispositivo, superficie, swapchain, fences, resize.
2. Recursos: buffers, texturas 1D/2D/3D/arrays, formatos, vistas, samplers, copias, mapeo.
3. Descriptores y shaders: bind groups, layouts, Naga HLSL directo, FXC, corpus de shaders.
4. Pipelines y comandos: input layouts, estados, render passes, draws, indirect, dynamic offsets.
5. Compatibilidad Bevy/Cinnabar: perfil legacy, limpieza de plugins, streaming, culling CPU.
6. Verificación: CI, pruebas de conformidad, goldens, instrumentación, rendimiento.

### Capa 3 — Implementadores y verificadores

Trabajan en worktrees y ramas cortas, con propiedad de archivos para evitar conflictos. Cada entrega incluye:

- cambio mínimo;
- prueba o ejemplo reproducible;
- limitaciones explícitas;
- comandos de CI;
- commit identificable.

No se abre una cuarta capa. Un líder de capa 2 que necesite más trabajo divide su propio paquete en ramas o tareas dentro de la capa 3.

## Estrategia de ramas

- `dx11-stabilization`: rama de integración, protegida. No recibe cambios directos.
- `dx11-hal-core`: instancia, adaptador, dispositivo, superficie, swapchain, fences.
- `dx11-hal-resources`: buffers, texturas, vistas, samplers, copias.
- `dx11-hal-shaders`: Naga HLSL, FXC, layouts, bind groups.
- `dx11-hal-pipelines`: pipelines, estados, draws, indirect.
- `dx11-bevy-compat`: perfil legacy, plugins, gating y pruebas de menú.
- `dx11-verification`: CI y corpus de shaders.
- `angle-compile-context`: se conserva congelada como fallback de referencia.

Cada rama de implementación se rebasa sobre `dx11-stabilization`; solo el agente de capa 1 mergea.

## Contrato de capacidades

El adaptador D3D11 debe publicar un contrato explícito:

### Soportado inicialmente

- D3D feature level 11_0 y 10_1.
- Superficie BGRA8 UNORM/SRGB.
- Buffers vertex, index, uniform, copy, map.
- Texturas 2D, arrays 2D y cubemaps cuando estén implementados.
- Samplers lineales, nearest, comparación, address modes.
- Render targets, depth/stencil, blends, rasterizer states.
- Render pipelines y draws directos.
- Copias buffer↔buffer, buffer↔textura y textura↔textura.
- FXC SM 5.0 y Naga HLSL con samplers directos.
- `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` solo para formatos consultados y probados.

### No anunciar hasta implementar

- Compute shaders y dispatch.
- Storage textures y UAVs.
- Dynamic storage buffer offsets.
- Multiview, MSAA, view formats, surface view formats.
- Timestamp queries, pipeline cache, multi-draw indirect count.
- Ray tracing, mesh shaders, subgroup, f16/i16/i64.
- `Stencil8` real.

El perfil de Cinnabar debe consultar el contrato y no intentar un camino no soportado.

## Puertas de aceptación

### Puerta 0 — Congelación y contrato

- Congelar la rama experimental.
- Documentar capacidades reales y limitaciones.
- Prohibir nuevas builds de usuario hasta terminar la puerta 1.
- Añadir un test que falle si el backend anuncia algo no implementado.

### Puerta 1 — Conformidad del HAL

Ejemplos y tests obligatorios:

- instancia, enumeración y apertura de dispositivo;
- offscreen triangle con readback exacto;
- textura con sampler y UBO;
- profundidad y dos render targets;
- array/cubemap;
- buffers con mapeo y copias;
- resize de swapchain;
- fences y espera de GPU;
- device loss controlado;
- debug layer sin errores en WARP y en hardware.

Ningún ejemplo usa `unimplemented!()`.

### Puerta 2 — Menú Bevy/Cinnabar

- Perfil `Dx11Legacy` activo.
- Sin HDR, MSAA, SMAA, sombras, motion blur, clip masks ni shaderpacks.
- Sin `MipGenerationPlugin` activo de forma incompatible; si se conserva, su textura dummy no usa storage.
- Menú, panorama, UI, texto e imágenes con readback/screenshot no negro.
- 5 minutos sin device removed ni corrupción de color.
- Logs que identifiquen `backend: Dx11`.

### Puerta 3 — Mundo local mínimo

- Mundo plano o radio mínimo.
- Culling CPU (`CINNABAR_CPU_CULLING=1` si corresponde).
- Chunks por anillos/sectores, nunca todos de golpe.
- Buffers de storage de solo lectura si son necesarios para el layout compacto.
- Sin actores, partículas ni shaderpacks al principio.

### Puerta 4 — Mundo vanilla

- Terreno natural, actores, partículas, nametags, UI, far terrain y atmósfera.
- Sin shaderpacks, HDR, Enhanced ni MSAA.
- Comparación visual contra Vulkan/DX12.
- Streaming progresivo y presupuesto de subida.
- Sin TDR durante una sesión prolongada.

### Puerta 5 — Gameplay completo

- Movimiento, cámara, interacción, inventario, chat, menús y transiciones.
- Entrada/salida de mundo y cambio de dimensión.
- Estabilidad de memoria y de recursos.
- Validación final en la HD 4400 nativa.

### Puerta 6 — Compute opcional

Solo después de gameplay estable:

- compute pipelines;
- UAV/storage textures;
- culling GPU;
- readback de oclusión;
- benchmarks y comparación con CPU culling.

## Orden técnico de implementación

1. **Superficie y presentación**: resize, fences, formato, present modes, recuperación.
2. **Recursos completos**: 1D/3D/arrays, mips, staging, mapeo, formatos de profundidad y stencil limitados.
3. **Vistas**: SRV/RTV/DSV/UAV cuando corresponda; UAV fuera del camino inicial.
4. **Bind groups**: registros planos `b/t/u/s`, layouts, offsets uniformes dinámicos, sin storage inicialmente.
5. **Shaders**: corpus real de Cinnabar → HLSL directo → FXC; cache de bytecode; errores con nombre de shader/entry point.
6. **Pipelines**: input layouts, estados, múltiples render targets, depth, blends, topologías.
7. **Comandos**: orden de passes, clears, draws, indirect directo, resolución de estado.
8. **Bevy/Cinnabar**: perfil de compatibilidad y limpieza segura de plugins/sistemas.
9. **Mundo**: CPU culling, storage buffers de lectura, streaming progresivo.
10. **Compute**: solo tras estabilidad de mundo.

## Compatibilidad Bevy/Cinnabar

El perfil `Dx11Legacy` debe ser una tabla de capacidades, no una colección de parches dispersos:

- `legacy_common`: sin HDR/Enhanced, sin MSAA, sin view formats, escena sRGB única.
- `gl_only`: arrays padding, `GL_COMPAT`, compile context, workers GL, fences GL.
- `dx11_only`: sin depth pyramid, sin compute, sin storage textures, sin timestamps, sin pipeline cache.
- `native_only`: características avanzadas desactivadas por el perfil.

Los plugins que se desactiven deben tener una prueba que verifique:

- el sistema no programado;
- el recurso asociado no se requiere;
- no hay panic de limpieza de schedules;
- la app llega al menú.

## Pruebas y observabilidad

- `cargo check` y `cargo fmt --check` en CI.
- Ejemplos raw para cada capacidad.
- Corpus de shaders de producción con todas las definiciones relevantes; FXC debe compilar y registrar fallos concretos.
- Readbacks de píxeles para color, profundidad, arrays y copias.
- Screenshot del menú y comparación de luminancia/color para detectar negro o corrupción.
- Logs estructurados: backend, adaptador, feature level, formato, pase, pipeline y error.
- Detección de `device_removed`, TDR y resource leaks.
- Presupuestos de subida y de chunks medidos, no estimados.

## Riesgos y mitigaciones

- **API wgpu-hal extensa**: portar patrones de `dx12` y `gfx-backend-dx11`, no improvisar fuera de los traits.
- **Bevy espera funciones no soportadas**: capability gating y perfil legacy; nunca fingir features.
- **FXC/Naga falla en shaders reales**: corpus previo y errores accionables; no avanzar a mundo sin corpus verde.
- **Intel TDR**: streaming progresivo, límites de subida, presupuesto de frames y CPU culling.
- **Parches de Bevy**: un solo punto de parcheo, documentado, con test de schedule.
- **Mezcla de artefactos**: nombres de build, commit y DLLs verificados; ANGLE queda separado.

## Criterio de éxito

Una build Windows que:

- no carga `libEGL.dll` ni `libGLESv2.dll`;
- selecciona `Dx11` y muestra `backend: Dx11`;
- abre el menú y la UI sin negro ni corrupción;
- carga un mundo local con terreno y jugabilidad básica;
- se mantiene estable en la HD 4400;
- no anuncia compute/storage hasta que existan;
- puede volver a ANGLE o GL como fallback sin mezclar binarios.
