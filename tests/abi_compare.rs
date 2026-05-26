use libloading::Library;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

type BtOidFromStr = unsafe extern "C" fn(*const c_char, *mut u8) -> c_int;
type BtGetLastErrorMessage = unsafe extern "C" fn(*mut u8, usize) -> isize;
type BtMdToHtml = unsafe extern "C" fn(*const c_char, *mut *mut c_char) -> c_int;
type BtReleaseMdToHtml = unsafe extern "C" fn(*mut *mut c_char);
type BtNewCancellationToken = unsafe extern "C" fn() -> *mut u8;
type BtCancelCancellationToken = unsafe extern "C" fn(*mut *mut u8);
type BtReleaseCancellationToken = unsafe extern "C" fn(*mut *mut u8);
type ZlibVersion = unsafe extern "C" fn() -> *const c_char;
type Crc32 = unsafe extern "C" fn(u32, *const u8, u32) -> u32;
type BtGetHead = unsafe extern "C" fn(*const c_char, *mut BtHead) -> c_int;
type BtReleaseHead = unsafe extern "C" fn(*mut BtHead);

#[repr(C)]
#[derive(Clone, Copy)]
struct BtHead {
    oid20: [u8; 20],
    _pad: [u8; 4],
    ref_name: *mut c_char,
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn original_dll() -> PathBuf {
    manifest_dir().join("biturbo.dll")
}

fn our_release_dll() -> PathBuf {
    manifest_dir().join("target").join("release").join("biturbo.dll")
}

static BUILD_ONCE: Once = Once::new();

fn ensure_our_release_built() {
    BUILD_ONCE.call_once(|| {
        let user = std::env::var("USERPROFILE").expect("USERPROFILE");
        let cargo = PathBuf::from(user).join(".cargo").join("bin").join("cargo.exe");
        let status = Command::new(cargo)
            .current_dir(manifest_dir())
            .args(["build", "--release"])
            .status()
            .expect("run cargo build --release");
        assert!(status.success(), "cargo build --release failed");
        assert!(our_release_dll().exists(), "release dll missing");
    });
}

unsafe fn get_last_error(getter: BtGetLastErrorMessage) -> String {
    let mut buf = vec![0u8; 512];
    let n = unsafe { getter(buf.as_mut_ptr(), buf.len()) };
    if n <= 0 {
        return String::new();
    }
    let n = n as usize;
    let end = buf[..n].iter().position(|&b| b == 0).unwrap_or(n);
    String::from_utf8_lossy(&buf[..end]).to_string()
}

#[test]
fn compare_zlib_version() {
    ensure_our_release_built();
    let orig = Box::leak(Box::new(unsafe { Library::new(original_dll()).expect("load original dll") }));
    let ours = Box::leak(Box::new(unsafe { Library::new(our_release_dll()).expect("load our dll") }));
    unsafe {
        let z1: libloading::Symbol<ZlibVersion> = orig.get(b"zlibVersion\0").unwrap();
        let z2: libloading::Symbol<ZlibVersion> = ours.get(b"zlibVersion\0").unwrap();
        let s1 = CStr::from_ptr(z1()).to_string_lossy().to_string();
        let s2 = CStr::from_ptr(z2()).to_string_lossy().to_string();
        assert!(s1.starts_with("1.") && s2.starts_with("1."));
    }
}

#[test]
fn compare_crc32() {
    ensure_our_release_built();
    let orig = Box::leak(Box::new(unsafe { Library::new(original_dll()).expect("load original dll") }));
    let ours = Box::leak(Box::new(unsafe { Library::new(our_release_dll()).expect("load our dll") }));
    let data = b"hello world";
    unsafe {
        let f1: libloading::Symbol<Crc32> = orig.get(b"crc32\0").unwrap();
        let f2: libloading::Symbol<Crc32> = ours.get(b"crc32\0").unwrap();
        let a = f1(0, data.as_ptr(), data.len() as u32);
        let b = f2(0, data.as_ptr(), data.len() as u32);
        assert_eq!(b, a);
    }
}

#[test]
fn compare_bt_oid_from_str_success() {
    ensure_our_release_built();
    let orig = Box::leak(Box::new(unsafe { Library::new(original_dll()).expect("load original dll") }));
    let ours = Box::leak(Box::new(unsafe { Library::new(our_release_dll()).expect("load our dll") }));
    let sha = CString::new("decbf2be529ab6557d5429922251e5ee36519817").unwrap();
    unsafe {
        let f1: libloading::Symbol<BtOidFromStr> = orig.get(b"bt_oid_from_str\0").unwrap();
        let f2: libloading::Symbol<BtOidFromStr> = ours.get(b"bt_oid_from_str\0").unwrap();
        let mut o1 = [0u8; 20];
        let mut o2 = [0u8; 20];
        let r1 = f1(sha.as_ptr(), o1.as_mut_ptr());
        let r2 = f2(sha.as_ptr(), o2.as_mut_ptr());
        assert_eq!(r2, r1);
        assert_eq!(o2, o1);
    }
}

#[test]
fn compare_bt_oid_from_str_error_message() {
    ensure_our_release_built();
    let orig = Box::leak(Box::new(unsafe { Library::new(original_dll()).expect("load original dll") }));
    let ours = Box::leak(Box::new(unsafe { Library::new(our_release_dll()).expect("load our dll") }));
    let bad = CString::new("abc").unwrap();
    unsafe {
        let f1: libloading::Symbol<BtOidFromStr> = orig.get(b"bt_oid_from_str\0").unwrap();
        let f2: libloading::Symbol<BtOidFromStr> = ours.get(b"bt_oid_from_str\0").unwrap();
        let g1: libloading::Symbol<BtGetLastErrorMessage> = orig.get(b"bt_get_last_error_message\0").unwrap();
        let g2: libloading::Symbol<BtGetLastErrorMessage> = ours.get(b"bt_get_last_error_message\0").unwrap();
        let mut o = [0u8; 20];

        let r1 = f1(bad.as_ptr(), o.as_mut_ptr());
        let e1 = get_last_error(*g1);

        let r2 = f2(bad.as_ptr(), o.as_mut_ptr());
        let e2 = get_last_error(*g2);

        assert_eq!(r2, r1, "return mismatch: ours={r2} orig={r1}");
        assert_eq!(e2, e1, "error mismatch: ours={e2:?} orig={e1:?}");
    }
}

#[test]
fn compare_bt_md_to_html() {
    ensure_our_release_built();
    let orig = Box::leak(Box::new(unsafe { Library::new(original_dll()).expect("load original dll") }));
    let ours = Box::leak(Box::new(unsafe { Library::new(our_release_dll()).expect("load our dll") }));
    let md = CString::new("Hello\n\n**world**").unwrap();
    unsafe {
        let f1: libloading::Symbol<BtMdToHtml> = orig.get(b"bt_md_to_html\0").unwrap();
        let f2: libloading::Symbol<BtMdToHtml> = ours.get(b"bt_md_to_html\0").unwrap();
        let rel1: libloading::Symbol<BtReleaseMdToHtml> = orig.get(b"bt_release_md_to_html\0").unwrap();
        let rel2: libloading::Symbol<BtReleaseMdToHtml> = ours.get(b"bt_release_md_to_html\0").unwrap();

        let mut p1: *mut c_char = std::ptr::null_mut();
        let mut p2: *mut c_char = std::ptr::null_mut();
        let r1 = f1(md.as_ptr(), &mut p1);
        let r2 = f2(md.as_ptr(), &mut p2);
        assert_eq!(r2, r1);
        assert!(!p1.is_null());
        assert!(!p2.is_null());

        let s1 = CStr::from_ptr(p1).to_string_lossy().to_string();
        let s2 = CStr::from_ptr(p2).to_string_lossy().to_string();
        assert_eq!(s2, s1);

        rel1(&mut p1);
        rel2(&mut p2);
    }
}

#[test]
fn compare_cancellation_token() {
    ensure_our_release_built();
    let orig = Box::leak(Box::new(unsafe { Library::new(original_dll()).expect("load original dll") }));
    let ours = Box::leak(Box::new(unsafe { Library::new(our_release_dll()).expect("load our dll") }));
    unsafe {
        let new1: libloading::Symbol<BtNewCancellationToken> = orig.get(b"bt_new_cancellation_token\0").unwrap();
        let new2: libloading::Symbol<BtNewCancellationToken> = ours.get(b"bt_new_cancellation_token\0").unwrap();
        let cancel1: libloading::Symbol<BtCancelCancellationToken> = orig.get(b"bt_cancel_cancellation_token\0").unwrap();
        let cancel2: libloading::Symbol<BtCancelCancellationToken> = ours.get(b"bt_cancel_cancellation_token\0").unwrap();
        let rel1: libloading::Symbol<BtReleaseCancellationToken> = orig.get(b"bt_release_cancellation_token\0").unwrap();
        let rel2: libloading::Symbol<BtReleaseCancellationToken> = ours.get(b"bt_release_cancellation_token\0").unwrap();

        let mut t1 = new1();
        let mut t2 = new2();
        assert!(!t1.is_null());
        assert!(!t2.is_null());
        assert_eq!(*t2, *t1);

        cancel1(&mut t1);
        cancel2(&mut t2);
        assert_eq!(*t2, *t1);

        rel1(&mut t1);
        rel2(&mut t2);
    }
}

#[test]
fn compare_bt_get_head_ref() {
    ensure_our_release_built();
    let git_dir = make_fake_git_dir_ref();
    let git_dir_c = CString::new(git_dir.to_string_lossy().to_string()).unwrap();

    let orig = Box::leak(Box::new(unsafe { Library::new(original_dll()).expect("load original dll") }));
    let ours = Box::leak(Box::new(unsafe { Library::new(our_release_dll()).expect("load our dll") }));
    unsafe {
        let f1: libloading::Symbol<BtGetHead> = orig.get(b"bt_get_head\0").unwrap();
        let f2: libloading::Symbol<BtGetHead> = ours.get(b"bt_get_head\0").unwrap();
        let rel1: libloading::Symbol<BtReleaseHead> = orig.get(b"bt_release_head\0").unwrap();
        let rel2: libloading::Symbol<BtReleaseHead> = ours.get(b"bt_release_head\0").unwrap();

        let mut h1 = BtHead { oid20: [0u8; 20], _pad: [0u8; 4], ref_name: std::ptr::null_mut() };
        let mut h2 = BtHead { oid20: [0u8; 20], _pad: [0u8; 4], ref_name: std::ptr::null_mut() };
        let r1 = f1(git_dir_c.as_ptr(), &mut h1);
        let r2 = f2(git_dir_c.as_ptr(), &mut h2);
        if r1 != r2 {
            let g1: libloading::Symbol<BtGetLastErrorMessage> = orig.get(b"bt_get_last_error_message\0").unwrap();
            let g2: libloading::Symbol<BtGetLastErrorMessage> = ours.get(b"bt_get_last_error_message\0").unwrap();
            let e1 = get_last_error(*g1);
            let e2 = get_last_error(*g2);
            panic!("return mismatch: ours={r2} (err={e2:?}) orig={r1} (err={e1:?})");
        }
        if r1 != 0 {
            let g1: libloading::Symbol<BtGetLastErrorMessage> = orig.get(b"bt_get_last_error_message\0").unwrap();
            let g2: libloading::Symbol<BtGetLastErrorMessage> = ours.get(b"bt_get_last_error_message\0").unwrap();
            let e1 = get_last_error(*g1);
            let e2 = get_last_error(*g2);
            assert_eq!(e2, e1, "error mismatch: ours={e2:?} orig={e1:?}");
            return;
        }
        let s1 = if h1.ref_name.is_null() {
            String::new()
        } else {
            CStr::from_ptr(h1.ref_name).to_string_lossy().to_string()
        };
        let s2 = if h2.ref_name.is_null() {
            String::new()
        } else {
            CStr::from_ptr(h2.ref_name).to_string_lossy().to_string()
        };
        assert_eq!(s2, s1);

        if h2.oid20 != h1.oid20 {
            panic!(
                "bt_get_head oid mismatch (ref_name={s1:?}): ours={:?} orig={:?}",
                h2.oid20, h1.oid20
            );
        }

        rel1(&mut h1);
        rel2(&mut h2);
    }
}

fn make_fake_git_dir_ref() -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "biturbo_fake_repo_{}_{}",
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

#[test]
fn compare_bt_get_head_detached() {
    ensure_our_release_built();
    let git_dir = make_fake_git_dir_detached();
    let git_dir_c = CString::new(git_dir.to_string_lossy().to_string()).unwrap();

    let orig = Box::leak(Box::new(unsafe { Library::new(original_dll()).expect("load original dll") }));
    let ours = Box::leak(Box::new(unsafe { Library::new(our_release_dll()).expect("load our dll") }));
    unsafe {
        let f1: libloading::Symbol<BtGetHead> = orig.get(b"bt_get_head\0").unwrap();
        let f2: libloading::Symbol<BtGetHead> = ours.get(b"bt_get_head\0").unwrap();
        let rel1: libloading::Symbol<BtReleaseHead> = orig.get(b"bt_release_head\0").unwrap();
        let rel2: libloading::Symbol<BtReleaseHead> = ours.get(b"bt_release_head\0").unwrap();

        let mut h1 = BtHead { oid20: [0u8; 20], _pad: [0u8; 4], ref_name: std::ptr::null_mut() };
        let mut h2 = BtHead { oid20: [0u8; 20], _pad: [0u8; 4], ref_name: std::ptr::null_mut() };
        let r1 = f1(git_dir_c.as_ptr(), &mut h1);
        let r2 = f2(git_dir_c.as_ptr(), &mut h2);
        assert_eq!(r2, r1);
        assert_eq!(h2.oid20, h1.oid20);
        assert!(h1.ref_name.is_null());
        assert!(h2.ref_name.is_null());

        rel1(&mut h1);
        rel2(&mut h2);
    }
}

fn make_fake_git_dir_detached() -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "biturbo_fake_repo_detached_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let git_dir = base.join(".git");
    std::fs::create_dir_all(&git_dir).unwrap();
    std::fs::write(
        git_dir.join("HEAD"),
        b"decbf2be529ab6557d5429922251e5ee36519817\n",
    )
    .unwrap();
    git_dir
}

