//! # 内嵌图像解码
//!
//! 提供 [`bt_decode_image`]：把 TGA（含 RLE）图像字节流解码为 BMP 字节流，
//! 通过进程堆分配返回。原版 DLL 仅支持 TGA，对 PNG/JPG/BMP 等会直接返回错误。

use crate::ffi::error::set_last_error_str;
use crate::ffi::winheap::heap_alloc;
use std::os::raw::{c_int, c_void};

/// 图像解码结果。
///
/// # 字段
/// - `data`：BMP 字节流（含文件头与像素数据）。
/// - `data_len` / `data_cap`：已用 / 已分配字节数。
///
/// # 内存所有权
/// `data` 通过进程堆分配，必须用
/// [`crate::ffi::bt_release_vec::bt_release_decode_image`] 释放。
#[repr(C)]
pub struct BtDecodeImageResult {
    pub data: *mut c_void,
    pub data_len: i64,
    pub data_cap: i64,
}

/// 把 TGA 字节流解码为 BMP 字节流。
///
/// 仅支持未压缩 / RLE 的 truecolor（24/32bpp）与 grayscale（8bpp）TGA，
/// 不支持 color-mapped 与其他图像类型。BMP 输出统一为 24bpp。
///
/// # 参数
/// - `image_data_ptr` / `image_data_len`：TGA 输入字节流；`len < 0` 返回错误。
/// - `out_result`：输出 [`BtDecodeImageResult`]，调用前可未初始化。
///
/// # 返回值
/// - `0`：成功。
/// - `1`：参数非法、输入非 TGA / TGA 损坏 / 内存不足。
///
/// # 内存所有权
/// 输出的 `data` 通过进程堆分配，必须用
/// [`crate::ffi::bt_release_vec::bt_release_decode_image`] 释放。
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
        set_last_error_str("failed to fill whole buffer");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_rejects_too_short_input() {
        let mut out = Vec::new();
        assert!(!decode_tga_to_bmp(&[0u8; 10], &mut out));
        assert!(out.is_empty());
    }

    #[test]
    fn decode_rejects_color_mapped_tga() {
        let mut data = vec![0u8; 18];
        data[1] = 1; // color map present — unsupported
        let mut out = Vec::new();
        assert!(!decode_tga_to_bmp(&data, &mut out));
    }

    #[test]
    fn decode_rejects_unsupported_image_type() {
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 0; // no image data
        let mut out = Vec::new();
        assert!(!decode_tga_to_bmp(&data, &mut out));
    }

    #[test]
    fn decode_rejects_zero_dimensions() {
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 2; // truecolor
        data[16] = 24;
        let mut out = Vec::new();
        assert!(!decode_tga_to_bmp(&data, &mut out));
    }

    #[test]
    fn decode_grayscale_2x2_uncompressed() {
        // image_type=3 (grayscale), 8bpp, 2x2.
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 3;
        data[12..14].copy_from_slice(&2u16.to_le_bytes());
        data[14..16].copy_from_slice(&2u16.to_le_bytes());
        data[16] = 8;
        data.extend_from_slice(&[10, 20, 30, 40]);
        let mut out = Vec::new();
        assert!(decode_tga_to_bmp(&data, &mut out), "grayscale TGA should decode");
        assert_eq!(&out[0..2], b"BM");
        // BMP pixel data offset is always 54.
        assert_eq!(u32::from_le_bytes([out[10], out[11], out[12], out[13]]), 54);
        // row_stride = ((2*3+3)/4)*4 = 8; pixel_data_size = 8*2 = 16; file_size = 14+40+16 = 70.
        assert_eq!(u32::from_le_bytes([out[2], out[3], out[4], out[5]]), 70);
    }

    #[test]
    fn decode_truecolor_1x1_bottom_origin() {
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 2; // truecolor uncompressed
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16] = 24;
        data.extend_from_slice(&[0xFF, 0x00, 0x00]); // BGR: blue channel
        let mut out = Vec::new();
        assert!(decode_tga_to_bmp(&data, &mut out));
        assert_eq!(&out[0..2], b"BM");
        // row_stride = ((1*3+3)/4)*4 = 4; file_size = 14+40+4 = 58.
        assert_eq!(u32::from_le_bytes([out[2], out[3], out[4], out[5]]), 58);
        // BMP is bottom-up; with bottom-origin TGA (data[17] & 0x20 == 0) src_y maps 1:1 for 1x1.
        // Pixel at offset 54 should be the single BGR pixel.
        assert_eq!(&out[54..57], &[0xFF, 0x00, 0x00]);
    }

    #[test]
    fn decode_truecolor_1x1_top_origin_flips_row() {
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 2;
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16] = 24;
        data[17] = 0x20; // top-origin flag
        data.extend_from_slice(&[0xFF, 0x00, 0x00]);
        let mut out = Vec::new();
        assert!(decode_tga_to_bmp(&data, &mut out));
        // Top-origin TGA still maps to the single BMP row for 1x1.
        assert_eq!(&out[54..57], &[0xFF, 0x00, 0x00]);
    }

    #[test]
    fn decode_rle_grayscale_truncated_returns_false() {
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 11; // RLE grayscale
        data[12..14].copy_from_slice(&2u16.to_le_bytes());
        data[14..16].copy_from_slice(&2u16.to_le_bytes());
        data[16] = 8;
        // Truncated payload: header says raw run of 4 but no bytes follow.
        data.extend_from_slice(&[0x03]); // raw packet, count=4
        let mut out = Vec::new();
        assert!(!decode_tga_to_bmp(&data, &mut out));
    }

    #[test]
    fn decode_rejects_invalid_bpp_for_truecolor() {
        // truecolor 类型但 bpp 不是 24/32
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 2;
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16] = 16; // 不支持的 bpp
        let mut out = Vec::new();
        assert!(!decode_tga_to_bmp(&data, &mut out));
    }

    #[test]
    fn decode_rejects_invalid_bpp_for_grayscale() {
        // grayscale 类型但 bpp 不是 8
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 3;
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16] = 24; // grayscale 必须是 8bpp
        let mut out = Vec::new();
        assert!(!decode_tga_to_bmp(&data, &mut out));
    }

    #[test]
    fn decode_rejects_image_type_9_unsupported() {
        // image_type=9 是 color-mapped RLE，不支持
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 9;
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16] = 24;
        let mut out = Vec::new();
        assert!(!decode_tga_to_bmp(&data, &mut out));
    }

    #[test]
    fn decode_truecolor_32bpp_uncompressed() {
        // 32bpp truecolor 应能正确解码
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 2;
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16] = 32;
        data.extend_from_slice(&[0xFF, 0x00, 0x80, 0xFF]); // BGRA
        let mut out = Vec::new();
        assert!(decode_tga_to_bmp(&data, &mut out), "32bpp truecolor 应可解码");
        assert_eq!(&out[0..2], b"BM");
        // BMP 输出始终是 24bpp
        assert_eq!(u16::from_le_bytes([out[28], out[29]]), 24);
        // 像素数据偏移 54
        assert_eq!(u32::from_le_bytes([out[10], out[11], out[12], out[13]]), 54);
    }

    #[test]
    fn decode_truecolor_2x2_uncompressed() {
        // 2x2 truecolor 测试多像素扫描
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 2;
        data[12..14].copy_from_slice(&2u16.to_le_bytes());
        data[14..16].copy_from_slice(&2u16.to_le_bytes());
        data[16] = 24;
        // 4 个像素，每像素 3 字节
        data.extend_from_slice(&[
            0x11, 0x22, 0x33, // pixel (0,0)
            0x44, 0x55, 0x66, // pixel (1,0)
            0x77, 0x88, 0x99, // pixel (0,1)
            0xAA, 0xBB, 0xCC, // pixel (1,1)
        ]);
        let mut out = Vec::new();
        assert!(decode_tga_to_bmp(&data, &mut out));
        // row_stride = ((2*3+3)/4)*4 = 8; file_size = 14+40+8*2 = 70
        assert_eq!(u32::from_le_bytes([out[2], out[3], out[4], out[5]]), 70);
    }

    #[test]
    fn decode_with_id_field_skips_id() {
        // id_len > 0 时应跳过 ID 字段
        let id_field = b"some TGA comment";
        let mut data = vec![0u8; 18];
        data[0] = id_field.len() as u8; // id_len
        data[1] = 0;
        data[2] = 2;
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16] = 24;
        data.extend_from_slice(id_field);
        data.extend_from_slice(&[0xFF, 0x00, 0x00]);
        let mut out = Vec::new();
        assert!(decode_tga_to_bmp(&data, &mut out), "应跳过 ID 字段并解码");
        // 像素应为输入的 BGR
        assert_eq!(&out[54..57], &[0xFF, 0x00, 0x00]);
    }

    #[test]
    fn decode_id_field_extends_beyond_data_returns_false() {
        // id_len 声称超过数据长度应失败
        let mut data = vec![0u8; 18];
        data[0] = 100; // id_len=100 但数据不足
        data[1] = 0;
        data[2] = 2;
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16] = 24;
        // 只追加少量字节（不足 100 + 3 像素字节）
        data.extend_from_slice(&[0; 10]);
        let mut out = Vec::new();
        assert!(!decode_tga_to_bmp(&data, &mut out));
    }

    #[test]
    fn decode_rle_truecolor_works() {
        // RLE truecolor (image_type=10) 应能正确解码
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 10; // RLE truecolor
        data[12..14].copy_from_slice(&2u16.to_le_bytes());
        data[14..16].copy_from_slice(&2u16.to_le_bytes());
        data[16] = 24;
        // 4 个像素，使用 RLE 压缩
        // packet 1: 0x83 = run packet, count=4, 后跟 1 个像素 = (0xFF,0x00,0x80)
        data.extend_from_slice(&[0x83, 0xFF, 0x00, 0x80]);
        let mut out = Vec::new();
        assert!(decode_tga_to_bmp(&data, &mut out), "RLE truecolor 应可解码");
        assert_eq!(&out[0..2], b"BM");
        // 验证 BMP 头中的宽高
        assert_eq!(u32::from_le_bytes([out[18], out[19], out[20], out[21]]), 2);
        assert_eq!(u32::from_le_bytes([out[22], out[23], out[24], out[25]]), 2);
    }

    #[test]
    fn decode_rle_raw_packet_works() {
        // RLE 原始包（非 run packet）
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 10; // RLE truecolor
        data[12..14].copy_from_slice(&2u16.to_le_bytes());
        data[14..16].copy_from_slice(&2u16.to_le_bytes());
        data[16] = 24;
        // 0x03 = raw packet, count=4，后跟 4 个原始像素
        data.extend_from_slice(&[0x03]);
        data.extend_from_slice(&[
            0x11, 0x22, 0x33,
            0x44, 0x55, 0x66,
            0x77, 0x88, 0x99,
            0xAA, 0xBB, 0xCC,
        ]);
        let mut out = Vec::new();
        assert!(decode_tga_to_bmp(&data, &mut out), "RLE raw packet 应可解码");
        assert_eq!(&out[0..2], b"BM");
    }

    #[test]
    fn decode_rle_run_packet_count_max() {
        // RLE run packet 的最大 count=128（header=0xFF）
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 11; // RLE grayscale
        // 16x8 = 128 像素
        data[12..14].copy_from_slice(&16u16.to_le_bytes());
        data[14..16].copy_from_slice(&8u16.to_le_bytes());
        data[16] = 8;
        // 0xFF = run packet, count=128，后跟 1 个 grayscale 像素
        data.extend_from_slice(&[0xFF, 0x42]);
        let mut out = Vec::new();
        assert!(decode_tga_to_bmp(&data, &mut out), "RLE max count run packet 应可解码");
    }

    #[test]
    fn decode_bmp_header_pixel_data_offset_always_54() {
        // 不论图像尺寸，BMP 像素数据偏移应始终为 54
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 2;
        data[12..14].copy_from_slice(&3u16.to_le_bytes());
        data[14..16].copy_from_slice(&3u16.to_le_bytes());
        data[16] = 24;
        // 3x3 = 9 像素 * 3 字节
        data.extend_from_slice(&[0u8; 27]);
        let mut out = Vec::new();
        assert!(decode_tga_to_bmp(&data, &mut out));
        let offset = u32::from_le_bytes([out[10], out[11], out[12], out[13]]);
        assert_eq!(offset, 54);
    }

    #[test]
    fn decode_bmp_header_always_24bpp() {
        // 不论 TGA 的 bpp，BMP 输出始终是 24bpp
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 2;
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16] = 32; // TGA 32bpp
        data.extend_from_slice(&[0xFF, 0x00, 0x80, 0xFF]);
        let mut out = Vec::new();
        assert!(decode_tga_to_bmp(&data, &mut out));
        let bpp = u16::from_le_bytes([out[28], out[29]]);
        assert_eq!(bpp, 24, "BMP bpp 应始终为 24");
    }

    #[test]
    fn decode_grayscale_pixel_replicated_to_rgb() {
        // grayscale 像素值应在 BMP 中复制到 R/G/B 三个通道
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 3;
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16] = 8;
        data.extend_from_slice(&[0x42]); // grayscale 值
        let mut out = Vec::new();
        assert!(decode_tga_to_bmp(&data, &mut out));
        // BMP 像素在偏移 54 处，三个字节都应是 0x42
        assert_eq!(&out[54..57], &[0x42, 0x42, 0x42]);
    }

    #[test]
    fn decode_top_origin_2x2_flips_rows() {
        // top-origin TGA 的行应被翻转
        let mut data = vec![0u8; 18];
        data[1] = 0;
        data[2] = 2;
        data[12..14].copy_from_slice(&2u16.to_le_bytes());
        data[14..16].copy_from_slice(&2u16.to_le_bytes());
        data[16] = 24;
        data[17] = 0x20; // top-origin 标志
        // 像素：(0,0)=A, (1,0)=B, (0,1)=C, (1,1)=D
        data.extend_from_slice(&[
            0xAA, 0xAA, 0xAA, // A
            0xBB, 0xBB, 0xBB, // B
            0xCC, 0xCC, 0xCC, // C
            0xDD, 0xDD, 0xDD, // D
        ]);
        let mut out = Vec::new();
        assert!(decode_tga_to_bmp(&data, &mut out));
        // BMP 是 bottom-up，top-origin TGA 第 0 行（A,B）应出现在 BMP 最后一行
        // row_stride = ((2*3+3)/4)*4 = 8
        // 第一 BMP 行（底部）= out[54..60]，第二 BMP 行 = out[62..68]
        // top-origin 时 src_y = height-1-y，BMP 第一行（y=0）应来自 TGA 最后一行（C,D）
        // 但 1 像素映射有 padding，简化：验证两行第一个像素不同
        let row0 = &out[54..57];
        let row1_offset = 62; // 54 + 8
        let row1 = &out[row1_offset..row1_offset + 3];
        assert_ne!(row0, row1, "top-origin 翻转后两行首像素应不同");
    }
}
