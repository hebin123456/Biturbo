//! # 仓库管理器（TOML 配置）
//!
//! 提供 [`bt_get_repository_manager`] / [`bt_save_repository_manager`] /
//! [`bt_release_repository_manager`]：读写应用级 TOML 配置文件，
//! 内容包括源目录、扫描深度、忽略规则以及已注册的仓库列表（含别名、颜色等）。
//!
//! 颜色用整数编码：`0` 表示未指定，`1..=6` 依次对应
//! Red / Orange / Yellow / Green / Blue / Violet，序列化时写成名称字符串。

use crate::ffi::error::set_last_error_str;
use crate::ffi::winheap::{heap_alloc, heap_alloc_c_string, heap_free};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;

/// 单个被管理仓库的配置条目。
///
/// # 字段
/// - `path`：仓库路径（NUL 终止 UTF-8）。
/// - `alias`：可选别名（NUL 终止 UTF-8）；无别名时为 `null`。
/// - `opened`：UI 中已打开的窗口数（用于恢复会话）。
/// - `color`：标签颜色编码（0=未指定，1..=6 见模块说明）。
///
/// # 内存所有权
/// `path` 与 `alias` 通过进程堆分配，由 [`bt_release_repository_manager`] 一并释放。
#[repr(C)]
pub struct BtRepositoryManagerRepository {
    pub path: *mut c_char,
    pub alias: *mut c_char,
    pub opened: u32,
    pub color: u8,
}

/// 仓库管理器配置根结构。
///
/// # 字段
/// - `source_dirs` / `source_dirs_len` / `source_dirs_cap`：源目录字符串数组。
/// - `scan_depth`：扫描子目录的最大深度，默认 5。
/// - `ignore` / `ignore_len` / `ignore_cap`：忽略规则字符串数组。
/// - `repositories` / `repositories_len` / `repositories_cap`：被管理仓库数组。
///
/// # 内存所有权
/// 所有内部指针均通过进程堆分配，必须用 [`bt_release_repository_manager`] 释放。
#[repr(C)]
pub struct BtRepositoryManager {
    pub source_dirs: *mut *mut c_char,
    pub source_dirs_len: i64,
    pub source_dirs_cap: i64,
    pub scan_depth: u8,
    pub ignore: *mut *mut c_char,
    pub ignore_len: i64,
    pub ignore_cap: i64,
    pub repositories: *mut BtRepositoryManagerRepository,
    pub repositories_len: i64,
    pub repositories_cap: i64,
}

#[derive(Serialize, Deserialize)]
struct TomlRepo {
    path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    alias: String,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    opened: u32,
    #[serde(default, deserialize_with = "deserialize_color", serialize_with = "serialize_color", skip_serializing_if = "is_default_color")]
    color: u8,
}

#[derive(Serialize, Deserialize)]
struct TomlConfig {
    #[serde(default)]
    source_dirs: Vec<String>,
    #[serde(default = "default_scan_depth")]
    scan_depth: u8,
    #[serde(default)]
    ignore: Vec<String>,
    #[serde(default, rename = "repository")]
    repositories: Vec<TomlRepo>,
    #[serde(default, rename = "repositories", skip_serializing)]
    repositories_compat: Vec<TomlRepo>,
}

fn default_scan_depth() -> u8 {
    5
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn is_default_color(v: &u8) -> bool {
    color_to_name(*v).is_none()
}

fn color_to_name(color: u8) -> Option<&'static str> {
    match color {
        1 => Some("Red"),
        2 => Some("Orange"),
        3 => Some("Yellow"),
        4 => Some("Green"),
        5 => Some("Blue"),
        6 => Some("Violet"),
        _ => None,
    }
}

fn color_from_name(name: &str) -> u8 {
    match name {
        "Red" => 1,
        "Orange" => 2,
        "Yellow" => 3,
        "Green" => 4,
        "Blue" => 5,
        "Violet" => 6,
        _ => 0,
    }
}

fn serialize_color<S>(color: &u8, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(color_to_name(*color).unwrap_or(""))
}

fn deserialize_color<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    struct ColorVisitor;

    impl<'de> serde::de::Visitor<'de> for ColorVisitor {
        type Value = u8;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a repository color string or integer")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(color_from_name(value))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value as u8)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value.max(0) as u8)
        }
    }

    deserializer.deserialize_any(ColorVisitor)
}

/// 从 TOML 配置文件加载仓库管理器配置。
///
/// # 参数
/// - `path`：TOML 文件路径（NUL 终止 UTF-8）。
/// - `out_result`：输出 [`BtRepositoryManager`]，调用前可未初始化。
///
/// # 返回值
/// - `0`：成功（含文件不存在时返回空配置的情况）。
/// - `1`：参数非法、文件读取失败、TOML 解析失败或内存不足。
///
/// # 内存所有权
/// 输出的所有字符串数组与仓库条目均通过进程堆分配，
/// 必须用 [`bt_release_repository_manager`] 释放。
#[no_mangle]
pub unsafe extern "C" fn bt_get_repository_manager(
    path: *const c_char,
    out_result: *mut BtRepositoryManager,
) -> c_int {
    if path.is_null() || out_result.is_null() {
        set_last_error_str("invalid input");
        return 1;
    }

    unsafe {
        (*out_result).source_dirs = core::ptr::null_mut();
        (*out_result).source_dirs_len = 0;
        (*out_result).source_dirs_cap = 0;
        (*out_result).scan_depth = 5;
        (*out_result).ignore = core::ptr::null_mut();
        (*out_result).ignore_len = 0;
        (*out_result).ignore_cap = 0;
        (*out_result).repositories = core::ptr::null_mut();
        (*out_result).repositories_len = 0;
        (*out_result).repositories_cap = 0;
    }

    let path_bytes = unsafe { CStr::from_ptr(path) }.to_bytes();
    let path_str = match std::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => {
            set_last_error_str("non-utf8 repositories path");
            return 1;
        }
    };

    let filepath = PathBuf::from(path_str);
    if !filepath.exists() {
        // File does not exist yet: return empty but success
        return 0;
    }

    let content = match std::fs::read_to_string(&filepath) {
        Ok(c) => c,
        Err(e) => {
            set_last_error_str(&format!("failed to read '{}': {e}", filepath.display()));
            return 1;
        }
    };

    let config: TomlConfig = match toml::from_str(&content) {
        Ok(c) => c,
        Err(e) => {
            set_last_error_str(&format!("failed to parse TOML config: {e}"));
            return 1;
        }
    };

    // 1. Allocate source_dirs
    let mut source_dirs_ptrs = Vec::new();
    for s in &config.source_dirs {
        let p = unsafe { heap_alloc_c_string(s) };
        if p.is_null() {
            set_last_error_str("insufficient memory");
            // Clean up already allocated
            for ptr in source_dirs_ptrs {
                unsafe { heap_free(ptr as _) };
            }
            return 1;
        }
        source_dirs_ptrs.push(p);
    }

    let source_dirs_len = source_dirs_ptrs.len() as i64;
    let source_dirs_ptr = if source_dirs_len > 0 {
        let alloc_bytes = source_dirs_ptrs.len() * std::mem::size_of::<*mut c_char>();
        let p = unsafe { heap_alloc(alloc_bytes) } as *mut *mut c_char;
        if p.is_null() {
            set_last_error_str("insufficient memory");
            for ptr in source_dirs_ptrs {
                unsafe { heap_free(ptr as _) };
            }
            return 1;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(source_dirs_ptrs.as_ptr(), p, source_dirs_ptrs.len());
        }
        p
    } else {
        core::ptr::null_mut()
    };

    // 2. Allocate ignore
    let mut ignore_ptrs = Vec::new();
    for s in &config.ignore {
        let p = unsafe { heap_alloc_c_string(s) };
        if p.is_null() {
            set_last_error_str("insufficient memory");
            for ptr in ignore_ptrs {
                unsafe { heap_free(ptr as _) };
            }
            if !source_dirs_ptr.is_null() {
                for ptr in source_dirs_ptrs {
                    unsafe { heap_free(ptr as _) };
                }
                unsafe { heap_free(source_dirs_ptr as _) };
            }
            return 1;
        }
        ignore_ptrs.push(p);
    }

    let ignore_len = ignore_ptrs.len() as i64;
    let ignore_ptr = if ignore_len > 0 {
        let alloc_bytes = ignore_ptrs.len() * std::mem::size_of::<*mut c_char>();
        let p = unsafe { heap_alloc(alloc_bytes) } as *mut *mut c_char;
        if p.is_null() {
            set_last_error_str("insufficient memory");
            for ptr in ignore_ptrs {
                unsafe { heap_free(ptr as _) };
            }
            for ptr in source_dirs_ptrs {
                unsafe { heap_free(ptr as _) };
            }
            unsafe { heap_free(source_dirs_ptr as _) };
            return 1;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(ignore_ptrs.as_ptr(), p, ignore_ptrs.len());
        }
        p
    } else {
        core::ptr::null_mut()
    };

    // 3. Allocate repositories
    let mut repos: Vec<BtRepositoryManagerRepository> = Vec::new();
    let repositories = if config.repositories.is_empty() {
        &config.repositories_compat
    } else {
        &config.repositories
    };

    for repo in repositories {
        let path_p = unsafe { heap_alloc_c_string(&repo.path) };
        let alias_p = if repo.alias.is_empty() {
            core::ptr::null_mut()
        } else {
            unsafe { heap_alloc_c_string(&repo.alias) }
        };

        if path_p.is_null() {
            set_last_error_str("insufficient memory");
            // Free all allocations
            for r in repos {
                if !r.path.is_null() { unsafe { heap_free(r.path as _) }; }
                if !r.alias.is_null() { unsafe { heap_free(r.alias as _) }; }
            }
            // Free other lists...
            for ptr in ignore_ptrs { unsafe { heap_free(ptr as _) }; }
            if !ignore_ptr.is_null() { unsafe { heap_free(ignore_ptr as _) }; }
            for ptr in source_dirs_ptrs { unsafe { heap_free(ptr as _) }; }
            if !source_dirs_ptr.is_null() { unsafe { heap_free(source_dirs_ptr as _) }; }
            return 1;
        }

        repos.push(BtRepositoryManagerRepository {
            path: path_p,
            alias: alias_p,
            opened: repo.opened,
            color: repo.color,
        });
    }

    let repositories_len = repos.len() as i64;
    let repositories_ptr = if repositories_len > 0 {
        let alloc_bytes = repos.len() * std::mem::size_of::<BtRepositoryManagerRepository>();
        let p = unsafe { heap_alloc(alloc_bytes) } as *mut BtRepositoryManagerRepository;
        if p.is_null() {
            set_last_error_str("insufficient memory");
            for r in repos {
                if !r.path.is_null() { unsafe { heap_free(r.path as _) }; }
                if !r.alias.is_null() { unsafe { heap_free(r.alias as _) }; }
            }
            for ptr in ignore_ptrs { unsafe { heap_free(ptr as _) }; }
            if !ignore_ptr.is_null() { unsafe { heap_free(ignore_ptr as _) }; }
            for ptr in source_dirs_ptrs { unsafe { heap_free(ptr as _) }; }
            if !source_dirs_ptr.is_null() { unsafe { heap_free(source_dirs_ptr as _) }; }
            return 1;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(repos.as_ptr(), p, repos.len());
        }
        p
    } else {
        core::ptr::null_mut()
    };

    unsafe {
        (*out_result).source_dirs = source_dirs_ptr;
        (*out_result).source_dirs_len = source_dirs_len;
        (*out_result).source_dirs_cap = source_dirs_len;
        (*out_result).scan_depth = config.scan_depth;
        (*out_result).ignore = ignore_ptr;
        (*out_result).ignore_len = ignore_len;
        (*out_result).ignore_cap = ignore_len;
        (*out_result).repositories = repositories_ptr;
        (*out_result).repositories_len = repositories_len;
        (*out_result).repositories_cap = repositories_len;
    }

    0
}

/// 把仓库管理器配置序列化为 TOML 并写入文件。
///
/// 各数组按索引一一对应：`paths[i]` / `aliases[i]` / `opened[i]` / `colors[i]`
/// 描述第 `i` 个仓库。长度不足的字段以默认值（空字符串 / 0）补齐。
///
/// # 参数
/// - `path`：目标 TOML 文件路径。
/// - `source_dirs_ptr` / `source_dirs_len`：源目录字符串数组。
/// - `scan_depth`：扫描深度。
/// - `ignore_ptr` / `ignore_len`：忽略规则字符串数组。
/// - `paths_ptr` / `paths_len`：仓库路径字符串数组。
/// - `aliases_ptr` / `aliases_len`：仓库别名字符串数组。
/// - `opened_ptr` / `opened_len`：每个仓库的打开计数（u32 数组）。
/// - `colors_ptr` / `colors_len`：每个仓库的颜色编码（u8 数组）。
///
/// # 返回值
/// - `0`：成功。
/// - `1`：参数非法、TOML 序列化失败或文件写入失败。
///
/// # 内存所有权
/// 本函数不持有任何输出缓冲区；输入数组归调用方所有，函数内部仅做读取。
#[no_mangle]
pub unsafe extern "C" fn bt_save_repository_manager(
    path: *const c_char,
    source_dirs_ptr: *const *const c_char,
    source_dirs_len: i64,
    scan_depth: u8,
    ignore_ptr: *const *const c_char,
    ignore_len: i64,
    paths_ptr: *const *const c_char,
    paths_len: i64,
    aliases_ptr: *const *const c_char,
    aliases_len: i64,
    opened_ptr: *const u32,
    opened_len: i64,
    colors_ptr: *const u8,
    colors_len: i64,
) -> c_int {
    if path.is_null() {
        set_last_error_str("invalid input path");
        return 1;
    }

    let path_bytes = unsafe { CStr::from_ptr(path) }.to_bytes();
    let path_str = match std::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => {
            set_last_error_str("non-utf8 path");
            return 1;
        }
    };
    let filepath = PathBuf::from(path_str);

    // Build source_dirs list
    let mut source_dirs = Vec::new();
    if !source_dirs_ptr.is_null() && source_dirs_len > 0 {
        for i in 0..source_dirs_len {
            let p = *source_dirs_ptr.add(i as usize);
            if !p.is_null() {
                if let Ok(s) = CStr::from_ptr(p).to_str() {
                    source_dirs.push(s.to_string());
                }
            }
        }
    }

    // Build ignore list
    let mut ignore = Vec::new();
    if !ignore_ptr.is_null() && ignore_len > 0 {
        for i in 0..ignore_len {
            let p = *ignore_ptr.add(i as usize);
            if !p.is_null() {
                if let Ok(s) = CStr::from_ptr(p).to_str() {
                    ignore.push(s.to_string());
                }
            }
        }
    }

    // Build repositories list
    let mut repositories = Vec::new();
    if !paths_ptr.is_null() && paths_len > 0 {
        for i in 0..paths_len {
            let p_ptr = *paths_ptr.add(i as usize);
            if p_ptr.is_null() { continue; }
            let p_str = match CStr::from_ptr(p_ptr).to_str() {
                Ok(s) => s.to_string(),
                Err(_) => continue,
            };

            let alias_str = if !aliases_ptr.is_null() && i < aliases_len {
                let a_ptr = *aliases_ptr.add(i as usize);
                if a_ptr.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(a_ptr).to_str().unwrap_or("").to_string()
                }
            } else {
                String::new()
            };

            let opened = if !opened_ptr.is_null() && i < opened_len {
                *opened_ptr.add(i as usize)
            } else {
                0
            };

            let color = if !colors_ptr.is_null() && i < colors_len {
                *colors_ptr.add(i as usize)
            } else {
                0
            };

            repositories.push(TomlRepo {
                path: p_str,
                alias: alias_str,
                opened,
                color,
            });
        }
    }

    let config = TomlConfig {
        source_dirs,
        scan_depth,
        ignore,
        repositories,
        repositories_compat: Vec::new(),
    };

    let serialized = match toml::to_string(&config) {
        Ok(s) => s,
        Err(e) => {
            set_last_error_str(&format!("failed to serialize TOML: {e}"));
            return 1;
        }
    };

    if let Err(e) = std::fs::write(&filepath, serialized) {
        set_last_error_str(&format!("failed to write TOML to '{}': {e}", filepath.display()));
        return 1;
    }

    0
}

/// 释放 [`bt_get_repository_manager`] 返回的 [`BtRepositoryManager`]。
///
/// 会逐个释放 `source_dirs`、`ignore` 数组中的字符串及数组本身，
/// 以及 `repositories` 数组中每个条目的 `path` / `alias` 字段、最后释放数组本身。
/// 调用后结构体字段会被清零，重复释放安全。传入 `null` 安全。
///
/// # 内存所有权
/// 仅可释放由 [`bt_get_repository_manager`] 填充的结构。
#[no_mangle]
pub unsafe extern "C" fn bt_release_repository_manager(p: *mut BtRepositoryManager) {
    if p.is_null() {
        return;
    }

    let source_dirs_ptr = std::ptr::replace(&mut (*p).source_dirs, core::ptr::null_mut());
    let source_dirs_len = (*p).source_dirs_len;
    (*p).source_dirs_len = 0;
    (*p).source_dirs_cap = 0;
    if !source_dirs_ptr.is_null() {
        for i in 0..source_dirs_len {
            let ptr = std::ptr::replace(&mut *source_dirs_ptr.add(i as usize), core::ptr::null_mut());
            if !ptr.is_null() {
                heap_free(ptr as _);
            }
        }
        heap_free(source_dirs_ptr as _);
    }

    let ignore_ptr = std::ptr::replace(&mut (*p).ignore, core::ptr::null_mut());
    let ignore_len = (*p).ignore_len;
    (*p).ignore_len = 0;
    (*p).ignore_cap = 0;
    if !ignore_ptr.is_null() {
        for i in 0..ignore_len {
            let ptr = std::ptr::replace(&mut *ignore_ptr.add(i as usize), core::ptr::null_mut());
            if !ptr.is_null() {
                heap_free(ptr as _);
            }
        }
        heap_free(ignore_ptr as _);
    }

    let repositories_ptr = std::ptr::replace(&mut (*p).repositories, core::ptr::null_mut());
    let repositories_len = (*p).repositories_len;
    (*p).repositories_len = 0;
    (*p).repositories_cap = 0;
    if !repositories_ptr.is_null() {
        for i in 0..repositories_len {
            let r = &mut *repositories_ptr.add(i as usize);
            let r_path = std::ptr::replace(&mut r.path, core::ptr::null_mut());
            if !r_path.is_null() {
                heap_free(r_path as _);
            }
            let r_alias = std::ptr::replace(&mut r.alias, core::ptr::null_mut());
            if !r_alias.is_null() {
                heap_free(r_alias as _);
            }
        }
        heap_free(repositories_ptr as _);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_name_roundtrip() {
        for c in 1..=6u8 {
            let name = color_to_name(c).unwrap();
            assert_eq!(color_from_name(name), c, "color {c} roundtrip failed");
        }
    }

    #[test]
    fn color_to_name_invalid_returns_none() {
        assert_eq!(color_to_name(0), None);
        assert_eq!(color_to_name(7), None);
        assert_eq!(color_to_name(255), None);
    }

    #[test]
    fn color_from_name_unknown_returns_zero() {
        assert_eq!(color_from_name("unknown"), 0);
        assert_eq!(color_from_name(""), 0);
    }

    #[test]
    fn color_names_match_spec() {
        assert_eq!(color_to_name(1), Some("Red"));
        assert_eq!(color_to_name(2), Some("Orange"));
        assert_eq!(color_to_name(3), Some("Yellow"));
        assert_eq!(color_to_name(4), Some("Green"));
        assert_eq!(color_to_name(5), Some("Blue"));
        assert_eq!(color_to_name(6), Some("Violet"));
    }

    #[test]
    fn default_scan_depth_is_five() {
        assert_eq!(default_scan_depth(), 5);
    }

    #[test]
    fn is_zero_u32_predicate() {
        assert!(is_zero_u32(&0));
        assert!(!is_zero_u32(&1));
    }

    #[test]
    fn is_default_color_predicate() {
        // Valid colors (1..=6) are NOT default; 0 and out-of-range are default (skipped).
        assert!(is_default_color(&0));
        assert!(is_default_color(&7));
        assert!(!is_default_color(&1));
        assert!(!is_default_color(&6));
    }

    #[test]
    fn toml_config_roundtrip_with_color_name() {
        let config = TomlConfig {
            source_dirs: vec!["/repo/a".to_string(), "/repo/b".to_string()],
            scan_depth: 3,
            ignore: vec!["*.tmp".to_string()],
            repositories: vec![TomlRepo {
                path: "/repo/a".to_string(),
                alias: "a".to_string(),
                opened: 2,
                color: 1, // Red
            }],
            repositories_compat: Vec::new(),
        };
        let toml_str = toml::to_string(&config).unwrap();
        // Color must serialize as the human-readable name, not the integer.
        assert!(toml_str.contains("color = \"Red\""), "color not serialized as name: {toml_str}");
        // Must use singular "repository" table name.
        assert!(toml_str.contains("[[repository]]"));

        let parsed: TomlConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.source_dirs, config.source_dirs);
        assert_eq!(parsed.scan_depth, 3);
        assert_eq!(parsed.ignore, config.ignore);
        assert_eq!(parsed.repositories.len(), 1);
        assert_eq!(parsed.repositories[0].path, "/repo/a");
        assert_eq!(parsed.repositories[0].alias, "a");
        assert_eq!(parsed.repositories[0].opened, 2);
        assert_eq!(parsed.repositories[0].color, 1);
    }

    #[test]
    fn toml_config_uses_defaults_when_fields_absent() {
        let toml_str = "source_dirs = []\n";
        let parsed: TomlConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.scan_depth, 5, "scan_depth should default to 5");
        assert!(parsed.ignore.is_empty());
        assert!(parsed.repositories.is_empty());
    }

    #[test]
    fn toml_config_reads_legacy_plural_repositories_field() {
        // Legacy config files used "repositories" (plural); must be read into
        // repositories_compat so old configs keep working.
        let toml_str = r#"
source_dirs = []
[[repositories]]
path = "/legacy"
color = "Blue"
"#;
        let parsed: TomlConfig = toml::from_str(toml_str).unwrap();
        assert!(parsed.repositories.is_empty());
        assert_eq!(parsed.repositories_compat.len(), 1);
        assert_eq!(parsed.repositories_compat[0].path, "/legacy");
        assert_eq!(parsed.repositories_compat[0].color, 5);
    }

    #[test]
    fn toml_color_accepts_integer_too() {
        // deserialize_color accepts both string names and raw integers.
        let toml_str = r#"
source_dirs = []
[[repository]]
path = "/r"
color = 3
"#;
        let parsed: TomlConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.repositories[0].color, 3);
    }

    #[test]
    fn toml_skip_serializing_default_color() {
        // color=0 (default) must be omitted from output to keep config clean.
        let config = TomlConfig {
            source_dirs: vec![],
            scan_depth: 5,
            ignore: vec![],
            repositories: vec![TomlRepo {
                path: "/r".to_string(),
                alias: String::new(),
                opened: 0,
                color: 0,
            }],
            repositories_compat: Vec::new(),
        };
        let toml_str = toml::to_string(&config).unwrap();
        assert!(!toml_str.contains("color"), "default color should be skipped: {toml_str}");
        assert!(!toml_str.contains("alias"), "empty alias should be skipped");
        assert!(!toml_str.contains("opened"), "zero opened should be skipped");
    }
}
