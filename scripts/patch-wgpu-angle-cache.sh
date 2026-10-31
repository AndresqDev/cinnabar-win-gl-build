#!/usr/bin/env bash
# Install a persistent ANGLE shader blob cache into the pinned wgpu EGL backend.
#
# Apply this script AFTER scripts/patch-wgpu-angle.sh. That script switches the
# Windows GLES backend to EGL/ANGLE; this one installs ANGLE's blob cache
# callbacks after eglInitialize and before any pipeline is created. It only
# touches the CI cargo checkout, never the Cinnabar sources or the base patch.
#
# Why this exists:
#   ANGLE compiles GLSL to HLSL and then D3D bytecode. EGL_ANDROID_blob_cache
#   lets ANGLE persist the resulting compiler blobs, so later process starts can
#   reuse them. The "ANDROID" in the extension name is historical: ANGLE's
#   frontend implements it on Windows too, where libEGL.dll exports
#   eglSetBlobCacheFuncsANDROID and the D3D11 display advertises the extension.
#
# LIMITATION: the cache does NOT avoid the first compile. It only lets a later
# run reuse what the first run compiled. The callbacks must be installed after
# eglInitialize and before any pipeline/shader compilation; this patch installs
# them in Inner::create before the first EGL context is created.
#
# Default location:
#   %LOCALAPPDATA%\Cinnabar\cache\angle-blobs-v1\blobs.bin
# Override the directory with CINNABAR_ANGLE_BLOB_CACHE_DIR (absolute path).
# If LOCALAPPDATA is missing, the path is relative, or the directory cannot be
# created/written, the cache disables itself with a log message and startup
# continues normally. Relative paths are rejected; the default never lives in
# Program Files or next to the packaged executable.
#
# The cache file has a magic/version header, a checksum, and length-prefixed
# identity strings plus key/value records. Identity includes the format
# version, EGL/ANGLE version and vendor strings, and (once an adapter has been
# enumerated) the GL vendor/renderer/version strings. A mismatch discards the
# old entries instead of handing driver-specific blobs to another GPU/driver.
#
# On-disk layout (all integers little-endian):
#   offset  size  field
#   0       8     magic "CINANGLE"
#   8       4     format version (currently 1)
#   12      4     flags (must be 0)
#   16      4     payload length in bytes
#   20      4     reserved (must be 0)
#   24      8     FNV-1a 64 checksum of the payload
#   32      ...   payload:
#                   identity_count u32, then per identity: len u32 + UTF-8
#                   entry_count u32, then per entry: key_len u32 + key,
#                   value_len u32 + value
# Writes are coalesced and committed with a temporary file + atomic rename.
# A background flusher persists pending entries every few seconds; a normal
# shutdown forces a final flush from Inner::drop. A crash leaves the previous
# snapshot intact. Multiple Cinnabar instances share the directory, so the last
# writer wins (the file itself is never left torn).
#
# The script is idempotent. It fails loudly if the expected post-base-patch
# anchors are missing, or if it finds a partially/conflicting patched state.
set -euo pipefail

checkout_root="${CARGO_HOME:-$HOME/.cargo}/git/checkouts"
egl=$(find "$checkout_root" -type f \
    -path '*/wgpu-*/060bd0b/wgpu-hal/src/gles/egl.rs' \
    -print -quit)

if [ -z "$egl" ]; then
    echo "error: pinned wgpu GLES sources not found under $checkout_root" >&2
    echo "run 'cargo fetch --locked' first and check the pinned wgpu revision" >&2
    exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
    echo "error: python3 is required to apply the ANGLE blob cache patch" >&2
    exit 1
fi

python3 - "$egl" <<'PY'
import pathlib
import sys

egl_path = pathlib.Path(sys.argv[1])
egl_dir = egl_path.parent
helper_path = egl_dir / "angle_blob_cache.rs"

try:
    egl = egl_path.read_text(encoding="utf-8")
except OSError as error:
    raise SystemExit(f"error: cannot read {egl_path}: {error}")

# This patch is designed for the tree produced by scripts/patch-wgpu-angle.sh.
# Without the base patch the EGL module is not selected on Windows, so installing
# callbacks here would silently do nothing. Fail instead of leaving a confusing
# half-configured checkout.
if "EGL_PLATFORM_ANGLE_TYPE_D3D11_ANGLE" not in egl:
    raise SystemExit(
        "error: the base ANGLE patch was not detected in egl.rs; "
        "run scripts/patch-wgpu-angle.sh first"
    )

MODULE_MARKER = "mod angle_blob_cache;"
INSTALL_MARKER = (
    "angle_blob_cache::install(&egl, display, &display_extensions, version);"
)
IDENTITY_MARKER = "angle_blob_cache::update_identity(&["
FLUSH_MARKER = "angle_blob_cache::flush();"
MARKERS = (MODULE_MARKER, INSTALL_MARKER, IDENTITY_MARKER, FLUSH_MARKER)

RUST_SOURCE = r'''//! Persistent shader blob cache for the Windows ANGLE EGL backend.
//!
//! `EGL_ANDROID_blob_cache` is exposed by ANGLE on Windows even though the
//! extension name is historical. ANGLE may call these callbacks from any
//! thread, including worker threads that compile pipelines, so all cache state
//! is behind a mutex. The callbacks have no user-data argument and are global
//! to the process; one cache is shared by every EGL display. `install` still
//! calls `eglSetBlobCacheFuncsANDROID` at most once per display, as the
//! extension requires.
//!
//! LIMITATION: this only lets later runs reuse ANGLE's compiled blobs. The
//! first run still compiles every shader. The callbacks must be installed
//! after `eglInitialize` and before creating or compiling any pipeline;
//! `Inner::create` calls `install` immediately after querying the display
//! extensions, before the first context exists.
//!
//! The on-disk format is a snapshot with a fixed header, a checksum, then
//! length-prefixed identity strings and key/value records. Writes go to a
//! unique temporary file in the same directory and are committed with an
//! atomic rename. A background flusher persists pending entries every few
//! seconds, and a normal shutdown forces a final flush from `Inner::drop`.
//! If the process is killed, the previous snapshot remains valid; changes
//! since the last periodic flush are lost.

use alloc::collections::btree_map::Entry;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use core::time::Duration;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use super::EglInstance;

const CACHE_DIR_ENV: &str = "CINNABAR_ANGLE_BLOB_CACHE_DIR";
const CACHE_DIR_NAME: &str = "angle-blobs-v1";
const CACHE_FILE_NAME: &str = "blobs.bin";

const MAGIC: &[u8; 8] = b"CINANGLE";
// Bump FORMAT_VERSION (and preferably CACHE_DIR_NAME) if this layout changes.
const FORMAT_VERSION: u32 = 1;
const HEADER_LEN: usize = 32;

/// Keep the snapshot comfortably bounded; shader blobs are usually kilobytes.
const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_BLOB_BYTES: usize = 8 * 1024 * 1024;
const MAX_KEY_BYTES: usize = 8 * 1024;
const MAX_ENTRIES: usize = 4096;
const MAX_IDENTITY_BYTES: usize = 16 * 1024;
const MAX_IDENTITY_PARTS: u32 = 32;
const MAX_FILE_BYTES: u64 = (MAX_CACHE_BYTES + 1024 * 1024) as u64;

/// Coalesce writes so a burst of pipeline compilations does not rewrite the
/// whole snapshot for every blob.
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);

type EglSizeiAndroid = isize;
type EglSetBlobFuncAndroid =
    unsafe extern "system" fn(*const c_void, EglSizeiAndroid, *const c_void, EglSizeiAndroid);
type EglGetBlobFuncAndroid = unsafe extern "system" fn(
    *const c_void,
    EglSizeiAndroid,
    *mut c_void,
    EglSizeiAndroid,
) -> EglSizeiAndroid;
type EglSetBlobCacheFuncsAndroid =
    unsafe extern "system" fn(khronos_egl::Display, EglSetBlobFuncAndroid, EglGetBlobFuncAndroid);

struct CacheState {
    inner: Option<CacheInner>,
}

struct CacheInner {
    file: PathBuf,
    base_identity: Vec<String>,
    identity: Vec<String>,
    identity_ready: bool,
    loaded: Option<LoadedCache>,
    entries: BTreeMap<Vec<u8>, BlobEntry>,
    total_bytes: usize,
    next_use: u64,
    dirty: bool,
    last_flush: Instant,
    flush_failures: u32,
}

struct LoadedCache {
    identity: Vec<String>,
    entries: BTreeMap<Vec<u8>, BlobEntry>,
}

struct BlobEntry {
    value: Vec<u8>,
    last_used: u64,
}

static CACHE: Mutex<CacheState> = Mutex::new(CacheState { inner: None });
// Display pointers are intentionally never removed. If an application terminates
// a display and EGL later reuses the same address, this cache stays disabled for
// the new display rather than violating the extension's once-per-lifetime rule.
// Cinnabar uses one long-lived display for the life of the process.
static INSTALLED_DISPLAYS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Installs the process-wide blob cache callbacks for one EGL display.
///
/// Must be called after `eglInitialize` and before creating or compiling any
/// pipeline. Repeating the call for the same display is ignored because the
/// extension only permits one successful call per display lifetime.
pub fn install(
    egl: &EglInstance,
    display: khronos_egl::Display,
    display_extensions: &str,
    egl_version: (i32, i32),
) {
    let display_key = display.as_ptr() as usize;

    if !display_extensions
        .split_whitespace()
        .any(|extension| extension == "EGL_ANDROID_blob_cache")
    {
        log::info!("ANGLE blob cache disabled: EGL_ANDROID_blob_cache is not advertised");
        return;
    }

    {
        let installed = INSTALLED_DISPLAYS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if installed.contains(&display_key) {
            log::trace!("ANGLE blob cache already installed for display 0x{display_key:x}");
            return;
        }
    }

    let Some(file) = resolve_cache_file() else {
        return;
    };
    let Some(set_funcs) = load_set_funcs(egl) else {
        return;
    };

    let mut base_identity = Vec::new();
    base_identity.push(format!("format={FORMAT_VERSION}"));
    base_identity.push(format!(
        "egl_client_vendor={}",
        query_string(egl, None, khronos_egl::VENDOR)
    ));
    base_identity.push(format!(
        "egl_display_vendor={}",
        query_string(egl, Some(display), khronos_egl::VENDOR)
    ));
    base_identity.push(format!(
        "egl_version={}",
        query_string(egl, Some(display), khronos_egl::VERSION)
    ));
    base_identity.push(format!(
        "egl_version_number={}.{}",
        egl_version.0, egl_version.1
    ));
    base_identity.push(format!(
        "egl_client_apis={}",
        query_string(egl, Some(display), khronos_egl::CLIENT_APIS)
    ));

    let loaded = match load_file(&file) {
        Ok(Some(loaded)) => {
            log::debug!(
                "ANGLE blob cache: loaded {} entries from {}; identity is checked after adapter enumeration",
                loaded.entries.len(),
                file.display()
            );
            Some(loaded)
        }
        Ok(None) => {
            log::debug!("ANGLE blob cache: no existing cache at {}", file.display());
            None
        }
        Err(error) => {
            log::warn!(
                "ANGLE blob cache: ignoring unreadable cache at {}: {error}",
                file.display()
            );
            None
        }
    };

    let mut state = lock_cache();
    let created = state.inner.is_none();
    if created {
        state.inner = Some(CacheInner {
            file: file.clone(),
            base_identity,
            identity: Vec::new(),
            identity_ready: false,
            loaded,
            entries: BTreeMap::new(),
            total_bytes: 0,
            next_use: 1,
            dirty: false,
            last_flush: Instant::now(),
            flush_failures: 0,
        });
        log::info!("ANGLE blob cache enabled at {}", file.display());
    } else {
        log::debug!("ANGLE blob cache already active; reusing process-wide cache");
    }
    drop(state);

    start_flusher();

    // Claim the display before calling into ANGLE. The extension permits only
    // one successful call per display lifetime, so a concurrent second init
    // must not race us into a second call.
    {
        let mut installed = INSTALLED_DISPLAYS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if installed.contains(&display_key) {
            log::trace!("ANGLE blob cache already installed for display 0x{display_key:x}");
            return;
        }
        installed.push(display_key);
    }

    // SAFETY: EGL_ANDROID_blob_cache defines exactly this entry point and
    // callback signature. The pointer comes from eglGetProcAddress on the
    // libEGL instance that stays loaded for the life of the process.
    unsafe {
        set_funcs(display, blob_cache_set, blob_cache_get);
    }
    log::debug!("ANGLE blob cache callbacks installed for display 0x{display_key:x}");
}

/// Adds the GPU/driver-specific half of the cache identity.
///
/// Called while a context is current and before pipelines are built. If the
/// identity in the loaded file is different, cached entries are dropped instead
/// of being served to a different driver. Entries set before this call are kept
/// only when they belong to the same identity.
pub fn update_identity(parts: &[&str]) {
    let mut state = lock_cache();
    let Some(inner) = state.inner.as_mut() else {
        return;
    };

    let mut identity = inner.base_identity.clone();
    for part in parts {
        identity.push((*part).to_string());
    }

    if inner.identity_ready && inner.identity == identity {
        return;
    }

    let mut must_write = false;

    if inner.identity_ready && inner.identity != identity {
        log::info!(
            "ANGLE blob cache identity changed; discarding {} entries",
            inner.entries.len()
        );
        inner.entries.clear();
        inner.total_bytes = 0;
        must_write = true;
    }

    if let Some(loaded) = inner.loaded.take() {
        if loaded.identity == identity {
            let loaded_count = loaded.entries.len();
            let loaded_bytes = loaded
                .entries
                .values()
                .map(|entry| entry.value.len())
                .sum::<usize>();
            for (key, mut entry) in loaded.entries {
                // Entries from disk are older than anything staged during this
                // run, so give them the same low LRU rank.
                entry.last_used = 0;
                let size = entry_size(&key, &entry.value);
                let inserted = match inner.entries.entry(key) {
                    Entry::Vacant(slot) => {
                        slot.insert(entry);
                        true
                    }
                    Entry::Occupied(_) => false,
                };
                if inserted {
                    inner.total_bytes += size;
                }
            }
            log::info!(
                "ANGLE blob cache identity matched: reusing {loaded_count} entries (~{loaded_bytes} bytes)"
            );
            // If callbacks staged a few entries before enumeration, persist
            // them, but do not rewrite an otherwise unchanged snapshot.
            must_write |= inner.dirty;
        } else {
            log::info!(
                "ANGLE blob cache identity changed; discarding {} cached entries",
                loaded.entries.len()
            );
            inner.entries.clear();
            inner.total_bytes = 0;
            must_write = true;
        }
    } else {
        // No file was loaded. Persist this run's staged entries now that the
        // full identity is known.
        must_write |= !inner.entries.is_empty();
    }

    inner.identity = identity;
    inner.identity_ready = true;
    prune_entries(inner);

    if must_write {
        inner.dirty = true;
        flush_locked(inner, true);
    }

    log::debug!(
        "ANGLE blob cache identity ready: {} entries, {} bytes",
        inner.entries.len(),
        inner.total_bytes
    );
}

/// Forces a snapshot write. Called from `Inner::drop` on graceful shutdown and
/// periodically by the background flusher.
pub fn flush() {
    let mut state = lock_cache();
    if let Some(inner) = state.inner.as_mut() {
        flush_locked(inner, true);
    }
}

fn start_flusher() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    let result = std::thread::Builder::new()
        .name(String::from("cinnabar-angle-blob-cache"))
        .spawn(|| loop {
            std::thread::sleep(FLUSH_INTERVAL);
            flush();
        });

    if let Err(error) = result {
        STARTED.store(false, Ordering::SeqCst);
        log::warn!("ANGLE blob cache: cannot start background flusher: {error}");
    }
}

unsafe extern "system" fn blob_cache_set(
    key: *const c_void,
    key_size: EglSizeiAndroid,
    value: *const c_void,
    value_size: EglSizeiAndroid,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if key.is_null() || value.is_null() {
            return;
        }
        let (Ok(key_size), Ok(value_size)) =
            (usize::try_from(key_size), usize::try_from(value_size))
        else {
            return;
        };
        if key_size == 0
            || key_size > MAX_KEY_BYTES
            || value_size == 0
            || value_size > MAX_BLOB_BYTES
        {
            log::debug!(
                "ANGLE blob cache set ignored: key={key_size} bytes, value={value_size} bytes"
            );
            return;
        }

        // SAFETY: ANGLE owns the buffers for the duration of the call and gave
        // us their sizes. The slices are never retained beyond this closure.
        let key = unsafe { core::slice::from_raw_parts(key.cast::<u8>(), key_size) };
        let value = unsafe { core::slice::from_raw_parts(value.cast::<u8>(), value_size) };

        let mut state = lock_cache();
        let Some(inner) = state.inner.as_mut() else {
            return;
        };
        let ready = inner.identity_ready;
        if insert_entry(inner, key, value) {
            log::debug!(
                "ANGLE blob cache set: key=0x{:016x}, {value_size} bytes ({})",
                fnv1a64(key),
                if ready { "live" } else { "staged" }
            );
            flush_locked(inner, false);
        }
    }));

    if result.is_err() {
        log::error!("ANGLE blob cache set callback panicked; ignoring blob");
    }
}

unsafe extern "system" fn blob_cache_get(
    key: *const c_void,
    key_size: EglSizeiAndroid,
    value: *mut c_void,
    value_size: EglSizeiAndroid,
) -> EglSizeiAndroid {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if key.is_null() {
            return 0;
        }
        let Ok(key_size) = usize::try_from(key_size) else {
            return 0;
        };
        if key_size == 0 || key_size > MAX_KEY_BYTES {
            return 0;
        }

        // SAFETY: ANGLE owns the key buffer for the duration of the call.
        let key = unsafe { core::slice::from_raw_parts(key.cast::<u8>(), key_size) };

        let mut state = lock_cache();
        let Some(inner) = state.inner.as_mut() else {
            return 0;
        };
        if !inner.identity_ready {
            log::trace!(
                "ANGLE blob cache miss (identity pending): key=0x{:016x}",
                fnv1a64(key)
            );
            return 0;
        }

        let tick = inner.next_use;
        let (found, value_len, copied) = match inner.entries.get_mut(key) {
            Some(entry) => {
                let value_len = entry.value.len();
                let fits =
                    value_size >= 0 && (value_size as usize) >= value_len && !value.is_null();
                if fits && value_len > 0 {
                    // SAFETY: The caller provided a buffer of at least
                    // `value_len` bytes. The source is a live Vec in the cache.
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            entry.value.as_ptr(),
                            value.cast::<u8>(),
                            value_len,
                        );
                    }
                }
                entry.last_used = tick;
                (true, value_len, fits)
            }
            None => (false, 0, false),
        };

        if found {
            inner.next_use = tick.wrapping_add(1);
            if copied {
                log::debug!(
                    "ANGLE blob cache hit: key=0x{:016x}, {value_len} bytes",
                    fnv1a64(key)
                );
            } else {
                log::debug!(
                    "ANGLE blob cache hit (caller buffer too small): key=0x{:016x}, {value_len} bytes",
                    fnv1a64(key)
                );
            }
            // EGL_ANDROID_blob_cache says to return the stored size even when
            // it does not fit the caller's buffer; only the copy is skipped.
            value_len as EglSizeiAndroid
        } else {
            log::debug!("ANGLE blob cache miss: key=0x{:016x}", fnv1a64(key));
            0
        }
    }));

    match result {
        Ok(size) => size,
        Err(_) => {
            log::error!("ANGLE blob cache get callback panicked; returning a miss");
            0
        }
    }
}

fn lock_cache() -> MutexGuard<'static, CacheState> {
    CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn load_set_funcs(egl: &EglInstance) -> Option<EglSetBlobCacheFuncsAndroid> {
    let address = match egl.get_proc_address("eglSetBlobCacheFuncsANDROID") {
        Some(address) => address,
        None => {
            log::warn!("ANGLE blob cache disabled: eglSetBlobCacheFuncsANDROID is not exported");
            return None;
        }
    };

    // SAFETY: EGL_ANDROID_blob_cache defines this exact entry point. The
    // function pointer belongs to the libEGL instance that remains loaded for
    // the life of the process.
    Some(unsafe {
        core::mem::transmute::<extern "system" fn(), EglSetBlobCacheFuncsAndroid>(address)
    })
}

fn query_string(
    egl: &EglInstance,
    display: Option<khronos_egl::Display>,
    name: khronos_egl::Int,
) -> String {
    match egl.query_string(display, name) {
        Ok(value) => value.to_string_lossy().into_owned(),
        Err(error) => {
            log::debug!("ANGLE blob cache: eglQueryString(0x{name:04x}) failed: {error:?}");
            String::new()
        }
    }
}

fn resolve_cache_file() -> Option<PathBuf> {
    let directory = match std::env::var_os(CACHE_DIR_ENV) {
        Some(value) => {
            if value.is_empty() {
                log::warn!("ANGLE blob cache disabled: {CACHE_DIR_ENV} is set but empty");
                return None;
            }
            PathBuf::from(value)
        }
        None => {
            let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") else {
                log::warn!("ANGLE blob cache disabled: LOCALAPPDATA is not set");
                return None;
            };
            if local_app_data.is_empty() {
                log::warn!("ANGLE blob cache disabled: LOCALAPPDATA is empty");
                return None;
            }
            PathBuf::from(local_app_data)
                .join("Cinnabar")
                .join("cache")
                .join(CACHE_DIR_NAME)
        }
    };

    if !directory.is_absolute() {
        log::warn!(
            "ANGLE blob cache disabled: cache directory is not absolute: {}",
            directory.display()
        );
        return None;
    }

    if let Err(error) = ensure_writable(&directory) {
        log::warn!(
            "ANGLE blob cache disabled: cannot create or write {}: {error}",
            directory.display()
        );
        return None;
    }

    Some(directory.join(CACHE_FILE_NAME))
}

fn ensure_writable(directory: &Path) -> io::Result<()> {
    fs::create_dir_all(directory)?;

    let probe = directory.join(format!(".cinnabar-angle-probe-{}", std::process::id()));
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&probe)?;
        file.write_all(b"cinnabar")?;
        file.sync_all()?;
    }

    match fs::remove_file(&probe) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn load_file(path: &Path) -> Result<Option<LoadedCache>, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot stat cache: {error}")),
    };
    if !metadata.is_file() {
        return Err(String::from("cache path is not a regular file"));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(format!("cache is too large ({} bytes)", metadata.len()));
    }

    let bytes = fs::read(path).map_err(|error| format!("cannot read cache: {error}"))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("cache grew too large ({} bytes)", bytes.len()));
    }

    decode(&bytes).map(Some)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "cache path has no parent directory",
        )
    })?;
    fs::create_dir_all(directory)?;

    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = directory.join(format!(
        ".{CACHE_FILE_NAME}.{}.{}.tmp",
        std::process::id(),
        counter
    ));

    let result = write_temp_and_rename(&temp, path, bytes);

    if result.is_err() {
        if let Err(error) = fs::remove_file(&temp) {
            log::trace!(
                "ANGLE blob cache: failed to remove temporary file {}: {error}",
                temp.display()
            );
        }
    }
    result
}

fn write_temp_and_rename(temp: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);

    // On Windows std::fs::rename replaces an existing destination with
    // MoveFileExW(MOVEFILE_REPLACE_EXISTING), so readers see either the old
    // complete snapshot or the new complete snapshot, never a torn file.
    fs::rename(temp, path)
}

fn insert_entry(inner: &mut CacheInner, key: &[u8], value: &[u8]) -> bool {
    let unchanged = inner
        .entries
        .get(key)
        .is_some_and(|existing| existing.value.as_slice() == value);
    if unchanged {
        let tick = inner.next_use;
        inner.next_use = tick.wrapping_add(1);
        if let Some(entry) = inner.entries.get_mut(key) {
            entry.last_used = tick;
        }
        return false;
    }

    let new_size = entry_size(key, value);
    if new_size > MAX_CACHE_BYTES {
        log::debug!("ANGLE blob cache set ignored: record is too large ({new_size} bytes)");
        return false;
    }

    let tick = inner.next_use;
    inner.next_use = tick.wrapping_add(1);
    if let Some(old) = inner.entries.insert(
        key.to_vec(),
        BlobEntry {
            value: value.to_vec(),
            last_used: tick,
        },
    ) {
        inner.total_bytes = inner
            .total_bytes
            .saturating_sub(entry_size(key, &old.value));
    }
    inner.total_bytes = inner.total_bytes.saturating_add(new_size);
    prune_entries(inner);
    inner.dirty = true;
    true
}

fn prune_entries(inner: &mut CacheInner) {
    if inner.total_bytes <= MAX_CACHE_BYTES && inner.entries.len() <= MAX_ENTRIES {
        return;
    }

    let target_bytes = MAX_CACHE_BYTES * 3 / 4;
    let target_entries = MAX_ENTRIES * 3 / 4;
    let mut order: Vec<(u64, Vec<u8>)> = inner
        .entries
        .iter()
        .map(|(key, entry)| (entry.last_used, key.clone()))
        .collect();
    order.sort_unstable_by_key(|(last_used, _)| *last_used);

    let before = inner.entries.len();
    for (_, key) in order {
        if inner.total_bytes <= target_bytes && inner.entries.len() <= target_entries {
            break;
        }
        if let Some(entry) = inner.entries.remove(&key) {
            inner.total_bytes = inner
                .total_bytes
                .saturating_sub(entry_size(&key, &entry.value));
        }
    }

    log::debug!(
        "ANGLE blob cache pruned {} entries; now {} entries, {} bytes",
        before.saturating_sub(inner.entries.len()),
        inner.entries.len(),
        inner.total_bytes
    );
}

fn flush_locked(inner: &mut CacheInner, force: bool) {
    if !inner.identity_ready || !inner.dirty {
        return;
    }
    if !force && inner.last_flush.elapsed() < FLUSH_INTERVAL {
        return;
    }

    prune_entries(inner);
    let bytes = encode(&inner.identity, &inner.entries);

    match atomic_write(&inner.file, &bytes) {
        Ok(()) => {
            inner.dirty = false;
            inner.flush_failures = 0;
            inner.last_flush = Instant::now();
            log::debug!(
                "ANGLE blob cache flushed {} entries ({} bytes) to {}",
                inner.entries.len(),
                bytes.len(),
                inner.file.display()
            );
        }
        Err(error) => {
            inner.flush_failures = inner.flush_failures.saturating_add(1);
            inner.last_flush = Instant::now();
            if inner.flush_failures <= 3 || inner.flush_failures % 64 == 0 {
                log::warn!(
                    "ANGLE blob cache: failed to write {} ({} failures): {error}",
                    inner.file.display(),
                    inner.flush_failures
                );
            }
        }
    }
}

fn encode(identity: &[String], entries: &BTreeMap<Vec<u8>, BlobEntry>) -> Vec<u8> {
    // Reserve the header and then serialize the payload directly into the same
    // buffer. This avoids keeping a second full copy of the cache in memory.
    let mut output: Vec<u8> = Vec::new();
    output.resize(HEADER_LEN, 0);

    output.extend_from_slice(&(identity.len() as u32).to_le_bytes());
    for part in identity {
        let bytes = part.as_bytes();
        output.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        output.extend_from_slice(bytes);
    }

    output.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (key, entry) in entries {
        output.extend_from_slice(&(key.len() as u32).to_le_bytes());
        output.extend_from_slice(key);
        output.extend_from_slice(&(entry.value.len() as u32).to_le_bytes());
        output.extend_from_slice(&entry.value);
    }

    let payload_len = output.len() - HEADER_LEN;
    output[0..8].copy_from_slice(MAGIC.as_slice());
    output[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    output[12..16].copy_from_slice(&0u32.to_le_bytes()); // flags
    output[16..20].copy_from_slice(&(payload_len as u32).to_le_bytes());
    output[20..24].copy_from_slice(&0u32.to_le_bytes()); // reserved
    let checksum = fnv1a64(&output[HEADER_LEN..]);
    output[24..32].copy_from_slice(&checksum.to_le_bytes());

    output
}

fn decode(bytes: &[u8]) -> Result<LoadedCache, String> {
    if bytes.len() < HEADER_LEN {
        return Err(format!("file is too short ({} bytes)", bytes.len()));
    }
    if !bytes.starts_with(MAGIC.as_slice()) {
        return Err(String::from("magic mismatch"));
    }

    let version = read_u32(bytes, 8).ok_or_else(|| String::from("truncated version"))?;
    if version != FORMAT_VERSION {
        return Err(format!(
            "format version {version} does not match {FORMAT_VERSION}"
        ));
    }

    let flags = read_u32(bytes, 12).ok_or_else(|| String::from("truncated flags"))?;
    if flags != 0 {
        return Err(format!("unsupported header flags 0x{flags:x}"));
    }

    let payload_len =
        read_u32(bytes, 16).ok_or_else(|| String::from("truncated payload length"))? as usize;
    let reserved = read_u32(bytes, 20).ok_or_else(|| String::from("truncated reserved field"))?;
    if reserved != 0 {
        return Err(String::from("nonzero reserved header field"));
    }
    if payload_len != bytes.len() - HEADER_LEN {
        return Err(format!(
            "payload length {payload_len} does not match file size {}",
            bytes.len() - HEADER_LEN
        ));
    }

    let checksum = read_u64(bytes, 24).ok_or_else(|| String::from("truncated checksum"))?;
    let payload = &bytes[HEADER_LEN..];
    if fnv1a64(payload) != checksum {
        return Err(String::from("checksum mismatch"));
    }

    let mut parser = Parser::new(payload);

    let identity_count = parser.read_u32()?;
    if identity_count > MAX_IDENTITY_PARTS {
        return Err(format!("too many identity parts: {identity_count}"));
    }
    let mut identity = Vec::with_capacity(identity_count as usize);
    let mut identity_bytes = 0usize;
    for _ in 0..identity_count {
        let len = parser.read_u32()? as usize;
        if len > MAX_IDENTITY_BYTES {
            return Err(format!("identity part is too large: {len} bytes"));
        }
        identity_bytes = identity_bytes.saturating_add(len);
        if identity_bytes > MAX_IDENTITY_BYTES {
            return Err(String::from("identity is too large"));
        }
        let raw = parser.read_bytes(len)?;
        let part = String::from_utf8(raw.to_vec())
            .map_err(|_| String::from("identity is not valid UTF-8"))?;
        identity.push(part);
    }

    let entry_count = parser.read_u32()? as usize;
    if entry_count > MAX_ENTRIES {
        return Err(format!("too many entries: {entry_count}"));
    }

    let mut entries = BTreeMap::new();
    let mut total_bytes = 0usize;
    for _ in 0..entry_count {
        let key_len = parser.read_u32()? as usize;
        if key_len == 0 || key_len > MAX_KEY_BYTES {
            return Err(format!("invalid key length: {key_len}"));
        }
        let key = parser.read_bytes(key_len)?.to_vec();

        let value_len = parser.read_u32()? as usize;
        if value_len == 0 || value_len > MAX_BLOB_BYTES {
            return Err(format!("invalid value length: {value_len}"));
        }
        let value = parser.read_bytes(value_len)?.to_vec();

        total_bytes = total_bytes.saturating_add(entry_size(&key, &value));
        if total_bytes > MAX_CACHE_BYTES {
            return Err(String::from("cache payload is too large"));
        }
        if entries
            .insert(
                key,
                BlobEntry {
                    value,
                    last_used: 0,
                },
            )
            .is_some()
        {
            return Err(String::from("duplicate key"));
        }
    }

    if !parser.is_empty() {
        return Err(format!(
            "trailing bytes after records: {}",
            parser.remaining()
        ));
    }

    for (index, entry) in entries.values_mut().enumerate() {
        entry.last_used = index as u64;
    }

    Ok(LoadedCache { identity, entries })
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset.checked_add(4)?)?;
    let mut raw = [0u8; 4];
    raw.copy_from_slice(slice);
    Some(u32::from_le_bytes(raw))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let slice = bytes.get(offset..offset.checked_add(8)?)?;
    let mut raw = [0u8; 8];
    raw.copy_from_slice(slice);
    Some(u64::from_le_bytes(raw))
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Parser<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let raw = self.read_bytes(4)?;
        let mut slice = [0u8; 4];
        slice.copy_from_slice(raw);
        Ok(u32::from_le_bytes(slice))
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .position
            .checked_add(len)
            .ok_or_else(|| String::from("record length overflow"))?;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| String::from("truncated record"))?;
        self.position = end;
        Ok(slice)
    }

    fn is_empty(&self) -> bool {
        self.position == self.bytes.len()
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }
}

fn entry_size(key: &[u8], value: &[u8]) -> usize {
    key.len().saturating_add(value.len()).saturating_add(8)
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
'''

MODULE_DECL_ANCHOR = (
    "use parking_lot::{MappedMutexGuard, Mutex, MutexGuard, RwLock};\n"
)
MODULE_DECL = (
    MODULE_DECL_ANCHOR
    + "\n"
    + "#[cfg(windows)]\n"
    + '#[path = "angle_blob_cache.rs"]\n'
    + "mod angle_blob_cache;\n"
)

INSTALL_ANCHOR = '''        log::debug!(
            "Display extensions: {:#?}",
            display_extensions.split_whitespace().collect::<Vec<_>>()
        );

        let srgb_kind = if version >= (1, 5) {
'''
INSTALL_REPLACEMENT = '''        log::debug!(
            "Display extensions: {:#?}",
            display_extensions.split_whitespace().collect::<Vec<_>>()
        );

        // Install the persistent blob cache after eglInitialize and before
        // any context or pipeline exists. ANGLE calls these callbacks during
        // shader compilation, so this must happen here rather than later.
        #[cfg(windows)]
        angle_blob_cache::install(&egl, display, &display_extensions, version);

        let srgb_kind = if version >= (1, 5) {
'''

IDENTITY_ANCHOR = '''        let gl = ManuallyDrop::new(gl);
        inner.egl.unmake_current();
'''
IDENTITY_REPLACEMENT = '''        // Complete the cache identity with the actual GPU/driver strings.
        // The context is current here and no pipeline has been created yet.
        #[cfg(windows)]
        {
            let vendor = unsafe { gl.get_parameter_string(glow::VENDOR) };
            let renderer = unsafe { gl.get_parameter_string(glow::RENDERER) };
            let version = unsafe { gl.get_parameter_string(glow::VERSION) };
            let sl_version =
                unsafe { gl.get_parameter_string(glow::SHADING_LANGUAGE_VERSION) };
            angle_blob_cache::update_identity(&[
                vendor.as_str(),
                renderer.as_str(),
                version.as_str(),
                sl_version.as_str(),
            ]);
        }

        let gl = ManuallyDrop::new(gl);
        inner.egl.unmake_current();
'''

FLUSH_ANCHOR = '''impl Drop for Inner {
    fn drop(&mut self) {
'''
FLUSH_REPLACEMENT = '''impl Drop for Inner {
    fn drop(&mut self) {
        // Persist the final snapshot on graceful shutdown. Writes use a
        // temporary file and atomic rename, so a crash keeps the old snapshot.
        #[cfg(windows)]
        angle_blob_cache::flush();

'''


def already_fully_patched(text: str) -> bool:
    # Marker-based detection keeps re-runs idempotent if the checkout is later
    # reformatted. The install marker includes the exact call signature, so a
    # stale or hand-edited variant is still reported as a partial patch.
    return all(marker in text for marker in MARKERS)


def helper_matches() -> bool:
    try:
        return helper_path.read_text(encoding="utf-8") == RUST_SOURCE
    except OSError:
        return False


if already_fully_patched(egl) and helper_matches():
    print("wgpu ANGLE blob cache is already patched; nothing to do")
    raise SystemExit(0)

# A partially applied or conflicting patch is not safe to guess about: a
# second eglSetBlobCacheFuncsANDROID insertion would not compile, and another
# cache implementation could fight this one for the same extension.
if "angle_blob_cache" in egl and not already_fully_patched(egl):
    raise SystemExit(
        "error: egl.rs already mentions angle_blob_cache but the patch state is "
        "incomplete; restore the wgpu checkout and re-run"
    )

if already_fully_patched(egl):
    # Only the generated helper changed (for example after an update of this
    # script). Keep the existing egl.rs anchors and refresh the helper.
    helper_path.parent.mkdir(parents=True, exist_ok=True)
    helper_path.write_bytes(RUST_SOURCE.encode("utf-8"))
    print(f"updated {helper_path}; egl.rs was already patched")
    raise SystemExit(0)


def replace_once(text: str, anchor: str, replacement: str, label: str) -> str:
    count = text.count(anchor)
    if count != 1:
        raise SystemExit(
            f"error: expected exactly one anchor for {label} in egl.rs, found {count}; "
            "apply scripts/patch-wgpu-angle.sh first and restore any local edits"
        )
    return text.replace(anchor, replacement, 1)


patched = replace_once(egl, MODULE_DECL_ANCHOR, MODULE_DECL, "module declaration")
patched = replace_once(patched, INSTALL_ANCHOR, INSTALL_REPLACEMENT, "display initialization")
patched = replace_once(patched, IDENTITY_ANCHOR, IDENTITY_REPLACEMENT, "adapter enumeration")
patched = replace_once(patched, FLUSH_ANCHOR, FLUSH_REPLACEMENT, "Inner::drop")

if not already_fully_patched(patched):
    raise SystemExit(
        "error: internal patch verification failed; egl.rs was not written"
    )

# Validate all anchors and build the result in memory first. Write the helper
# before egl.rs so a failed helper write cannot leave egl.rs referencing a
# missing module.
helper_path.parent.mkdir(parents=True, exist_ok=True)
helper_path.write_bytes(RUST_SOURCE.encode("utf-8"))
try:
    egl_path.write_bytes(patched.encode("utf-8"))
except OSError as error:
    raise SystemExit(f"error: cannot write {egl_path}: {error}")

print(f"patched {egl_path}")
print(f"wrote {helper_path}")
print("ANGLE blob cache installed; default cache directory is %LOCALAPPDATA%\\Cinnabar\\cache\\angle-blobs-v1")
PY
