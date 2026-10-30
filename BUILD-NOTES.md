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

Everything else is upstream source. The default startup order stays **DX12 → Vulkan → GL**.
To force OpenGL, set `WGPU_BACKEND=gl` before launching `cinnabar.exe`, or pick **GL** under
**Settings → Video → Graphics API**.

## Why not DX11?

Cinnabar renders with Bevy + wgpu. wgpu has no Direct3D 11 backend; its Windows backends are
DX12, Vulkan and (here, experimentally) OpenGL. DX11 support would require replacing the
renderer, not changing a build flag.

OpenGL was removed from the upstream backend policy in PR #1010 (October 2026). It is
re-enabled here on a best-effort basis and may be less stable or slower than DX12/Vulkan.
If the game refuses OpenGL startup, use Auto/DX12/Vulkan or an older GPU driver with OpenGL 4.3+.

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
