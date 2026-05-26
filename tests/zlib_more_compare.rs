use libloading::Library;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_ulong};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;

type Adler32 = unsafe extern "C" fn(c_ulong, *const u8, c_ulong) -> c_ulong;
type CompressBound = unsafe extern "C" fn(c_ulong) -> c_ulong;
type Compress2 = unsafe extern "C" fn(*mut u8, *mut c_ulong, *const u8, c_ulong, c_int) -> c_int;
type Uncompress = unsafe extern "C" fn(*mut u8, *mut c_ulong, *const u8, c_ulong) -> c_int;
type ZlibVersion = unsafe extern "C" fn() -> *const c_char;

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

#[test]
fn compare_adler32() {
    ensure_our_release_built();
    let orig = Box::leak(Box::new(unsafe { Library::new(original_dll()).expect("load original dll") }));
    let ours = Box::leak(Box::new(unsafe { Library::new(our_release_dll()).expect("load our dll") }));
    let data = b"The quick brown fox jumps over the lazy dog";
    unsafe {
        let f1: libloading::Symbol<Adler32> = orig.get(b"adler32\0").unwrap();
        let f2: libloading::Symbol<Adler32> = ours.get(b"adler32\0").unwrap();
        let a = f1(1, data.as_ptr(), data.len() as c_ulong);
        let b = f2(1, data.as_ptr(), data.len() as c_ulong);
        assert_eq!(b, a);
    }
}

#[test]
fn compare_compress2_uncompress_roundtrip() {
    ensure_our_release_built();
    let orig = Box::leak(Box::new(unsafe { Library::new(original_dll()).expect("load original dll") }));
    let ours = Box::leak(Box::new(unsafe { Library::new(our_release_dll()).expect("load our dll") }));

    let src = b"hello hello hello hello hello hello hello hello hello hello\n";

    unsafe {
        // Also sanity check we are using the same zlib version.
        let zv1: libloading::Symbol<ZlibVersion> = orig.get(b"zlibVersion\0").unwrap();
        let zv2: libloading::Symbol<ZlibVersion> = ours.get(b"zlibVersion\0").unwrap();
        let s1 = CStr::from_ptr(zv1()).to_string_lossy().to_string();
        let s2 = CStr::from_ptr(zv2()).to_string_lossy().to_string();
        assert!(s1.starts_with("1.") && s2.starts_with("1."));

        let bound1: libloading::Symbol<CompressBound> = orig.get(b"compressBound\0").unwrap();
        let bound2: libloading::Symbol<CompressBound> = ours.get(b"compressBound\0").unwrap();
        let b1 = bound1(src.len() as c_ulong);
        let b2 = bound2(src.len() as c_ulong);
        assert!(b1 > 0 && b2 > 0);

        let c1: libloading::Symbol<Compress2> = orig.get(b"compress2\0").unwrap();
        let c2: libloading::Symbol<Compress2> = ours.get(b"compress2\0").unwrap();
        let u1: libloading::Symbol<Uncompress> = orig.get(b"uncompress\0").unwrap();
        let u2: libloading::Symbol<Uncompress> = ours.get(b"uncompress\0").unwrap();

        // Compress using each DLL (might produce different compressed bytes across versions/options,
        // but should be decompressible and round-trip to the same plaintext).
        let mut dst1 = vec![0u8; b1 as usize];
        let mut dst1_len: c_ulong = dst1.len() as c_ulong;
        let rc1 = c1(
            dst1.as_mut_ptr(),
            &mut dst1_len as *mut c_ulong,
            src.as_ptr(),
            src.len() as c_ulong,
            6,
        );
        assert_eq!(rc1, 0, "orig compress2 failed: {rc1}");
        dst1.truncate(dst1_len as usize);

        let mut dst2 = vec![0u8; b2 as usize];
        let mut dst2_len: c_ulong = dst2.len() as c_ulong;
        let rc2 = c2(
            dst2.as_mut_ptr(),
            &mut dst2_len as *mut c_ulong,
            src.as_ptr(),
            src.len() as c_ulong,
            6,
        );
        assert_eq!(rc2, 0, "ours compress2 failed: {rc2}");
        dst2.truncate(dst2_len as usize);

        // Decompress both outputs using both DLLs (cross-check).
        let mut out = vec![0u8; src.len() * 4];
        let mut out_len: c_ulong = out.len() as c_ulong;
        let rc = u1(out.as_mut_ptr(), &mut out_len, dst1.as_ptr(), dst1.len() as c_ulong);
        assert_eq!(rc, 0, "orig uncompress(orig-bytes) failed: {rc}");
        out.truncate(out_len as usize);
        assert_eq!(out.as_slice(), src);

        let mut out = vec![0u8; src.len() * 4];
        let mut out_len: c_ulong = out.len() as c_ulong;
        let rc = u2(out.as_mut_ptr(), &mut out_len, dst2.as_ptr(), dst2.len() as c_ulong);
        assert_eq!(rc, 0, "ours uncompress(ours-bytes) failed: {rc}");
        out.truncate(out_len as usize);
        assert_eq!(out.as_slice(), src);

        let mut out = vec![0u8; src.len() * 4];
        let mut out_len: c_ulong = out.len() as c_ulong;
        let rc = u1(out.as_mut_ptr(), &mut out_len, dst2.as_ptr(), dst2.len() as c_ulong);
        assert_eq!(rc, 0, "orig uncompress(ours-bytes) failed: {rc}");
        out.truncate(out_len as usize);
        assert_eq!(out.as_slice(), src);

        let mut out = vec![0u8; src.len() * 4];
        let mut out_len: c_ulong = out.len() as c_ulong;
        let rc = u2(out.as_mut_ptr(), &mut out_len, dst1.as_ptr(), dst1.len() as c_ulong);
        assert_eq!(rc, 0, "ours uncompress(orig-bytes) failed: {rc}");
        out.truncate(out_len as usize);
        assert_eq!(out.as_slice(), src);
    }
}

