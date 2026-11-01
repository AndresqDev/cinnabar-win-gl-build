# Contrato de capacidades del backend D3D11

| Campo | Valor |
| --- | --- |
| Estado | Puerta 0 — contrato v0 para revisión |
| Fecha | 2026-11-01 |
| Rama | `dx11-stabilization` |
| Base | `dea0b3d` |
| Backend de referencia | `AndresqDev/wgpu` @ `b85f3a16a9dfc6bbcc9da650755c23f92b6cf636` |
| Alcance | `wgpu-hal` DX11 y su integración con Cinnabar |
| Compilación local | Ninguna; este documento no aprueba una build |

Este documento concreta el contrato de la Puerta 0 del
[plan de estabilización](dx11-backend-plan.md). El plan lo llama
`dx11-contract.md`; se usa `dx11-capability-contract.md` para distinguirlo de
los contratos de integración. No sustituye al plan: lo hace verificable.

## 0. Resultado del piloto de anidamiento

Esta sesión es capa 1 y **no tiene disponible la herramienta `subagent`** ni
otra herramienta de delegación. Por eso no se lanzaron agentes de capa 2, no
hubo capa 2 que intentara lanzar capa 3, y no se creó ningún worktree hijo. No
se simulan hijos. El detalle y las limitaciones están en
[dx11-agent-map.md](dx11-agent-map.md).

Consecuencia para este documento: las capacidades se derivan de inspección de
la fuente del pin, no de una ejecución. La validación runtime y el piloto real
de tres capas siguen pendientes.

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

**Ninguna fila de este documento tiene evidencia `R` (runtime) en este
piloto.** Los workflows `dx11-*.yml` del fork existen, pero no se ejecutaron.

## 2. Resumen ejecutivo

El backend del pin ya no es el esqueleto descrito en `BUILD-NOTES.md`:
enumera adaptadores, crea dispositivo, configura una swapchain BGRA8, crea
buffers y texturas 2D, vistas, samplers, bind groups, compila Naga/HLSL con FXC
SM 5.0, crea render pipelines y ejecuta draws con readback. Los ejemplos
`raw-dx11`, `raw-dx11-resources`, `raw-dx11-shader`, `raw-dx11-triangle` y
`raw-dx11-window` cubren parte de ese camino.

Pero el anuncio actual es **más amplio que la implementación**. Los bloqueos
principales de Puerta 0 son:

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

La Puerta 0 no está cerrada: faltan el test de contrato y la corrección de
estos anuncios. Este documento no publica ni aprueba ninguna build.

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
`DXGI_SWAP_EFFECT_DISCARD` y un buffer. Es un camino real, pero la Puerta 1
todavía no tiene una prueba de resize ni de pérdida de dispositivo.

## 4. Features

### 4.1 Features anunciadas hoy

`conv::features_for_feature_level` anuncia exactamente:

| Feature | FL | Estado | Evidencia | Acción v0 |
| --- | --- | --- | --- | --- |
| `DEPTH_CLIP_CONTROL` | 11_0/10_1 | `S` | `create_rasterizer_state` usa `DepthClipEnable` | Mantener; añadir test |
| `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` | 11_0/10_1 | `S`, parcial | `adapter.rs::texture_format_capabilities` consulta `CheckFeatureSupport` | Mantener solo si la matriz por formato pasa; corregir `MULTISAMPLE_X4` |
| `TEXTURE_COMPRESSION_BC` | 11_0 | `S`, **falso** | Se anuncia, pero `conv::map_texture_format` no mapea BC1–BC7 | Retirar o implementar BC completo |

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

### 4.3 Features que deben retirarse o implementarse antes de cerrar Puerta 0

| Elemento | Problema | Decisión |
| --- | --- | --- |
| `TEXTURE_COMPRESSION_BC` | No hay mapeo BC | Retirar en el contrato v0; implementar en un paquete posterior |
| `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` | Las caps por formato sobre-anuncian MSAA | Mantener solo con caps corregidas y matriz verde |
| `DEPTH_CLIP_CONTROL` | Implementado, sin test dedicado | Mantener; test de profundidad recortada |

## 5. Downlevel flags

### 5.1 Cómo se calculan hoy

`conv::downlevel_for_feature_level` parte de
`wgpu_types::DownlevelFlags::compliant()`. En esta versión, `compliant()` es
**todas** las flags menos `ANISOTROPIC_FILTERING`. Después elimina:

- `VIEW_FORMATS`;
- `SURFACE_VIEW_FORMATS`;
- `NONBLOCKING_QUERY_RESOLVE`;
- `COMPUTE_SHADERS`;
- `FRAGMENT_WRITABLE_STORAGE`.

Para feature level 10_1 elimina además `FRAGMENT_STORAGE` y `VERTEX_STORAGE`.
El resultado en 11_0 sigue siendo una lista heredada, no una lista de
capacidades comprobadas. Esto viola la regla 1 del plan.

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

### 5.3 Conjunto objetivo v0

El contrato v0 no parte de `compliant()`. Propone una lista explícita mínima:

```
BASE_VERTEX
NON_POWER_OF_TWO_MIPMAPPED_TEXTURES
CUBE_ARRAY_TEXTURES
COMPARISON_SAMPLERS
INDEPENDENT_BLEND
FULL_DRAW_INDEX_UINT32
DEPTH_BIAS_CLAMP
```

`DEPTH_TEXTURE_AND_BUFFER_COPIES` y `UNRESTRICTED_INDEX_BUFFER` se añaden solo
si sus tests pasan. Cualquier otra flag queda fuera hasta que exista
implementación y prueba. Esto hace que `is_webgpu_compliant()` sea `false`,
que es la respuesta honesta para un backend downlevel.

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
| `RENDER_TARGET` | `COLOR_ATTACHMENT | MULTISAMPLE_X4` |
| `BLENDABLE` | `COLOR_ATTACHMENT_BLEND` |
| `DEPTH_STENCIL` | `DEPTH_STENCIL_ATTACHMENT` |

El problema es `MULTISAMPLE_X4`: la creación de texturas y render passes exige
`sample_count == 1`, y no hay resolve. Anunciar `MULTISAMPLE_X4` permite que
`wgpu-core` valide una textura o pipeline MSAA y después el HAL la rechace.
Debe retirarse hasta que exista MSAA real y una prueba de resolve.

`TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` solo puede mantenerse si esta
traducción se corrige y se prueba formato por formato. Mientras tanto, el
contrato v0 no garantiza ninguna capacidad por formato que no esté en la
matriz de P6.

### 6.3 Superficie

Solo `Bgra8Unorm` y `Bgra8UnormSrgb`, con `TextureUses::COLOR_TARGET`,
`SurfaceColorSpaces::SRGB`, `CompositeAlphaMode::Opaque`, `Fifo` e `Immediate`.
No se anuncian 10 bits, HDR, `Mailbox`, `ExclusiveFullScreen` ni
`PostMultiplied`/`PreMultiplied`.

## 7. Límites

Los valores salen de `Limits::downlevel_defaults()` con los cambios de
`conv::limits_for_feature_level`: dimensión 1D/2D 16384 en 11_0 y 8192 en
10_1, dimensión 3D 2048 y arrays 2048. El resto queda en los valores
downlevel por defecto.

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
| `max_samplers_per_shader_stage` | 16 | `OK` | Coincide con el máximo de D3D11 (16) |
| `max_storage_buffers_per_shader_stage` | 4 | `SOBRE` | `set_bind_group` descarta storage buffers |
| `max_storage_textures_per_shader_stage` | 4 | `SOBRE` | Las vistas UAV se rechazan |
| `max_uniform_buffers_per_shader_stage` | 12 | `OK` | Conservador |
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

El contrato v0 **no anuncia**:

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

El test todavía no existe. Debe vivir en el fork de wgpu, ejecutarse en
Windows CI (WARP y hardware) y usar la API pública de `wgpu` cuando sea
posible, además de HAL para los casos internos.

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

## 10. Estado de las puertas

| Puerta | Estado | Evidencia |
| --- | --- | --- |
| 0 — Congelación y contrato | Documentos creados; test y correcciones pendientes | Este documento, `dx11-agent-map.md`, `dx11-task-packets.md` |
| 1 — Conformidad HAL | Parcial | `raw-dx11`, `raw-dx11-resources`, `raw-dx11-shader`, `raw-dx11-triangle`, `raw-dx11-window`; faltan 2 RT, arrays/cubemap, resize, fences reales, device loss y debug layer |
| 2 — Menú Bevy/Cinnabar | No iniciada | Sin perfil `Dx11Legacy` verificado ni captura de menú |
| 3 — Mundo local mínimo | No iniciada | Sin build ni prueba |
| 4 — Mundo vanilla | No iniciada | Sin build ni prueba |
| 5 — Gameplay completo | No iniciada | Sin build ni prueba |
| 6 — Compute opcional | No iniciada | Fuera del contrato v0 |

No se publica ninguna build de usuario hasta que la puerta correspondiente
esté cerrada.

## 11. Riesgos y bloqueos

- **BC falso**: cualquier asset BC falla al crear la textura. Es el bloqueo
  más probable para el menú de Cinnabar.
- **MSAA falso**: una ruta que pida 4x pasa la validación y falla en el HAL.
- **Storage/compute falsos**: layouts aceptados que no se pueden bindear o
  draws que leen datos vacíos.
- **Immediates silenciosos**: un shader con push constants lee ceros sin error.
- **Fences de CPU**: `Queue::submit` señaliza el fence en CPU y
  `wait_for_idle` solo hace `Flush`; no hay espera real de GPU. La Puerta 1 no
  puede cerrarse con esto.
- **Sin debug layer ni device loss**: no se pueden detectar errores de driver
  ni validar recuperación.
- **Sin resize probado**: la recreación de swapchain existe, pero no hay test
  de resize ni de recursos asociados.
- **`BUILD-NOTES.md` desactualizado**: afirma que ningún fork de wgpu
  implementa DX11; el pin actual sí tiene un HAL DX11. Es documentación de un
  build GL temporal y no debe usarse como contrato.
- **Assets BC**: si el resource pack usa BC y P2 no implementa BC, el menú de
  Cinnabar no llega. Hay que decidir entre implementar BC o usar una ruta de
  assets sin comprimir; no se anuncia BC mientras no exista.

## 12. Referencias

- Plan: [`docs/dx11-backend-plan.md`](dx11-backend-plan.md)
- Mapa de agentes: [`docs/dx11-agent-map.md`](dx11-agent-map.md)
- Paquetes de tareas: [`docs/dx11-task-packets.md`](dx11-task-packets.md)
- Fuente del pin: `wgpu-hal/src/dx11/{adapter,conv,device,command,surface,queue,shader,types,instance,mod}.rs`
- Ejemplos del fork: `wgpu-hal/examples/raw-dx11{,-resources,-shader,-triangle,-window}.rs`
- Workflows del fork: `.github/workflows/dx11-{hal,resources,descriptors,pipeline,surface,core}-check.yml`
- Límites de wgpu: `wgpu-types/src/limits.rs`
- Capacidades de formato: `wgpu-hal/src/dx11/adapter.rs::texture_format_capabilities`
