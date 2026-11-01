# Mapa de agentes y propiedad del backend D3D11

| Campo | Valor |
| --- | --- |
| Estado | Puerta 0 — jerarquía operativa propuesta |
| Fecha | 2026-11-01 |
| Rama | `dx11-stabilization` |
| Base | `dea0b3d` |
| Alcance | Coordinación de agentes y propiedad de archivos del backend D3D11 |
| Aprobación de builds | Ninguna; no se publican builds |

Este mapa desarrolla el modelo de agentes del
[plan de estabilización](dx11-backend-plan.md) y lo combina con el
[contrato de capacidades](dx11-capability-contract.md). Define quién puede
tocar cada archivo, cómo se entrega el trabajo y cómo se mergea. No implementa
render.

## 1. Reglas no negociables

1. **Máximo tres capas de hijos**: capa 0 (principal) → capa 1 (programa) →
   capa 2 (dominio) → capa 3 (implementación/verificación). La capa 3 no crea
   una capa 4. No existe una “capa 4” ni un agente auxiliar fuera de la
   jerarquía.
2. **Máximo tres hijos activos por padre.** Un hijo bloqueado sigue contando
   hasta que el padre acepte, rechace o aborte el handoff.
3. **Un worktree y una rama por hijo.** Nada de dos hijos en el mismo checkout
   ni de `target`/`build-dir` compartidos.
4. **Capa 2 y capa 3 no pushean ni mergean.** La capa 1 es la única
   autorizada a preparar/mergear en la fase de implementación, y solo con
   aprobación de capa 0. En este piloto no se mergea nada.
5. **Nadie publica builds** hasta la puerta correspondiente. CI puede compilar
   y probar; un artefacto de usuario no se publica.
6. **Propiedad de archivos.** Un hijo no edita fuera de `docs/` o de su
   worktree aislado (investigación o implementación). En implementación, no
   edita archivos de otro dueño sin handoff.
7. **Sin anidamiento simulado.** Si un agente no tiene la herramienta de
   subagentes, se coordina en paralelo desde su padre y se documenta. No se
   inventan hijos, ramas ni resultados.
8. **Sin `unimplemented!()` en caminos anunciados.** Un camino no soportado
   devuelve error explícito y la app decide.
9. **Una sola capa de parches Bevy.** Los parches de compatibilidad viven en
   un único punto documentado y con test de schedules.

## 2. Jerarquía

| Capa | Rol | Puede lanzar | Entrega | No hace |
| --- | --- | --- | --- | --- |
| 0 | Principal / integración | Capa 1 | Decisiones de programa, aprobación de merges, revisión final | No implementa módulos completos |
| 1 | Dirección de programa | Capa 2 (máx. 3 activos) | Contrato, matriz de puertas, merges revisados | No implementa módulos de dominio |
| 2 | Líder de dominio | Capa 3 (máx. 3 activos) | Investigación, descomposición, revisión de hijos, handoff | No mergea; no pushea; no toca otros dominios |
| 3 | Implementador/verificador | Nadie | Cambio mínimo, prueba, limitaciones, commit | No lanza agentes; no mergea; no pushea |

La capa 3 es terminal. Si un líder de capa 2 necesita más trabajo, divide su
propio paquete en más tareas de capa 3, reasigna prioridades o devuelve el
excedente a capa 1. No se crea una cuarta capa.

### Capa 1 — Dirección de programa

Responsabilidades:

- mantener el contrato de capacidades y la matriz de puertas;
- decidir qué se mergea, en qué orden y con qué evidencia;
- proteger `dx11-stabilization` y el pin de wgpu;
- revisar que cada cambio tenga prueba y que no haya anuncios falsos;
- detectar conflictos de propiedad y serializar archivos calientes;
- vigilar el límite de tres hijos activos por padre;
- rechazar parches que desactiven sistemas sin análisis de schedules.

### Capa 2 — Líderes de dominio

Los seis líderes del plan:

1. **HAL core**: instancia, adaptador, dispositivo, superficie, swapchain,
   fences, resize, device loss y debug layer.
2. **Recursos**: buffers, texturas, formatos, vistas, samplers, copias y
   mapeo.
3. **Shaders y descriptores**: Naga HLSL, FXC, bind groups, layouts y corpus.
4. **Pipelines y comandos**: input layouts, estados, render passes, draws,
   indirect y copias.
5. **Compatibilidad Bevy/Cinnabar**: perfil legacy, gating de plugins,
   limpieza de schedules y menú.
6. **Verificación**: test de contrato, CI, WARP/hardware, goldens,
   instrumentación y rendimiento.

Cada líder investiga primero, escribe un plan corto y después lanza hasta tres
implementadores/verificadores de capa 3.

### Capa 3 — Implementadores y verificadores

Trabajan en worktrees y ramas cortas. Cada entrega incluye:

- cambio mínimo y acotado;
- prueba o ejemplo reproducible;
- limitaciones explícitas;
- comandos de CI ejecutados o pendientes;
- commit identificable;
- impacto en el contrato de capacidades.

No abren una cuarta capa. No mergean. No pushean.

## 3. Piloto de anidamiento: resultado real

### Qué se comprobó

Esta sesión es **capa 1**, lanzada por la sesión principal (capa 0). Antes de
crear documentos se comprobó el catálogo de herramientas de esta sesión:

- `search("subagent")` no devolvió ninguna herramienta de subagentes; solo
  `tools.opencode.models`;
- `search("agent")` devolvió solo `tools.opencode.models`;
- `search("delegate")`, `search("child")`, `search("task")`,
  `search("spawn")`, `search("launch")`, `search("parallel")` y
  `search("worker")` no devolvieron herramientas de delegación;
- los recursos MCP disponibles están vacíos;
- el catálogo de herramientas de Code Mode solo expone OpenCode (modelos,
  recursos MCP, mover/renombrar sesión, worktrees) y Supabase; las
  herramientas genéricas de edición/lectura no incluyen delegación.

### Resultado

| Comprobación | Resultado |
| --- | --- |
| ¿Esta sesión tiene `subagent`? | **No** |
| Agentes de capa 2 lanzados | **0** |
| Agentes de capa 3 lanzados | **0** |
| ¿Capa 2 pudo intentar capa 3? | **No aplica: no hubo capa 2** |
| Worktrees/ramas hijas creadas | **0** |
| Ediciones de capa 2/3 fuera de `docs/` | **0** |
| Builds, pushes o merges de capa 2/3 | **0** |

No se simularon hijos. No se inventaron ramas, commits ni reportes. El bloqueo
está en esta sesión de capa 1, no necesariamente en la sesión principal: la
capa 0 puede tener una herramienta de delegación que esta sesión no recibió.

### Consecuencia

Se aplica la regla del plan: si un subagente no tiene herramienta de
subagentes, su padre coordina en paralelo y no se simula el anidamiento. La
validación de tres capas queda pendiente y debe repetirse en una sesión que sí
tenga `subagent`/Task:

1. capa 1 lanza un líder de capa 2 en modo lectura;
2. capa 2 intenta lanzar un verificador de capa 3 solo para una comprobación
   de API/lectura;
3. capa 3 reporta si pudo ejecutar y qué limitaciones encontró;
4. se registran los tres niveles, los worktrees y la ausencia de ediciones
   fuera de `docs/`/worktree.

Criterio de éxito del piloto real: tres niveles observados, ningún agente de
capa 2/3 edita código, compila, pushea o mergea, y el reporte de capa 3 llega
a capa 1.

## 4. Activación y límite de tres hijos

### Definición de “activo”

Un hijo está activo desde que el padre le asigna el paquete o crea su worktree
hasta que el padre acepta, rechaza o aborta el handoff. Un hijo bloqueado
sigue contando. Un hijo terminado libera el slot solo cuando el handoff queda
registrado.

### Slots actuales

| Padre | Hijos activos | Evidencia |
| --- | --- | --- |
| Capa 0 (principal) | 1 (esta sesión de capa 1) | Sesión actual |
| Capa 1 (esta sesión) | 0 | Sin herramienta de delegación |
| Capa 2 | 0 | No existen líderes lanzados |
| Capa 3 | 0 | No existen implementadores lanzados |

### Orden de activación recomendado

Con seis dominios y un máximo de tres hijos activos, la capa 1 no puede tener
los seis líderes a la vez. Orden propuesto:

1. **Verificación** (slot 1): escribe el test de contrato de Puerta 0 y
   congela los anuncios. Debe ir primero porque su resultado condiciona a
   todos los demás.
2. **HAL core** (slot 2): superficie, dispositivo, fences, resize y device
   loss. Es la dependencia técnica de recursos y pipelines.
3. **Recursos** (slot 3): buffers, texturas, vistas, samplers y copias.
4. Cuando HAL core entregue, **shaders/descriptores** ocupa su slot.
5. Cuando recursos entregue, **pipelines/comandos** ocupa su slot.
6. Cuando shaders y pipelines entreguen, **Bevy/Cinnabar** ocupa el slot.
7. Verificación permanece activa o se reactiva en cada puerta.

La capa 1 puede reordenar por dependencias, pero nunca superar tres líderes
activos. Cada líder, a su vez, puede tener hasta tres hijos de capa 3; esos
hijos no cuentan contra los slots de capa 1.

## 5. Dueños de dominio

| Dominio | Líder | Misión | Interfaces que posee | Rama propuesta | Puerta |
| --- | --- | --- | --- | --- | --- |
| HAL core | `lead-hal-core` | Instancia, adaptador, dispositivo, superficie, swapchain, fences, resize, device loss, debug layer | `Instance`, `Adapter`, `Surface`, `Queue`, `Fence`, ciclo de vida del `Device` | `dx11-hal-core` | 1 |
| Recursos | `lead-resources` | Buffers, texturas 2D/arrays/cubemaps, formatos, vistas, samplers, copias, mapeo | `Buffer`, `Texture`, `TextureView`, `Sampler`, reglas de formato | `dx11-hal-resources` | 1 |
| Shaders/descriptores | `lead-shaders` | Naga HLSL, FXC SM 5.0, bind groups, layouts, registros planos, corpus | `ShaderModule`, `BindGroupLayout`, `PipelineLayout`, `BindGroup` | `dx11-hal-shaders` | 1 |
| Pipelines/comandos | `lead-pipelines` | Input layouts, estados, render passes, clears, draws, indirect, copias | `RenderPipeline`, `CommandBuffer` | `dx11-hal-pipelines` | 1 |
| Bevy/Cinnabar | `lead-bevy-compat` | Perfil `Dx11Legacy`, gating de plugins, schedules, menú, logs, artefactos | Tabla de perfil, puntos de parcheo, settings de Video | `dx11-bevy-compat` | 2 |
| Verificación | `lead-verification` | Test de contrato, CI, WARP/hardware, corpus, goldens, rendimiento | Workflows, ejemplos, evidencia, umbrales | `dx11-verification` | 0–4 |

### Contratos entre dominios

- **HAL core → todos**: garantiza que `Device`/`Queue` existen y que la
  superficie presenta. No implementa recursos ni pipelines.
- **Recursos → shaders/pipelines**: garantiza que un `TextureView` tiene el
  SRV/RTV/DSV correcto y que un `Buffer` tiene los bind flags correctos.
- **Shaders/descriptores → pipelines**: garantiza que un `ShaderModule`
  compila para el layout y que el `BindGroup` se puede bindear.
- **Pipelines/comandos → Bevy**: garantiza que un `RenderPipeline` y un
  `CommandBuffer` ejecutan el draw con el estado declarado.
- **Bevy → verificación**: consume solo la API pública de wgpu; no usa tipos
  HAL ni se salta el contrato.
- **Verificación → todos**: puede leer cualquier archivo y escribir tests/CI,
  pero no implementa render.

## 6. Archivos propietarios

Los archivos del HAL viven en el fork `AndresqDev/wgpu` en el pin
`b85f3a1`; los de Cinnabar viven en este repositorio. El mismo nombre de rama
del plan se usa en el repositorio que corresponda.

| Dominio | Repositorio | Archivos propios |
| --- | --- | --- |
| HAL core | wgpu | `wgpu-hal/src/dx11/instance.rs`, `adapter.rs`, `surface.rs`, `queue.rs`, `mod.rs`; funciones de ciclo de vida, fence y debug de `device.rs`; wiring de `wgpu-core`/`wgpu` solo con aprobación de capa 1 |
| Recursos | wgpu | Funciones de buffer/textura/vista/sampler/copias/mapeo de `device.rs`; `types.rs` para `Buffer`, `Texture`, `TextureView`, `Sampler`; mapas de formato de `conv.rs` |
| Shaders/descriptores | wgpu | `shader.rs`; `types.rs` para `BindGroupLayout`, `PipelineLayout`, `BindGroup`; mapas de sampler y vertex format de `conv.rs` |
| Pipelines/comandos | wgpu | Funciones de pipeline/estado de `device.rs`; `command.rs`; mapas de estado de `conv.rs` |
| Bevy/Cinnabar | Cinnabar | `app/src/app/render_setup/*`, `scripts/patch-bevy-dx11.sh`, `Cargo.toml` (pins), `crates/launcher/src/graphics_backend.rs`, `packaging/windows/run-dx11.bat`; `crates/render` solo si el gating lo exige |
| Verificación | wgpu y Cinnabar | `.github/workflows/dx11-*.yml`, `wgpu-hal/examples/raw-dx11*.rs`, `docs/evidence/*`, harness del contrato |

### Archivos calientes y regla de un solo escritor

| Archivo | Dominios que lo tocan | Regla |
| --- | --- | --- |
| `wgpu-hal/src/dx11/adapter.rs` | HAL core, recursos | HAL core es dueño; recursos propone reglas por formato y `texture_format_capabilities` por handoff |
| `wgpu-hal/src/dx11/device.rs` | HAL core, recursos, pipelines | Un solo escritor por ventana de merge; antes de paralelizar, dividir en `device/resources.rs` y `device/pipelines.rs` |
| `wgpu-hal/src/dx11/types.rs` | Recursos, shaders/descriptores | Un solo escritor o división por tipo antes de dos hijos simultáneos |
| `wgpu-hal/src/dx11/conv.rs` | Recursos, shaders, pipelines | Un solo escritor; los cambios de formato se revisan con verificación |
| `wgpu-hal/src/dx11/mod.rs` | HAL core | Solo HAL core; cualquier otro dominio pide handoff |
| `Cargo.toml` / pins de wgpu | HAL core, Bevy/Cinnabar | Solo capa 1 o un designado; nunca dos hijos |
| `.github/workflows/dx11-*.yml` | Verificación | Verificación es dueña; los demás proponen cambios por handoff |
| `app/src/app/render_setup/*` | Bevy/Cinnabar | Solo Bevy/Cinnabar; no se toca desde HAL |

Si dos dominios necesitan el mismo archivo, la capa 1 serializa: un hijo
entrega, se mergea/rebasea y el siguiente empieza. No se resuelve con merges
cruzados ni con ediciones concurrentes.

## 7. Protocolo de handoff

Cada hijo devuelve un bloque como este:

```text
Handoff <id>
- Padre:
- Hijo / capa:
- Rama:
- Worktree:
- Base (commit):
- Alcance:
- Archivos cambiados:
- Commit:
- Pruebas ejecutadas (comando + resultado):
- Pruebas no ejecutadas (motivo):
- Impacto en el contrato:
- Limitaciones / bloqueos:
- Rebase necesario:
- Revisión solicitada:
```

Pasos:

1. El padre crea el worktree y la rama desde la base acordada.
2. El padre entrega el paquete con alcance, archivos propios, pruebas y
   criterio de aceptación.
3. El hijo confirma que no editará fuera de su alcance y que no compilará,
   pusheará ni mergeará.
4. El hijo investiga, implementa o verifica, y hace commits locales.
5. El hijo entrega el handoff completo; no declara “listo” sin evidencia.
6. El padre revisa el diff, el contrato, las pruebas y la propiedad de
   archivos.
7. Si acepta, la capa 1 prepara el merge; si rechaza, devuelve motivos y el
   hijo corrige.
8. El padre libera el slot solo cuando el handoff queda registrado.

Un hijo que no puede lanzar capa 3 lo declara en el handoff. El padre puede
paralelizar la tarea desde su propia capa, pero no simula un hijo.

## 8. Reglas de merge

1. **Solo capa 1 mergea** en la rama de integración, después de revisar el
   handoff y con aprobación de capa 0. Capa 2 y capa 3 nunca mergean.
2. **Ningún agente de capa 2/3 pushea.** En este piloto no se pushea nada. En
   la fase de implementación, la capa 1 puede preparar el merge local o abrir
   el PR con autorización; el push no lo hace ningún agente de capa 2/3.
3. **Rebase obligatorio** sobre el head actual de `dx11-stabilization` (o del
   fork de wgpu) antes de mergear. Un hijo no refresca su rama solo porque
   está detrás; se comprueba el candidato de integración.
4. **CI verde** para los targets afectados. No se mergea con anuncios falsos,
   `unimplemented!()` en caminos alcanzables ni tests deshabilitados.
5. **Test de contrato verde** para cualquier cambio que toque features,
   downlevel, límites, formatos o superficies.
6. **Propiedad respetada**: sin cambios fuera del paquete, sin archivos de
   otro dueño, sin artefactos, credenciales ni assets.
7. **Sin cuarta capa**: si un cambio necesita más trabajo, se divide en otro
   paquete de capa 3 o se devuelve a capa 1.
8. **Sin builds de usuario**: el merge no publica artefactos hasta la puerta
   correspondiente.

### Orden de merge

1. Test de contrato de Puerta 0 **verde**. Puede desarrollarse antes, pero se
   mergea junto con las correcciones de P1–P4 que lo ponen verde; no se mergea
   un test en rojo.
2. HAL core (superficie, dispositivo, fences, resize, device loss).
3. Recursos (buffers, texturas, formatos, vistas, samplers, copias).
4. Shaders/descriptores (Naga/FXC, layouts, bind groups).
5. Pipelines/comandos (estados, draws, indirect, copias).
6. Bevy/Cinnabar (perfil, plugins, menú).
7. Verificación de cada puerta antes de abrir la siguiente.

La verificación no es un merge final: acompaña cada paso y puede bloquear
cualquier merge.

### Excepción de Puerta 0

El plan declara `dx11-stabilization` protegida y sin cambios directos. La
creación de estos tres documentos se hace directamente en esa rama por
instrucción explícita del usuario para el paquete documental de Puerta 0. Es
una excepción acotada a documentación: no cambia código, no compila, no
publica builds y no mergea trabajo de implementación. Los cambios de código
siguientes vuelven a la regla normal.

## 9. Contradicciones detectadas y resolución

| Punto | Plan / documento | Situación | Resolución |
| --- | --- | --- | --- |
| Rama protegida | `dx11-stabilization` no recibe cambios directos | El usuario pide commit directo de docs | Excepción documental de Puerta 0, registrada arriba |
| Nombre del contrato | El plan entrega `dx11-contract.md` | Se crea `dx11-capability-contract.md` | Se considera el mismo entregable, con nombre más preciso; el plan queda satisfecho por referencia |
| `BUILD-NOTES.md` | Dice que ningún fork de wgpu implementa DX11 | El pin actual tiene un HAL DX11 real | Documento obsoleto de un build GL temporal; no es contrato. Actualizarlo o retirarlo en un cambio aparte |
| “Soportado inicialmente” | El plan lista capacidades objetivo | La fuente implementa solo una parte | El contrato de capacidades manda para el estado actual; el plan describe el objetivo |
| Merge de capa 1 | El plan dice “la capa 1 mergea”; el usuario dice “los hijos no mergean” | Ambigüedad de alcance | Capas 2 y 3 nunca mergean. Capa 1 solo mergea en implementación con aprobación de capa 0; en este piloto nadie mergea |
| Herramienta de subagentes | El plan asume anidamiento | Esta sesión de capa 1 no la tiene | Se aplica el fallback del plan: coordinación desde el padre, sin simular |
| Capa 4 | El plan prohíbe una cuarta capa | Este mapa la prohíbe explícitamente | Sin excepciones; el trabajo extra se divide en capa 3 o se reasigna |

No se introduce ninguna capa adicional. La jerarquía máxima sigue siendo
0 → 1 → 2 → 3.

## 10. Riesgos de coordinación

- **Archivos calientes**: `device.rs`, `types.rs` y `conv.rs` son puntos de
  conflicto. Sin división o serialización, dos hijos se pisan.
- **Anuncios falsos**: un líder puede “desbloquear” una prueba declarando una
  capability no implementada. Verificación debe revisar el contrato, no solo
  el test.
- **Pruebas locales**: el plan prohíbe builds locales en paralelo; la tentación
  de compilar “solo un check” rompe la disciplina. Las pruebas van a CI.
- **Padres inactivos**: un padre que delega y no vigila deja hijos bloqueados.
  Debe actuar en cuanto llega cada handoff.
- **Piloto no validado**: la ausencia de `subagent` en esta sesión impide
  afirmar que las tres capas funcionan. Hay que repetirlo antes de coordinación
  masiva.
- **Mezcla de artefactos**: ANGLE, GL y DX11 comparten nombres de build. La
  compatibilidad debe verificar commit, DLLs y `WGPU_BACKEND`.

## 11. Próximo paso

El primer líder de capa 2 que debe activarse es **Verificación**
(`lead-verification`, rama `dx11-verification`), porque su primer entregable
es el test de contrato de Puerta 0 y la retirada de los anuncios falsos. Sin
ese test, cualquier implementación puede avanzar sobre una base que miente.

Inmediatamente después debe activarse **HAL core** (`lead-hal-core`, rama
`dx11-hal-core`) para superficie, fences reales, resize, device loss y debug
layer. Recursos, shaders, pipelines y Bevy/Cinnabar esperan sus handoffs y los
slots disponibles.
