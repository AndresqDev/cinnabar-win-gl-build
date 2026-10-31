#!/usr/bin/env bash
# ANGLE's D3D11 translator cannot lower a whole-struct load from a shader
# storage block. The generated HLSL helper is a non-void function with no
# return statement, which D3DCompile reports as:
#
#   _Load__UiModelBone_std430_cm_x: Not all control paths return a value
#
# The UI 3D-model branch is the only whole-struct load in the menu shaders.
# For this ANGLE spike, collapse those vertices off-screen instead of drawing
# untransformed geometry. The model renderer, binding and bone buffer stay in
# place until that translator path is fixed. World shaders use field-wise
# record construction, which ANGLE's SSBO access-chain path supports.
#
# The script is idempotent: rerunning it on an already patched tree is a no-op.
set -euo pipefail

file="crates/render/src/ui.wgsl"
python3 - "$file" <<'PY'
import pathlib
import re
import sys

path = pathlib.Path(sys.argv[1])
root = pathlib.Path("crates/render/src")

# ANGLE's GLSL compiler rejects the `invariant` qualifier that naga emits for
# @invariant position builtins. Cinnabar uses it for exact cross-pass position
# matching; the reduced ANGLE build can skip that guarantee.
patched = 0
for shader in sorted(root.rglob("*")):
    if shader.suffix not in {".wgsl", ".wesl"}:
        continue
    source = shader.read_text(encoding="utf-8")
    if "@invariant" not in source:
        continue
    updated, count = re.subn(r"@invariant\s+", "", source)
    shader.write_text(updated, encoding="utf-8")
    patched += count
    print(f"removed {count} @invariant qualifier(s) from {shader}")

text = path.read_text(encoding="utf-8")

old = """    if (style_flags & UI_STYLE_MODEL_VERTEX) != 0u {
        let base = (texture_page >> UI_MODEL_LAYER_BITS) - 1u;
        let bone = ui_model_bones[base + u32(position.w) - 1u];
        point = bone.screen * vec4<f32>(position.xyz, 1.0);
        let normal = vec3<f32>(dot(bone.normal_x.xyz, overlay_color.xyz), dot(bone.normal_y.xyz, overlay_color.xyz), dot(bone.normal_z.xyz, overlay_color.xyz));
        if dot(normal, normal) > 0.0 {
            let n = normalize(normal);
            light = ((1.0 + n.y) * ACTOR_SHADE[0] + n.x * n.x * ACTOR_SHADE[1]) + n.z * n.z * ACTOR_SHADE[2] + ACTOR_SHADE[3];
        }
        overlay = bone.overlay;
        model_uv = bone.uv_rect.xy + uv * (bone.uv_rect.zw - bone.uv_rect.xy) + uv.yx * bone.uv_cross.xy;
        model_color = color * bone.tint;
        model_styles |= bone.styles.x;
    }
"""
new = """    // ANGLE spike: the D3D11 translator cannot lower a whole-struct load from
    // this storage buffer. Collapse skinned UI model vertices off-screen
    // instead of drawing untransformed geometry; the model renderer, binding
    // and bone buffer stay in place until that translator path is fixed.
    if (style_flags & UI_STYLE_MODEL_VERTEX) != 0u {
        point = vec4<f32>(
            3.0 * viewport.viewport_size.x,
            3.0 * viewport.viewport_size.y,
            0.0,
            1.0,
        );
    }
"""
marker = "// ANGLE spike: the D3D11 translator cannot lower a whole-struct load"
if old in text:
    text = text.replace(old, new, 1)
elif marker not in text:
    raise SystemExit("error: expected UI model branch or patch marker not found in ui.wgsl")

path.write_text(text, encoding="utf-8")

remaining = [
    str(shader)
    for shader in sorted(root.rglob("*"))
    if shader.suffix in {".wgsl", ".wesl"}
    and "@invariant" in shader.read_text(encoding="utf-8")
]
if remaining:
    raise SystemExit("error: @invariant remains in: " + ", ".join(remaining))
if "ui_model_bones[" in text:
    raise SystemExit("error: ui.wgsl still reads the UI model bone storage buffer")

# Imported shader text outside crates/render/src is composed into the same
# programs. Do not edit those owned files, but fail loudly if a future change
# reintroduces the qualifier there.
external = [
    pathlib.Path("crates/render-api/src"),
    pathlib.Path("crates/render-model/src"),
    pathlib.Path("crates/mod-render/src"),
]
for directory in external:
    if not directory.is_dir():
        continue
    for shader in sorted(directory.rglob("*")):
        if shader.suffix not in {".wgsl", ".wesl"}:
            continue
        if "@invariant" in shader.read_text(encoding="utf-8"):
            raise SystemExit(
                f"error: @invariant in composed shader outside crates/render/src: {shader}"
            )

print(f"removed {patched} @invariant qualifier(s); UI model vertices are collapsed")
PY

grep -nF 'ANGLE spike: the D3D11 translator' "$file"
