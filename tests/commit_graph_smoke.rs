use libloading::Library;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::path::PathBuf;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct BtOid {
    s0: u32,
    s1: u32,
    s2: u32,
    s3: u32,
    s4: u32,
}

#[repr(C)]
struct BtCommitStorage {
    oids: *mut BtOid,
    oids_len: i64,
    oids_cap: i64,
    indexes: *mut u32,
    indexes_len: i64,
    indexes_cap: i64,
    has_more: u8,
}

type BtNewCommitGraphCache = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type BtReleaseCommitGraphCache = unsafe extern "C" fn(*mut *mut c_void);
type BtGetCommits = unsafe extern "C" fn(
    *const c_char,
    *const BtOid,
    i64,
    u8,
    i64,
    i64,
    i64,
    *const BtOid,
    i64,
    *mut *mut c_void,
    *mut *mut u8,
    *mut BtCommitStorage,
) -> c_int;
type BtReleaseCommitStorage = unsafe extern "C" fn(*mut BtCommitStorage);

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

fn pick_git_dir() -> PathBuf {
    for name in ["git-fork", "orm-cpp", "testBiturbo"] {
        let p = PathBuf::from(r"C:\git").join(name);
        let git_dir = p.join(".git");
        if git_dir.exists() {
            return git_dir;
        }
    }
    panic!("no repo found under C:\\git (expected git-fork/orm-cpp/testBiturbo)");
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn parse_hex_u32_8(s: &[u8]) -> u32 {
    let mut v: u32 = 0;
    for &b in s {
        v = (v << 4) | (hex_nibble(b).unwrap_or(0) as u32);
    }
    v
}

fn parse_oid(hex40: &str) -> BtOid {
    let b = hex40.as_bytes();
    assert_eq!(b.len(), 40);
    BtOid {
        s0: parse_hex_u32_8(&b[0..8]),
        s1: parse_hex_u32_8(&b[8..16]),
        s2: parse_hex_u32_8(&b[16..24]),
        s3: parse_hex_u32_8(&b[24..32]),
        s4: parse_hex_u32_8(&b[32..40]),
    }
}

fn head_oid(git_dir: &PathBuf) -> String {
    let out = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(git_dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse HEAD");
    assert!(out.status.success(), "git rev-parse failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn smoke_bt_get_commits_returns_data() {
    println!("Starting smoke test...");
    ensure_our_release_built();
    let git_dir = pick_git_dir();
    let git_dir_c = CString::new(git_dir.to_string_lossy().to_string()).unwrap();

    let head = head_oid(&git_dir);
    let tip = parse_oid(&head);

    println!("Loading release DLL...");
    let ours = Box::leak(Box::new(unsafe { Library::new(our_release_dll()).expect("load our dll") }));
    unsafe {
        let new_cache: libloading::Symbol<BtNewCommitGraphCache> = ours.get(b"bt_new_commit_graph_cache\0").unwrap();
        let rel_cache: libloading::Symbol<BtReleaseCommitGraphCache> =
            ours.get(b"bt_release_commit_graph_cache\0").unwrap();
        let get_commits: libloading::Symbol<BtGetCommits> = ours.get(b"bt_get_commits\0").unwrap();
        let rel_storage: libloading::Symbol<BtReleaseCommitStorage> =
            ours.get(b"bt_release_commit_storage\0").unwrap();

        println!("Calling new_cache...");
        let mut cache = new_cache(git_dir_c.as_ptr());
        println!("new_cache returned: {:?}", cache);

        let mut out = BtCommitStorage {
            oids: std::ptr::null_mut(),
            oids_len: 0,
            oids_cap: 0,
            indexes: std::ptr::null_mut(),
            indexes_len: 0,
            indexes_cap: 0,
            has_more: 0,
        };

        println!("Calling get_commits...");
        type BtNewCancellationToken = unsafe extern "C" fn() -> *mut u8;
        let new_token: libloading::Symbol<BtNewCancellationToken> = ours.get(b"bt_new_cancellation_token\0").unwrap();
        let mut token = new_token();

        let rc = get_commits(
            git_dir_c.as_ptr(),
            &tip as *const BtOid,
            1,
            0,
            2000,
            0,
            1,
            std::ptr::null(),
            0,
            &mut cache,
            &mut token,
            &mut out,
        );
        println!("get_commits returned code: {}", rc);
        assert_eq!(rc, 0, "bt_get_commits should succeed");
        assert!(out.indexes_len > 0, "should return at least one commit");
        assert!(out.oids_len >= out.indexes_len, "oids should include commits");

        let indexes = std::slice::from_raw_parts(out.indexes, out.indexes_len as usize);
        for &idx in indexes {
            assert!((idx as i64) < out.oids_len);
        }

        println!("Releasing storage...");
        rel_storage(&mut out);
        println!("Releasing cache...");
        rel_cache(&mut cache);
        println!("Finished successfully!");
    }
}
