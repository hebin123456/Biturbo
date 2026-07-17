use crate::ffi::error::set_last_error_str;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

/// Parse a 40-hex SHA-1 string into a 20-byte OID buffer.
///
/// The output matches the observed behavior in the original DLL:
/// each 32-bit word is byte-swapped (4-byte chunk reverse).
#[no_mangle]
pub unsafe extern "C" fn bt_oid_from_str(sha_string: *const c_char, out_oid20: *mut u8) -> c_int {
    if sha_string.is_null() || out_oid20.is_null() {
        set_last_error_str("invalid hash id");
        return 1;
    }

    let bytes = unsafe { CStr::from_ptr(sha_string) }.to_bytes();
    let input = String::from_utf8_lossy(bytes);
    if bytes.len() != 40 {
        set_last_error_str(&format!("parse SHA1 in '{}': OID length must be 40", input));
        return 1;
    }

    let mut raw = [0u8; 20];
    for i in 0..20 {
        let hi = bytes[i * 2];
        let lo = bytes[i * 2 + 1];
        let nib_hi = hex_nibble(hi);
        let nib_lo = hex_nibble(lo);
        match (nib_hi, nib_lo) {
            (Some(a), Some(b)) => raw[i] = (a << 4) | b,
            _ => {
                set_last_error_str(&format!("parse SHA1 in '{}': invalid hash id", input));
                return 1;
            }
        }
    }

    unsafe {
        for word in 0..5 {
            let base = word * 4;
            *out_oid20.add(base + 0) = raw[base + 3];
            *out_oid20.add(base + 1) = raw[base + 2];
            *out_oid20.add(base + 2) = raw[base + 1];
            *out_oid20.add(base + 3) = raw[base + 0];
        }
    }

    0
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_nibble_decimal_digits() {
        assert_eq!(hex_nibble(b'0'), Some(0));
        assert_eq!(hex_nibble(b'9'), Some(9));
    }

    #[test]
    fn hex_nibble_lowercase_hex() {
        assert_eq!(hex_nibble(b'a'), Some(10));
        assert_eq!(hex_nibble(b'f'), Some(15));
    }

    #[test]
    fn hex_nibble_uppercase_hex() {
        assert_eq!(hex_nibble(b'A'), Some(10));
        assert_eq!(hex_nibble(b'F'), Some(15));
    }

    #[test]
    fn hex_nibble_invalid_chars() {
        assert_eq!(hex_nibble(b'g'), None);
        assert_eq!(hex_nibble(b'G'), None);
        assert_eq!(hex_nibble(b' '), None);
        assert_eq!(hex_nibble(b'-'), None);
        assert_eq!(hex_nibble(b':'), None); // char just after '9'
    }

    #[test]
    fn hex_nibble_boundary() {
        // Boundary between decimal and hex ranges
        assert_eq!(hex_nibble(b'/'), None); // before '0'
        assert_eq!(hex_nibble(b'`'), None); // before 'a'
        assert_eq!(hex_nibble(b'@'), None); // before 'A'
    }
}

