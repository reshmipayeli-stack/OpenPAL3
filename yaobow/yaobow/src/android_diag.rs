//! Diagnostics for the Android build.
//!
//! Some Android containers (for example the one used by HarmonyOS
//! compatibility apps) do not expose logcat. This module mirrors every log
//! line and any panic to:
//!
//! - a TCP connection to 127.0.0.1:47321, which a developer machine can
//!   receive through `hdc rport tcp:47321 tcp:47321` / `adb reverse`, and
//! - a log file in the app's own storage.
//!
//! Both sinks are best-effort: if neither is reachable the game runs as
//! usual and logs only go to logcat.

use log::Log;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const DIAG_PORT: u16 = 47321;

static TCP: Mutex<Option<TcpStream>> = Mutex::new(None);
static FILE: Mutex<Option<File>> = Mutex::new(None);
/// Broadcast every log line on the LAN (UDP 47322) so a PC on the same
/// Wi-Fi can read it even when the container's storage and loopback are
/// not reachable from outside.
static UDP: Mutex<Option<UdpSocket>> = Mutex::new(None);
const UDP_PORT: u16 = 47322;

fn emit(line: &str) {
    if let Ok(guard) = UDP.lock() {
        if let Some(sock) = guard.as_ref() {
            for chunk in line.as_bytes().chunks(1200) {
                let _ = sock.send_to(chunk, ("255.255.255.255", UDP_PORT));
            }
        }
    }
    if let Ok(mut guard) = TCP.lock() {
        if let Some(stream) = guard.as_mut() {
            let ok = stream
                .write_all(line.as_bytes())
                .and_then(|_| stream.write_all(b"\n"))
                .is_ok();
            if !ok {
                *guard = None;
            }
        }
    }
    if let Ok(mut guard) = FILE.lock() {
        if let Some(file) = guard.as_mut() {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }
}

struct TeeLogger {
    android: android_logger::AndroidLogger,
}

impl log::Log for TeeLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        self.android.log(record);
        emit(&format!(
            "[{}] {}: {}",
            record.level(),
            record.target(),
            record.args()
        ));
    }

    fn flush(&self) {}
}

/// Install the logger and panic hook. Call first thing in `android_main`.
pub fn init(log_dir: Option<PathBuf>) {
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.set_broadcast(true).is_ok() {
            *UDP.lock().unwrap() = Some(sock);
        }
    }
    // Try several candidate paths: the HarmonyOS Android container maps
    // shared storage somewhere unexpected, so cover the common Android
    // locations as well as app-private storage. Write to ALL of them;
    // whichever one is visible from the host wins.
    for candidate in [
        PathBuf::from("/sdcard/Download"),
        PathBuf::from("/storage/emulated/0/Download"),
        PathBuf::from("/mnt/sdcard/Download"),
        PathBuf::from("/storage/media/100/local/files/Docs/Download"),
    ] {
        init_file(&candidate.join("openpal3-debug.log"));
    }

    let addr = SocketAddr::from(([127, 0, 0, 1], DIAG_PORT));
    if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(800)) {
        let _ = stream.set_nodelay(true);
        *TCP.lock().unwrap() = Some(stream);
    }

    if let Some(dir) = log_dir {
        init_file(&dir.join("openpal3.log"));
    }

    let logger = TeeLogger {
        android: android_logger::AndroidLogger::new(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("yaobow"),
        ),
    };
    if log::set_logger(Box::leak(Box::new(logger))).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }

    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        emit(&format!("[PANIC] {info}\n{bt}"));
        log::error!("panic: {info}");
    }));

    emit("===== OpenPAL3 android diagnostics start =====");
}

fn list_dir(path: &Path) {
    match std::fs::read_dir(path) {
        Ok(entries) => {
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| {
                    let dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    format!("{}{}", e.file_name().to_string_lossy(), if dir { "/" } else { "" })
                })
                .collect();
            names.sort();
            names.truncate(60);
            log::info!("ls {} -> {:?}", path.display(), names);
        }
        Err(e) => log::info!("ls {} -> error: {e}", path.display()),
    }
}

/// Log where things are inside the container, to find out where shared
/// files land and whether the GPU stack is present.
pub fn report_environment(app_dir: &Path) {
    log::info!("app storage dir: {}", app_dir.display());
    for p in [
        "/sdcard",
        "/sdcard/Download",
        "/sdcard/Documents",
        "/sdcard/Games",
        "/storage/emulated/0",
        "/storage",
    ] {
        list_dir(Path::new(p));
    }
    list_dir(app_dir);
    for lib in [
        "/system/lib64/libvulkan.so",
        "/vendor/lib64/libvulkan.so",
        "/system/lib64/libEGL.so",
    ] {
        log::info!("{lib} exists: {}", Path::new(lib).exists());
    }
    for dir in ["/vendor/lib64/hw", "/system/lib64/hw"] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            let vk: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.contains("vulkan"))
                .collect();
            log::info!("vulkan drivers in {dir}: {vk:?}");
        }
    }
}

/// Open (or replace) the active log file.
fn init_file(path: &Path) {
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(path) {
        *FILE.lock().unwrap() = Some(file);
        emit(&format!("===== log opened: {} =====", path.display()));
    }
}
