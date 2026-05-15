//! Panic logging.
//!
//! Installs a process-wide panic hook that writes panic info (payload, location,
//! thread name, backtrace) to a platform-appropriate logs directory. The path
//! is chosen to match Electron's `app.getPath('logs')` so the file appears
//! where users (and OS tooling) expect:
//!
//! | OS      | Path                                              |
//! |---------|---------------------------------------------------|
//! | macOS   | `~/Library/Logs/Operator/`                        |
//! | Linux   | `~/.config/Operator/logs/`                        |
//! | Windows | `%APPDATA%\Operator\logs\`                        |
//!
//! Falls back to the system temp dir if the relevant env var is unset. Calls
//! the previously installed panic hook so stderr output is preserved.
//!
//! Idempotent: install_panic_hook may be called many times; only the first
//! installation takes effect.

use std::backtrace::Backtrace;
use std::fs::{File, OpenOptions, create_dir_all};
use std::io::Write;
use std::panic;
use std::path::PathBuf;
use std::sync::Once;
use std::time::{SystemTime, UNIX_EPOCH};

static INSTALL: Once = Once::new();

pub fn install_panic_hook() {
  INSTALL.call_once(|| {
    let prev = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
      let _ = write_panic_log(info);
      prev(info);
    }));
  });
}

pub fn panic_log_dir() -> PathBuf {
  #[cfg(target_os = "macos")]
  {
    if let Some(home) = std::env::var_os("HOME") {
      let mut p = PathBuf::from(home);
      p.push("Library/Logs/Operator");
      return p;
    }
  }
  #[cfg(target_os = "linux")]
  {
    if let Some(home) = std::env::var_os("HOME") {
      let mut p = PathBuf::from(home);
      p.push(".config/Operator/logs");
      return p;
    }
  }
  #[cfg(target_os = "windows")]
  {
    if let Some(appdata) = std::env::var_os("APPDATA") {
      let mut p = PathBuf::from(appdata);
      p.push("Operator");
      p.push("logs");
      return p;
    }
  }
  std::env::temp_dir().join("operator-panic-logs")
}

fn write_panic_log(info: &panic::PanicHookInfo<'_>) -> std::io::Result<()> {
  let dir = panic_log_dir();
  create_dir_all(&dir)?;

  let epoch = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_secs())
    .unwrap_or(0);
  let pid = std::process::id();
  let path = dir.join(format!("panic-{epoch}-{pid}.log"));

  let mut f: File = OpenOptions::new()
    .create(true)
    .append(true)
    .open(&path)?;

  let payload: &str = if let Some(s) = info.payload().downcast_ref::<&'static str>() {
    s
  } else if let Some(s) = info.payload().downcast_ref::<String>() {
    s.as_str()
  } else {
    "<non-string panic payload>"
  };

  let location = info
    .location()
    .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
    .unwrap_or_else(|| "<unknown>".to_string());

  let thread_handle = std::thread::current();
  let thread_name = thread_handle.name().unwrap_or("<unnamed>");

  let backtrace = Backtrace::force_capture();

  writeln!(f, "==== Operator panic ====")?;
  writeln!(f, "epoch: {epoch}")?;
  writeln!(f, "pid:   {pid}")?;
  writeln!(f, "thread: {thread_name}")?;
  writeln!(f, "location: {location}")?;
  writeln!(f, "payload: {payload}")?;
  writeln!(f, "backtrace:\n{backtrace}")?;
  writeln!(f, "========================\n")?;
  f.flush()
}
