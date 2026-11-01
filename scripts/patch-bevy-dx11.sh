#!/usr/bin/env bash
# Keep the DX11 compatibility build from initializing Bevy's occlusion depth
# pyramid. The D3D11 HAL does not implement compute or storage textures yet;
# Cinnabar's stock-systems plugin already removes the pyramid *systems*, but
# Bevy's CorePipelinePlugin still creates a dummy R32Float storage texture at
# RenderStartup, which fails before the client can reach its menu.
#
# This patch is applied to the locked Bevy git checkout only. It is deliberately
# narrow: the MipGenerationPlugin remains compiled and available, but is not
# installed by CorePipelinePlugin in this build.
set -euo pipefail

checkout_root="${CARGO_HOME:-$HOME/.cargo}/git/checkouts"
lib=$(find "$checkout_root" -type f \
    -path '*/bevy_core_pipeline/src/lib.rs' \
    -path '*/35c4988/*' \
    -print -quit)

if [ -z "$lib" ]; then
    echo "error: locked Bevy CorePipelinePlugin sources not found under $checkout_root" >&2
    echo "run 'cargo fetch --locked' first and check the pinned Bevy revision" >&2
    exit 1
fi

python3 - "$lib" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
source = path.read_text()
if "MipGenerationPlugin" not in source:
    print("bevy CorePipelinePlugin is already patched for DX11; nothing to do")
    raise SystemExit(0)

replacements = (
    (
        "    deferred::copy_lighting_id::CopyDeferredLightingIdPlugin, mip_generation::MipGenerationPlugin,\n",
        "    deferred::copy_lighting_id::CopyDeferredLightingIdPlugin,\n",
    ),
    (
        "                OrderIndependentTransparencyPlugin,\n                MipGenerationPlugin,\n",
        "                OrderIndependentTransparencyPlugin,\n",
    ),
)
for old, new in replacements:
    if old not in source:
        raise SystemExit(
            "error: expected Bevy CorePipelinePlugin pattern not found; "
            "the pinned Bevy revision changed"
        )
    source = source.replace(old, new, 1)

path.write_text(source)
print(f"patched {path}: MipGenerationPlugin is not installed for DX11")
PY
