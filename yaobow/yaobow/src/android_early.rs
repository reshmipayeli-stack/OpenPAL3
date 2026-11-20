//! Earliest possible diagnostics for the Android build.
//!
//! HarmonyOS Android containers hide their guest filesystem and logcat from
//! the host, so the only reliable evidence that the native library loaded at
//! all is something the library itself emits. This probe runs from
//! `.init_array`, i.e. during `dlopen`, before `android_main` and before the
//! Rust runtime starts, and therefore only uses raw libc calls.
//!
//! The libc functions are imported under private Rust names via
//! `#[link_name]`: declaring them as `fn open` / `fn close` is rejected by
//! rustc because those identifiers are reserved for the standard library's
//! own runtime symbols.

use std::ffi::CString;
use std::os::raw::{c_char, c_int};

unsafe extern "C" {
    #[link_name = "open"]
    fn sys_open(path: *const c_char, flags: c_int, mode: c_int) -> c_int;
    #[link_name = "write"]
    fn sys_write(fd: c_int, buf: *const u8, count: usize) -> isize;
    #[link_name = "close"]
    fn sys_close(fd: c_int);
    #[link_name = "socket"]
    fn sys_socket(domain: c_int, ty: c_int, protocol: c_int) -> c_int;
    #[link_name = "connect"]
    fn sys_connect(fd: c_int, addr: *const u8, len: u32) -> c_int;
}

const O_WRONLY: c_int = 1;
const O_CREAT: c_int = 64;
const O_APPEND: c_int = 1024;

/// Write `msg` to every candidate log path, ignoring all errors.
fn append_everywhere(msg: &str) {
    for path in [
        "/sdcard/Download/openpal3-early.txt",
        "/storage/emulated/0/Download/openpal3-early.txt",
        "/mnt/sdcard/Download/openpal3-early.txt",
        "/storage/media/100/local/files/Docs/Download/openpal3-early.txt",
        "/data/local/tmp/openpal3-early.txt",
    ] {
        if let Ok(cpath) = CString::new(path) {
            let fd = unsafe { sys_open(cpath.as_ptr(), O_WRONLY | O_CREAT | O_APPEND, 0o666) };
            if fd >= 0 {
                unsafe {
                    sys_write(fd, msg.as_ptr(), msg.len());
                    sys_close(fd);
                }
            }
        }
    }
}

/// Best effort IPv4 TCP connect that writes one line and closes.
fn tcp_probe(msg: &str, ip: [u8; 4], port: u16) {
    let fd = unsafe { sys_socket(2 /* AF_INET */, 1 /* SOCK_STREAM */, 0) };
    if fd < 0 {
        return;
    }
    let mut addr = [0u8; 16];
    addr[0] = 2; // sin_family = AF_INET, little endian low byte
    addr[2] = (port >> 8) as u8;
    addr[3] = (port & 0xff) as u8;
    addr[4..8].copy_from_slice(&ip);
    let ok = unsafe { sys_connect(fd, addr.as_ptr(), 16) } == 0;
    if ok {
        unsafe {
            sys_write(fd, msg.as_ptr(), msg.len());
            sys_write(fd, b"\n".as_ptr(), 1);
        }
    }
    unsafe { sys_close(fd) };
}

extern "C" fn early_probe() {
    let msg = "===== libyaobow_lib.so loaded (init_array probe) =====\n";
    append_everywhere(msg);
    let line = msg.trim_end();
    tcp_probe(line, [127, 0, 0, 1], 47321);
    tcp_probe(line, [192, 168, 1, 33], 47321);
}

#[used]
#[unsafe(link_section = ".init_array")]
static EARLY_PROBE: extern "C" fn() = early_probe;
