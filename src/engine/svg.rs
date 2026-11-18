//! SVG 光栅化（resvg + usvg，纯 Rust，不依赖 Cairo）。
//!
//! 旧 Python 版用 resvg-py 在 SVG 自身尺寸下栅格化；矢量图标往往只有 24–48px，
//! 放大后必然糊。这里在保留原始比例的前提下把长边抬到至少 256（上限 1024），
//! 让后续导出的 256 帧有真实细节，而不是把 48px 位图拉伸到 256。

use std::path::Path;

use image::RgbaImage;

/// 栅格化的最小长边
const MIN_RASTER_EDGE: u32 = 256;
/// 栅格化的最大长边，避免超大 SVG 吃掉内存
const MAX_RASTER_EDGE: u32 = 1024;

/// 把 SVG 文件栅格化为 RGBA 位图。
pub fn render_file(path: &Path) -> Result<RgbaImage, String> {
    let data = std::fs::read(path).map_err(|e| format!("无法读取 SVG: {e}"))?;
    // 先做一次轻量校验，给出比解析器更直白的错误
    let head = String::from_utf8_lossy(&data[..data.len().min(2048)]).to_lowercase();
    if !head.contains("<svg") {
        return Err("文件不是有效的 SVG 格式".to_string());
    }

    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data(&data, &options).map_err(|e| format!("SVG 解析失败: {e}"))?;

    let size = tree.size();
    let (sw, sh) = (size.width(), size.height());
    if !(sw.is_finite() && sh.is_finite()) || sw <= 0.0 || sh <= 0.0 {
        return Err("SVG 缺少有效的尺寸信息".to_string());
    }

    let long_edge = sw.max(sh).round() as u32;
    let target = long_edge.clamp(MIN_RASTER_EDGE, MAX_RASTER_EDGE);
    // 等比缩放，短边按比例取整，至少 1 像素
    let (pw, ph) = if sw >= sh {
        (target, ((target as f32) * sh / sw).round().max(1.0) as u32)
    } else {
        (((target as f32) * sw / sh).round().max(1.0) as u32, target)
    };

    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(pw, ph).ok_or_else(|| "无法分配 SVG 渲染画布".to_string())?;
    let scale_x = pw as f32 / sw;
    let scale_y = ph as f32 / sh;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale_x, scale_y),
        &mut pixmap.as_mut(),
    );

    // tiny-skia 是预乘 alpha，直接编码 PNG 再解码可得到直通 RGBA
    let png = pixmap
        .encode_png()
        .map_err(|e| format!("SVG 输出编码失败: {e}"))?;
    crate::engine::raster::decode_bytes(&png)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRIANGLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48" viewBox="0 0 48 48">
        <rect width="48" height="48" fill="#1e88e5"/>
        <circle cx="24" cy="24" r="10" fill="#ffffff"/>
    </svg>"##;

    #[test]
    fn rasterizes_small_svg_at_high_resolution() {
        let path = std::env::temp_dir().join("icon_tool_svg_probe.svg");
        std::fs::write(&path, TRIANGLE).unwrap();
        let image = render_file(&path).expect("render svg");
        // 原始 48px，长边被抬到至少 256
        assert_eq!(image.width().max(image.height()), 256);
        assert_eq!(image.width(), image.height());
        // 中心应是白色圆点
        let center = image.get_pixel(128, 128);
        assert!(center[0] > 240 && center[1] > 240 && center[2] > 240);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_non_svg_content() {
        let path = std::env::temp_dir().join("icon_tool_svg_bad.svg");
        std::fs::write(&path, "not an svg at all").unwrap();
        assert!(render_file(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }
}
