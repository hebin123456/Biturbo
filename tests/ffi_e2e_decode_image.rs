//! 端到端测试：`bt_decode_image` 的 TGA→BMP 图像解码。
//!
//! 覆盖未压缩 truecolor TGA（24bpp）、grayscale TGA（8bpp）、
//! null 输入、过短数据、不支持格式等场景。

use biturbo::ffi::bt_decode_image::{bt_decode_image, BtDecodeImageResult};
use biturbo::ffi::bt_release_vec::bt_release_decode_image;

/// 构造一个最小的 1×1 24bpp 未压缩 truecolor TGA。
fn make_1x1_24bpp_tga() -> Vec<u8> {
    let mut data = vec![
        0x00, // ID 长度 = 0
        0x00, // 无颜色表
        0x02, // 图像类型 = 2（未压缩 truecolor）
        0x00, 0x00, 0x00, 0x00, 0x00, // 颜色表规格（5 字节全零）
        0x00, 0x00, // x 原点 = 0
        0x00, 0x00, // y 原点 = 0
        0x01, 0x00, // 宽度 = 1
        0x01, 0x00, // 高度 = 1
        0x18, // 像素深度 = 24bpp
        0x00, // 图像描述符 = 0
    ];
    // 1 个像素，3 字节 BGR
    data.extend_from_slice(&[0xFF, 0x00, 0x00]); // 蓝色像素
    data
}

/// 构造一个 2×2 24bpp 未压缩 truecolor TGA。
fn make_2x2_24bpp_tga() -> Vec<u8> {
    let mut data = vec![
        0x00, 0x00, 0x02,
        0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x02, 0x00, 0x02, 0x00, // 宽=2, 高=2
        0x18, 0x00,
    ];
    // 4 个像素 × 3 字节
    data.extend_from_slice(&[
        0xFF, 0x00, 0x00, // 蓝
        0x00, 0xFF, 0x00, // 绿
        0x00, 0x00, 0xFF, // 红
        0xFF, 0xFF, 0xFF, // 白
    ]);
    data
}

#[test]
fn decode_1x1_tga_succeeds() {
    let tga = make_1x1_24bpp_tga();
    let mut result = BtDecodeImageResult {
        data: std::ptr::null_mut(),
        data_len: 0,
        data_cap: 0,
    };
    let rc = unsafe { bt_decode_image(tga.as_ptr(), tga.len() as i64, &mut result) };
    assert_eq!(rc, 0, "1×1 TGA 解码应返回 0");
    assert!(!result.data.is_null(), "输出数据不应为 null");
    assert!(result.data_len > 0, "输出长度应大于 0");

    // BMP 文件以 "BM" 魔数开头
    let first_bytes = unsafe { std::slice::from_raw_parts(result.data as *const u8, 2) };
    assert_eq!(first_bytes[0], b'B', "BMP 魔数第一字节");
    assert_eq!(first_bytes[1], b'M', "BMP 魔数第二字节");

    unsafe { bt_release_decode_image(&mut result as *mut _ as *mut _) };
}

#[test]
fn decode_2x2_tga_succeeds() {
    let tga = make_2x2_24bpp_tga();
    let mut result = BtDecodeImageResult {
        data: std::ptr::null_mut(),
        data_len: 0,
        data_cap: 0,
    };
    let rc = unsafe { bt_decode_image(tga.as_ptr(), tga.len() as i64, &mut result) };
    assert_eq!(rc, 0, "2×2 TGA 解码应返回 0");
    assert!(result.data_len > 0);

    // 验证 BMP 头部
    let header = unsafe { std::slice::from_raw_parts(result.data as *const u8, 14) };
    assert_eq!(&header[0..2], b"BM");

    // BMP 文件头中的文件大小字段（偏移 2，4 字节小端）
    let file_size = u32::from_le_bytes([header[2], header[3], header[4], header[5]]);
    assert_eq!(file_size, result.data_len as u32, "BMP 文件大小应与输出长度一致");

    unsafe { bt_release_decode_image(&mut result as *mut _ as *mut _) };
}

#[test]
fn decode_null_input_returns_error() {
    let mut result = BtDecodeImageResult {
        data: std::ptr::null_mut(),
        data_len: 0,
        data_cap: 0,
    };
    let rc = unsafe { bt_decode_image(std::ptr::null(), 10, &mut result) };
    assert_eq!(rc, 1, "null 输入应返回 1");
    assert!(result.data.is_null());
    assert_eq!(result.data_len, 0);
}

#[test]
fn decode_negative_length_returns_error() {
    let tga = make_1x1_24bpp_tga();
    let mut result = BtDecodeImageResult {
        data: std::ptr::null_mut(),
        data_len: 0,
        data_cap: 0,
    };
    let rc = unsafe { bt_decode_image(tga.as_ptr(), -1, &mut result) };
    assert_eq!(rc, 1, "负长度应返回 1");
}

#[test]
fn decode_too_short_data_returns_error() {
    // 只有 10 字节，远小于 18 字节 TGA 头
    let short = vec![0u8; 10];
    let mut result = BtDecodeImageResult {
        data: std::ptr::null_mut(),
        data_len: 0,
        data_cap: 0,
    };
    let rc = unsafe { bt_decode_image(short.as_ptr(), short.len() as i64, &mut result) };
    assert_eq!(rc, 1, "过短数据应返回 1");
}

#[test]
fn decode_color_mapped_tga_returns_error() {
    // color_map_type = 1（有颜色表），不受支持
    let mut data = vec![0u8; 18];
    data[1] = 1;
    data[2] = 2;
    let mut result = BtDecodeImageResult {
        data: std::ptr::null_mut(),
        data_len: 0,
        data_cap: 0,
    };
    let rc = unsafe { bt_decode_image(data.as_ptr(), data.len() as i64, &mut result) };
    assert_eq!(rc, 1, "颜色表 TGA 应返回 1");
}

#[test]
fn decode_zero_dimensions_returns_error() {
    let mut data = vec![0u8; 18];
    data[2] = 2; // truecolor
    data[16] = 24; // 24bpp
    // width=0, height=0
    let mut result = BtDecodeImageResult {
        data: std::ptr::null_mut(),
        data_len: 0,
        data_cap: 0,
    };
    let rc = unsafe { bt_decode_image(data.as_ptr(), data.len() as i64, &mut result) };
    assert_eq!(rc, 1, "零尺寸应返回 1");
}

#[test]
fn decode_grayscale_8bpp_tga_succeeds() {
    // 构造 1×1 8bpp grayscale TGA（image_type=3）
    let mut data = vec![
        0x00, 0x00, 0x03, // image_type=3（未压缩 grayscale）
        0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, // 1×1
        0x08, // 8bpp
        0x00,
    ];
    data.push(0x80); // 1 个灰度像素
    let mut result = BtDecodeImageResult {
        data: std::ptr::null_mut(),
        data_len: 0,
        data_cap: 0,
    };
    let rc = unsafe { bt_decode_image(data.as_ptr(), data.len() as i64, &mut result) };
    assert_eq!(rc, 0, "8bpp grayscale TGA 解码应返回 0");
    assert!(result.data_len > 0);
    // 验证 BMP 魔数
    let magic = unsafe { std::slice::from_raw_parts(result.data as *const u8, 2) };
    assert_eq!(&magic[0..2], b"BM");

    unsafe { bt_release_decode_image(&mut result as *mut _ as *mut _) };
}

#[test]
fn decode_bmp_data_returns_error() {
    // 非 TGA 格式（如 BMP）应返回错误
    let bmp_header = vec![b'B', b'M', 0x00, 0x00, 0x00, 0x00];
    let mut result = BtDecodeImageResult {
        data: std::ptr::null_mut(),
        data_len: 0,
        data_cap: 0,
    };
    let rc = unsafe { bt_decode_image(bmp_header.as_ptr(), bmp_header.len() as i64, &mut result) };
    assert_eq!(rc, 1, "BMP 输入应返回 1");
}

#[test]
fn decode_rle_truecolor_tga_succeeds() {
    // 构造 2×2 24bpp RLE 压缩 TGA（image_type=10）
    let mut data = vec![
        0x00, 0x00, 0x0A, // image_type=10（RLE truecolor）
        0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
        0x02, 0x00, 0x02, 0x00, // 2×2
        0x18, 0x00,
    ];
    // RLE 包：1 个 run-length 包，count=3（header=0x82，表示 3 个相同像素）
    data.push(0x82); // 0x80 | (3-1) = 0x82
    data.extend_from_slice(&[0xFF, 0x00, 0x00]); // 蓝色像素 ×3
    // 第 4 个像素用 raw 包：count=1（header=0x00）
    data.push(0x00); // 0x00 | (1-1) = 0x00
    data.extend_from_slice(&[0x00, 0xFF, 0x00]); // 绿色像素 ×1

    let mut result = BtDecodeImageResult {
        data: std::ptr::null_mut(),
        data_len: 0,
        data_cap: 0,
    };
    let rc = unsafe { bt_decode_image(data.as_ptr(), data.len() as i64, &mut result) };
    assert_eq!(rc, 0, "RLE TGA 解码应返回 0");
    assert!(result.data_len > 0);

    unsafe { bt_release_decode_image(&mut result as *mut _ as *mut _) };
}

#[test]
fn release_null_is_safe() {
    // 释放 null 不应崩溃
    unsafe { bt_release_decode_image(std::ptr::null_mut()) };
}
