//! Earliest possible diagnostics for the Android build.
//!
//! HarmonyOS Android containers hide their guest filesystem and logcat from
//! the host, so the only reliable evidence that the native library loaded at
//! all is something the library itself emits. This probe runs from
//! `.init_array`, i.e. during `dlopen`, before `android_main` and before the
//! Rust runtime starts, and therefore only uses raw libc calls.
//!
//! The libc functions come from the `libc` crate rather than from our own
//! extern block: rustc validates the signature of a handful of "runtime"
//! symbols (`open`, `close`, `write`, ...) that the standard library also
//! uses, and rejects a hand written declaration for them.

use std::ffi::CString;

/// Write `msg` to every candidate log path, ignoring all errors.
fn append_everywhere(msg: &str) {
    for path in [
        "/sdcard/Download/openpal3-early.txt",
        "/storage/emulated/0/Download/openpal3-early.txt",
        "/mnt/sdcard/Download/openpal3-early.txt",
        "/storage/media/100/local/files/Docs/Download/openpal3-early.txt",
        "/data/local/tmp/openpal3-early.txt",
    ] {
        let Ok(cpath) = CString::new(path) else {
            continue;
        };
        let fd = unsafe {
            libc::open(
                cpath.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND,
                0o666 as libc::c_int,
            )
        };
        if fd >= 0 {
            unsafe {
                libc::write(fd, msg.as_ptr() as *const libc::c_void, msg.len());
                libc::close(fd);
            }
        }
    }
}

/// Best effort IPv4 TCP connect that writes one line and closes.
fn tcp_probe(msg: &str, ip: [u8; 4], port: u16) {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return;
    }
    let mut addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    addr.sin_family = libc::AF_INET as libc::sa_family_t;
    addr.sin_port = port.to_be();
    addr.sin_addr.s_addr = u32::from_ne_bytes(ip);
    let connected = unsafe {
        libc::connect(
            fd,
            &addr as *const libc::sockaddr_in as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        )
    } == 0;
    if connected {
        unsafe {
            libc::write(fd, msg.as_ptr() as *const libc::c_void, msg.len());
            libc::write(fd, b"\n".as_ptr() as *const libc::c_void, 1);
        }
    }
    unsafe { libc::close(fd) };
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
