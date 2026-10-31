#!/usr/bin/env bash
# ANGLE's D3D11 shader translator rejects the dynamically indexed structured
# buffer load used by the UI 3D-model branch:
#
#   _Load__UiModelBone_std430_cm_x: Not all control paths return a value
#
# The UI model is cosmetic. For this ANGLE spike, remove the model branch so the
# generated UI vertex shader does not contain that storage-buffer load. The
# binding and the model renderer can stay in place; the shader simply never
# samples the bone array.
set -euo pipefail

file="crates/render/src/ui.wgsl"
python3 - "$file" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
text = path.read_text()

# ANGLE's GLSL compiler rejects the `invariant` qualifier that naga emits for
# @invariant position builtins. Cinnabar uses it for exact cross-pass position
# matching; the reduced ANGLE build can skip that guarantee.
for shader in pathlib.Path("crates/render/src").rglob("*"):
    if shader.suffix not in {".wgsl", ".wesl"} or shader == path:
        continue
    source = shader.read_text()
    if "@invariant" in source:
        shader.write_text(source.replace("@invariant ", ""))
        print(f"removed @invariant from {shader}")
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
new = """    // ANGLE spike: the D3D11 translator rejects the dynamic structured-buffer
    // bone load above (X3507). UI model geometry is disabled until that ANGLE
    // translation path is fixed.
"""
if old not in text:
    raise SystemExit("error: expected UI model branch not found in ui.wgsl")
path.write_text(text.replace(old, new, 1))
PY

grep -nF 'ANGLE spike: the D3D11 translator' "$file"
