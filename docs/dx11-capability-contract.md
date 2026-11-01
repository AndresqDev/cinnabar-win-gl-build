# Contrato de capacidades del backend D3D11

| Campo | Valor |
| --- | --- |
| Estado | Puerta 0 — contrato v1 implementado y verificado en CI sobre WARP; hardware pendiente |
| Fecha | 2026-11-01 |
| Rama | `dx11-stabilization` (documentación) |
| Base documental | `dea0b3d` |
| Integración wgpu | `AndresqDev/wgpu` `dx11-stabilization` @ `9bc4fc056` |
| Pin de referencia (histórico) | `b85f3a16a9dfc6bbcc9da650755c23f92b6cf636` |
| Test de contrato | `wgpu-hal/tests/dx11_contract.rs`, workflow `DX11 Contract` |
| Evidencia | Run rojo del pin `41306698696`; run verde del candidato P1+P6 `41308541042`; run verde de integración `41308953949` |
| Alcance | `wgpu-hal` DX11 y su integración con Cinnabar |
| Compilación local | Ninguna; este documento no aprueba una build |

Este documento concreta el contrato de la Puerta 0 del
[plan de estabilización](dx11-backend-plan.md). El plan lo llama
`dx11-contract.md`; se usa `dx11-capability-contract.md` para distinguirlo de
los contratos de integración. No sustituye al plan: lo hace verificable.

## 0. Resultado del piloto de anidamiento

El piloto real de tres capas se ejecutó en esta fase:

- **Capa 0** (sesión principal) lanzó una sesión de **capa 1**.
- **Capa 1** lanzó dos líderes de **capa 2** en paralelo: `lead-verification`
  (P6) y `lead-hal-core` (P1).
- Cada líder de capa 2 lanzó **dos implementadores de capa 3** (cuatro en
  total), cada uno con worktree y rama propios.
- Los implementadores de capa 3 **no tienen `subagent`** y no crearon capa 4.
  Ambos líderes lo reportaron explícitamente; no se simuló ningún hijo.
- Dos implementadores agotaron su límite de pasos antes de commitear; su
  líder revisó, corrigió y commiteó el trabajo. Esto es una limitación
  operativa, no un fallo de jerarquía.
- Ningún agente de capa 2/3 mergeó en `dx11-stabilization` ni pusheó a
  `dx11-integration`. La capa 1 revisó y mergeó.

Consecuencia: la jerarquía de tres capas está validada y la evidencia de
runtime existe. Sigue siendo obligatorio repetir la disciplina de propiedad de
archivos y de CI remoto en cada paquete.

## 1. Regla de anuncio

Un elemento solo puede aparecer en `Features`, `DownlevelFlags`, `Limits`,
`TextureFormatFeatures` o `SurfaceCapabilities` si se cumplen las cuatro
condiciones:

1. existe una ruta de código que lo implementa;
2. un test de conformidad lo ejecuta en CI;
3. un fallo devuelve un error explícito, nunca `unimplemented!()`, `panic!` ni
   un no-op silencioso;
4. este contrato lo enumera.

Si no se cumplen, el elemento se retira del anuncio o se implementa. Un
comentario o una intención en el plan no cuentan como implementación.

### Niveles de evidencia

| Nivel | Significado |
| --- | --- |
| `S` | Verificado por inspección de la fuente en el pin; no ejecutado en este piloto |
| `W` | Existe un workflow o ejemplo en el fork, pero no se re-ejecutó aquí |
| `X` | Ausente, rechazado explícitamente o nunca anunciado |
| `?` | No verificado; no puede usarse como garantía |

**La evidencia de runtime de la Puerta 0 está en §9.1.** Las filas `S` se
conservan como inspección del pin; el test de contrato verde complementa la
validación runtime en WARP. La paridad con hardware sigue sin evidencia `R`.

## 2. Resumen ejecutivo

El backend del pin ya no es el esqueleto descrito en `BUILD-NOTES.md`:
enumera adaptadores, crea dispositivo, configura una swapchain BGRA8, crea
buffers y texturas 2D, vistas, samplers, bind groups, compila Naga/HLSL con FXC
SM 5.0, crea render pipelines y ejecuta draws con readback. Los ejemplos
`raw-dx11`, `raw-dx11-resources`, `raw-dx11-shader`, `raw-dx11-triangle` y
`raw-dx11-window` cubren parte de ese camino.

Pero el anuncio del pin era **más amplio que la implementación**. Los
bloqueos principales de Puerta 0 eran:

- anuncia `TEXTURE_COMPRESSION_BC` sin mapear ningún formato BC;
- anuncia `MULTISAMPLE_X4` por formato aunque solo acepta `sample_count == 1`;
- deja activos `VERTEX_STORAGE`, `FRAGMENT_STORAGE`, límites de storage y
  límites de compute aunque el binding de storage y el compute no existen;
- `set_immediates` es un no-op que deja leer ceros, aunque el layout acepta
  un tamaño de immediates;
- el binding de storage buffers se acepta en el layout y se descarta con un
  log al hacer `set_bind_group`, sin fallar el draw;
- los límites de texturas 1D/3D se anuncian aunque la creación solo acepta 2D;
- faltan la capa de debug, fences de GPU reales, `wait_for_idle` real, pérdida
  de dispositivo controlada y las pruebas de resize.

Los sobre-anuncios anteriores están corregidos en la integración v1:
`TEXTURE_COMPRESSION_BC` retirada; `MULTISAMPLE_X4` retirada; storage y
compute a 0; `set_immediates` inalcanzable mediante layout rechazado; storage
buffers/texturas rechazados en el layout; límites 1D/3D a 0. La Puerta 0 queda
**cerrada en WARP** con el test de contrato verde y la Puerta 0 roja del pin
como evidencia. Queda pendiente la paridad con hardware, la capa de debug, la
pérdida de dispositivo controlada y los fences sin serialización de submits.

La evidencia de ejecución está en `docs/evidence/dx11-contract-gate0.md` del
fork de wgpu (run rojo `41306698696`, run verde `41308541042`, integración
`41308953949`). No se publica ni aprueba ninguna build.

## 3. Identidad, instancia y presentación

| Capacidad | Estado | Evidencia | Contrato v0 |
| --- | --- | --- | --- |
| Backend `wgpu::Backend::Dx11` | `S` | `dx11/mod.rs`, `adapter.rs` | Se anuncia `Dx11` |
| Feature level 11_0 y 10_1 | `S` | `adapter.rs::create_device` pide solo 11_0 y 10_1 | Se aceptan ambos |
| Feature level 11_1 | `X` | No se pide 11_1 | No se anuncia |
| Enumeración DXGI de adaptadores | `S` | `instance.rs::enumerate_adapters` | Sí |
| Apertura de dispositivo D3D11 | `S` | `adapter.rs::open` | Sí |
| Superficie Win32 (`HWND`) | `S` | `instance.rs::create_surface` | Solo Win32 |
| Formatos de superficie BGRA8 UNORM/SRGB | `S` | `surface.rs::configure` | Solo esos dos |
| Espacio de color sRGB | `S` | `surface.rs::configure` | `Auto`/`Srgb`; el resto se rechaza |
| Present modes FIFO e Immediate | `S` | `surface.rs::configure` | Solo esos; `AutoVsync`/`AutoNoVsync` se mapean |
| Alpha de composición Opaque | `S` | `surface.rs::configure` | `Auto`/`Inherit`/`Opaque`; el resto se rechaza |
| Swapchain flip-discard de 2 buffers | `S`/`W` | `surface.rs`; `raw-dx11-window` | Detalle de implementación; test pendiente |
| Fallback discard de 1 buffer | `S` | `surface.rs::configure` | Fallback para drivers que no exponen el segundo buffer |
| Resize por recreación de swapchain | `S` | `configure` suelta la swapchain anterior | Sí, pero sin test de resize todavía |
| `desired_maximum_frame_latency` 1..=2 | `?` | `surface_capabilities` lo anuncia; `configure` no lo usa | No garantizarlo hasta medirlo |
| Debug layer D3D11 | `X` | `D3D11_CREATE_DEVICE_BGRA_SUPPORT` únicamente | No se anuncia |
| Detección y recuperación de device loss | `X` | `Present` devuelve `Lost`; no hay recuperación ni motivo | No se anuncia |
| Timestamp de presentación | `X` | `INVALID_TIMESTAMP` | No se anuncia |
| Frame-latency waitable object | `X` | No se usa | No se anuncia |

La swapchain se crea con `DXGI_SWAP_EFFECT_FLIP_DISCARD`, formato
`B8G8R8A8_UNORM` y una vista RTV sRGB cuando se pide
`Bgra8UnormSrgb`. Si el flip model falla, se reintenta con
`DXGI_SWAP_EFFECT_DISCARD` y un buffer. Es un camino real; la prueba de resize
ya existe (`raw-dx11-resize`), pero la pérdida de dispositivo controlada sigue
pendiente de la Puerta 1.

## 4. Features

### 4.1 Features anunciadas hoy

`conv::features_for_feature_level` anuncia exactamente:

| Feature | FL | Estado | Evidencia | Acción v0 |
| --- | --- | --- | --- | --- |
| `DEPTH_CLIP_CONTROL` | 11_0/10_1 | `S` | `create_rasterizer_state` usa `DepthClipEnable` | Mantener; añadir test |
| `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` | 11_0/10_1 | `S`, parcial | `adapter.rs::texture_format_capabilities` consulta `CheckFeatureSupport` | Mantener solo si la matriz por formato pasa; corregir `MULTISAMPLE_X4` |
| `TEXTURE_COMPRESSION_BC` | 11_0 | `S`, **falso** | Se anunciaba, pero `conv::map_texture_format` no mapea BC1–BC7 | **Retirada en v1**; no se anuncia |

`TEXTURE_COMPRESSION_BC` es el caso más peligroso: `wgpu-core` puede validar
una textura BC porque la feature está activa y después la creación falla en
`map_texture_format`. Cinnabar descarga y compila recursos; cualquier textura
BC del pipeline de assets entra por este camino. Hasta que exista mapeo BC y
prueba de readback, la feature no se anuncia.

### 4.2 Features no anunciadas

`features_for_feature_level` no anuncia ninguna otra feature. En particular,
no se anuncian:

- compute shaders ni dispatch;
- storage textures, UAVs ni escritura de storage;
- dynamic storage buffer offsets;
- multiview;
- MSAA;
- view formats ni surface view formats;
- timestamp queries, occlusion queries ni pipeline statistics;
- pipeline cache;
- multi-draw indirect count;
- ray tracing, mesh/task shaders, subgroup ni cooperative matrix;
- `SHADER_F16`, `SHADER_INT16`, `SHADER_INT64` ni `SHADER_F64`;
- `TEXTURE_COMPRESSION_ETC2`, `TEXTURE_COMPRESSION_ASTC` ni
  `TEXTURE_COMPRESSION_BC_SLICED_3D`;
- `TEXTURE_BINDING_ARRAY`, `SAMPLER_BINDING_ARRAY` ni
  `BUFFER_BINDING_ARRAY`;
- `PUSH_CONSTANTS` (el límite `max_immediate_size` es 0);
- `EXTERNAL_TEXTURE`, acceleration structures ni ray tracing;
- `MULTISAMPLE_ARRAY`, `MULTIVIEW`, `DUAL_SOURCE_BLENDING` ni
  `CONSERVATIVE_RASTERIZATION`.

### 4.3 Features que debían retirarse o implementarse antes de cerrar Puerta 0

| Elemento | Problema | Decisión v1 |
| --- | --- | --- |
| `TEXTURE_COMPRESSION_BC` | No hay mapeo BC | **Retirada**; verificado por el test de contrato |
| `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` | Las caps por formato sobre-anunciaban MSAA | **Mantenida** con `MULTISAMPLE_X4` retirada y matriz de formatos verificada en WARP |
| `DEPTH_CLIP_CONTROL` | Implementado, sin test dedicado | Mantenida; el test de contrato no ejecuta depth clipping (Puerta 1) |

## 5. Downlevel flags

### 5.1 Cómo se calculan en v1

`conv::downlevel_for_feature_level` ya **no parte de
`wgpu_types::DownlevelFlags::compliant()`**. Construye `DownlevelFlags::empty()`
y añade exactamente:

```
BASE_VERTEX
NON_POWER_OF_TWO_MIPMAPPED_TEXTURES
CUBE_ARRAY_TEXTURES
COMPARISON_SAMPLERS
INDEPENDENT_BLEND
FULL_DRAW_INDEX_UINT32
DEPTH_BIAS_CLAMP
```

`shader_model` es `Sm5` en 11_0 y `Sm4` en 10_1. `is_webgpu_compliant()` es
siempre `false`, que es la respuesta honesta para un backend downlevel.

La tabla de §5.2 se conserva como análisis histórico del pin; la columna
«Acción v0» se ejecutó en v1 y el conjunto final es el de §5.3.

### 5.2 Flags activas y verificación

| Flag | En 11_0 | Estado real | Acción v0 |
| --- | --- | --- | --- |
| `COMPUTE_SHADERS` | Eliminada | No hay compute pipeline ni dispatch | Correcto: no anunciar |
| `FRAGMENT_WRITABLE_STORAGE` | Eliminada | No hay storage texture/UAV | Correcto: no anunciar |
| `VIEW_FORMATS` | Eliminada | Solo pares sRGB internos RGBA8/BGRA8; no se expone una vista arbitraria | Correcto: no anunciar |
| `SURFACE_VIEW_FORMATS` | Eliminada | No hay vistas alternativas de superficie | Correcto: no anunciar |
| `NONBLOCKING_QUERY_RESOLVE` | Eliminada | No hay query sets | Correcto: no anunciar |
| `VERTEX_STORAGE` | Activa en 11_0 | El layout acepta storage, pero `set_bind_group` descarta el buffer | **Retirar** |
| `FRAGMENT_STORAGE` | Activa en 11_0 | Igual que `VERTEX_STORAGE` | **Retirar** |
| `INDIRECT_EXECUTION` | Activa | Hay draws indirect directos e indexados; no hay dispatch | Retirar hasta compute, o separar la semántica |
| `BASE_VERTEX` | Activa | `DrawIndexedInstanced` recibe `base_vertex` | Mantener; test |
| `READ_ONLY_DEPTH_STENCIL` | Activa | No se crea DSV read-only | **Retirar** |
| `NON_POWER_OF_TWO_MIPMAPPED_TEXTURES` | Activa | D3D11 lo soporta; no hay restricción en el código | Mantener; test |
| `CUBE_ARRAY_TEXTURES` | Activa | Hay SRV `TEXTURECUBEARRAY` | Mantener; test |
| `COMPARISON_SAMPLERS` | Activa | `map_filter` y `map_comparison` implementan comparación | Mantener; test |
| `INDEPENDENT_BLEND` | Activa | `IndependentBlendEnable = true` | Mantener; test |
| `MULTISAMPLED_SHADING` | Activa | `sample_count == 1` obligatorio | **Retirar** |
| `DEPTH_TEXTURE_AND_BUFFER_COPIES` | Activa | Hay copia textura↔buffer, sin prueba de profundidad | Mantener solo tras test |
| `WEBGPU_TEXTURE_FORMAT_SUPPORT` | Activa | Faltan 1D/3D, MSAA y BC | **Retirar** |
| `BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED` | Activa | Se exigen offsets/tamaños uniformes de 16 bytes | **Retirar** |
| `UNRESTRICTED_INDEX_BUFFER` | Activa | `INDEX` puede combinarse con `VERTEX`; `UNIFORM` se rechaza | Mantener solo tras test; si no, retirar |
| `FULL_DRAW_INDEX_UINT32` | Activa | D3D11 usa índices de 32 bits | Mantener; test |
| `DEPTH_BIAS_CLAMP` | Activa | `DepthBiasClamp` se pasa al rasterizador | Mantener; test |
| `UNRESTRICTED_EXTERNAL_TEXTURE_COPIES` | Activa | `copy_external_image_to_texture` no está implementado | **Retirar** |
| `SHADER_F16_IN_F32` | Activa | No hay validación de Naga/FXC para f16 | **Retirar** |
| `MSL2_1` | Activa | Es una capacidad de Metal, no de D3D11 | **Retirar** |
| `TEXTURE_COMPRESSION` | Activa | BC/ETC2/ASTC no mapeados | **Retirar** |
| `ANISOTROPIC_FILTERING` | Inactiva | El sampler acepta anisotropía, pero la flag no se anuncia | Correcto: no anunciar |

### 5.3 Conjunto implementado en v1

El contrato v1 implementado no parte de `compliant()`. Anuncia exactamente:

```
BASE_VERTEX
NON_POWER_OF_TWO_MIPMAPPED_TEXTURES
CUBE_ARRAY_TEXTURES
COMPARISON_SAMPLERS
INDEPENDENT_BLEND
FULL_DRAW_INDEX_UINT32
DEPTH_BIAS_CLAMP
```

`DEPTH_TEXTURE_AND_BUFFER_COPIES` y `UNRESTRICTED_INDEX_BUFFER` siguen fuera
hasta que sus tests dedicados pasen. Cualquier otra flag queda fuera hasta que
exista implementación y prueba. Esto hace que `is_webgpu_compliant()` sea
`false`.

## 6. Formatos

### 6.1 Allowlist mapeada

`conv::map_texture_format` mapea estos formatos a DXGI:

| Grupo | Formatos |
| --- | --- |
| 8 bits | `R8Unorm`, `R8Snorm`, `R8Uint`, `R8Sint`, `Rg8Unorm`, `Rg8Snorm`, `Rg8Uint`, `Rg8Sint`, `Rgba8Unorm`, `Rgba8UnormSrgb`, `Rgba8Snorm`, `Rgba8Uint`, `Rgba8Sint`, `Bgra8Unorm`, `Bgra8UnormSrgb` |
| 16 bits | `R16Uint`, `R16Sint`, `R16Unorm`, `R16Snorm`, `R16Float`, `Rg16Uint`, `Rg16Sint`, `Rg16Float`, `Rgba16Float`, `Rgba16Unorm`, `Rgba16Snorm`, `Rgba16Uint`, `Rgba16Sint` |
| 32 bits | `R32Uint`, `R32Sint`, `R32Float`, `Rgba32Float`, `Rgba32Uint`, `Rgba32Sint` |
| 10/11 bits | `Rgb10a2Unorm`, `Rgb10a2Uint`, `Rg11b10Ufloat` |
| Profundidad/stencil | `Depth16Unorm`, `Depth24Plus`, `Depth24PlusStencil8`, `Depth32Float`, `Depth32FloatStencil8` |

No están mapeados, y por tanto no se anuncian:

- `Bc1RgbaUnorm`, `Bc1RgbaUnormSrgb`, `Bc2RgbaUnorm`, `Bc2RgbaUnormSrgb`,
  `Bc3RgbaUnorm`, `Bc3RgbaUnormSrgb`, `Bc4RUnorm`, `Bc4RSnorm`,
  `Bc5RgUnorm`, `Bc5RgSnorm`, `Bc6hRgbUfloat`, `Bc6hRgbFloat`,
  `Bc7RgbaUnorm`, `Bc7RgbaUnormSrgb`;
- `Etc2Rgb8Unorm`, `Etc2Rgb8UnormSrgb`, `Etc2Rgba8Unorm`,
  `Etc2Rgba8UnormSrgb`, `EacR11Unorm`, `EacR11Snorm`, `EacRg11Unorm`,
  `EacRg11Snorm`;
- `Astc*`;
- `Stencil8`;
- `Rgb9e5Ufloat`;
- formatos 1D/3D no contemplados por la creación 2D.

`Depth24Plus` y `Depth24PlusStencil8` comparten el DXGI
`D24_UNORM_S8_UINT`. `Depth32FloatStencil8` usa `D32_FLOAT_S8X24_UINT`. No
hay un `Stencil8` real: el plan lo prohíbe y el contrato lo mantiene fuera.

### 6.2 Capacidades por formato

`Adapter::texture_format_capabilities` consulta
`D3D11_FEATURE_FORMAT_SUPPORT` y traduce:

| Consulta D3D11 | Capability anunciada |
| --- | --- |
| `TEXTURE2D` | `SAMPLED | COPY_SRC | COPY_DST` |
| `SHADER_SAMPLE` | `SAMPLED_LINEAR` |
| `RENDER_TARGET` | `COLOR_ATTACHMENT` |
| `BLENDABLE` | `COLOR_ATTACHMENT_BLEND` |
| `DEPTH_STENCIL` | `DEPTH_STENCIL_ATTACHMENT` |

El problema histórico era `MULTISAMPLE_X4`: la creación de texturas y render
passes exige `sample_count == 1`, y no hay resolve. Anunciarlo permitía que
`wgpu-core` validara una textura o pipeline MSAA y después el HAL la rechazara.
**v1 retira todos los bits `MULTISAMPLE_*`**; el test de contrato verifica que
ningún formato de la allowlist los anuncia y que MSAA x4 falla con error
explícito. `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` se mantiene con esta
matriz corregida.

`TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` se mantiene porque la traducción se
corrigió y el test de contrato verifica la matriz por formato. El contrato v1
solo garantiza las capacidades por formato cubiertas por esa matriz.

### 6.3 Superficie

Solo `Bgra8Unorm` y `Bgra8UnormSrgb`, con `TextureUses::COLOR_TARGET`,
`SurfaceColorSpaces::SRGB`, `CompositeAlphaMode::Opaque`, `Fifo` e `Immediate`.
No se anuncian 10 bits, HDR, `Mailbox`, `ExclusiveFullScreen` ni
`PostMultiplied`/`PreMultiplied`.

## 7. Límites

### 7.1 Tabla implementada en v1

Estos son los valores que publica `conv::limits_for_feature_level` en la
integración y que el test de contrato verifica campo por campo:

| Límite | v1 | Estado |
| --- | --- | --- |
| `max_texture_dimension_1d` | 0 | 1D no implementado; no se anuncia |
| `max_texture_dimension_2d` | 16384 (11_0) / 8192 (10_1) | Implementado |
| `max_texture_dimension_3d` | 0 | 3D no implementado; no se anuncia |
| `max_texture_array_layers` | 2048 | Implementado |
| `max_bind_groups` | 4 | Implementado |
| `max_bind_groups_plus_vertex_buffers` | 24 | Conservador |
| `max_bindings_per_bind_group` | 64 | Límite verificado; sustituye a 1000 |
| `max_dynamic_uniform_buffers_per_pipeline_layout` | 8 | Implementado con `Context1` |
| `max_dynamic_storage_buffers_per_pipeline_layout` | 0 | Storage no implementado |
| `max_sampled_textures_per_shader_stage` | 16 | Implementado |
| `max_samplers_per_shader_stage` | 8 | Implementado |
| `max_storage_buffers_per_shader_stage` | 0 | Storage no implementado |
| `max_storage_textures_per_shader_stage` | 0 | Storage no implementado |
| `max_uniform_buffers_per_shader_stage` | 7 | Implementado |
| `max_binding_array_elements_per_shader_stage` | 0 | Binding arrays no implementados |
| `max_binding_array_acceleration_structure_elements_per_shader_stage` | 0 | No aplica |
| `max_binding_array_sampler_elements_per_shader_stage` | 0 | No aplica |
| `max_uniform_buffer_binding_size` | 16384 | Implementado |
| `max_storage_buffer_binding_size` | 0 | Storage no implementado |
| `max_vertex_buffers` | 8 | Implementado |
| `max_buffer_size` | 268435456 | Conservador |
| `max_vertex_attributes` | 16 | Implementado |
| `max_vertex_buffer_array_stride` | 2048 | Implementado |
| `max_inter_stage_shader_variables` | 15 | Conservador |
| `min_uniform_buffer_offset_alignment` | 256 | Implementado |
| `min_storage_buffer_offset_alignment` | 256 | Mantenido; storage no anunciado |
| `max_color_attachments` | 4 | Implementado |
| `max_color_attachment_bytes_per_sample` | 32 | Sin MSAA, suficiente |
| `max_compute_workgroup_storage_size` | 0 | Compute no implementado |
| `max_compute_invocations_per_workgroup` | 0 | Compute no implementado |
| `max_compute_workgroup_size_x/y/z` | 0/0/0 | Compute no implementado |
| `max_compute_workgroups_per_dimension` | 0 | Compute no implementado |
| `max_immediate_size` | 0 | Push constants no implementados |
| `max_buffers_and_acceleration_structures_per_shader_stage` | 12 | Recalculado para storage 0 |
| Límites mesh/task/ray tracing/multiview | 0 | No se anuncian |
| `Alignments` | sin cambios (`buffer_copy_offset = 1`, etc.) | **Pendiente**; follow-up de Puerta 1 |

**Consecuencia operativa:** `wgpu-core` valida `required_limits` contra los
límites del adaptador al crear el dispositivo. Con storage/compute en 0 y
`max_bindings_per_bind_group = 64`, una petición con
`Limits::downlevel_defaults()` o `Limits::default()` será rechazada. El perfil
futuro `Dx11Legacy` debe pedir los límites del adaptador o un conjunto
explícito bajo. Esto es honesto y deliberado; no se “arregla” re-anunciando
storage.

### 7.2 Tabla histórica del pin (pre-v1)

Los valores del pin salían de `Limits::downlevel_defaults()` con los cambios
de `conv::limits_for_feature_level`: dimensión 1D/2D 16384 en 11_0 y 8192 en
10_1, dimensión 3D 2048 y arrays 2048. El resto quedaba en los valores
downlevel por defecto. Se conserva como análisis histórico; la tabla
normativa es §7.1.

Estado: `OK` = soportado y conservador; `SOBRE` = anunciado sin
implementación; `COND` = depende de una condición o no está probado.

| Límite | Valor anunciado | Estado | Nota |
| --- | --- | --- | --- |
| `max_texture_dimension_1d` | 16384 / 8192 | `SOBRE` | La creación solo acepta `TextureDimension::D2` |
| `max_texture_dimension_2d` | 16384 / 8192 | `OK` | Coincide con el máximo de D3D11 por feature level |
| `max_texture_dimension_3d` | 2048 | `SOBRE` | La creación solo acepta 2D |
| `max_texture_array_layers` | 2048 | `OK` | D3D11 FL11 permite 2048 |
| `max_bind_groups` | 4 | `OK` | No hay descriptor heaps; los registros se aplanan |
| `max_bindings_per_bind_group` | 1000 | `SOBRE` | D3D11 tiene 128 SRV, 16 samplers y 14 CBV por etapa; 1000 no es verificable |
| `max_dynamic_uniform_buffers_per_pipeline_layout` | 8 | `COND` | Requiere `ID3D11DeviceContext1` y offsets de 256 bytes |
| `max_dynamic_storage_buffers_per_pipeline_layout` | 4 | `SOBRE` | El layout rechaza offsets dinámicos de storage |
| `max_sampled_textures_per_shader_stage` | 16 | `OK` | Conservador frente a 128 |
| `max_samplers_per_shader_stage` | 8 | `OK` | Coincide con el máximo de D3D11 (16) |
| `max_storage_buffers_per_shader_stage` | 4 | `SOBRE` | `set_bind_group` descarta storage buffers |
| `max_storage_textures_per_shader_stage` | 4 | `SOBRE` | Las vistas UAV se rechazan |
| `max_uniform_buffers_per_shader_stage` | 7 | `OK` | Conservador |
| `max_uniform_buffer_binding_size` | 16 KiB | `OK` | La implementación acepta hasta 64 KiB |
| `max_storage_buffer_binding_size` | 128 MiB | `SOBRE` | No hay binding de storage |
| `max_vertex_buffers` | 8 | `OK` | Conservador |
| `max_vertex_attributes` | 16 | `OK` | Conservador |
| `max_vertex_buffer_array_stride` | 2048 | `OK` | Coincide con D3D11 |
| `max_inter_stage_shader_variables` | 15 | `OK` | Conservador |
| `max_color_attachments` | 4 | `OK` | La pipeline acepta hasta 8; el límite es conservador |
| `max_color_attachment_bytes_per_sample` | 32 | `OK` | Sin MSAA, suficiente |
| `max_compute_workgroup_storage_size` | 16352 | `SOBRE` | No hay compute |
| `max_compute_invocations_per_workgroup` | 256 | `SOBRE` | No hay compute |
| `max_compute_workgroup_size_x/y/z` | 256/256/64 | `SOBRE` | No hay compute |
| `max_compute_workgroups_per_dimension` | 65535 | `SOBRE` | No hay compute |
| `max_buffer_size` | 256 MiB | `OK` | Conservador |
| `max_immediate_size` | 0 | `OK` | No se anuncian push constants |
| `min_uniform_buffer_offset_alignment` | 256 | `OK` | Coincide con la ventana de `Context1` |
| `min_storage_buffer_offset_alignment` | 256 | `SOBRE` | No hay storage |
| `max_buffers_and_acceleration_structures_per_shader_stage` | 24 | `COND` | Recalcular cuando storage sea 0 |
| `max_binding_array_elements_per_shader_stage` | 0 | `OK` | No se anuncian arrays |
| `max_multiview_view_count` | 0 | `OK` | No hay multiview |
| Límites mesh/task/ray tracing | 0 | `OK` | No se anuncian |
| `Alignments::buffer_copy_offset` | 1 | `?` | D3D11 exige alineación de 4 bytes para copias; falta prueba |
| `Alignments::buffer_copy_pitch` | 1 | `?` | El row pitch de staging no es arbitrario; falta prueba |
| `Alignments::uniform_bounds_check_alignment` | 1 | `?` | Los bindings uniformes exigen 16 bytes; falta prueba |

Los límites de compute y storage no son “límites máximos inofensivos”: wgpu-core
los usa para validar layouts y bind groups. Un valor 4 permite crear un layout
que después no se puede bindear. Deben ser 0 hasta que la capacidad exista.

## 8. Lista explícita de lo que NO se anuncia

El contrato v1 **no anuncia**:

- compute shaders, compute pipelines ni dispatch;
- storage buffers, storage textures, UAVs, escritura de storage ni
  `STORAGE_BINDING`;
- offsets dinámicos de storage buffers;
- MSAA x2/x4/x8/x16, `MULTISAMPLE_ARRAY`, resolve de MSAA ni sample-rate
  shading;
- `alpha_to_coverage`;
- multiview ni `multiview_mask`;
- view formats arbitrarios ni surface view formats;
- timestamp queries, occlusion queries, pipeline statistics, query sets ni
  resolve de queries;
- pipeline cache;
- multi-draw indirect count;
- ray tracing, acceleration structures, mesh shaders, task shaders,
  subgroup ni cooperative matrix;
- `SHADER_F16`, `SHADER_INT16`, `SHADER_INT64`, `SHADER_F64` ni
  `SHADER_F16_IN_F32`;
- `TEXTURE_COMPRESSION_BC`, `TEXTURE_COMPRESSION_ETC2`,
  `TEXTURE_COMPRESSION_ASTC`, `TEXTURE_COMPRESSION_BC_SLICED_3D` ni
  `TEXTURE_COMPRESSION`;
- texturas 1D y 3D, cubemaps 3D, slices 3D de render target ni
  `TextureViewDimension::D1/D3`;
- `Stencil8` real;
- `Rgb9e5Ufloat`, BC, ETC2, ASTC ni cualquier formato fuera de la allowlist
  de §6.1;
- texturas 10 bits/HDR en la superficie; solo BGRA8 UNORM/SRGB;
- dual-source blending, `Src1`/`OneMinusSrc1`, `Src1Alpha` ni
  `OneMinusSrc1Alpha`;
- conservative rasterization, polygon mode `Point`, indexed strip primitive
  restart ni `alpha_to_coverage`;
- push constants; `max_immediate_size` es 0 y `set_immediates` no debe ser
  alcanzable desde la API pública;
- sampler arrays, texture binding arrays, buffer binding arrays ni
  `max_binding_array_* > 0`;
- external textures, `copy_external_image_to_texture` ni
  `UNRESTRICTED_EXTERNAL_TEXTURE_COPIES`;
- feature level 11_1;
- debug layer, device-loss recovery, present timestamps, waitable objects ni
  frame-latency control;
- cualquier `unimplemented!()`, `panic!` o no-op silencioso como respuesta a
  una petición no soportada.

Si el código actual anuncia alguno de estos elementos, es un defecto de
Puerta 0, no una capacidad del backend.

## 9. Diseño del test de contrato (Puerta 0)

El test está implementado en `wgpu-hal/tests/dx11_contract.rs` del fork de
wgpu y se ejecuta en Windows CI con el workflow `DX11 Contract`. Usa la API
HAL (`Instance`, `Adapter`, `Device`, `CommandEncoder`) porque la instantánea
es exactamente el anuncio del backend, y crea una superficie Win32 oculta para
las comprobaciones de superficie. Los ocho puntos siguientes son la
especificación que el test implementa; el estado real de ejecución está en
§9.1.

1. **Instantánea del anuncio.** Leer `adapter.features()`,
   `adapter.downlevel_capabilities()`, `adapter.limits()`,
   `adapter.get_texture_format_features(format)` para cada formato de la
   allowlist y `surface_capabilities`.
2. **Comparación con el contrato.** Serializar esa instantánea y compararla
   con la tabla esperada de este documento. Un cambio sin actualización del
   contrato falla.
3. **Positivos.** Para cada feature/flag/límite/formato anunciado, ejecutar la
   operación mínima: crear recurso, pipeline, bind group, draw, copia o
   readback. Un anuncio sin operación verde falla.
4. **Negativos.** Pedir explícitamente lo no anunciado (BC, MSAA, storage,
   compute, 1D/3D, push constants, view formats, etc.). Debe fallar con error
   de validación o HAL, nunca con panic, `unimplemented!()` ni un draw vacío.
5. **Sin no-ops.** Instrumentar `set_immediates`, storage buffers y
   `copy_external_image_to_texture`; si se aceptan, deben tener efecto o
   devolver error. Un log y un draw sin datos falla el test.
6. **Debug layer.** Crear el dispositivo con la capa de debug cuando el test
   lo pida y fallar si hay errores de D3D11.
7. **WARP y hardware.** El mismo contrato debe pasar en ambos; las diferencias
   se documentan como limitaciones, no se ignoran.
8. **Cierre de Puerta 0.** El test verde, los sobre-anuncios retirados o
   implementados y este documento actualizado. Sin eso no se abre la Puerta 1.

### 9.1 Resultado de ejecución

| Ejecución | Commit | Resultado |
| --- | --- | --- |
| Pin, solo test P6 | `10115febd` (base `b85f3a16a`) | **Rojo esperado**: 146 violaciones (~73 por adaptador); `check` y `fmt` verdes. Run `41306698696` |
| Candidato P6+P1 | `1deb02958` (merge de `10115febd` + `640c64917`) | **Verde**: `1 passed; 0 failed`; `check` y `fmt` verdes. Run `41308541042` |
| Integración `dx11-stabilization` | `77da36e6c` | **Verde**: mismo test, sin cambios. Run `41308953949` |

El run rojo enumera exactamente los sobre-anuncios del pin: BC, 13 flags
extra de downlevel, 14 límites de storage/compute/1D/3D, `MULTISAMPLE_X4` en
37 formatos, storage BGL aceptado, `immediate_size = 4` aceptado y cuatro
`unimplemented!()`. El run verde demuestra que las correcciones de P1 hacen
pasar el mismo test sin debilitarlo.

Limitaciones de la evidencia: el runner hosted solo expone
`Microsoft Basic Render Driver` (software, `0x1414:0x008c`, FL 11_0); no hay
paridad con hardware. El flag impreso `software/WARP` es `false` por una
clasificación conservadora de `DeviceType::Cpu`; el nombre del adaptador y la
ausencia de hardware confirman que es software. `maximum_frame_latency` no se
aplica en `configure`; `Alignments` queda fuera de alcance; los métodos de
ray tracing del command encoder siguen siendo no-ops silenciosos en el pin y
no se asertan porque los entry points de descriptor fallan primero.

## 10. Estado de las puertas

| Puerta | Estado | Evidencia |
| --- | --- | --- |
| 0 — Congelación y contrato | **Cerrada en WARP**; hardware pendiente | Test `wgpu-hal/tests/dx11_contract.rs` verde en `41308541042`/`41308953949`; rojo del pin en `41306698696`; evidencia `docs/evidence/dx11-contract-gate0.md` |
| 1 — Conformidad HAL | Parcial; avances P1 | `raw-dx11`, `raw-dx11-resources`, `raw-dx11-shader`, `raw-dx11-triangle`, `raw-dx11-window`, nuevo `raw-dx11-resize`; fence `D3D11_QUERY_EVENT` y readback verdes en `41309216229`; faltan 2 RT, arrays/cubemap, device loss controlado, debug layer, depth clipping y test dedicado de fence |
| 2 — Menú Bevy/Cinnabar | No iniciada | Sin perfil `Dx11Legacy` verificado ni captura de menú |
| 3 — Mundo local mínimo | No iniciada | Sin build ni prueba |
| 4 — Mundo vanilla | No iniciada | Sin build ni prueba |
| 5 — Gameplay completo | No iniciada | Sin build ni prueba |
| 6 — Compute opcional | No iniciada | Fuera del contrato v1 |

No se publica ninguna build de usuario hasta que la puerta correspondiente
esté cerrada.

## 11. Riesgos y bloqueos

### Resueltos en v1

- **BC falso**: `TEXTURE_COMPRESSION_BC` retirada; una textura BC falla con
  error explícito, verificado en el test de contrato.
- **MSAA falso**: `MULTISAMPLE_X4` retirada de todas las caps; MSAA x4 falla
  en textura, render pass y pipeline.
- **Storage/compute falsos**: límites a 0 y storage BGL rechazado en el HAL y
  en el layout; no se acepta y descarta.
- **Immediates silenciosos**: `max_immediate_size = 0` y todo layout con
  `immediate_size > 0` se rechaza.
- **Fences de CPU**: sustituidos por `ID3D11Query` `D3D11_QUERY_EVENT` con
  espera real; `raw-dx11-resources` y `raw-dx11-triangle` verifican submit,
  wait y readback en CI (`41309216229`).
- **Sin resize probado**: `raw-dx11-resize` verifica 320x240 → 640x360 en CI.
- **`unimplemented!()` alcanzable**: los stubs de compute, query set y ray
  tracing devuelven `Err`; el test lo comprueba con `catch_unwind`.

### Pendientes

- **Hardware sin verificar**: la evidencia verde es solo WARP/software. La
  paridad con la Intel HD 4400 sigue siendo una puerta abierta de Puerta 1.
- **Device loss**: hay detección (`GetDeviceRemovedReason` → `Lost`) y
  `present` devuelve error, pero no hay recuperación controlada ni test que
  fuerce un reset.
- **Debug layer D3D11**: no implementada; no hay validación de errores de
  driver en WARP ni hardware.
- **Fence serializa submits**: el mismo `ID3D11Query` no puede re-armarse
  mientras su marcador está en vuelo; `submit` espera al marcador anterior.
  Es correcto pero puede bloquear hasta 5 s. Diseñar un pool de queries es
  follow-up de Puerta 1.
- **`request_device` con defaults**: storage/compute en 0 y
  `max_bindings_per_bind_group = 64` hacen que
  `Limits::downlevel_defaults()`/`default()` sean rechazados. `Dx11Legacy`
  debe pedir los límites del adaptador.
- **Alignments**: `buffer_copy_offset`, `buffer_copy_pitch` y
  `uniform_bounds_check_alignment` siguen en 1 sin prueba; follow-up.
- **1D/3D reales y BC**: no implementados y no anunciados. Si los assets de
  Cinnabar usan BC, hay que decidir entre implementar BC con readback o usar
  assets sin comprimir antes de la Puerta 2.
- **Ray tracing del command encoder**: métodos no-op silenciosos en el pin;
  los entry points de descriptor fallan primero, así que no son alcanzables
  desde una API válida, pero deben devolver error explícito en un follow-up.
- **`maximum_frame_latency`**: se anuncia 1..=2 pero `configure` no lo aplica;
  no debe tratarse como capacidad verificada.
- **`BUILD-NOTES.md` desactualizado**: afirma que ningún fork de wgpu
  implementa DX11; el pin actual sí tiene un HAL DX11. Es documentación de un
  build GL temporal y no debe usarse como contrato.

## 12. Referencias

- Plan: [`docs/dx11-backend-plan.md`](dx11-backend-plan.md)
- Mapa de agentes: [`docs/dx11-agent-map.md`](dx11-agent-map.md)
- Paquetes de tareas: [`docs/dx11-task-packets.md`](dx11-task-packets.md)
- Fuente del pin: `wgpu-hal/src/dx11/{adapter,conv,device,command,surface,queue,shader,types,instance,mod}.rs`
- Test de contrato: `wgpu-hal/tests/dx11_contract.rs`
- Evidencia: [`docs/evidence/dx11-contract-gate0.md`](https://github.com/AndresqDev/wgpu/blob/dx11-stabilization/docs/evidence/dx11-contract-gate0.md)
- Ejemplos del fork: `wgpu-hal/examples/raw-dx11{,-resources,-shader,-triangle,-window,-resize}.rs`
- Workflows del fork: `.github/workflows/dx11-{hal-resources,descriptors,pipeline,surface,core,hal-core}-check.yml` y `.github/workflows/dx11-contract.yml`
- Límites de wgpu: `wgpu-types/src/limits.rs`
- Capacidades de formato: `wgpu-hal/src/dx11/adapter.rs::texture_format_capabilities`
- Integración: `AndresqDev/wgpu` `dx11-stabilization` @ `9bc4fc056`
