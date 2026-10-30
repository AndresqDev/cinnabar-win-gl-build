# Cinnabar Windows build with OpenGL fallback

This is a **temporary, unofficial** build repository. It is a snapshot of
[`cinnabar-mc/cinnabar`](https://github.com/cinnabar-mc/cinnabar) `dev` used to produce a
Windows executable. It is not affiliated with or endorsed by the Cinnabar project.

## What changed

- `crates/launcher/src/graphics_backend.rs`: OpenGL (`gl`) is back in the backend registry and is
  the last automatic fallback on Windows. The Video settings list therefore offers
  **Auto / Vulkan / DX12 / GL**.
- `app/Cargo.toml`: enables wgpu's `gles` feature on Windows so the native WGL backend is
  compiled.
- `crates/render/src/gl_compat.rs`: a small runtime compatibility layer, active only when the
  selected backend is GL. It flattens the scene and artwork views, preserves the native leaf
  filtering with a linear companion atlas, and forces Vanilla rendering.
- `SceneTarget` uses one sRGB attachment on GL. The gamma-space transparent/name-tag pipelines
  are retargeted to sRGB so they remain valid without alternate texture views.

Everything else is upstream source. The default startup order stays **DX12 → Vulkan → GL**.
To force OpenGL, set `WGPU_BACKEND=gl` before launching `cinnabar.exe`, or pick **GL** under
**Settings → Video → Graphics API**.

## Why not DX11?

Cinnabar renders with Bevy + wgpu. wgpu's old Direct3D 11 backend was only an adapter
skeleton: `wgpu-hal/src/dx11/device.rs` and `command.rs` were full of `todo!()`, and it
never rendered anything. It was removed in wgpu 0.19 (PR #4828). No wgpu fork implements it.
`gfx-rs/gfx` has a DX11 backend, but it is a different, maintenance-only HAL and cannot be
dropped into wgpu/Bevy.

Upstream wgpu now has an ANGLE/GLES backend for Windows (PR #9422), which translates GLES to
D3D11. ANGLE's D3D11 backend only exposes GLES 3.0, however, while Cinnabar requires GLES 3.1
storage/compute features, so ANGLE cannot replace a real DX11 backend here.

## OpenGL status (important)

This build compiles wgpu's native WGL/OpenGL backend on Windows and runs the renderer in a
degraded GL compatibility mode. It no longer requires `wgpu::DownlevelFlags::VIEW_FORMATS`,
because the compatibility layer removes every alternate-format view:

- The shared main-pass target is one sRGB attachment instead of a linear texture plus an sRGB
  view.
- Chunk leaves use a companion `Rgba8Unorm` atlas, so their gamma-space filtering is preserved.
- Raw artwork and shader-pack views are flattened; shader packs and Enhanced/HDR are disabled
  while GL is selected.

Requirements and known sacrifices:

- Native OpenGL 4.3+ is required by the remaining renderer capabilities.
- Ordinary transparents, name tags, and entity shadows blend in **linear** space instead of the
  native gamma-space path. This can change water, glass, particle, and shadow tint slightly;
  it is a deliberate compatibility trade-off, not a random corruption.
- HDR/Enhanced and shader packs are unavailable in this mode.
- DX12 and Vulkan keep the original quality and are still the default backends.

If GL is selected but the driver lacks another required capability, startup reports the missing
flag or limit instead of using an unsupported path. DX12 and Vulkan remain the recommended
backends.

## Artifact layout

The GitHub Actions artifact is a portable folder:

```
cinnabar.exe
bedrock-core.exe
bedrock-local-server.exe
cinnabar-updater.exe
dxcompiler.dll
resources/
  assets/block-physics-v2225.bin
  fonts/CinnanglesSans.ttf
  licenses/...
  prep-kit/bin/assetc.exe
  prep-kit/...                # asset compiler inputs
```

Keep the files together and run `cinnabar.exe`. On first launch the client downloads the vanilla
resource pack after you accept the Minecraft EULA, then compiles it locally. No Mojang assets are
bundled here.

The build is unsigned, so Windows SmartScreen may warn. Only run the artifact produced by this
repository's Actions run.

## License

Cinnabar is licensed under PolyForm Noncommercial 1.0.0. See `LICENSE` and
`THIRD_PARTY_NOTICES.md`. This temporary repository exists only to host the source used by the
GitHub Actions build and can be deleted after the artifact is downloaded.
