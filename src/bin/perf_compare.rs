use std::ffi::{CStr, CString};
use std::hint::black_box;
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use libloading::Library;

#[cfg(windows)]
mod win_stdio {
    use core::ffi::c_void;
    use std::os::raw::c_char;

    const STD_OUTPUT_HANDLE: i32 = -11;
    const STD_ERROR_HANDLE: i32 = -12;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(nStdHandle: i32) -> *mut c_void;
        fn SetStdHandle(nStdHandle: i32, hHandle: *mut c_void) -> i32;
        fn CreateFileW(
            lpFileName: *const u16,
            dwDesiredAccess: u32,
            dwShareMode: u32,
            lpSecurityAttributes: *mut c_void,
            dwCreationDisposition: u32,
            dwFlagsAndAttributes: u32,
            hTemplateFile: *mut c_void,
        ) -> *mut c_void;
        fn CloseHandle(hObject: *mut c_void) -> i32;
    }

    pub struct MuteGuard {
        orig_out: *mut c_void,
        orig_err: *mut c_void,
        nul: *mut c_void,
    }

    fn wide_null() -> [u16; 8] {
        // "\\\\.\\NUL\0"
        [b'\\' as u16, b'\\' as u16, b'.' as u16, b'\\' as u16, b'N' as u16, b'U' as u16, b'L' as u16, 0]
    }

    pub unsafe fn mute() -> Option<MuteGuard> {
        let orig_out = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        let orig_err = unsafe { GetStdHandle(STD_ERROR_HANDLE) };
        let path = wide_null();
        let nul = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                core::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                core::ptr::null_mut(),
            )
        };
        if nul.is_null() {
            return None;
        }
        unsafe {
            let _ = SetStdHandle(STD_OUTPUT_HANDLE, nul);
            let _ = SetStdHandle(STD_ERROR_HANDLE, nul);
        }
        Some(MuteGuard { orig_out, orig_err, nul })
    }

    pub unsafe fn unmute(g: MuteGuard) {
        unsafe {
            let _ = SetStdHandle(STD_OUTPUT_HANDLE, g.orig_out);
            let _ = SetStdHandle(STD_ERROR_HANDLE, g.orig_err);
            let _ = CloseHandle(g.nul);
        }
    }

    // quiet unused warning if needed on non-windows builds
    #[allow(dead_code)]
    fn _keep(_p: *const c_char) {}
}

type BtOidFromStr = unsafe extern "C" fn(*const c_char, *mut u8) -> c_int;
type BtMdToHtml = unsafe extern "C" fn(*const c_char, *mut *mut c_char) -> c_int;
type BtReleaseMdToHtml = unsafe extern "C" fn(*mut *mut c_char);
type BtGetHead = unsafe extern "C" fn(*const c_char, *mut BtHead) -> c_int;
type BtReleaseHead = unsafe extern "C" fn(*mut BtHead);
type Crc32 = unsafe extern "C" fn(u32, *const u8, u32) -> u32;
type BtGetReferences = unsafe extern "C" fn(*const c_char, u8, *mut BtReferences) -> c_int;
type BtReleaseReferences = unsafe extern "C" fn(*mut BtReferences);
type BtGetGitConfig = unsafe extern "C" fn(*const c_char, *mut BtGitConfig) -> c_int;
type BtReleaseGitConfig = unsafe extern "C" fn(*mut BtGitConfig);

#[repr(C)]
#[derive(Clone, Copy)]
struct BtHead {
    oid20: [u8; 20],
    _pad: [u8; 4],
    ref_name: *mut c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BtBuf {
    ptr: *mut core::ffi::c_void,
    len: usize,
    cap: usize,
}

#[repr(C)]
struct BtReferences {
    a: BtBuf,
    b: BtBuf,
    c: BtBuf,
    d: BtBuf,
    e: BtBuf,
    hash: u64,
}

#[repr(C)]
struct BtReferencesOut {
    base: BtReferences,
    // The original DLL appears to write additional fields beyond the 5 BtBufs.
    // Keep extra space so benchmarks don't corrupt the stack.
    _pad: [u8; 256],
}

#[repr(C)]
struct BtGitConfig {
    ptr: *mut BtGitConfigEntry,
    len: usize,
    cap: usize,
}

#[repr(C)]
struct BtGitConfigOut {
    base: BtGitConfig,
    // Similar to BtReferences, keep some slack for unknown trailing fields.
    _pad: [u8; 128],
}

#[repr(C)]
struct BtGitConfigEntry {
    a: *mut c_char,
    b: *mut c_char,
    kv_ptr: *mut BtGitConfigKv,
    kv_len: usize,
    kv_cap: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BtGitConfigKv {
    k: *mut c_char,
    v: *mut c_char,
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn original_dll() -> PathBuf {
    manifest_dir().join("biturbo.dll")
}

fn our_dll() -> PathBuf {
    manifest_dir().join("target").join("release").join("biturbo.dll")
}

fn fmt_ns_per_op(d: Duration, iters: u64) -> f64 {
    (d.as_secs_f64() * 1e9) / (iters as f64)
}

fn bench_run(iters: u64, mut f: impl FnMut()) -> Duration {
    for _ in 0..(iters / 10).max(1000).min(50_000) {
        f();
    }
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    start.elapsed()
}

fn bench_muted(name: &str, iters: u64, mut f: impl FnMut()) -> Duration {
    #[cfg(windows)]
    unsafe {
        if let Some(g) = win_stdio::mute() {
            let d = bench_run(iters, &mut f);
            win_stdio::unmute(g);
            println!("{name}: {:.1} ns/op (iters={iters})", fmt_ns_per_op(d, iters));
            return d;
        }
    }
    let d = bench_run(iters, f);
    println!("{name}: {:.1} ns/op (iters={iters})", fmt_ns_per_op(d, iters));
    d
}

fn make_fake_git_dir_ref() -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "biturbo_perf_repo_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let git_dir = base.join(".git");
    std::fs::create_dir_all(git_dir.join("refs").join("heads")).unwrap();
    std::fs::write(git_dir.join("HEAD"), b"ref: refs/heads/main\n").unwrap();
    std::fs::write(
        git_dir.join("refs").join("heads").join("main"),
        b"decbf2be529ab6557d5429922251e5ee36519817\n",
    )
    .unwrap();
    git_dir
}

fn pick_real_git_dir() -> Option<PathBuf> {
    let base = PathBuf::from(r"C:\git");
    for name in ["git-fork", "orm-cpp", "testBiturbo"] {
        let git_dir = base.join(name).join(".git");
        if git_dir.exists() {
            return Some(git_dir);
        }
    }
    None
}

fn main() {
    let iters_fast: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(500_000);
    let iters_md: u64 = 50_000;
    let iters_head: u64 = 50_000;
    let iters_git: u64 = 200;

    println!("original: {}", original_dll().display());
    println!("ours    : {}", our_dll().display());

    let orig = unsafe { Library::new(original_dll()).expect("load original dll") };
    let ours = unsafe { Library::new(our_dll()).expect("load our dll") };

    unsafe {
        // ---- zlib crc32 ----
        let crc1: libloading::Symbol<Crc32> = orig.get(b"crc32\0").unwrap();
        let crc2: libloading::Symbol<Crc32> = ours.get(b"crc32\0").unwrap();
        let data = vec![0xA5u8; 4096];

        println!("\n== crc32(4KiB) ==");
        {
            let d = bench_run(iters_fast, || {
                let v = crc1(0, data.as_ptr(), data.len() as u32);
                black_box(v);
            });
            println!("orig: {:.1} ns/op (iters={iters_fast})", fmt_ns_per_op(d, iters_fast));
        }
        {
            let d = bench_run(iters_fast, || {
                let v = crc2(0, data.as_ptr(), data.len() as u32);
                black_box(v);
            });
            println!("ours: {:.1} ns/op (iters={iters_fast})", fmt_ns_per_op(d, iters_fast));
        }

        // ---- bt_oid_from_str ----
        let oid1: libloading::Symbol<BtOidFromStr> = orig.get(b"bt_oid_from_str\0").unwrap();
        let oid2: libloading::Symbol<BtOidFromStr> = ours.get(b"bt_oid_from_str\0").unwrap();
        let sha = CString::new("decbf2be529ab6557d5429922251e5ee36519817").unwrap();

        println!("\n== bt_oid_from_str(success) ==");
        bench_muted("orig", iters_fast, || {
            let mut out = [0u8; 20];
            let r = oid1(sha.as_ptr(), out.as_mut_ptr());
            black_box((r, out[0]));
        });
        bench_muted("ours", iters_fast, || {
            let mut out = [0u8; 20];
            let r = oid2(sha.as_ptr(), out.as_mut_ptr());
            black_box((r, out[0]));
        });

        // ---- bt_md_to_html ----
        let md1: libloading::Symbol<BtMdToHtml> = orig.get(b"bt_md_to_html\0").unwrap();
        let md2: libloading::Symbol<BtMdToHtml> = ours.get(b"bt_md_to_html\0").unwrap();
        let rel1: libloading::Symbol<BtReleaseMdToHtml> = orig.get(b"bt_release_md_to_html\0").unwrap();
        let rel2: libloading::Symbol<BtReleaseMdToHtml> = ours.get(b"bt_release_md_to_html\0").unwrap();
        let md = CString::new("Hello\n\n**world**\n\n- a\n- b\n- c").unwrap();

        println!("\n== bt_md_to_html(small) ==");
        {
            let d = bench_run(iters_md, || {
                let mut p: *mut c_char = std::ptr::null_mut();
                let r = md1(md.as_ptr(), &mut p);
                black_box(r);
                if !p.is_null() {
                    black_box(CStr::from_ptr(p).to_bytes().len());
                    rel1(&mut p);
                }
            });
            println!("orig: {:.1} ns/op (iters={iters_md})", fmt_ns_per_op(d, iters_md));
        }
        {
            let d = bench_run(iters_md, || {
                let mut p: *mut c_char = std::ptr::null_mut();
                let r = md2(md.as_ptr(), &mut p);
                black_box(r);
                if !p.is_null() {
                    black_box(CStr::from_ptr(p).to_bytes().len());
                    rel2(&mut p);
                }
            });
            println!("ours: {:.1} ns/op (iters={iters_md})", fmt_ns_per_op(d, iters_md));
        }

        // ---- bt_get_head ----
        let head1: libloading::Symbol<BtGetHead> = orig.get(b"bt_get_head\0").unwrap();
        let head2: libloading::Symbol<BtGetHead> = ours.get(b"bt_get_head\0").unwrap();
        let head_rel1: libloading::Symbol<BtReleaseHead> = orig.get(b"bt_release_head\0").unwrap();
        let head_rel2: libloading::Symbol<BtReleaseHead> = ours.get(b"bt_release_head\0").unwrap();
        let git_dir = make_fake_git_dir_ref();
        let git_dir_c = CString::new(git_dir.to_string_lossy().to_string()).unwrap();

        println!("\n== bt_get_head(ref) ==");
        {
            let d = bench_run(iters_head, || {
                let mut h = BtHead { oid20: [0u8; 20], _pad: [0u8; 4], ref_name: std::ptr::null_mut() };
                let r = head1(git_dir_c.as_ptr(), &mut h);
                black_box((r, h.oid20[0]));
                head_rel1(&mut h);
            });
            println!("orig: {:.1} ns/op (iters={iters_head})", fmt_ns_per_op(d, iters_head));
        }
        {
            let d = bench_run(iters_head, || {
                let mut h = BtHead { oid20: [0u8; 20], _pad: [0u8; 4], ref_name: std::ptr::null_mut() };
                let r = head2(git_dir_c.as_ptr(), &mut h);
                black_box((r, h.oid20[0]));
                head_rel2(&mut h);
            });
            println!("ours: {:.1} ns/op (iters={iters_head})", fmt_ns_per_op(d, iters_head));
        }

        // ---- git-heavy: references + git config (use a real repo under C:\git if available) ----
        if let Some(real_git_dir) = pick_real_git_dir() {
            let real_git_dir_c = CString::new(real_git_dir.to_string_lossy().to_string()).unwrap();

            println!("\n== bt_get_references(real repo) ==");
            let r1: libloading::Symbol<BtGetReferences> = orig.get(b"bt_get_references\0").unwrap();
            let r2: libloading::Symbol<BtGetReferences> = ours.get(b"bt_get_references\0").unwrap();
            let rr1: libloading::Symbol<BtReleaseReferences> = orig.get(b"bt_release_references\0").unwrap();
            let rr2: libloading::Symbol<BtReleaseReferences> = ours.get(b"bt_release_references\0").unwrap();
            let include_tags = 1u8;
            {
                let d = bench_run(iters_git, || {
                    let mut out = BtReferencesOut {
                        base: BtReferences {
                            a: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                            b: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                            c: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                            d: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                            e: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                            hash: 0,
                        },
                        _pad: [0u8; 256],
                    };
                    let rc = r1(real_git_dir_c.as_ptr(), include_tags, &mut out.base);
                    black_box((rc, out.base.a.len));
                    rr1(&mut out.base);
                });
                println!("orig: {:.1} ms/op (iters={iters_git})", (d.as_secs_f64() * 1e3) / (iters_git as f64));
            }
            {
                let d = bench_run(iters_git, || {
                    let mut out = BtReferencesOut {
                        base: BtReferences {
                            a: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                            b: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                            c: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                            d: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                            e: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                            hash: 0,
                        },
                        _pad: [0u8; 256],
                    };
                    let rc = r2(real_git_dir_c.as_ptr(), include_tags, &mut out.base);
                    black_box((rc, out.base.a.len));
                    rr2(&mut out.base);
                });
                println!("ours: {:.1} ms/op (iters={iters_git})", (d.as_secs_f64() * 1e3) / (iters_git as f64));
            }

            println!("\n== bt_get_git_config(real repo) ==");
            let c1: libloading::Symbol<BtGetGitConfig> = orig.get(b"bt_get_git_config\0").unwrap();
            let c2: libloading::Symbol<BtGetGitConfig> = ours.get(b"bt_get_git_config\0").unwrap();
            let cr1: libloading::Symbol<BtReleaseGitConfig> = orig.get(b"bt_release_git_config\0").unwrap();
            let cr2: libloading::Symbol<BtReleaseGitConfig> = ours.get(b"bt_release_git_config\0").unwrap();
            {
                let d = bench_run(iters_git, || {
                    let mut out = BtGitConfigOut {
                        base: BtGitConfig { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                        _pad: [0u8; 128],
                    };
                    let rc = c1(real_git_dir_c.as_ptr(), &mut out.base);
                    black_box((rc, out.base.len));
                    cr1(&mut out.base);
                });
                println!("orig: {:.1} ms/op (iters={iters_git})", (d.as_secs_f64() * 1e3) / (iters_git as f64));
            }
            {
                let d = bench_run(iters_git, || {
                    let mut out = BtGitConfigOut {
                        base: BtGitConfig { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                        _pad: [0u8; 128],
                    };
                    let rc = c2(real_git_dir_c.as_ptr(), &mut out.base);
                    black_box((rc, out.base.len));
                    cr2(&mut out.base);
                });
                println!("ours: {:.1} ms/op (iters={iters_git})", (d.as_secs_f64() * 1e3) / (iters_git as f64));
            }
        } else {
            println!("\n(no real repo found under C:\\git; skipping git-heavy benchmarks)");
        }
    }
}

