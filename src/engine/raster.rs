//! 位图解码、缩放、缩略图、保存与剪贴板 DIB 编码。

use std::path::Path;

use image::{DynamicImage, RgbaImage};

/// 解码结果：像素 + 位深描述（位深描述用于列表展示，沿用原始色彩类型）
pub struct Decoded {
    /// 统一转成 RGBA8，便于后续缩放与保存
    pub image: RgbaImage,
    /// 例如 “32位RGBA”“24位RGB”“8位灰度”
    pub bit_label: String,
}

/// 把 `DynamicImage` 的色彩类型翻译成中文位深描述。
pub fn bit_label_of(color: image::ColorType) -> String {
    use image::ColorType as C;
    match color {
        C::L8 => "8位灰度".to_string(),
        C::La8 => "16位灰度+Alpha".to_string(),
        C::Rgb8 => "24位RGB".to_string(),
        C::Rgba8 => "32位RGBA".to_string(),
        C::L16 => "16位灰度".to_string(),
        C::La16 => "32位灰度+Alpha".to_string(),
        C::Rgb16 => "48位RGB".to_string(),
        C::Rgba16 => "64位RGBA".to_string(),
        C::Rgb32F => "96位浮点RGB".to_string(),
        C::Rgba32F => "128位浮点RGBA".to_string(),
        other => format!("{other:?}"),
    }
}

/// 读取位图文件（PNG/JPEG/WEBP/BMP/GIF）。
pub fn load_raster(path: &Path) -> Result<Decoded, String> {
    let reader = image::ImageReader::open(path)
        .map_err(|e| format!("无法打开文件: {e}"))?
        .with_guessed_format()
        .map_err(|e| format!("无法识别图像格式: {e}"))?;
    let dynamic: DynamicImage = reader.decode().map_err(|e| format!("图像解码失败: {e}"))?;
    let bit_label = bit_label_of(dynamic.color());
    Ok(Decoded {
        image: dynamic.to_rgba8(),
        bit_label,
    })
}

/// 从内存字节解码（用于 PE 资源里的 PNG 图标）
pub fn decode_bytes(bytes: &[u8]) -> Result<RgbaImage, String> {
    image::load_from_memory(bytes)
        .map(|d| d.to_rgba8())
        .map_err(|e| format!("图像解码失败: {e}"))
}

/// 缩放到任意宽高
pub fn resize_to(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    image::imageops::resize(
        image,
        width.max(1),
        height.max(1),
        image::imageops::FilterType::Lanczos3,
    )
}

/// 编码为 PNG 字节
pub fn encode_png(image: &RgbaImage) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let _ = DynamicImage::ImageRgba8(image.clone()).write_to(&mut buf, image::ImageFormat::Png);
    buf.into_inner()
}

/// 保存 PNG（optimize 语义由编码器默认提供）
pub fn save_png(path: &Path, image: &RgbaImage) -> Result<(), String> {
    std::fs::write(path, encode_png(image)).map_err(|e| format!("PNG 保存失败: {e}"))
}

/// 保存有损 WEBP（质量 0-100）
pub fn save_webp(path: &Path, image: &RgbaImage, quality: u8) -> Result<(), String> {
    let encoder = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height());
    let encoded = encoder.encode(quality.clamp(1, 100) as f32);
    std::fs::write(path, encoded.to_vec()).map_err(|e| format!("WEBP 保存失败: {e}"))
}

/// 编码为 CF_DIB 可用的 DIB（去掉 BMP 文件头，24 位 BGR、行序自下而上）。
///
/// 24 位而非 32 位：画图、PS 等程序对 32 位 CF_DIB 的 alpha 处理不一致，
/// 24 位是粘贴到这些程序最稳的形态（与旧版行为一致）。
pub fn to_dib(image: &RgbaImage) -> Vec<u8> {
    let (w, h) = image.dimensions();
    let stride = (w as usize * 3).div_ceil(4) * 4;
    let mut out = Vec::with_capacity(40 + stride * h as usize);
    out.extend_from_slice(&40u32.to_le_bytes()); // biSize
    out.extend_from_slice(&(w as i32).to_le_bytes()); // biWidth
    out.extend_from_slice(&(h as i32).to_le_bytes()); // biHeight（正数 = 自下而上）
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&24u16.to_le_bytes()); // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    out.extend_from_slice(&((stride * h as usize) as u32).to_le_bytes()); // biSizeImage
    out.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    out.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant

    let raw = image.as_raw();
    for y in (0..h as usize).rev() {
        let mut row = Vec::with_capacity(stride);
        let src = &raw[y * w as usize * 4..(y + 1) * w as usize * 4];
        for px in src.as_chunks::<4>().0 {
            row.push(px[2]);
            row.push(px[1]);
            row.push(px[0]);
        }
        row.resize(stride, 0);
        out.extend_from_slice(&row);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dib_header_matches_bitmapinfoheader() {
        let img = RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255]));
        let dib = to_dib(&img);
        assert_eq!(u32::from_le_bytes(dib[0..4].try_into().unwrap()), 40);
        assert_eq!(i32::from_le_bytes(dib[4..8].try_into().unwrap()), 3);
        assert_eq!(i32::from_le_bytes(dib[8..12].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(dib[14..16].try_into().unwrap()), 24);
        // 3 字节/像素 => 行 9 字节对齐到 12 字节，2 行共 24 字节
        assert_eq!(dib.len(), 40 + 24);
        // 首个像素 BGR
        assert_eq!(&dib[40..43], &[3, 2, 1]);
    }

    #[test]
    fn bit_labels_cover_common_types() {
        assert_eq!(bit_label_of(image::ColorType::Rgba8), "32位RGBA");
        assert_eq!(bit_label_of(image::ColorType::Rgb8), "24位RGB");
        assert_eq!(bit_label_of(image::ColorType::L8), "8位灰度");
    }
}
