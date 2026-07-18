//! 公用 C ABI 类型定义。
//!
//! 本模块集中存放跨 FFI 边界使用的 `#[repr(C)]` 结构体，包括 `BtOid`（20 字节 SHA-1）、
//! `BtBuf`（Vec/String-like 缓冲区）、`BtRange`（半开区间）、`BtReferences`（引用列表）、
//! `BtGitConfig` 系列（Git 配置）等。这些结构体的字段顺序与布局必须与原版 `biturbo.dll`
//! 保持一致，调用方按 C ABI 直接读写。

use core::ffi::c_void;
use std::os::raw::c_char;

/// 跨 FFI 边界的原始 Vec/String-like 缓冲区。
///
/// 由 Biturbo 在 Windows 进程堆上分配，调用方必须通过对应的 `bt_release_*` 函数释放，
/// 不能混用 C 侧的 `free`。释放例程会检查 `cap != 0`，然后调用
/// `HeapFree(GetProcessHeap(), 0, ptr)`。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BtBuf {
    /// 数据指针（指向 Windows 进程堆分配的内存）。
    pub ptr: *mut c_void,
    /// 已使用长度（字节数）。
    pub len: usize,
    /// 已分配容量（字节数）。
    pub cap: usize,
}

/// 引用列表（分支 / 标签 / 远程等）的 FFI 返回结构。
///
/// 字段 `a`..`e` 为五条并行的字节缓冲区（分别承载引用名、目标 OID、类型标记等），
/// `hash` 为引用集合的快速比对哈希。调用方用 [`bt_release_references`](crate::ffi::bt_references::bt_release_references) 释放。
#[repr(C)]
pub struct BtReferences {
    /// 引用缓冲区 a（承载引用名等文本）。
    pub a: BtBuf,
    /// 引用缓冲区 b（承载目标 OID 等二进制数据）。
    pub b: BtBuf,
    /// 引用缓冲区 c（承载类型标记等）。
    pub c: BtBuf,
    /// 引用缓冲区 d。
    pub d: BtBuf,
    /// 引用缓冲区 e。
    pub e: BtBuf,
    /// 引用集合的快速比对哈希（用于检测引用是否变化）。
    pub hash: u64,
}

/// Git 配置的 FFI 返回结构（顶层容器）。
///
/// 持有一组 [`BtGitConfigEntry`]，调用方用
/// [`bt_release_git_config`](crate::ffi::bt_git_config::bt_release_git_config) 释放。
#[repr(C)]
pub struct BtGitConfig {
    /// 配置条目数组指针。
    pub ptr: *mut BtGitConfigEntry,
    /// 已使用条目数。
    pub len: usize,
    /// 已分配容量。
    pub cap: usize,
}

/// 单条 Git 配置条目（对应一个 section）。
///
/// 字段 `a` / `b` 为 section 名与子段名，`kv_*` 为该 section 下的键值对数组。
#[repr(C)]
pub struct BtGitConfigEntry {
    /// section 名（如 `user`、`remote`）。
    pub a: *mut c_char,
    /// 子段名（如 `origin`，可为空）。
    pub b: *mut c_char,
    /// 键值对数组指针。
    pub kv_ptr: *mut BtGitConfigKv,
    /// 键值对数量。
    pub kv_len: usize,
    /// 键值对容量。
    pub kv_cap: usize,
}

/// Git 配置键值对。
#[repr(C)]
pub struct BtGitConfigKv {
    /// 键（如 `name`、`email`、`url`）。
    pub k: *mut c_char,
    /// 值。
    pub v: *mut c_char,
}

/// 20 字节 SHA-1 对象 ID。
///
/// 用 5 个 `u32` 大端序字存储，便于跨 FFI 边界按值传递。字段 `s0`..`s4` 依次对应
/// SHA-1 的第 0..3、4..7、8..11、12..15、16..19 字节。`BtOid` 实现了 `Ord` / `Hash`，
/// 可直接作为排序与哈希键使用。
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct BtOid {
    /// 第 0..4 字节（大端序）。
    pub s0: u32,
    /// 第 4..8 字节（大端序）。
    pub s1: u32,
    /// 第 8..12 字节（大端序）。
    pub s2: u32,
    /// 第 12..16 字节（大端序）。
    pub s3: u32,
    /// 第 16..20 字节（大端序）。
    pub s4: u32,
}

impl BtOid {
    /// 从 20 字节数组构造 `BtOid`（按大端序解析）。
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

    /// 转回 20 字节数组（大端序）。
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

/// 半开区间 `[start, end)`。
///
/// 用于行号范围、字节范围等场景。
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BtRange {
    /// 起点（含）。
    pub start: u32,
    /// 终点（不含）。
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

    #[test]
    fn btoid_roundtrip_pattern_alternating() {
        // 0xAA 0x55 交替字节，验证每个 word 的边界
        let mut bytes = [0u8; 20];
        for i in 0..20 {
            bytes[i] = if i % 2 == 0 { 0xAA } else { 0x55 };
        }
        let oid = BtOid::from_bytes(bytes);
        assert_eq!(oid.to_bytes(), bytes);
        // 每 4 字节 word 应为 0xAA55AA55
        assert_eq!(oid.s0, 0xAA55AA55);
        assert_eq!(oid.s1, 0xAA55AA55);
        assert_eq!(oid.s2, 0xAA55AA55);
        assert_eq!(oid.s3, 0xAA55AA55);
        assert_eq!(oid.s4, 0xAA55AA55);
    }

    #[test]
    fn btoid_each_word_isolated() {
        // 仅设置某一个 word，验证 from_bytes/to_bytes 不串位
        let make = |word: usize, value: u32| {
            let mut bytes = [0u8; 20];
            bytes[word * 4..word * 4 + 4].copy_from_slice(&value.to_be_bytes());
            BtOid::from_bytes(bytes)
        };
        assert_eq!(make(0, 0xDEADBEEF).s0, 0xDEADBEEF);
        assert_eq!(make(1, 0xDEADBEEF).s1, 0xDEADBEEF);
        assert_eq!(make(2, 0xDEADBEEF).s2, 0xDEADBEEF);
        assert_eq!(make(3, 0xDEADBEEF).s3, 0xDEADBEEF);
        assert_eq!(make(4, 0xDEADBEEF).s4, 0xDEADBEEF);
        // 其他 word 应保持 0
        let oid = make(2, 0xDEADBEEF);
        assert_eq!((oid.s0, oid.s1, oid.s3, oid.s4), (0, 0, 0, 0));
    }

    #[test]
    fn btoid_eq_and_hash_consistent() {
        let a = BtOid::from_bytes([1u8; 20]);
        let b = BtOid::from_bytes([1u8; 20]);
        let c = BtOid::from_bytes([2u8; 20]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        // Hash 应一致：相同值在不同内存位置也应产生相同 hash
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut ha = DefaultHasher::new();
        let mut hb = DefaultHasher::new();
        a.hash(&mut ha);
        b.hash(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn btoid_ordering_total_per_word() {
        // 比较 s0 不同的情况
        let a = BtOid { s0: 1, s1: 0, s2: 0, s3: 0, s4: 0 };
        let b = BtOid { s0: 2, s1: 0, s2: 0, s3: 0, s4: 0 };
        assert!(a < b);
        // s0 相同、s1 不同：由 s1 决定
        let c = BtOid { s0: 1, s1: 5, s2: 0, s3: 0, s4: 0 };
        let d = BtOid { s0: 1, s1: 9, s2: 0, s3: 0, s4: 0 };
        assert!(c < d);
        // 完全相等：不小于
        assert!(!(a < a));
    }

    #[test]
    fn btoid_copy_does_not_alias() {
        // Copy 语义：复制后修改副本不影响原件（用 to_bytes 间接验证）
        let original = BtOid::from_bytes([7u8; 20]);
        let copied = original;
        // 如果是引用语义，下面这一行不会改变 original；Copy 应保证两者独立
        let _ = copied.to_bytes();
        assert_eq!(original.to_bytes(), [7u8; 20]);
    }

    #[test]
    fn btbuf_default_fields_zero() {
        // BtBuf 未实现 Default，但可手工构造“空”状态
        let buf = BtBuf { ptr: core::ptr::null_mut(), len: 0, cap: 0 };
        assert_eq!(buf.len, 0);
        assert_eq!(buf.cap, 0);
        assert!(buf.ptr.is_null());
    }

    #[test]
    fn btbuf_is_copy() {
        let buf = BtBuf { ptr: core::ptr::null_mut(), len: 42, cap: 100 };
        let copied = buf;
        // Copy 后字段值应一致
        assert_eq!(copied.len, 42);
        assert_eq!(copied.cap, 100);
    }

    #[test]
    fn btrange_half_open_semantics() {
        // 半开区间 [start, end)：start == end 表示空区间
        let empty = BtRange { start: 5, end: 5 };
        assert_eq!(empty.start, empty.end);

        let r = BtRange { start: 0, end: 10 };
        assert_eq!(r.end - r.start, 10);
    }

    #[test]
    fn btrange_eq_and_copy() {
        let a = BtRange { start: 1, end: 5 };
        let b = a; // Copy
        assert_eq!(a, b);
        let c = BtRange { start: 1, end: 6 };
        assert_ne!(a, c);
    }

    #[test]
    fn btreferences_default_zero_hash() {
        // 手工构造空 BtReferences，hash 字段应为 0
        let null_buf = BtBuf { ptr: core::ptr::null_mut(), len: 0, cap: 0 };
        let refs = BtReferences {
            a: null_buf,
            b: null_buf,
            c: null_buf,
            d: null_buf,
            e: null_buf,
            hash: 0,
        };
        assert_eq!(refs.hash, 0);
    }

    #[test]
    fn btgitconfig_layout_field_types() {
        // 验证 BtGitConfig 字段类型与文档一致（不实际分配）
        let cfg = BtGitConfig {
            ptr: core::ptr::null_mut(),
            len: 0,
            cap: 0,
        };
        assert_eq!(cfg.len, 0);
        assert_eq!(cfg.cap, 0);
        assert!(cfg.ptr.is_null());
    }
}

