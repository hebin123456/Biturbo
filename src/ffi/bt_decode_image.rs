use crate::ffi::error::set_last_error_str;
use crate::ffi::winheap::heap_alloc;
use std::os::raw::{c_int, c_void};

#[repr(C)]
pub struct BtDecodeImageResult {
    pub data: *mut c_void,
    pub data_len: i64,
    pub data_cap: i64,
}

#[no_mangle]
pub unsafe extern "C" fn bt_decode_image(
    image_data_ptr: *const u8,
    image_data_len: i64,
    out_result: *mut BtDecodeImageResult,
) -> c_int {
    if image_data_ptr.is_null() || image_data_len < 0 || out_result.is_null() {
        set_last_error_str("invalid image data");
        return 1;
    }

    unsafe {
        (*out_result).data = core::ptr::null_mut();
        (*out_result).data_len = 0;
        (*out_result).data_cap = 0;
    }

    let data = unsafe { std::slice::from_raw_parts(image_data_ptr, image_data_len as usize) };
    // The original DLL only decodes TGA to BMP, and returns Err (1) on non-TGA images (like PNG, JPG, BMP etc.).
    // Let's match the original DLL's strict TGA decoding requirement.

    // Decode TGA to BMP
    let mut bmp_bytes = Vec::new();
    if !decode_tga_to_bmp(data, &mut bmp_bytes) {
        set_last_error_str("Unsupported image format");
        return 1;
    }

    let ptr = unsafe { heap_alloc(bmp_bytes.len()) };
    if ptr.is_null() {
        set_last_error_str("insufficient memory");
        return 1;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(bmp_bytes.as_ptr(), ptr, bmp_bytes.len());
        (*out_result).data = ptr as _;
        (*out_result).data_len = bmp_bytes.len() as i64;
        (*out_result).data_cap = bmp_bytes.len() as i64;
    }
    0
}


fn decode_tga_to_bmp(data: &[u8], bmp: &mut Vec<u8>) -> bool {
    if data.len() < 18 {
        return false;
    }
    let id_len = data[0] as usize;
    let color_map_type = data[1];
    let image_type = data[2];

    if color_map_type != 0 || (image_type != 2 && image_type != 3 && image_type != 10 && image_type != 11) {
        return false;
    }

    let width = u16::from_le_bytes([data[12], data[13]]) as u32;
    let height = u16::from_le_bytes([data[14], data[15]]) as u32;
    let bpp = data[16];
    let grayscale = image_type == 3 || image_type == 11;

    if width == 0 || height == 0 || (!grayscale && bpp != 24 && bpp != 32) || (grayscale && bpp != 8) {
        return false;
    }

    let bytes_per_pixel = if grayscale { 1 } else { (bpp / 8) as usize };
    let pixel_offset = 18 + id_len;
    if pixel_offset > data.len() {
        return false;
    }
    let pixel_count = width as usize * height as usize;
    let pixel_bytes = pixel_count * bytes_per_pixel;

    if image_type == 2 && pixel_offset + pixel_bytes > data.len() {
        return false;
    }

    let mut decoded_pixels = Vec::new();
    let mut pixel_data = &data[pixel_offset..];

    if image_type == 10 || image_type == 11 {
        decoded_pixels.reserve(pixel_bytes);
        let mut pos = pixel_offset;
        while decoded_pixels.len() < pixel_bytes && pos < data.len() {
            let header = data[pos];
            pos += 1;
            let count = ((header & 0x7f) + 1) as usize;
            if (header & 0x80) != 0 {
                if pos + bytes_per_pixel > data.len() {
                    return false;
                }
                let val = &data[pos..pos + bytes_per_pixel];
                pos += bytes_per_pixel;
                for _ in 0..count {
                    if decoded_pixels.len() < pixel_bytes {
                        decoded_pixels.extend_from_slice(val);
                    }
                }
            } else {
                let bytes = count * bytes_per_pixel;
                if pos + bytes > data.len() {
                    return false;
                }
                decoded_pixels.extend_from_slice(&data[pos..pos + bytes]);
                pos += bytes;
            }
        }
        if decoded_pixels.len() < pixel_bytes {
            return false;
        }
        pixel_data = &decoded_pixels;
    }

    let top_origin = (data[17] & 0x20) != 0;
    let row_stride = ((width * 3 + 3) / 4) * 4;
    let pixel_data_size = row_stride * height;
    let file_size = 14 + 40 + pixel_data_size;

    bmp.clear();
    bmp.reserve(file_size as usize);

    bmp.push(b'B');
    bmp.push(b'M');
    bmp.extend_from_slice(&file_size.to_le_bytes());
    bmp.extend_from_slice(&0u16.to_le_bytes());
    bmp.extend_from_slice(&0u16.to_le_bytes());
    bmp.extend_from_slice(&54u32.to_le_bytes());

    // DIB Header
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&width.to_le_bytes());
    bmp.extend_from_slice(&height.to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&24u16.to_le_bytes()); // Always 24 BPP
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&pixel_data_size.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());

    for y in 0..height as usize {
        let src_y = if top_origin { height as usize - 1 - y } else { y };
        let start_len = bmp.len();
        for x in 0..width as usize {
            let p = (src_y * width as usize + x) * bytes_per_pixel;
            if grayscale {
                let val = pixel_data[p];
                bmp.push(val);
                bmp.push(val);
                bmp.push(val);
            } else {
                bmp.push(pixel_data[p]);
                bmp.push(pixel_data[p + 1]);
                bmp.push(pixel_data[p + 2]);
            }
        }
        while bmp.len() - start_len < row_stride as usize {
            bmp.push(0);
        }
    }

    true
}
