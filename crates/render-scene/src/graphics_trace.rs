//! Opt-in, line-flushed graphics tracing for native renderer investigations.
//!
//! Enable with `CINNABAR_GRAPHICS_TRACE=1`. Files are written beside the client logs:
//! `graphics-adapter.log`, `graphics-textures.log`, `graphics-pipelines.log`,
//! `graphics-chunks.log`, `graphics-frames.log` and `graphics-errors.log`.
//! Every line is flushed immediately so a crash or device loss still leaves the
//! last observed values.

use std::{
    env, fmt,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

/// A trace destination. Keeping channels separate makes the files small enough to
/// inspect individually while preserving one ordered stream per subsystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Adapter,
    Texture,
    Pipeline,
    Chunk,
    Frame,
    Error,
}

impl Channel {
    const ALL: [Self; 6] = [
        Self::Adapter,
        Self::Texture,
        Self::Pipeline,
        Self::Chunk,
        Self::Frame,
        Self::Error,
    ];

    const fn file_name(self) -> &'static str {
        match self {
            Self::Adapter => "graphics-adapter.log",
            Self::Texture => "graphics-textures.log",
            Self::Pipeline => "graphics-pipelines.log",
            Self::Chunk => "graphics-chunks.log",
            Self::Frame => "graphics-frames.log",
            Self::Error => "graphics-errors.log",
        }
    }
}

struct Sinks {
    files: [Option<File>; 6],
}

impl Sinks {
    fn empty() -> Self {
        Self {
            files: std::array::from_fn(|_| None),
        }
    }

    fn file(&mut self, channel: Channel) -> Option<&mut File> {
        self.files[channel as usize].as_mut()
    }
}

static SINKS: OnceLock<Mutex<Sinks>> = OnceLock::new();
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Enables tracing from the environment and opens one append-only file per channel.
///
/// This is a no-op unless `CINNABAR_GRAPHICS_TRACE` is set to a value other than
/// `0`, `false`, `off` or `no`.
pub fn install(log_dir: impl AsRef<Path>) -> std::io::Result<()> {
    if !configured() {
        return Ok(());
    }
    if ENABLED.load(Ordering::Acquire) {
        return Ok(());
    }
    let dir = env::var_os("CINNABAR_GRAPHICS_TRACE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| log_dir.as_ref().to_path_buf());
    if let Some(parent) = dir.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir_all(&dir)?;
    let mut sinks = Sinks::empty();
    for channel in Channel::ALL {
        let path = dir.join(channel.file_name());
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        sinks.files[channel as usize] = Some(file);
    }
    let header = format!(
        "graphics trace start epoch_ms={} trace_dir={}\n",
        epoch_millis(),
        dir.display()
    );
    for file in sinks.files.iter_mut().flatten() {
        let _ = file.write_all(header.as_bytes());
        let _ = file.flush();
    }
    let opened = sinks;
    let global = SINKS.get_or_init(|| Mutex::new(Sinks::empty()));
    *global
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = opened;
    ENABLED.store(true, Ordering::Release);
    Ok(())
}

/// Installs the tracer using the client log directory as the default destination.
pub fn install_from_env(log_dir: impl AsRef<Path>) -> std::io::Result<()> {
    install(log_dir)
}

#[must_use]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// Writes one already-formatted line and flushes it. Call through
/// [`graphics_trace_line!`](crate::graphics_trace_line) so disabled builds do not format.
pub fn write(channel: Channel, arguments: fmt::Arguments<'_>) {
    if !enabled() {
        return;
    }
    let Some(sinks) = SINKS.get() else {
        return;
    };
    let Ok(mut sinks) = sinks.lock() else {
        return;
    };
    let Some(file) = sinks.file(channel) else {
        return;
    };
    let _ = writeln!(file, "epoch_ms={} {}", epoch_millis(), arguments);
    let _ = file.flush();
}

fn configured() -> bool {
    env::var("CINNABAR_GRAPHICS_TRACE")
        .ok()
        .is_some_and(|value| {
            !value.is_empty()
                && !matches!(
                    value.to_ascii_lowercase().as_str(),
                    "0" | "false" | "off" | "no"
                )
        })
}

fn epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

/// Writes one graphics trace line without formatting it when tracing is disabled.
#[macro_export]
macro_rules! graphics_trace_line {
    ($channel:expr, $($arg:tt)*) => {{
        if $crate::graphics_trace::enabled() {
            $crate::graphics_trace::write($channel, format_args!($($arg)*));
        }
    }};
}
