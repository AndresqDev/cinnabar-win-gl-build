#!/usr/bin/env bash
# Avoid alternate sRGB surface views on the ANGLE/GLES path.
#
# Bevy normally configures a window surface with a non-sRGB base format and an
# sRGB view format. wgpu's GLES backend, including ANGLE, has no surface view
# formats, so Surface::configure rejects that configuration. This experimental
# build prefers an advertised sRGB base surface format when one exists and
# otherwise renders to the base surface format. The scene target remains sRGB;
# Cinnabar's gamma-space presentation path handles the final output.
#
# The script is idempotent: a checkout that already contains every replacement
# is accepted as-is, so a re-run after a failed job does not fail the build.
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

old_prepare = """            let formats = caps.formats;
            // For future HDR output support, we'll need to request a format that supports HDR,
            // but as of wgpu 0.15 that is not yet supported.
            // Prefer sRGB formats for surfaces, but fall back to first available format if no sRGB formats are available.
            let mut format = *formats.first().expect("No supported formats for surface");
            for available_format in formats {
                // Rgba8UnormSrgb and Bgra8UnormSrgb and the only sRGB formats wgpu exposes that we can use for surfaces.
                if available_format == TextureFormat::Rgba8UnormSrgb
                    || available_format == TextureFormat::Bgra8UnormSrgb
                {
                    format = available_format;
                    break;
                }
            }

            let texture_view_format = srgb_view_format(format);
"""
old_prepare_old_patch = """            let formats = caps.formats;
            // For future HDR output support, we'll need to request a format that supports HDR,
            // but as of wgpu 0.15 that is not yet supported.
            // Prefer sRGB formats for surfaces, but fall back to first available format if no sRGB formats are available.
            let mut format = *formats.first().expect("No supported formats for surface");
            for available_format in formats {
                // Rgba8UnormSrgb and Bgra8UnormSrgb and the only sRGB formats wgpu exposes that we can use for surfaces.
                if available_format == TextureFormat::Rgba8UnormSrgb
                    || available_format == TextureFormat::Bgra8UnormSrgb
                {
                    format = available_format;
                    break;
                }
            }

            // ANGLE/GLES has no alternate surface format views.
            let texture_view_format = None;
"""
new_prepare = """            let formats = caps.formats;
            // For future HDR output support, we'll need to request a format that supports HDR,
            // but as of wgpu 0.15 that is not yet supported.
            // Prefer sRGB formats for surfaces, but fall back to first available format if no sRGB formats are available.
            let mut format = *formats.first().expect("No supported formats for surface");
            for &available_format in &formats {
                // Rgba8UnormSrgb and Bgra8UnormSrgb and the only sRGB formats wgpu exposes that we can use for surfaces.
                if available_format == TextureFormat::Rgba8UnormSrgb
                    || available_format == TextureFormat::Bgra8UnormSrgb
                {
                    format = available_format;
                    break;
                }
            }
            // wgpu's GLES/ANGLE surface path has no alternate surface view formats.
            // Prefer a native sRGB base format when the surface advertises one, so
            // presentation still gets hardware sRGB encoding instead of treating
            // linear shader output as display-encoded bytes.
            if !format.is_srgb() {
                let srgb = format.add_srgb_suffix();
                if formats.contains(&srgb) {
                    format = srgb;
                }
            }
            if !format.is_srgb() {
                warn!(
                    "GLES surface exposes no sRGB base format; linear shader output will not be display-encoded"
                );
            }

            let texture_view_format = None;
"""

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


def replace_once(source, label, old_blocks, new):
    if new in source:
        print(f"already patched: {label}")
        return source, False
    for old in old_blocks:
        if old in source:
            print(f"patched: {label}")
            return source.replace(old, new, 1), True
    raise SystemExit(
        f"error: expected Bevy {label} block not found; "
        "the pinned Bevy revision or this patch is inconsistent"
    )


changed = False
text, replaced = replace_once(
    text, "SurfaceData::set_format", (old_set_format,), new_set_format
)
changed |= replaced
text, replaced = replace_once(
    text, "prepare_windows view format", (old_prepare, old_prepare_old_patch), new_prepare
)
changed |= replaced
text, replaced = replace_once(
    text, "set_swapchain_texture", (old_swapchain,), new_swapchain
)
changed |= replaced

missing = [
    name
    for name, marker in [
        ("SurfaceData::set_format", "self.configuration.view_formats.clear();"),
        ("prepare_windows view format", "let texture_view_format = None;"),
        ("set_swapchain_texture", "Some(frame.texture.format());"),
    ]
    if marker not in text
]
if missing:
    raise SystemExit("error: patch verification failed for: " + ", ".join(missing))

if changed:
    path.write_text(text)
else:
    print("Bevy window surface source already patched")
PY

echo "Patched Bevy window surfaces to avoid GLES surface view formats:"
grep -nF 'has no alternate surface view formats' "$file" | head -2
grep -nF 'The GLES surface has no alternate view formats' "$file" | head -2
