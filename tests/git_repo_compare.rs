use libloading::Library;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;

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
struct BtGitConfig {
    ptr: *mut BtGitConfigEntry,
    len: usize,
    cap: usize,
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
struct BtGitConfigKv {
    k: *mut c_char,
    v: *mut c_char,
}

type BtGetLastErrorMessage = unsafe extern "C" fn(*mut u8, usize) -> isize;
type BtGetReferences = unsafe extern "C" fn(*const c_char, u8, *mut BtReferences) -> c_int;
type BtReleaseReferences = unsafe extern "C" fn(*mut BtReferences);
type BtGetGitConfig = unsafe extern "C" fn(*const c_char, *mut BtGitConfig) -> c_int;
type BtReleaseGitConfig = unsafe extern "C" fn(*mut BtGitConfig);

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn original_dll() -> PathBuf {
    manifest_dir().join("biturbo.dll")
}

fn our_release_dll() -> PathBuf {
    manifest_dir().join("target").join("release").join("biturbo.dll")
}

fn ensure_our_release_built() {
    let user = std::env::var("USERPROFILE").expect("USERPROFILE");
    let cargo = PathBuf::from(user).join(".cargo").join("bin").join("cargo.exe");
    let status = std::process::Command::new(cargo)
        .current_dir(manifest_dir())
        .args(["build", "--release"])
        .status()
        .expect("run cargo build --release");
    assert!(status.success(), "cargo build --release failed");
    assert!(our_release_dll().exists(), "release dll missing");
}

unsafe fn get_last_error(getter: BtGetLastErrorMessage) -> String {
    let mut buf = vec![0u8; 4096];
    let n = unsafe { getter(buf.as_mut_ptr(), buf.len()) };
    if n <= 0 {
        return String::new();
    }
    let n = n as usize;
    let end = buf[..n].iter().position(|&b| b == 0).unwrap_or(n);
    String::from_utf8_lossy(&buf[..end]).to_string()
}

unsafe fn btbuf_bytes(b: &BtBuf) -> Vec<u8> {
    if b.ptr.is_null() || b.len == 0 {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(b.ptr as *const u8, b.len) }.to_vec()
}

unsafe fn cstr_opt(p: *mut c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().to_string()
}

fn pick_git_dir() -> PathBuf {
    // Prefer the user's repos in C:\git
    for name in ["git-fork", "orm-cpp", "testBiturbo"] {
        let p = PathBuf::from(r"C:\git").join(name);
        let git_dir = p.join(".git");
        if git_dir.exists() {
            return git_dir;
        }
    }
    panic!("no repo found under C:\\git (expected git-fork/orm-cpp/testBiturbo)");
}

#[test]
fn compare_bt_get_references_on_repo() {
    ensure_our_release_built();
    let git_dir = pick_git_dir();
    let git_dir_c = CString::new(git_dir.to_string_lossy().to_string()).unwrap();

    let orig = Box::leak(Box::new(unsafe { Library::new(original_dll()).expect("load original dll") }));
    let ours = Box::leak(Box::new(unsafe { Library::new(our_release_dll()).expect("load our dll") }));
    unsafe {
        let f1: libloading::Symbol<BtGetReferences> = orig.get(b"bt_get_references\0").unwrap();
        let f2: libloading::Symbol<BtGetReferences> = ours.get(b"bt_get_references\0").unwrap();
        let rel1: libloading::Symbol<BtReleaseReferences> = orig.get(b"bt_release_references\0").unwrap();
        let rel2: libloading::Symbol<BtReleaseReferences> = ours.get(b"bt_release_references\0").unwrap();
        let g1: libloading::Symbol<BtGetLastErrorMessage> = orig.get(b"bt_get_last_error_message\0").unwrap();
        let g2: libloading::Symbol<BtGetLastErrorMessage> = ours.get(b"bt_get_last_error_message\0").unwrap();

        let mut r1 = BtReferences {
            a: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
            b: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
            c: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
            d: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
            e: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
            hash: 0,
        };
        let mut r2 = BtReferences {
            a: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
            b: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
            c: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
            d: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
            e: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
            hash: 0,
        };

        let include_tags = 1u8;
        let a = f1(git_dir_c.as_ptr(), include_tags, &mut r1);
        let ea = get_last_error(*g1);
        let b = f2(git_dir_c.as_ptr(), include_tags, &mut r2);
        let eb = get_last_error(*g2);
        assert_eq!(b, a, "ret mismatch ours={b} orig={a}, ours_err={eb:?} orig_err={ea:?}");
        assert_eq!(eb, ea, "err mismatch ours={eb:?} orig={ea:?}");

        let v1 = [
            btbuf_bytes(&r1.a),
            btbuf_bytes(&r1.b),
            btbuf_bytes(&r1.c),
            btbuf_bytes(&r1.d),
            btbuf_bytes(&r1.e),
        ];
        let v2 = [
            btbuf_bytes(&r2.a),
            btbuf_bytes(&r2.b),
            btbuf_bytes(&r2.c),
            btbuf_bytes(&r2.d),
            btbuf_bytes(&r2.e),
        ];
        assert_eq!(v2, v1);

        rel1(&mut r1);
        rel2(&mut r2);

        // Test with other repos if they exist
        for name in ["orm-cpp", "testBiturbo"] {
            let p = PathBuf::from(r"C:\git").join(name);
            let git_dir_x = p.join(".git");
            if git_dir_x.exists() {
                let git_dir_cx = CString::new(git_dir_x.to_string_lossy().to_string()).unwrap();
                let mut rx1 = BtReferences {
                    a: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                    b: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                    c: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                    d: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                    e: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                    hash: 0,
                };
                let mut rx2 = BtReferences {
                    a: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                    b: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                    c: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                    d: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                    e: BtBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 },
                    hash: 0,
                };
                let ax1 = f1(git_dir_cx.as_ptr(), include_tags, &mut rx1);
                let ax2 = f2(git_dir_cx.as_ptr(), include_tags, &mut rx2);
                assert_eq!(ax2, ax1);
                let vx1 = [
                    btbuf_bytes(&rx1.a),
                    btbuf_bytes(&rx1.b),
                    btbuf_bytes(&rx1.c),
                    btbuf_bytes(&rx1.d),
                    btbuf_bytes(&rx1.e),
                ];
                let vx2 = [
                    btbuf_bytes(&rx2.a),
                    btbuf_bytes(&rx2.b),
                    btbuf_bytes(&rx2.c),
                    btbuf_bytes(&rx2.d),
                    btbuf_bytes(&rx2.e),
                ];
                assert_eq!(vx2, vx1);
                rel1(&mut rx1);
                rel2(&mut rx2);
            }
        }
    }
}

#[test]
fn compare_bt_get_git_config_on_repo() {
    ensure_our_release_built();
    let mut git_dir = pick_git_dir();
    git_dir.push("config");
    let git_dir_c = CString::new(git_dir.to_string_lossy().to_string()).unwrap();

    let orig = Box::leak(Box::new(unsafe { Library::new(original_dll()).expect("load original dll") }));
    let ours = Box::leak(Box::new(unsafe { Library::new(our_release_dll()).expect("load our dll") }));
    unsafe {
        let f1: libloading::Symbol<BtGetGitConfig> = orig.get(b"bt_get_git_config\0").unwrap();
        let f2: libloading::Symbol<BtGetGitConfig> = ours.get(b"bt_get_git_config\0").unwrap();
        let rel1: libloading::Symbol<BtReleaseGitConfig> = orig.get(b"bt_release_git_config\0").unwrap();
        let rel2: libloading::Symbol<BtReleaseGitConfig> = ours.get(b"bt_release_git_config\0").unwrap();
        let g1: libloading::Symbol<BtGetLastErrorMessage> = orig.get(b"bt_get_last_error_message\0").unwrap();
        let g2: libloading::Symbol<BtGetLastErrorMessage> = ours.get(b"bt_get_last_error_message\0").unwrap();

        let mut c1 = BtGitConfig { ptr: std::ptr::null_mut(), len: 0, cap: 0 };
        let mut c2 = BtGitConfig { ptr: std::ptr::null_mut(), len: 0, cap: 0 };

        let a = f1(git_dir_c.as_ptr(), &mut c1);
        let ea = get_last_error(*g1);
        let b = f2(git_dir_c.as_ptr(), &mut c2);
        let eb = get_last_error(*g2);
        assert_eq!(b, a, "ret mismatch ours={b} orig={a}, ours_err={eb:?} orig_err={ea:?}");
        assert_eq!(eb, ea, "err mismatch ours={eb:?} orig={ea:?}");

        // Compare a stable projection of the config entries (strings only).
        let mut p1 = Vec::new();
        let mut p2 = Vec::new();
        for i in 0..c1.len {
            let e = &*c1.ptr.add(i);
            let kvs = std::slice::from_raw_parts(e.kv_ptr, e.kv_len)
                .iter()
                .map(|kv| (cstr_opt(kv.k), cstr_opt(kv.v)))
                .collect::<Vec<_>>();
            p1.push((cstr_opt(e.a), cstr_opt(e.b), kvs));
        }
        for i in 0..c2.len {
            let e = &*c2.ptr.add(i);
            let kvs = std::slice::from_raw_parts(e.kv_ptr, e.kv_len)
                .iter()
                .map(|kv| (cstr_opt(kv.k), cstr_opt(kv.v)))
                .collect::<Vec<_>>();
            p2.push((cstr_opt(e.a), cstr_opt(e.b), kvs));
        }
        assert_eq!(p2, p1);

        rel1(&mut c1);
        rel2(&mut c2);
    }
}

