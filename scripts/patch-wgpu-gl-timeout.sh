#!/usr/bin/env bash
# Widen wgpu's WGL context-lock watchdog for slow Windows OpenGL drivers.
#
# wgpu-hal uses a one-second try-lock timeout in wgl.rs and reports a timeout as
# "Could not lock adapter context. This is most-likely a deadlock." On the Intel
# HD 4400, large texture/buffer uploads can legitimately hold the WGL context for
# longer than one second, so the render thread aborts even though there is no
# deadlock. Raise the watchdog to 60 seconds; a real deadlock still aborts, just
# after enough time for slow but valid uploads to finish.
set -euo pipefail

checkout_root="${CARGO_HOME:-$HOME/.cargo}/git/checkouts"
file=$(find "$checkout_root" -type f \
    -path '*/wgpu-*/060bd0b/wgpu-hal/src/gles/wgl.rs' \
    -print -quit)

if [ -z "$file" ]; then
    echo "error: wgpu WGL source not found under $checkout_root" >&2
    echo "run 'cargo fetch --locked' first and check the pinned wgpu revision" >&2
    exit 1
fi

old='const CONTEXT_LOCK_TIMEOUT_SECS: u64 = 1;'
new='const CONTEXT_LOCK_TIMEOUT_SECS: u64 = 60;'
if ! grep -qF "$old" "$file"; then
    echo "error: expected timeout declaration not found in $file" >&2
    exit 1
fi

sed -i "s/$old/$new/" "$file"
grep -nF 'CONTEXT_LOCK_TIMEOUT_SECS' "$file"
