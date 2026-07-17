use core::ffi::c_void;
use std::os::raw::c_char;

/// Raw Vec/String-like buffer as used across the FFI boundary.
///
/// The original DLL release routine checks `cap != 0` and frees `ptr` using
/// `HeapFree(GetProcessHeap(), 0, ptr)`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BtBuf {
    pub ptr: *mut c_void,
    pub len: usize,
    pub cap: usize,
}

#[repr(C)]
pub struct BtReferences {
    pub a: BtBuf,
    pub b: BtBuf,
    pub c: BtBuf,
    pub d: BtBuf,
    pub e: BtBuf,
    pub hash: u64,
}

#[repr(C)]
pub struct BtGitConfig {
    pub ptr: *mut BtGitConfigEntry,
    pub len: usize,
    pub cap: usize,
}

#[repr(C)]
pub struct BtGitConfigEntry {
    pub a: *mut c_char,
    pub b: *mut c_char,
    pub kv_ptr: *mut BtGitConfigKv,
    pub kv_len: usize,
    pub kv_cap: usize,
}

#[repr(C)]
pub struct BtGitConfigKv {
    pub k: *mut c_char,
    pub v: *mut c_char,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct BtOid {
    pub s0: u32,
    pub s1: u32,
    pub s2: u32,
    pub s3: u32,
    pub s4: u32,
}

impl BtOid {
    #[allow(dead_code)]
    pub fn from_bytes(bytes: [u8; 20]) -> Self {
        BtOid {
            s0: u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            s1: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            s2: u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            s3: u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            s4: u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
        }
    }

    pub fn to_bytes(&self) -> [u8; 20] {
        let mut out = [0u8; 20];
        out[0..4].copy_from_slice(&self.s0.to_be_bytes());
        out[4..8].copy_from_slice(&self.s1.to_be_bytes());
        out[8..12].copy_from_slice(&self.s2.to_be_bytes());
        out[12..16].copy_from_slice(&self.s3.to_be_bytes());
        out[16..20].copy_from_slice(&self.s4.to_be_bytes());
        out
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BtRange {
    pub start: u32,
    pub end: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btoid_roundtrip_all_zero() {
        let oid = BtOid::from_bytes([0u8; 20]);
        assert_eq!(oid.to_bytes(), [0u8; 20]);
        assert_eq!((oid.s0, oid.s1, oid.s2, oid.s3, oid.s4), (0, 0, 0, 0, 0));
    }

    #[test]
    fn btoid_roundtrip_all_ff() {
        let oid = BtOid::from_bytes([0xFFu8; 20]);
        assert_eq!(oid.to_bytes(), [0xFFu8; 20]);
        assert_eq!(oid.s0, 0xFFFFFFFF);
        assert_eq!(oid.s4, 0xFFFFFFFF);
    }

    #[test]
    fn btoid_from_bytes_is_big_endian() {
        // First 32-bit word 0x01020304 must map to s0 == 0x01020304.
        let mut bytes = [0u8; 20];
        bytes[0..4].copy_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        let oid = BtOid::from_bytes(bytes);
        assert_eq!(oid.s0, 0x01020304);
        assert_eq!(oid.to_bytes(), bytes);
    }

    #[test]
    fn btoid_ordering() {
        let a = BtOid::from_bytes([0u8; 20]);
        let mut b_bytes = [0u8; 20];
        b_bytes[0] = 1;
        let b = BtOid::from_bytes(b_bytes);
        assert!(a < b);
    }

    #[test]
    fn btoid_debug_format() {
        let oid = BtOid { s0: 1, s1: 2, s2: 3, s3: 4, s4: 5 };
        assert_eq!(format!("{oid:?}"), "BtOid { s0: 1, s1: 2, s2: 3, s3: 4, s4: 5 }");
    }
}

