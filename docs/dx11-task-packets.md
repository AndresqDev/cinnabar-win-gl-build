# Paquetes de tareas iniciales del backend D3D11

| Campo | Valor |
| --- | --- |
| Estado | Puerta 0 — paquetes propuestos, ninguno activo |
| Fecha | 2026-11-01 |
| Rama de integración | `dx11-stabilization` @ `dea0b3d` |
| Backend de referencia | `AndresqDev/wgpu` @ `b85f3a16a9dfc6bbcc9da650755c23f92b6cf636` |
| Compilación local | Prohibida en los agentes. HAL/pruebas DX11: VPS Linux con Podman y ejecución en Windows por SSH. Cinnabar: GitHub Actions. |
| Publicación de builds | Prohibida hasta la puerta correspondiente |

Este documento divide el trabajo de la
[Puerta 0](dx11-backend-plan.md) y de las puertas siguientes en paquetes que
puede recibir un líder de capa 2. Cada paquete tiene alcance, archivos,
pruebas, rama propuesta y criterio de aceptación. No implementa código.

## 0. Resultado del piloto de anidamiento

El piloto real se ejecutó con tres capas: capa 1 lanzó 2 líderes de capa 2
(`lead-verification`, `lead-hal-core`); cada líder lanzó 2 implementadores de
capa 3. Los implementadores no tienen `subagent` y no crearon capa 4. Dos
agotaron su límite de pasos y sus líderes revisaron y commitearon el trabajo.

**P6 y P1 están entregados** en las ramas `dx11-verification` y
`dx11-hal-core`, y la capa 1 mergeó ambos en `dx11-stabilization` con el
contrato verde en WARP. P2–P5 siguen propuestos y no se activan sin nueva
instrucción.

El detalle está en [dx11-agent-map.md](dx11-agent-map.md).

## 1. Reglas comunes

- Un paquete lo recibe un líder de capa 2. Ese líder puede tener hasta tres
  hijos de capa 3 activos.
- Cada hijo trabaja en su propio worktree y rama, con `build-dir` y `target`
  separados.
- Ningún agente de capa 2/3 pushea, mergea ni publica builds. La capa 1
  prepara el merge con aprobación de capa 0.
- Un paquete no toca archivos de otro dueño sin handoff y sin serialización
  de la capa 1.
- Las pruebas DX11/HAL se compilan en el VPS Linux con Podman y se ejecutan
  en la PC Windows por SSH; las de Cinnabar se compilan en GitHub Actions.
  Ningún agente compila Rust en su checkout local. Las pruebas de
  ventana/swapchain requieren una sesión interactiva de Windows.
- Cada entrega incluye commit, diff, pruebas, limitaciones, impacto en el
  contrato y base de rebase.
- No se abre una cuarta capa. El trabajo extra se divide en otro paquete de
  capa 3.

## 2. Resumen y orden de activación

| ID | Paquete | Líder | Rama propuesta | Puerta | Depende de | Activación |
| --- | --- | --- | --- | --- | --- | --- |
| P6 | Verificación y contrato | `lead-verification` | `dx11-verification` | 0–4 | Plan, contrato | **Primero** |
| P1 | HAL core | `lead-hal-core` | `dx11-hal-core` | 1 | P6 (test base) | Segundo |
| P2 | Recursos | `lead-resources` | `dx11-hal-resources` | 1 | P1 | Tercero |
| P3 | Shaders y descriptores | `lead-shaders` | `dx11-hal-shaders` | 1 | P1, P2 | Cuarto |
| P4 | Pipelines y comandos | `lead-pipelines` | `dx11-hal-pipelines` | 1 | P2, P3 | Quinto |
| P5 | Compatibilidad Bevy/Cinnabar | `lead-bevy-compat` | `dx11-bevy-compat` | 2 | P1–P4, P6 | Sexto |

La capa 1 puede solapar P2/P3 o P3/P4 si la propiedad de archivos lo permite,
pero nunca tendrá más de tres líderes de capa 2 activos. Verificación ocupa un
slot permanente o se reactiva en cada puerta.

Estos seis paquetes cubren Puerta 0, Puerta 1 y Puerta 2. La Puerta 3 (mundo
local mínimo) y las siguientes requieren paquetes posteriores; no se adelantan
aquí porque dependen del resultado de las puertas anteriores.

## 3. P6 — Verificación y contrato

**Líder:** `lead-verification`
**Rama propuesta:** `dx11-verification` (fork wgpu y workflows del repo)
**Base:** `b85f3a1` para el HAL; `dx11-stabilization` para docs/CI de Cinnabar
**Puerta:** 0, y acompañamiento de 1–4
**Prioridad:** activar primero

### Alcance

- Escribir el test de contrato de Puerta 0 descrito en
  [dx11-capability-contract.md](dx11-capability-contract.md):
  instantánea de features/downlevel/límites/formatos, positivos, negativos y
  detección de no-ops.
- Convertir en fallo de CI cualquier anuncio sin implementación: BC, MSAA,
  storage, compute, 1D/3D, push constants, view formats, external texture,
  device loss no controlado y `unimplemented!()` alcanzable.
- Mantener la matriz de CI Windows: WARP y hardware; ejemplos `raw-dx11*`;
  workflows `dx11-*.yml`; corpus de shaders; goldens de readback.
- Registrar evidencia en `docs/evidence/` con commit, backend, feature level,
  adaptador, formato y resultado. No commitear capturas crudas ni assets.
- Definir el snapshot del contrato y el procedimiento de revisión cuando
  cambie una capability.
- Medir presupuestos de subida, frames y memoria solo cuando exista una build
  de prueba; no estimar como si fuera medición.

### Archivos propios

- `wgpu-hal/examples/raw-dx11*.rs` y el harness de contrato del fork.
- `.github/workflows/dx11-*.yml`.
- `docs/evidence/*` y documentación de resultados.
- Propuestas de actualización de `docs/dx11-capability-contract.md`; la capa 1
  mantiene el documento y aprueba el cambio.

### Fuera de alcance

- Implementar recursos, shaders, pipelines o compatibilidad Bevy.
- Cambiar el contrato para que pase un test sin implementación.
- Publicar builds o artefactos de usuario.

### Hijos de capa 3 sugeridos (máx. 3)

1. **Test de contrato**: snapshot, positivos/negativos, no-ops.
2. **Matriz CI**: WARP + hardware, workflows, logs y artefactos de evidencia.
3. **Corpus y goldens**: shaders de producción, readbacks, device loss/TDR.

### Pruebas

- `cargo check -p wgpu-hal --features dx11` en Windows CI.
- `cargo run -p wgpu-hal --example raw-dx11 --features dx11`.
- `cargo run -p wgpu-hal --example raw-dx11-resources --features dx11`.
- `cargo run -p wgpu-hal --example raw-dx11-shader --features dx11`.
- `cargo run -p wgpu-hal --example raw-dx11-triangle --features dx11`.
- `cargo run -p wgpu-hal --example raw-dx11-window --features dx11`.
- Test nuevo de contrato: debe fallar contra el pin actual por BC, MSAA,
  storage/compute, 1D/3D y no-ops; después de las correcciones debe pasar.
- `cargo fmt --all -- --check`.

### Criterio de aceptación

- El test de contrato se ejecuta en CI y falla si el backend anuncia algo no
  implementado. No se mergea en rojo: se mantiene en la rama de verificación
  hasta que las correcciones de P1–P4 lo pongan verde.
- Ningún anuncio falso de §4–§7 del contrato queda sin retirar o implementar.
- WARP y hardware producen el mismo contrato, salvo limitaciones documentadas.
- La evidencia incluye commit, adaptador, feature level y resultado.
- No se publica ninguna build.

### Riesgos

- WARP puede aceptar formatos o swapchains que el hardware rechaza; hay que
  probar ambos.
- Un test de ventana puede ser inestable en CI; usar timeout y logs.
- La tentación de “arreglar” el contrato bajando el test en vez de corregir
  el backend.

## 4. P1 — HAL core

**Líder:** `lead-hal-core`
**Rama propuesta:** `dx11-hal-core` (fork wgpu)
**Base:** `b85f3a1`
**Puerta:** 1

### Alcance

- Instancia DXGI, enumeración de adaptadores y apertura de dispositivo.
- Superficie Win32, swapchain BGRA8, adquisición, present, resize por
  recreación y fallback discard.
- Cola inmediata, `submit`, `present`, `Flush`.
- Fences reales con `ID3D11Query` de tipo `D3D11_QUERY_EVENT`; espera de GPU
  real y `wait_for_idle` que no sea solo `Flush`.
- Detección de device removed/reset, error explícito y ruta de recuperación o
  rechazo controlado.
- Debug layer D3D11 opcional para tests; errores de debug no silenciados.
- Información de adaptador útil en logs (nombre, vendor, device, feature
  level, memoria); no inventar strings de driver vacíos.
- Ajustar los límites núcleo del contrato a lo que realmente se implementa.

### Archivos propios

- `wgpu-hal/src/dx11/instance.rs`
- `wgpu-hal/src/dx11/adapter.rs`
- `wgpu-hal/src/dx11/surface.rs`
- `wgpu-hal/src/dx11/queue.rs`
- `wgpu-hal/src/dx11/mod.rs`
- Funciones de ciclo de vida, fence y debug de `device.rs`
- Wiring de `wgpu-core`/`wgpu` solo con aprobación de capa 1
- `raw-dx11` y `raw-dx11-window` en coordinación con P6

### Fuera de alcance

- Creación de buffers, texturas, shaders, bind groups o pipelines.
- Compute, storage, MSAA, multiview.
- Compatibilidad Bevy/Cinnabar.

### Hijos de capa 3 sugeridos (máx. 3)

1. **Superficie y resize**: swapchain, present, recreación, fallback.
2. **Fences y device loss**: query event, wait, `wait_for_idle`, recuperación.
3. **Debug layer y adaptador**: creación debug, logs, límites núcleo.

### Pruebas

- `raw-dx11`: enumera y abre dispositivo.
- `raw-dx11-window`: configura, limpia, presenta N frames y cierra.
- Test nuevo de resize: cambiar tamaño, reconfigurar, presentar y verificar que
  no quedan recursos ni vistas colgadas.
- Test nuevo de fence: submit, wait, comprobar que la GPU terminó antes de
  leer un staging buffer; no aceptar un fence de CPU.
- Test nuevo de device loss: forzar o simular removed/reset y verificar error
  controlado y ausencia de panic.
- Test de debug layer en WARP y hardware: sin errores de D3D11.
- `cargo check`/`fmt` en CI.

### Criterio de aceptación

- Dispositivo y superficie funcionan en 11_0 y 10_1.
- Resize y present no filtran swapchain ni vistas.
- `wait` espera realmente a la GPU; `wait_for_idle` no es un simple `Flush`.
- Device loss devuelve `Lost`/error explícito y no deja la app en un estado
  inconsistente.
- Debug layer limpia en WARP y hardware.
- No hay `unimplemented!()` en los caminos anunciados.
- El contrato de §3 y §7 queda actualizado y verde.

### Riesgos

- El driver Intel puede fallar el flip model; el fallback discard debe seguir
  probado.
- `ID3D11DeviceContext1` puede no estar disponible; los offsets dinámicos
  deben degradar con error, no con datos incorrectos.
- La pérdida de dispositivo no siempre es reproducible; documentar el método
  de simulación.

## 5. P2 — Recursos

**Líder:** `lead-resources`
**Rama propuesta:** `dx11-hal-resources` (fork wgpu)
**Base:** `b85f3a1`
**Puerta:** 1

### Alcance

- Buffers vertex, index, uniform, copy, indirect y map; reglas de bind flags y
  tamaños; storage buffers explícitamente rechazados.
- Texturas 2D, arrays 2D, mips y cubemaps por vista; 1D/3D explícitamente no
  soportadas.
- Vistas SRV/RTV/DSV; formatos compatibles; sin UAV/storage views.
- Samplers lineales, nearest, comparación, address modes y LOD; anisotropía
  solo como detalle no anunciado, nunca como criterio de gating.
- Copias buffer↔buffer, buffer↔textura, textura↔textura; staging, row pitch y
  readback.
- Mapeo read/write, coherencia y errores de rango.
- Matriz de formatos de la allowlist y capacidades reales por formato.
- Retirar `MULTISAMPLE_X4` de las caps y no anunciar BC mientras no exista.

### Archivos propios

- Funciones de buffer/textura/vista/sampler/copia/mapeo de `device.rs`
- `types.rs` para `Buffer`, `Texture`, `TextureView`, `Sampler`
- Mapas de formato y sampler de `conv.rs`
- `adapter.rs::texture_format_capabilities` solo por handoff con HAL core
- `raw-dx11-resources` en coordinación con P6

### Fuera de alcance

- Pipelines, shaders, bind groups y draws.
- Compute, storage, UAV, MSAA, 1D/3D.
- Compatibilidad Bevy.

### Hijos de capa 3 sugeridos (máx. 3)

1. **Buffers y mapeo**: bind flags, staging, dynamic, readback.
2. **Texturas y vistas**: 2D, arrays, cubemaps, depth/stencil, mips.
3. **Matriz de formatos y copias**: allowlist, caps, row pitch, negativos.

### Pruebas

- `raw-dx11-resources`: buffer, textura 2D, depth DSV, sampler, copia y
  readback exacto.
- Test nuevo de arrays 2D y cubemap con readback.
- Test nuevo de formatos: cada formato de la allowlist crea textura y, si
  aplica, sampler/RTV/DSV; los no listados fallan explícitamente.
- Test negativo de BC, MSAA x4, 1D/3D, storage y UAV.
- Test de mapeo: rangos inválidos, doble map, read/write y unmap.
- Test de copias con row pitch no trivial y offsets alineados.

### Criterio de aceptación

- Todos los formatos de la allowlist funcionan según sus caps; ninguno se
  anuncia de más.
- `MULTISAMPLE_X4` no aparece hasta que exista MSAA real.
- BC no se anuncia; si se implementa, hay mapeo y readback.
- 1D/3D y storage fallan con error explícito y no se anuncian.
- Copias y mapeo son exactos, sin corrupción ni lectura de datos viejos.
- El contrato de §6 y §7 queda actualizado y verde.

### Riesgos

- DXGI puede aceptar un formato que luego falla como RTV/DSV; la matriz debe
  probar cada uso.
- El row pitch de staging no es arbitrario; documentar la regla.
- El mapeo de profundidad/stencil tiene aspectos distintos; no asumir que un
  solo SRV cubre todo.

## 6. P3 — Shaders y descriptores

**Líder:** `lead-shaders`
**Rama propuesta:** `dx11-hal-shaders` (fork wgpu)
**Base:** `b85f3a1`
**Puerta:** 1

### Alcance

- Naga IR → HLSL con samplers directos y registros planos `b/t/u/s`.
- Entrada HLSL directa cuando el pipeline lo permita.
- FXC SM 5.0 (`D3DCompile`), cache de bytecode por entry point y errores con
  nombre de shader/entry point.
- Bind group layouts ordenados, pipeline layouts, flattening de registros,
  límites de slots.
- Dynamic uniform offsets con `ID3D11DeviceContext1`, alineación de 256 bytes
  y error explícito si no está disponible.
- Samplers directos, comparación, arrays rechazados.
- Storage buffers/texturas, push constants y acceleration structures
  explícitamente rechazados; no aceptar layouts que después no se bindean.
- Corpus de shaders de producción y negativos de compilación.

### Archivos propios

- `wgpu-hal/src/dx11/shader.rs`
- `types.rs` para `BindGroupLayout`, `PipelineLayout`, `BindGroup`
- Mapas de sampler y vertex format de `conv.rs`
- `raw-dx11-shader` en coordinación con P6

### Fuera de alcance

- Crear pipelines o ejecutar draws (coordinación con P4).
- Compute, storage, push constants, ray tracing.
- Compatibilidad Bevy.

### Hijos de capa 3 sugeridos (máx. 3)

1. **Compilador y Naga**: HLSL, FXC, entry points, cache, errores.
2. **Layouts y offsets**: bind groups, registros, dinámicos, rechazos.
3. **Corpus y negativos**: shaders reales, storage/push constants, límites.

### Pruebas

- `raw-dx11-shader`: layout, bind group, Naga→HLSL, FXC de vertex/fragment.
- Corpus de shaders de producción con todas las defines relevantes; FXC debe
  compilar y reportar fallos concretos.
- Test de dynamic uniform offsets: 256 bytes, rango válido, Context1 ausente.
- Test negativo de storage buffer/texture, sampler array, push constants y
  binding array.
- Test de error de sintaxis/entry point: mensaje con nombre y stage.
- `cargo check`/`fmt` en CI.

### Criterio de aceptación

- El corpus compila en FXC SM 5.0 o falla con un error accionable; no hay
  shaders “verdes” sin bytecode.
- Los samplers directos funcionan y los registros no colisionan.
- Un layout no anunciado no se acepta; un layout aceptado se puede bindear.
- `set_immediates` no es un camino alcanzable con `max_immediate_size == 0`.
- El contrato de §4 y §5 queda actualizado y verde.

### Riesgos

- FXC es más estricto que Naga; el corpus debe cubrir loops, derivadas,
  arrays y precisión.
- Los límites de registros D3D11 son bajos; el flattening puede agotar slots.
- `Context1` no siempre está; no degradar a offsets incorrectos.

## 7. P4 — Pipelines y comandos

**Líder:** `lead-pipelines`
**Rama propuesta:** `dx11-hal-pipelines` (fork wgpu)
**Base:** `b85f3a1`
**Puerta:** 1

### Alcance

- Render pipelines vertex/fragment, input layouts `LOC<n>`, step mode.
- Rasterizer, depth/stencil, blend, write mask, topologías, culling, depth
  bias/clamp, scissor, viewport.
- Múltiples render targets (hasta el límite anunciado) y profundidad.
- Render passes, clears, draws directos, indexados, indirectos directos e
  indexados, copias y clears de buffer/textura.
- Rechazo explícito de compute, multiview, MSAA/resolve, mesh/task,
  indirect count y slices 3D.
- Validación de que pipeline y render pass coinciden en formatos y sample
  count; no dibujar con estado inconsistente.
- Readback exacto de color y profundidad para las pruebas.

### Archivos propios

- Funciones de pipeline/estado de `device.rs`
- `wgpu-hal/src/dx11/command.rs`
- Mapas de estado de `conv.rs`
- `raw-dx11-triangle` y ejemplos nuevos en coordinación con P6

### Fuera de alcance

- Compute, storage, MSAA, multiview, mesh shaders.
- Recursos y shaders (se consumen de P2/P3).
- Compatibilidad Bevy.

### Hijos de capa 3 sugeridos (máx. 3)

1. **Estados y pipelines**: input layout, raster, depth, blend, topologías.
2. **Comandos y draws**: passes, clears, directos, indexados, indirectos.
3. **Readback y negativos**: dos RT, profundidad, indirect count, MSAA.

### Pruebas

- `raw-dx11-triangle`: triángulo offscreen, UBO, readback exacto.
- Test nuevo de dos render targets + profundidad con readback.
- Test nuevo de clears y viewport/scissor.
- Test nuevo de draw indexado e indirecto; comparar con el directo.
- Test negativo de MSAA, multiview, resolve, indirect count, mesh y compute.
- Test de mismatch pipeline/render pass: error, no draw silencioso.
- `cargo check`/`fmt` en CI.

### Criterio de aceptación

- El triángulo y los dos RT producen píxeles exactos.
- Los draws declarados no usan `unimplemented!()` ni no-ops.
- Lo no soportado falla explícitamente antes de dibujar.
- El estado del pipeline y el render pass coinciden; no hay corrupción.
- El contrato de §3–§7 queda actualizado y verde.

### Riesgos

- El contexto inmediato de D3D11 ejecuta al codificar; el orden de passes debe
  ser determinista.
- La validación de formatos y sample count es fácil de olvidar.
- El readback de profundidad requiere formato y aspecto correctos.

## 8. P5 — Compatibilidad Bevy/Cinnabar

**Líder:** `lead-bevy-compat`
**Rama propuesta:** `dx11-bevy-compat` (Cinnabar)
**Base:** `dx11-stabilization`
**Puerta:** 2

### Alcance

- Perfil `Dx11Legacy` como tabla de capacidades, no como parches dispersos:
  - `legacy_common`: sin HDR/Enhanced, sin MSAA, sin view formats, escena
    sRGB única.
  - `gl_only`: arrays padding, `GL_COMPAT`, compile context, workers GL,
    fences GL.
  - `dx11_only`: sin depth pyramid, sin compute, sin storage textures, sin
    timestamps, sin pipeline cache.
  - `native_only`: capacidades avanzadas desactivadas.
- Desactivar HDR, MSAA, SMAA, sombras, motion blur, clip masks y shaderpacks
  en DX11.
- Mantener `MipGenerationPlugin` si Cinnabar lo exige, con la textura dummy
  sin storage (ya hay parche en `scripts/patch-bevy-dx11.sh`).
- Limpieza segura de plugins/sistemas: sin schedules rotos, sin recursos
  requeridos, sin panic de limpieza.
- Menú, panorama, UI, texto e imágenes con captura no negra.
- Logs que identifiquen `backend: Dx11`, adaptador y feature level.
- No mezclar DLLs, ejecutables ni perfiles de builds distintas; ANGLE queda
  congelado como fallback.

### Archivos propios

- `app/src/app/render_setup/*`
- `scripts/patch-bevy-dx11.sh`
- `Cargo.toml` (pins de wgpu/Bevy)
- `crates/launcher/src/graphics_backend.rs`
- `packaging/windows/run-dx11.bat`
- `crates/render` solo si el gating lo exige; no se toca el backend D3D11 ni
  ANGLE

### Fuera de alcance

- Implementar el HAL DX11.
- Cambiar ANGLE o el código de render D3D11.
- Publicar una build de usuario.

### Hijos de capa 3 sugeridos (máx. 3)

1. **Perfil y gating**: tabla `Dx11Legacy`, features y plugins.
2. **Limpieza de schedules**: plugin dummy, recursos, tests de menú.
3. **Captura y logs**: menú no negro, 5 minutos, backend Dx11, artefactos.

### Pruebas

- Test de schedule: los sistemas retirados no están programados; el recurso
  asociado no se requiere; no hay panic de limpieza.
- Test de la textura dummy de depth pyramid: sin `STORAGE_BINDING`.
- Captura del menú/UI con `WGPU_BACKEND=dx11`: luminancia/color no negro,
  sin corrupción.
- 5 minutos de menú sin device removed.
- Log que identifique `backend: Dx11`, adaptador y feature level.
- `ui-snapshot` para estados aislados cuando aplique; cliente headless para
  el menú real.
- `cargo check`/`fmt` en CI; no build local.

### Criterio de aceptación

- El perfil `Dx11Legacy` se aplica y bloquea HDR/MSAA/SMAA/sombras/motion
  blur/clip masks/shaderpacks.
- El menú llega a pantalla sin negro ni corrupción.
- 5 minutos sin device removed.
- No hay recursos storage creados por plugins de Bevy.
- No se mezclan artefactos de ANGLE/GL/DX11.
- La Puerta 2 se cierra solo con esta evidencia; no se publica build.

### Riesgos

- Bevy puede requerir sistemas que Cinnabar retira; el test de schedules debe
  cubrirlo.
- El parche de la textura dummy depende de la revisión de Bevy; verificar el
  patrón antes de aplicar.
- Una captura “no negra” puede ocultar corrupción de color; comparar
  luminancia y regiones, no solo el promedio.
- Si el resource pack usa BC y P2 no lo implementa, el menú no llega. Decidir
  implementación BC o ruta de assets sin comprimir antes de la Puerta 2.

## 9. Matriz de dependencias

| Necesidad | Provee | Consume | Bloquea si falta |
| --- | --- | --- | --- |
| Test de contrato | P6 | P1–P5 | No se puede cerrar Puerta 0 |
| Dispositivo y superficie | P1 | P2–P5 | Nada se puede probar en ventana |
| Buffer/textura/vista/sampler | P2 | P3, P4 | No hay bind groups ni pipelines reales |
| Shader/layout/bind group | P3 | P4 | No hay pipeline completo |
| Render pass/draw | P4 | P5 | No hay menú |
| Perfil y gating | P5 | P6 | No hay Puerta 2 |
| Evidencia y CI | P6 | Todos | No hay merge |

## 10. Criterio de aceptación común

Un paquete se acepta solo si:

1. el handoff está completo y el commit es identificable;
2. el diff respeta la propiedad de archivos;
3. las pruebas del paquete pasan en CI, o se documenta exactamente qué falta
   y por qué;
4. el contrato de capacidades se actualiza si cambia un anuncio;
5. no hay `unimplemented!()`, panic ni no-op silencioso en un camino
   anunciado;
6. no se pushea, no se mergea y no se publica build;
7. no se crea una cuarta capa.

## 11. Estado actual y siguiente paso

**P6 — Verificación** y **P1 — HAL core** entregaron su primera fase. La rama
`dx11-stabilization` de `AndresqDev/wgpu` @ `9bc4fc056` contiene:

- `wgpu-hal/tests/dx11_contract.rs` y `.github/workflows/dx11-contract.yml`;
- correcciones de Puerta 0 en `conv.rs`, `adapter.rs`, `types.rs`, `device.rs`;
- `raw-dx11-resize.rs` y fence real `D3D11_QUERY_EVENT`;
- workflow `dx11-hal-core-check.yml` con check, tests puros y ejemplos.

Evidencia verde: contrato `41308953949` (WARP), HAL core `41309216229`
(incluye `raw-dx11-resources` y `raw-dx11-triangle` con readback). Puerta 0
cerrada en WARP; hardware, debug layer y recuperación de device loss siguen
abiertos.

Siguiente: **P2 — Recursos**, después **P3 — Shaders/descriptores**, después
**P4 — Pipelines/comandos**. P1 mantiene follow-ups de debug layer, device
loss, alignments y paridad con hardware. Nada de esto se activa sin una nueva
instrucción de capa 0. Sin la Puerta 1 completa, no se toca Bevy/Cinnabar ni
se publica una build.
