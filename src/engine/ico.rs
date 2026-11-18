//! ICO 容器写入/读取。
//!
//! 这里刻意不使用第三方 ICO 编码器：多尺寸 ICO 的条目顺序、编码方式与
//! 解析器兼容性都直接影响“导出后被别的程序只认小图”这类问题，必须自己掌控。
//!
//! 两个关键决定（沿用 Python 版踩坑后的结论）：
//! 1. 尺寸阶梯恒为标准档，不随“当前所选尺寸/源图大小”塌缩；
//! 2. 默认全部用 DIB 条目编码，256 档也不例外——只认 DIB 的解析器
//!    （.NET `System.Drawing.Icon`、rsrc 类工具）会跳过 PNG 条目，
//!    导致文件里明明有 256，对方却只看到 128。

use std::io::{self, Write};
use std::path::Path;

use image::RgbaImage;

use crate::model::{ico_size_ladder, ICO_SIZE_LIMIT, ICO_USE_PNG_FOR_256};

/// 一帧待写入的图标（尺寸 + 像素）
pub struct IcoFrame {
    /// 边长
    pub size: u32,
    /// 像素数据
    pub image: RgbaImage,
}

/// 把单帧图像编码为 ICO 内部数据。
///
/// 默认走 DIB/BMP：`BITMAPINFOHEADER` + 自下而上的 BGRA XOR 位图 +
/// 4 字节对齐的全 0 AND 掩码（32 位用 alpha 通道，掩码留空即可）。
pub fn encode_frame(image: &RgbaImage) -> Vec<u8> {
    let (w, h) = image.dimensions();
    if ICO_USE_PNG_FOR_256 && w >= ICO_SIZE_LIMIT && h >= ICO_SIZE_LIMIT {
        return encode_png(image);
    }

    let mut out = Vec::with_capacity(40 + (w * h * 4) as usize);
    // BITMAPINFOHEADER
    out.extend_from_slice(&40u32.to_le_bytes()); // biSize
    out.extend_from_slice(&(w as i32).to_le_bytes()); // biWidth
    out.extend_from_slice(&((h as i32) * 2).to_le_bytes()); // biHeight = XOR + AND
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    out.extend_from_slice(&0u32.to_le_bytes()); // biSizeImage
    out.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    out.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    // XOR 位图：BGRA、行序自下而上
    let raw = image.as_raw();
    let stride = (w * 4) as usize;
    for y in (0..h as usize).rev() {
        let row = &raw[y * stride..(y + 1) * stride];
        for px in row.as_chunks::<4>().0 {
            out.push(px[2]); // B
            out.push(px[1]); // G
            out.push(px[0]); // R
            out.push(px[3]); // A
        }
    }

    // AND 掩码：1bpp，每行按 4 字节对齐
    let mask_stride = (w as usize).div_ceil(32) * 4;
    out.resize(out.len() + mask_stride * h as usize, 0);
    out
}

fn encode_png(image: &RgbaImage) -> Vec<u8> {
    let mut buf = io::Cursor::new(Vec::new());
    // 纯内存编码失败只可能是分配失败，这里退化为 DIB 之外无更好选择
    let _ =
        image::DynamicImage::ImageRgba8(image.clone()).write_to(&mut buf, image::ImageFormat::Png);
    buf.into_inner()
}

/// 写入多尺寸 ICO，保持传入顺序（最大尺寸排在最前）。
///
/// 返回实际写入的尺寸列表，便于保存后回读校验。
pub fn write_multisize(path: &Path, frames: &[IcoFrame]) -> io::Result<Vec<u32>> {
    let count = frames.len();
    let mut entries = Vec::with_capacity(count * 16);
    let mut data = Vec::new();
    let mut offset = 6 + count * 16;

    for frame in frames {
        let blob = encode_frame(&frame.image);
        // 目录项宽高为 1 字节：256 用 0 表示
        let dim = if frame.size >= ICO_SIZE_LIMIT {
            0
        } else {
            frame.size as u8
        };
        entries.push(dim); // bWidth
        entries.push(dim); // bHeight
        entries.push(0); // bColorCount
        entries.push(0); // bReserved
        entries.extend_from_slice(&1u16.to_le_bytes()); // wPlanes
        entries.extend_from_slice(&32u16.to_le_bytes()); // wBitCount
        entries.extend_from_slice(&(blob.len() as u32).to_le_bytes()); // dwBytesInRes
        entries.extend_from_slice(&(offset as u32).to_le_bytes()); // dwImageOffset
        data.extend_from_slice(&blob);
        offset += blob.len();
    }

    let mut file = io::BufWriter::new(std::fs::File::create(path)?);
    file.write_all(&0u16.to_le_bytes())?; // reserved
    file.write_all(&1u16.to_le_bytes())?; // type = icon
    file.write_all(&(count as u16).to_le_bytes())?; // count
    file.write_all(&entries)?;
    file.write_all(&data)?;
    file.flush()?;

    Ok(frames.iter().map(|f| f.size).collect())
}

/// 读取 ICO 目录，返回实际存在的尺寸（用于保存后自检）。
pub fn read_sizes(path: &Path) -> io::Result<Vec<u32>> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < 6 {
        return Ok(Vec::new());
    }
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    let mut sizes = Vec::with_capacity(count);
    for i in 0..count {
        let start = 6 + i * 16;
        if start + 2 > bytes.len() {
            break;
        }
        let w = if bytes[start] == 0 {
            256
        } else {
            bytes[start] as u32
        };
        let h = if bytes[start + 1] == 0 {
            256
        } else {
            bytes[start + 1] as u32
        };
        sizes.push(w.min(h));
    }
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes.dedup();
    Ok(sizes)
}

/// 按标准阶梯为源图生成全部帧（最大尺寸在最前）。
pub fn build_frames(source: &RgbaImage, preferred: u32) -> Vec<IcoFrame> {
    ico_size_ladder(preferred)
        .into_iter()
        .map(|size| IcoFrame {
            size,
            image: image::imageops::resize(
                source,
                size,
                size,
                image::imageops::FilterType::Lanczos3,
            ),
        })
        .collect()
}

/// 一次性把源图按标准阶梯写为多尺寸 ICO。
pub fn save_multisize(path: &Path, source: &RgbaImage, preferred: u32) -> io::Result<Vec<u32>> {
    let frames = build_frames(source, preferred);
    write_multisize(path, &frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(size: u32, rgba: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(size, size, image::Rgba(rgba))
    }

    /// 目录项读回：尺寸、bpp、偏移是否自洽
    fn parse_dir(bytes: &[u8]) -> Vec<(u32, u16, usize, usize)> {
        let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
        (0..count)
            .map(|i| {
                let e = &bytes[6 + i * 16..6 + (i + 1) * 16];
                let w = if e[0] == 0 { 256 } else { e[0] as u32 };
                let bpp = u16::from_le_bytes([e[6], e[7]]);
                let size = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
                let off = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
                (w, bpp, size, off)
            })
            .collect()
    }

    #[test]
    fn writes_full_ladder_with_largest_first() {
        let dir = std::env::temp_dir().join("icon_tool_ico_ladder.ico");
        let src = solid(256, [40, 160, 90, 255]);
        let written = save_multisize(&dir, &src, 16).expect("write ico");
        assert_eq!(written, vec![256, 128, 96, 64, 48, 32, 24, 16]);

        let bytes = std::fs::read(&dir).expect("read ico");
        let entries = parse_dir(&bytes);
        assert_eq!(entries.len(), 8);
        assert_eq!(entries[0].0, 256, "最大尺寸必须排在最前");
        assert!(entries.iter().all(|e| e.1 == 32), "全部应为 32 位");
        // 偏移必须落在文件内，且不等于目录区
        for (_, _, size, off) in &entries {
            assert!(*off >= 6 + 8 * 16);
            assert!(off + size <= bytes.len());
        }
        assert_eq!(
            read_sizes(&dir).unwrap(),
            vec![256, 128, 96, 64, 48, 32, 24, 16]
        );
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn dib_entry_has_expected_header_and_pixels() {
        let img = RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]));
        let blob = encode_frame(&img);
        assert_eq!(u32::from_le_bytes(blob[0..4].try_into().unwrap()), 40);
        assert_eq!(i32::from_le_bytes(blob[4..8].try_into().unwrap()), 4);
        // biHeight 为两倍高（XOR + AND）
        assert_eq!(i32::from_le_bytes(blob[8..12].try_into().unwrap()), 8);
        assert_eq!(u16::from_le_bytes(blob[14..16].try_into().unwrap()), 32);
        // 首个像素应为 BGRA
        assert_eq!(&blob[40..44], &[30, 20, 10, 255]);
        // 4x4 32bpp XOR = 64 字节，AND 掩码 4 字节对齐 => 每行 4 字节 × 4 行
        assert_eq!(blob.len(), 40 + 64 + 16);
    }

    #[test]
    fn ladder_survives_tiny_source() {
        // 旧 bug 现场：源图 16x16 + 默认尺寸 16，曾经只写出一帧 16x16
        let dir = std::env::temp_dir().join("icon_tool_ico_tiny.ico");
        let src = solid(16, [220, 60, 60, 255]);
        let written = save_multisize(&dir, &src, 16).expect("write ico");
        assert_eq!(written.len(), 8);
        let bytes = std::fs::read(&dir).unwrap();
        assert_eq!(parse_dir(&bytes)[0].0, 256);
        let _ = std::fs::remove_file(&dir);
    }
}
