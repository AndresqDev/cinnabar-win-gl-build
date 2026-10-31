#!/usr/bin/env bash
# Avoid alternate sRGB surface views on the ANGLE/GLES path.
#
# Bevy normally configures a window surface with a non-sRGB base format and an
# sRGB view format. wgpu's GLES backend, including ANGLE, has no surface view
# formats, so Surface::configure rejects that configuration. This experimental
# build renders to the base surface format instead; the scene target remains
# sRGB and Cinnabar's gamma-space presentation path handles the final output.
set -euo pipefail

checkout_root="${CARGO_HOME:-$HOME/.cargo}/git/checkouts"
file=$(find "$checkout_root" -type f \
    -path '*/bevy-*/35c4988/crates/bevy_render/src/view/window/mod.rs' \
    -print -quit)

if [ -z "$file" ]; then
    echo "error: pinned Bevy window surface source not found under $checkout_root" >&2
    echo "run 'cargo fetch --locked' first and check the pinned Bevy revision" >&2
    exit 1
fi

python3 - "$file" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
text = path.read_text()

old_set_format = """        self.texture_view_format = srgb_view_format(format);
        self.configuration.format = format;
        self.configuration.color_space = color_space;
        self.configuration.view_formats = self.texture_view_format.into_iter().collect();
"""
new_set_format = """        self.texture_view_format = None;
        self.configuration.format = format;
        self.configuration.color_space = color_space;
        self.configuration.view_formats.clear();
"""
if old_set_format not in text:
    raise SystemExit("error: Bevy SurfaceData::set_format block not found")
text = text.replace(old_set_format, new_set_format, 1)

old_prepare = """            let texture_view_format = srgb_view_format(format);
            let configuration = SurfaceConfiguration {
"""
new_prepare = """            // ANGLE/GLES has no alternate surface format views.
            let texture_view_format = None;
            let configuration = SurfaceConfiguration {
"""
if old_prepare not in text:
    raise SystemExit("error: Bevy prepare_windows view-format block not found")
text = text.replace(old_prepare, new_prepare, 1)

old_swapchain = """    fn set_swapchain_texture(&mut self, frame: wgpu::SurfaceTexture) {
        self.swap_chain_texture_view_format = Some(frame.texture.format().add_srgb_suffix());
        let texture_view_descriptor = TextureViewDescriptor {
            format: self.swap_chain_texture_view_format,
            ..default()
        };
"""
new_swapchain = """    fn set_swapchain_texture(&mut self, frame: wgpu::SurfaceTexture) {
        // The GLES surface has no alternate view formats; use its base format.
        self.swap_chain_texture_view_format = Some(frame.texture.format());
        let texture_view_descriptor = TextureViewDescriptor {
            format: self.swap_chain_texture_view_format,
            ..default()
        };
"""
if old_swapchain not in text:
    raise SystemExit("error: Bevy set_swapchain_texture block not found")
text = text.replace(old_swapchain, new_swapchain, 1)

path.write_text(text)
PY

echo "Patched Bevy window surfaces to avoid GLES surface view formats:"
grep -nF 'no alternate surface format views' "$file" | head -2
grep -nF 'The GLES surface has no alternate view formats' "$file" | head -2
