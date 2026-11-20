//! Diagnostics for the Android build.
//!
//! Some Android containers (for example the one used by HarmonyOS
//! compatibility apps) do not expose logcat and hide the guest filesystem
//! from the host. This module therefore mirrors every log line and any panic
//! to as many independent sinks as possible:
//!
//! - raw TCP to a developer machine, reachable either through
//!   `hdc rport tcp:47321 tcp:47321` (loopback) or directly over the LAN,
//! - UDP broadcast on the LAN,
//! - log files in every plausible shared-storage location,
//! - logcat, when the container has one.
//!
//! All sinks are best effort and reconnect lazily, so a listener that is
//! started after the app keeps working.

use log::Log;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const DIAG_PORT: u16 = 47321;
const UDP_PORT: u16 = 47322;

/// Hosts to mirror the log to. The loopback entry works when the container
/// shares the host network namespace (or through `hdc rport`); the LAN entry
/// works through the container's NAT.
const TCP_TARGETS: [[u8; 4]; 2] = [[127, 0, 0, 1], [192, 168, 1, 33]];

struct TcpSink {
    target: [u8; 4],
    stream: Option<TcpStream>,
    last_attempt: Option<Instant>,
}

static TCP: Mutex<Vec<TcpSink>> = Mutex::new(Vec::new());
static FILE: Mutex<Vec<File>> = Mutex::new(Vec::new());
static UDP: Mutex<Option<UdpSocket>> = Mutex::new(None);

fn connect_target(target: [u8; 4]) -> Option<TcpStream> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(target[0], target[1], target[2], target[3])), DIAG_PORT);
    TcpStream::connect_timeout(&addr, Duration::from_millis(400))
        .map(|s| {
            let _ = s.set_nodelay(true);
            s
        })
        .ok()
}

fn emit(line: &str) {
    if let Ok(guard) = UDP.lock() {
        if let Some(sock) = guard.as_ref() {
            for chunk in line.as_bytes().chunks(1200) {
                let _ = sock.send_to(chunk, (Ipv4Addr::BROADCAST, UDP_PORT));
                let _ = sock.send_to(chunk, (Ipv4Addr::new(192, 168, 1, 33), UDP_PORT));
            }
        }
    }
    if let Ok(mut guard) = TCP.lock() {
        for sink in guard.iter_mut() {
            // Retry at most once a second so a listener started later still
            // receives everything from that point on.
            let retry = sink.stream.is_none()
                && sink
                    .last_attempt
                    .map(|t| t.elapsed() > Duration::from_secs(1))
                    .unwrap_or(true);
            if retry {
                sink.last_attempt = Some(Instant::now());
                sink.stream = connect_target(sink.target);
            }
            if let Some(stream) = sink.stream.as_mut() {
                let ok = stream
                    .write_all(line.as_bytes())
                    .and_then(|_| stream.write_all(b"\n"))
                    .is_ok();
                if !ok {
                    sink.stream = None;
                }
            }
        }
    }
    if let Ok(mut guard) = FILE.lock() {
        guard.retain_mut(|file| {
            let ok = writeln!(file, "{line}").and_then(|_| file.flush()).is_ok();
            ok
        });
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

/// Every place worth writing a log file to.
fn log_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for dir in [
        "/sdcard/Download",
        "/storage/emulated/0/Download",
        "/mnt/sdcard/Download",
        "/storage/media/100/local/files/Docs/Download",
        "/data/local/tmp",
    ] {
        paths.push(PathBuf::from(dir).join("openpal3-debug.log"));
    }
    paths
}

/// Install the logger and panic hook. Call first thing in `android_main`.
pub fn init(log_dir: Option<PathBuf>) {
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        if sock.set_broadcast(true).is_ok() {
            *UDP.lock().unwrap() = Some(sock);
        }
    }
    if let Ok(mut guard) = TCP.lock() {
        for target in TCP_TARGETS {
            guard.push(TcpSink {
                target,
                stream: connect_target(target),
                last_attempt: Some(Instant::now()),
            });
        }
    }
    for path in log_paths() {
        init_file(&path);
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
        if let Ok(mut guard) = FILE.lock() {
            guard.push(file);
        }
        emit(&format!("===== log opened: {} =====", path.display()));
    }
}
