# Intel HD Graphics 4400 — límites y fallos documentados para la implementación DX11

| Campo | Valor |
| --- | --- |
| Fecha | 2026-11-06 |
| Estado | Corrección implementada en la rama; pendiente de build CI y validación nativa |
| Objetivo | Documentar lo que el HD 4400 no soporta o soporta mal, los fallos conocidos que afectan a Cinnabar y las correcciones necesarias en la ruta DX11 |
| Alcance | Intel HD Graphics 4400 (Haswell GT2), Windows nativo, Linux/Mesa y Wine/Proton/DXVK |
| Documento relacionado | [`dx11-capability-contract.md`](dx11-capability-contract.md) |

> Este documento separa tres cosas que no son equivalentes:
>
> 1. lo que el hardware Haswell no puede hacer;
> 2. lo que el driver de Intel/Mesa hace mal;
> 3. lo que el backend DX11 de wgpu/Cinnabar no implementa o decide no usar.
>
> Decir “el HD 4400 no soporta X” sin aclarar la capa es incorrecto. La mayoría de los problemas de Cinnabar son de la capa 2 o 3, no de que el silicio carezca de la función.

## 1. Resumen ejecutivo

El HD 4400 es una GPU **Haswell GT2 de 2013**. Sí soporta la base de DX11 que necesita Cinnabar, pero está en una zona problemática:

- **DX11/FL11_1:** soportado por el hardware; el backend de Cinnabar solo contempla FL11_0/10_1 en su contrato DX11.
- **DX12:** Intel retiró el soporte de DirectX 12 en los drivers de 4.ª generación a partir de `15.40.44.5107`. No es una ruta viable.
- **Vulkan en Linux:** existe vía Mesa/ANV, pero Mesa avisa: **“Haswell Vulkan support is incomplete”**. No es una base estable para validar.
- **Vulkan/Windows:** no es la ruta oficial de esta GPU; la ruta soportada es DX11.
- **Indirect drawing:** hay fallos documentados en iGPU Intel; en particular, corrupción de vértices/materiales/objetos con Bevy + Vulkan + indirect. La solución documentada fue `NoIndirectDrawing`.
- **MDI (multi-draw indirect):** D3D11 no tiene MDI nativo y hay reportes de MDI roto en Intel integrados en OpenGL.
- **Naga/Intel:** un `||`/`&&` que no cortocircuitaba provocaba una lectura fuera de rango; Intel reaccionaba con corrupción. Fue corregido en Bevy `#21475` y wgpu `#7339`.
- **Backend DX11 de Cinnabar:** el contrato v1 no implementa compute/storage buffers/UAV, MSAA, multi-draw indirect count ni compresión BC; por tanto el culling GPU no puede ser una garantía en esa ruta.

La hipótesis más fuerte para el último bug visual es:

> **La ruta de culling/indirect de modelos es el cambio que rompió la estabilidad en Intel, no necesariamente el formato compacto de 12 bytes.**

El formato compacto puede ser correcto y aun así fallar si el draw indirecto que lo acompaña entra en el camino defectuoso del driver. Wine/RADV no lo reproduce porque nuestro test usó `draw_mode=Direct`; en Intel hay que comprobar si entra por `MultiDrawIndirect` o por otro camino.

## 2. Línea base del hardware

| Elemento | Dato |
| --- | --- |
| GPU | Intel HD Graphics 4400 |
| Generación | Haswell GT2 (Gen7.5) |
| Lanzamiento | 2013 |
| Shaders / TMUs / ROPs | 160 / 8 / 4 |
| Direct3D | DX11, feature level 11_1 (hardware) |
| OpenGL | 4.3 nominal |
| OpenCL | 1.2 |
| Vulkan Windows | No es la ruta oficial de esta GPU |
| Vulkan Linux | Mesa/ANV, soporte incompleto en Haswell |
| DX12 | Retirado por Intel en drivers de 4.ª generación |
| Driver Windows 10 | Familia 15.40.x; rama antigua, sin soporte moderno |

Fuentes: Intel Support, TechPowerUp y el contrato DX11 del repositorio.

## 3. Matriz de capacidades: hardware, driver y backend

### 3.1 Lo que el hardware no tiene

Estas ausencias son de hardware/arquitectura y no se arreglan con un flag:

| Capacidad | Estado | Consecuencia para Cinnabar |
| --- | --- | --- |
| Ray tracing por hardware | No | No usar ray tracing ni aceleración por hardware |
| Mesh/task shaders | No | No usar pipelines mesh/task |
| DX12 moderno | No disponible con el driver actual | No usar DX12 como fallback |
| Vulkan completo en Linux | Incompleto en Haswell | No validar Vulkan como si fuera DX11 |
| Memoria/anchura de banda de gama alta | Muy limitada | Evitar MSAA, HDR, shaderpacks y overdraw |

### 3.2 Lo que el driver/de la plataforma no garantiza

| Área | Riesgo documentado | Implicación |
| --- | --- | --- |
| Indirect drawing en iGPU Intel | Corrupción de vértices/materiales/objetos; `NoIndirectDrawing` lo evita | Evitar draws indirectos para terreno en esta GPU |
| MDI en Intel integrado | Reportes de fallos/crash en OpenGL | No usar MDI como camino estable |
| Vulkan Haswell | Soporte incompleto | No usar Vulkan para la validación objetivo |
| DXVK en HD 4000/4400 | Soporte incompleto/no garantizado | Wine+DXVK no es evidencia de estabilidad Intel |
| DX12 en 4.ª gen | Retirado por Intel | No usar DX12 |
| Driver Windows Haswell | Rama antigua sin actualizaciones modernas | No asumir correcciones de driver recientes |
| Naga/Intel con `&&`/`||` | Un operador lógico podía no cortocircuitar y provocar OOB | Auditar shaders y usar versiones corregidas |

### 3.3 Lo que el contrato DX11 de Cinnabar no implementa

El contrato actual del backend (`docs/dx11-capability-contract.md`) es explícito: la Puerta 0 está cerrada en WARP y la paridad con hardware sigue pendiente. En su versión v1:

- feature level solicitado: **11_0/10_1**, no 11_1;
- **sin compute shaders ni dispatch**;
- **sin storage buffers/UAV/storage textures** en el layout;
- límites de storage y compute en **0**;
- **sin MSAA**;
- **sin multi-draw indirect count**;
- **sin compresión BC**;
- texturas 1D/3D no soportadas;
- sin view formats ni surface view formats;
- sin timestamp queries, pipeline cache ni multiview.

Consecuencia directa: **el culling GPU por compute y los draws de modelos con storage/indirect no pueden considerarse una ruta disponible en el contrato DX11 v1**. Si el binario usa una ruta distinta, hay que demostrarlo con el `draw_mode` y las capacidades reales del adaptador, no asumirlo.

### 3.4 Lista operativa de cosas que no debe usar la ruta HD 4400

| Función / camino | Motivo | Decisión para HD 4400 |
| --- | --- | --- |
| `MULTI_DRAW_INDIRECT` | D3D11 no lo ofrece de forma nativa; backend/contrato no lo garantiza | No usar; emitir draws directos |
| `MULTI_DRAW_INDIRECT_COUNT` | No implementado en el contrato DX11 v1 | No usar |
| `INDIRECT_FIRST_INSTANCE` | No hay base-instance fiable en indirect D3D11; drivers Intel problemáticos | No usar; `first_instance = 0` o draw directo |
| Compute shaders / dispatch | El contrato DX11 v1 no los implementa | CPU culling |
| Storage buffers / UAV / storage textures | El contrato DX11 v1 no los implementa | Streams CPU/GPU sin storage |
| MSAA x2/x4/x8/x16 | El backend/contrato no lo expone; el perfil lo desactiva | `sample_count = 1` |
| Compresión BC | El backend no mapea BC1–BC7 | Evitar texturas BC en el perfil |
| Texturas 1D/3D | El backend solo implementa 2D | Usar atlas 2D |
| Multi-view / view formats | No soportado por el contrato | Una vista sRGB |
| Timestamp queries / pipeline cache | No implementado | No usarlo para decisiones |
| DX12 | Intel retiró DX12 en 4.ª generación | No usar DX12 |
| Vulkan nativo como validación | Haswell Vulkan incompleto | Validar en DX11 |
| HDR / Enhanced / shaderpacks | Fuera del perfil de estabilidad y muy caros | Desactivados |
| MDI en OpenGL/ANGLE | Reportes de corrupción/crash en Intel integrado | No usar ANGLE como atajo |

`INDIRECT_EXECUTION` no es equivalente a “indirect moderno”: permite draws indirectos básicos, pero no garantiza `first_instance`, MDI ni MDI count.

## 4. Fallos documentados y cómo se manifiestan

### F-01 — Corrupción con indirect drawing en iGPU Intel

**Síntomas:**

- objetos que desaparecen;
- materiales que se cruzan entre mallas;
- posiciones de vértices aleatorias;
- geometría “abstracta”, caras que parecen plegarse o desaparecer;
- resultados distintos entre ejecuciones.

**Condición:** Intel integrado + camino indirecto. En los reportes de Bevy se reproducía en Vulkan y desaparecía con `NoIndirectDrawing`.

**Evidencia:**

- Bevy #19000: https://github.com/bevyengine/bevy/issues/19000
- Bevy #18904: https://github.com/bevyengine/bevy/issues/18904

**Corrección inmediata:** forzar draws directos en Intel o eliminar el indirect drawing para terreno/modelos.

### F-02 — Naga no cortocircuitaba `||`/`&&`

**Síntomas:** corrupción dependiente del orden de los work items; lecturas fuera de rango en Intel; el mismo shader funciona en otras GPU.

**Causa documentada:** en el shader de Bevy se evaluaba `work_items[instance_index - 1]` cuando `instance_index == 0`; Naga no generaba el cortocircuito y la lectura OOB corrompía el estado en Intel.

**Corrección:**

- Bevy PR #21475: https://github.com/bevyengine/bevy/pull/21475
- wgpu PR #7339: https://github.com/gfx-rs/wgpu/pull/7339
- Bevy 0.17.3 o posterior.

**Acción para Cinnabar:** auditar los WGSL propios. Cualquier `if (a && b)` o `if (a || b)` que lea un array en `b` debe reescribirse con un `if` anidado o con una guarda explícita. No confiar en el cortocircuito del compilador.

### F-03 — MDI roto en Intel integrado

**Síntomas:** crash, corrupción o resultados incorrectos al usar `glMultiDrawElementsIndirect`/MDI.

**Evidencia:**

- Khronos: https://community.khronos.org/t/opengl-glmultidrawelementsindirect-crash-on-integrated-intel-gpu/111444

**Acción:** no usar MDI en la ruta Intel. D3D11 tampoco ofrece MDI nativo; cualquier emulación debe tratarse como no validada.

### F-04 — Vulkan Haswell incompleto

**Síntomas:** advertencia de Mesa, crashes, features no disponibles, inestabilidad.

**Evidencia:**

- `MESA-INTEL: warning: Haswell Vulkan support is incomplete`.
- Mesa/ANV considera Haswell una plataforma antigua con soporte incompleto.
- DXVK tampoco garantiza Haswell/HD 4000: https://github.com/doitsujin/dxvk/issues/1059

**Acción:** la validación objetivo debe ser DX11. No usar una corrida Vulkan/DXVK como prueba de que la ruta DX11 está bien.

### F-05 — DX12 retirado en Haswell

Intel retiró DirectX 12 en los drivers de 4.ª generación desde `15.40.44.5107`.

**Evidencia:** https://www.intel.com/content/www/us/en/support/articles/000057520/graphics.html

**Acción:** no usar DX12 como fallback ni como comparación de estabilidad.

### F-06 — El backend DX11 no ofrece compute/storage/MSAA/MDI count

No es un fallo del HD 4400: es el contrato actual de nuestro backend.

**Riesgo:** si el renderer cree que tiene compute/storage y el backend los rechaza o los ignora, el resultado puede ser ausencia de draws, no una caída explícita.

**Acción:** la implementación debe consultar las capacidades reales y usar CPU culling/draws directos cuando no estén.

### F-07 — Culling de modelos por cámara

Este es el cambio de Cinnabar más sospechoso para la regresión visual:

- antes de `76f5533`, la ruta de modelos usaba `CubeQuadLayout::default()` y dibujaba todas las referencias;
- después, los chunks puros se marcan como `compat_model`/`compact_faces` y el culling emite runs por dirección de cámara;
- el shader `cull.wgsl` y el `model.wesl` compacto son caminos nuevos, no validados en HD 4400.

**Riesgo:** si el driver entra en el camino indirecto o si una condición del shader se compila con OOB, el culling puede emitir rangos equivocados y producir caras ausentes o geometría corrupta. En Wine/RADV el draw mode observado fue `Direct`, así que ese camino no se ejercitó.

## 5. Qué debe hacer la implementación DX11

### 5.1 Política de adaptador para HD 4400

1. Detectar Intel Haswell/HD 4400 por `vendor == 0x8086` y nombre/ID de adaptador.
2. Forzar `ChunkDrawMode::Direct` para esa GPU.
3. No habilitar GPU culling, `MultiDrawIndirect`, `INDIRECT_FIRST_INSTANCE` ni `MULTI_DRAW_INDIRECT_COUNT`.
4. Usar CPU frustum culling y CPU face culling para las caras direccionales de modelos.
5. Registrar en el log qué política se aplicó.

Variables de diagnóstico implementadas:

```text
CINNABAR_FORCE_DIRECT_DRAWS=1
CINNABAR_CPU_CULLING=1
```

La política automática reconoce Intel Haswell/HD 4400 por nombre y por los IDs PCI Haswell (`0x0A16` para la HD 4400). El resultado se registra al arrancar con `terrain draw path`, incluyendo backend, adaptador y `draw_mode`. `CINNABAR_CPU_CULLING=1` también fuerza draws directos: no tiene sentido hacer culling en CPU y enviar el resultado por un camino indirecto.

La cámara también inserta `NoIndirectDrawing` cuando se usa DX11 o cualquiera de esas variables, de modo que las entidades de Bevy usan preprocesado CPU en lugar de indirect.

### 5.2 Culling de modelos

Implementado:

- el `CullRecord` de GPU ya no marca `model_compat` ni `model_solid_ends`; el modelo se dibuja completo si el culling GPU se usa en otra GPU;
- la ruta directa de modelos aplica **culling direccional de caras en CPU** con `CubeQuadLayout::solid_runs`, y emite uno o varios draws directos;
- las caras alpha-tested/two-sided se añaden siempre como cola visible;
- no se usa `MultiDrawIndirect`, ni compute shader de culling, ni `INDIRECT_FIRST_INSTANCE` en la ruta Intel.

El compacto sigue siendo opt-in (`CINNABAR_HEAVY_MODEL_TERRAIN=0` y `CINNABAR_HEAVY_CUBE_TERRAIN=0`). La ruta estable del lanzador usa modelos completos.

### 5.3 Shaders

- Revisar todos los WGSL con `&&`/`||` que puedan leer buffers.
- Sustituir condiciones del tipo:

```wgsl
if (a && array[index]) { ... }
```

por:

```wgsl
if (a) {
    if (index < arrayLength(&array)) {
        if (array[index]) { ... }
    }
}
```

- Mantener todas las comprobaciones de índice antes de cualquier acceso.
- Actualizar Bevy/wgpu a versiones con `#21475`/`#7339` si es viable.
- No introducir `first_instance` no soportado en el backend DX11.

### 5.4 Selección de terreno

Para la HD 4400, hasta nueva validación:

```text
WGPU_BACKEND=dx11
CINNABAR_HEAVY_TERRAIN=1
CINNABAR_HEAVY_MODEL_TERRAIN=1
CINNABAR_HEAVY_CUBE_TERRAIN=0
CINNABAR_FORCE_DIRECT_DRAWS=1
CINNABAR_CPU_CULLING=1
```

La ruta compacta puede probarse solo con draws directos y sin culling GPU, y siempre comparando contra la ruta de modelos completa.

### 5.5 Límites del backend

- No anunciar compute/storage/MSAA/MDI count hasta implementarlos y probarlos.
- No usar BC si el backend no lo mapea.
- No usar texturas 1D/3D si el backend solo implementa 2D.
- No usar MSAA/HDR/Enhanced/shaderpacks en el perfil HD 4400.
- Mantener texturas 2D BGRA8/SRGB, sampler nearest y UV locales.

## 6. Matriz mínima de pruebas

| Prueba | Ruta | Qué demuestra |
| --- | --- | --- |
| A | Modelos completos + direct draws + CPU culling | Línea base estable de la ruta modelo |
| B | Modelos completos + indirect/culling GPU | Aísla el culling/indirect |
| C | Compacto + direct draws + CPU culling | Aísla el formato compacto |
| D | Compacto + indirect/culling GPU | Ruta actual sospechosa |

Para cada prueba conservar:

- `client.log`;
- `graphics-adapter.log` si existe;
- captura o vídeo;
- `draw_mode`;
- nombre exacto del adaptador, backend y driver;
- si hubo fallback de compacto a modelos/cubos.

La conclusión no debe basarse en una sola captura. El bug puede ser intermitente y depender del orden de los draws.

## 7. Evidencia observada en nuestras pruebas

En la prueba Proton/RADV del build `36e4eec`:

- backend: `Dx11`;
- adaptador: `AMD Radeon RX 570 Series (RADV POLARIS10)`;
- `draw_mode=Direct`;
- no se reprodujo la corrupción visual del HD 4400.

Esto **no invalida** el bug: demuestra que Wine/RADV no estaba ejercitando el camino indirecto. La validación debe hacerse en la HD 4400 nativa.

## 8. Referencias

- Bevy #18904 — Meshes not rendering with default GpuPreprocessing on Intel integrated GPU:  
  https://github.com/bevyengine/bevy/issues/18904
- Bevy #19000 — Corrupted rendering with Integrated Intel GPUs + Vulkan + Indirect:  
  https://github.com/bevyengine/bevy/issues/19000
- Bevy PR #21475 — Fix Intel iGPU Rendering:  
  https://github.com/bevyengine/bevy/pull/21475
- wgpu PR #7339 — Short-circuiting de `&&` y `||`:  
  https://github.com/gfx-rs/wgpu/pull/7339
- Intel — DirectX 12 retirado en 4.ª generación:  
  https://www.intel.com/content/www/us/en/support/articles/000057520/graphics.html
- Intel — APIs soportadas:  
  https://www.intel.com/content/www/us/en/support/articles/000005524/graphics.html
- Intel — Driver 15.40 para 4.ª generación:  
  https://www.intel.com/content/www/us/en/download/18388/intel-graphics-driver-for-windows-10-15-40-4th-gen.html
- Mesa/ANV:  
  https://docs.mesa3d.org/drivers/anv.html
- Reporte de `Haswell Vulkan support is incomplete`:  
  https://forums.linuxmint.com/viewtopic.php?t=434171
- MDI en Intel integrado:  
  https://community.khronos.org/t/opengl-glmultidrawelementsindirect-crash-on-integrated-intel-gpu/111444
- DXVK y soporte limitado en HD 4000/Haswell:  
  https://github.com/doitsujin/dxvk/issues/1059
- Contrato de capacidades DX11 de Cinnabar:  
  [`dx11-capability-contract.md`](dx11-capability-contract.md)
- Plan DX11:  
  [`dx11-backend-plan.md`](dx11-backend-plan.md)

## 9. Preguntas abiertas

1. ¿Qué `draw_mode` reporta la HD 4400 en la corrida fallida?
2. ¿El bug aparece con la ruta de modelos completa (`CINNABAR_HEAVY_MODEL_TERRAIN=1`) o solo con compactos?
3. ¿Desaparece con draws directos (`CINNABAR_FORCE_DIRECT_DRAWS=1`)?
4. ¿Desaparece con `CINNABAR_CPU_CULLING=1`?
5. ¿El adaptador usa DX11 nativo, DX11 sobre Wine, wined3d/OpenGL o DXVK/Vulkan?
6. ¿Qué versión de Mesa/Intel driver y de wgpu/Bevy está usando el binario?

Sin esas respuestas, cualquier cambio de shader o de layout es una hipótesis, no una corrección verificada.
