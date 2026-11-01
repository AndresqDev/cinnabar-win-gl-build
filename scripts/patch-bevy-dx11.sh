#!/usr/bin/env bash
# Keep the DX11 compatibility build from creating a storage texture for Bevy's
# depth-pyramid placeholder. Cinnabar removes the depth-pyramid systems from its
# own schedule, but Bevy's MipGenerationPlugin still creates a 1x1 R32Float
# STORAGE_BINDING dummy at RenderStartup. D3D11 storage textures/UAVs are not
# implemented yet, so make that placeholder a normal sampled R32Float texture.
#
# The plugin and its scheduled systems stay installed: Cinnabar's stock-system
# cleanup asserts that create_downsample_depth_pipelines is still present.
set -euo pipefail

checkout_root="${CARGO_HOME:-$HOME/.cargo}/git/checkouts"
depth=$(find "$checkout_root" -type f \
    -path '*/bevy_core_pipeline/src/mip_generation/experimental/depth.rs' \
    -path '*/35c4988/*' \
    -print -quit)

if [ -z "$depth" ]; then
    echo "error: locked Bevy depth-pyramid sources not found under $checkout_root" >&2
    echo "run 'cargo fetch --locked' first and check the pinned Bevy revision" >&2
    exit 1
fi

python3 - "$depth" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
source = path.read_text()
old = """            format: TextureFormat::R32Float,
            usage: TextureUsages::STORAGE_BINDING,
            view_formats: &[],
"""
new = """            format: TextureFormat::R32Float,
            // DX11 has no UAV storage support yet. The dummy is only sampled
            // when no depth pyramid is needed, so an SRV is sufficient.
            usage: TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
"""
if new in source:
    print("bevy depth-pyramid dummy is already patched for DX11; nothing to do")
    raise SystemExit(0)
if old not in source:
    raise SystemExit(
        "error: expected Bevy depth-pyramid dummy texture pattern not found; "
        "the pinned Bevy revision changed"
    )
path.write_text(source.replace(old, new, 1))
print(f"patched {path}: depth-pyramid dummy uses TEXTURE_BINDING")
PY
