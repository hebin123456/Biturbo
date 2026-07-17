use crate::ffi::error::set_last_error_str;
use crate::ffi::winheap::heap_alloc;
use std::os::raw::c_int;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BtPatchToken {
    pub kind: u8,
    pub start: u32,
    pub end: u32,
}

#[repr(C)]
pub struct BtParsePatchResult {
    pub tokens: *mut BtPatchToken,
    pub tokens_len: i64,
    pub tokens_cap: i64,
}

#[no_mangle]
pub unsafe extern "C" fn bt_parse_patch(
    patch_utf8: *const u8,
    patch_utf8_len: u64,
    src_prefix_utf8: *const u8,
    src_prefix_utf8_len: u64,
    dst_prefix_utf8: *const u8,
    dst_prefix_utf8_len: u64,
    out_result: *mut BtParsePatchResult,
) -> c_int {
    if out_result.is_null() || patch_utf8.is_null() {
        set_last_error_str("invalid arguments");
        return 1;
    }

    unsafe {
        (*out_result).tokens = core::ptr::null_mut();
        (*out_result).tokens_len = 0;
        (*out_result).tokens_cap = 0;
    }

    let patch_bytes = unsafe { std::slice::from_raw_parts(patch_utf8, patch_utf8_len as usize) };
    let src_prefix = unsafe {
        if src_prefix_utf8.is_null() || src_prefix_utf8_len == 0 {
            String::new()
        } else {
            let slice = std::slice::from_raw_parts(src_prefix_utf8, src_prefix_utf8_len as usize);
            String::from_utf8_lossy(slice).into_owned()
        }
    };
    let dst_prefix = unsafe {
        if dst_prefix_utf8.is_null() || dst_prefix_utf8_len == 0 {
            String::new()
        } else {
            let slice = std::slice::from_raw_parts(dst_prefix_utf8, dst_prefix_utf8_len as usize);
            String::from_utf8_lossy(slice).into_owned()
        }
    };

    let patch = String::from_utf8_lossy(patch_bytes);
    let mut tokens = Vec::new();

    let mut pos = 0;
    while pos < patch.len() {
        let line_end = patch[pos..].find('\n').map(|idx| pos + idx).unwrap_or(patch.len());
        let content_end = if line_end > pos && patch.as_bytes()[line_end - 1] == b'\r' {
            line_end - 1
        } else {
            line_end
        };
        let token_end = line_end + if line_end < patch.len() { 1 } else { 0 };
        let line = &patch[pos..content_end];

        if line.starts_with("diff --git ") {
            add_token(&mut tokens, 0, pos + 7, pos + 10);
            add_diff_header_path_tokens(line, pos, &src_prefix, &dst_prefix, &mut tokens);
        } else if line.starts_with("index ") {
            let first = 6;
            if let Some(dots) = line[first..].find("..").map(|idx| first + idx) {
                let after_dots = dots + 2;
                let space = line[after_dots..].find(' ').map(|idx| after_dots + idx).unwrap_or(line.len());
                add_token(&mut tokens, 3, pos + first, pos + dots);
                add_token(&mut tokens, 4, pos + after_dots, pos + space);
                if space < line.len() {
                    add_token(&mut tokens, 5, pos + space + 1, content_end);
                }
            }
        } else if line.starts_with("similarity index ") {
            add_token(&mut tokens, 6, pos + 17, content_end);
        } else if line.starts_with("copy from ") {
            add_token(&mut tokens, 7, pos + 10, content_end);
        } else if line.starts_with("copy to ") {
            add_token(&mut tokens, 8, pos + 8, content_end);
        } else if line.starts_with("rename from ") {
            add_token(&mut tokens, 9, pos + 12, content_end);
        } else if line.starts_with("rename to ") {
            add_token(&mut tokens, 10, pos + 10, content_end);
        } else if line.starts_with("deleted file mode ") {
            add_token(&mut tokens, 11, pos + 18, content_end);
        } else if line.starts_with("new file mode ") {
            add_token(&mut tokens, 12, pos + 14, content_end);
        } else if line.starts_with("old mode ") {
            add_token(&mut tokens, 13, pos + 9, content_end);
        } else if line.starts_with("new mode ") {
            add_token(&mut tokens, 14, pos + 9, content_end);
        } else if line.starts_with("Binary files ") {
            add_token(&mut tokens, 15, pos, token_end);
        } else if line.starts_with("GIT binary patch") {
            add_token(&mut tokens, 15, pos, token_end);
        } else if line.starts_with("@@ ") {
            tokenize_chunk_header(line, pos, token_end, &mut tokens);
        } else if !line.is_empty() && line.as_bytes()[0] == b' ' {
            add_token(&mut tokens, 22, pos + 1, token_end);
        } else if !line.is_empty() && line.as_bytes()[0] == b'+' {
            add_token(&mut tokens, 23, pos + 1, token_end);
        } else if !line.is_empty() && line.as_bytes()[0] == b'-' {
            add_token(&mut tokens, 24, pos + 1, token_end);
        } else if line.starts_with("\\ ") {
            add_token(&mut tokens, 25, pos + 2, token_end);
        } else {
            add_token(&mut tokens, 26, pos, token_end);
        }

        pos = line_end + if line_end < patch.len() { 1 } else { 0 };
    }

    let bytes_len = tokens.len() * std::mem::size_of::<BtPatchToken>();
    let ptr = unsafe { heap_alloc(bytes_len) } as *mut BtPatchToken;
    if !ptr.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(tokens.as_ptr(), ptr, tokens.len());
            (*out_result).tokens = ptr;
            (*out_result).tokens_len = tokens.len() as i64;
            (*out_result).tokens_cap = tokens.len() as i64;
        }
    }

    0
}

fn add_token(tokens: &mut Vec<BtPatchToken>, kind: u8, start: usize, end: usize) {
    tokens.push(BtPatchToken {
        kind,
        start: start as u32,
        end: end as u32,
    });
}

fn tokenize_chunk_header(line: &str, line_start: usize, line_token_end: usize, tokens: &mut Vec<BtPatchToken>) {
    add_token(tokens, 16, line_start, line_token_end);
    if let Some(minus) = line.find('-') {
        let mut p = minus + 1;
        let start = p;
        while p < line.len() && line.as_bytes()[p].is_ascii_digit() {
            p += 1;
        }
        add_token(tokens, 17, line_start + start, line_start + p);
        if p < line.len() && line.as_bytes()[p] == b',' {
            p += 1;
            let len_start = p;
            while p < line.len() && line.as_bytes()[p].is_ascii_digit() {
                p += 1;
            }
            add_token(tokens, 18, line_start + len_start, line_start + p);
        }
    }
    if let Some(plus) = line.find('+') {
        let mut p = plus + 1;
        let start = p;
        while p < line.len() && line.as_bytes()[p].is_ascii_digit() {
            p += 1;
        }
        add_token(tokens, 19, line_start + start, line_start + p);
        if p < line.len() && line.as_bytes()[p] == b',' {
            p += 1;
            let len_start = p;
            while p < line.len() && line.as_bytes()[p].is_ascii_digit() {
                p += 1;
            }
            add_token(tokens, 20, line_start + len_start, line_start + p);
        }
    }
    if let Some(ctx) = line.rfind("@@") {
        if ctx + 2 < line.len() {
            let mut begin = ctx + 2;
            while begin < line.len() && line.as_bytes()[begin] == b' ' {
                begin += 1;
            }
            if begin < line.len() {
                add_token(tokens, 21, line_start + begin, line_token_end);
            }
        }
    }
}

fn add_diff_header_path_tokens(
    line: &str,
    line_start: usize,
    src_prefix: &str,
    dst_prefix: &str,
    tokens: &mut Vec<BtPatchToken>,
) -> bool {
    let src_marker = format!(" {src_prefix}");
    let dst_marker = format!(" {dst_prefix}");
    let src = line.find(&src_marker);
    let dst = if let Some(src_idx) = src {
        line[src_idx + src_marker.len()..].find(&dst_marker).map(|idx| src_idx + src_marker.len() + idx)
    } else {
        line.find(&dst_marker)
    };

    if let (Some(src_idx), Some(dst_idx)) = (src, dst) {
        add_token(tokens, 1, line_start + src_idx + src_marker.len(), line_start + dst_idx);
        add_token(tokens, 2, line_start + dst_idx + dst_marker.len(), line_start + line.len());
        return true;
    }

    let src = line.find(" a/");
    let dst = if let Some(src_idx) = src {
        line[src_idx + 3..].find(" b/").map(|idx| src_idx + 3 + idx)
    } else {
        line.find(" b/")
    };

    if let (Some(src_idx), Some(dst_idx)) = (src, dst) {
        add_token(tokens, 1, line_start + src_idx + 3, line_start + dst_idx);
        add_token(tokens, 2, line_start + dst_idx + 3, line_start + line.len());
        return true;
    }

    if let Some(first_quote) = line.find('"') {
        let second_quote = line[first_quote + 1..].find('"').map(|idx| first_quote + 1 + idx);
        let third_quote = if let Some(sec) = second_quote {
            line[sec + 1..].find('"').map(|idx| sec + 1 + idx)
        } else {
            None
        };
        let fourth_quote = if let Some(third) = third_quote {
            line[third + 1..].find('"').map(|idx| third + 1 + idx)
        } else {
            None
        };

        if let (Some(sec_q), Some(third_q), Some(fourth_q)) = (second_quote, third_quote, fourth_quote) {
            let mut src_start = first_quote + 1;
            let mut dst_start = third_q + 1;
            if line[src_start..].starts_with(src_prefix) {
                src_start += src_prefix.len();
            } else if line[src_start..].starts_with("a/") {
                src_start += 2;
            }
            if line[dst_start..].starts_with(dst_prefix) {
                dst_start += dst_prefix.len();
            } else if line[dst_start..].starts_with("b/") {
                dst_start += 2;
            }
            add_token(tokens, 1, line_start + src_start, line_start + sec_q);
            add_token(tokens, 2, line_start + dst_start, line_start + fourth_q);
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(tokens: &[BtPatchToken]) -> Vec<u8> {
        tokens.iter().map(|t| t.kind).collect()
    }

    #[test]
    fn diff_header_ab_prefix_splits_src_and_dst() {
        let line = "diff --git a/foo.txt b/bar.txt";
        let mut tokens = Vec::new();
        assert!(add_diff_header_path_tokens(line, 0, "a/", "b/", &mut tokens));

        let src = tokens.iter().find(|t| t.kind == 1).expect("src path token");
        assert_eq!(&line[src.start as usize..src.end as usize], "foo.txt");
        let dst = tokens.iter().find(|t| t.kind == 2).expect("dst path token");
        assert_eq!(&line[dst.start as usize..dst.end as usize], "bar.txt");
    }

    #[test]
    fn diff_header_custom_prefix() {
        let line = "diff --git prefix/foo other/bar";
        let mut tokens = Vec::new();
        assert!(add_diff_header_path_tokens(line, 0, "prefix/", "other/", &mut tokens));
        let src = tokens.iter().find(|t| t.kind == 1).unwrap();
        assert_eq!(&line[src.start as usize..src.end as usize], "foo");
        let dst = tokens.iter().find(|t| t.kind == 2).unwrap();
        assert_eq!(&line[dst.start as usize..dst.end as usize], "bar");
    }

    #[test]
    fn diff_header_quoted_paths() {
        let line = "diff --git \"a/space path.txt\" \"b/space path.txt\"";
        let mut tokens = Vec::new();
        assert!(add_diff_header_path_tokens(line, 0, "a/", "b/", &mut tokens));
        let src = tokens.iter().find(|t| t.kind == 1).unwrap();
        assert_eq!(&line[src.start as usize..src.end as usize], "space path.txt");
        let dst = tokens.iter().find(|t| t.kind == 2).unwrap();
        assert_eq!(&line[dst.start as usize..dst.end as usize], "space path.txt");
    }

    #[test]
    fn diff_header_no_match_returns_false() {
        let line = "not a diff header at all";
        let mut tokens = Vec::new();
        assert!(!add_diff_header_path_tokens(line, 0, "a/", "b/", &mut tokens));
    }

    #[test]
    fn chunk_header_tokenizes_full_hunk() {
        let line = "@@ -10,5 +20,7 @@ fn main()";
        let mut tokens = Vec::new();
        tokenize_chunk_header(line, 0, line.len(), &mut tokens);

        let ks = kinds(&tokens);
        assert!(ks.contains(&16), "whole-header token (16) missing");
        assert_eq!(
            &line[tokens.iter().find(|t| t.kind == 17).unwrap().start as usize
                ..tokens.iter().find(|t| t.kind == 17).unwrap().end as usize],
            "10"
        );
        assert_eq!(
            &line[tokens.iter().find(|t| t.kind == 18).unwrap().start as usize
                ..tokens.iter().find(|t| t.kind == 18).unwrap().end as usize],
            "5"
        );
        assert_eq!(
            &line[tokens.iter().find(|t| t.kind == 19).unwrap().start as usize
                ..tokens.iter().find(|t| t.kind == 19).unwrap().end as usize],
            "20"
        );
        assert_eq!(
            &line[tokens.iter().find(|t| t.kind == 20).unwrap().start as usize
                ..tokens.iter().find(|t| t.kind == 20).unwrap().end as usize],
            "7"
        );
        assert_eq!(
            &line[tokens.iter().find(|t| t.kind == 21).unwrap().start as usize
                ..tokens.iter().find(|t| t.kind == 21).unwrap().end as usize],
            "fn main()"
        );
    }

    #[test]
    fn chunk_header_without_counts_omits_len_tokens() {
        let line = "@@ -1 +1 @@";
        let mut tokens = Vec::new();
        tokenize_chunk_header(line, 0, line.len(), &mut tokens);
        let ks = kinds(&tokens);
        assert!(ks.contains(&17), "minus start token missing");
        assert!(ks.contains(&19), "plus start token missing");
        assert!(!ks.contains(&18), "minus len token should be absent");
        assert!(!ks.contains(&20), "plus len token should be absent");
    }

    #[test]
    fn add_token_records_kind_and_offsets() {
        let mut tokens = Vec::new();
        add_token(&mut tokens, 23, 10, 20);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, 23);
        assert_eq!(tokens[0].start, 10);
        assert_eq!(tokens[0].end, 20);
    }
}

