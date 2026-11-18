//! 应用状态与状态迁移（ZSUI 的 `State → view → Msg → update` 中的 State/update）。
//!
//! 为什么状态放在 `Arc<Mutex<..>>` 里：ZSUI 没有「从任意线程投递消息」的公开
//! 通道，而文件拖放来自 Win32 消息线程（见 `host.rs`）。框架文档给出的做法正是
//! 「后台/宿主侧改应用自有状态，然后 request_rebuild」。界面自身的交互仍然走
//! 标准消息；两条路径都收敛到同一个 `AppData`，视图只读它。

use std::sync::{Arc, Mutex};

use zsui::{
    AppCx, DialogButtons, DialogLevel, FileDialogSpec, NativeDialogSpec, SaveFileDialogSpec,
    ZsImageFrame, ZsImageFrameId, ZsImagePreviewSnapshot,
};

use crate::engine;
use crate::model::{
    self, default_output_path, retarget_extension, Format, IconEntry, SIZE_MAX, SIZE_MIN,
};

/// 界面全部可变状态
pub struct AppData {
    /// 输入文件路径
    pub input: String,
    /// 输出文件路径
    pub output: String,
    /// 状态栏文本
    pub status: String,
    /// 已提取的图标
    pub icons: Vec<IconEntry>,
    /// 每个图标对应的列表缩略图帧（与 `icons` 同序）
    pub thumbs: Vec<ZsImageFrame>,
    /// 右侧大预览帧
    pub preview: Option<ZsImageFrame>,
    /// 预览帧代数，用于让渲染层区分不同帧
    pub preview_gen: u64,
    /// 当前选中的图标下标
    pub selected: usize,
    /// 输出宽
    pub width: u32,
    /// 输出高
    pub height: u32,
    /// 输出格式
    pub format: Format,
    /// 压缩质量（仅 WEBP 生效）
    pub quality: u8,
    /// 是否只显示高质量图标
    pub filter_quality: bool,
    /// 是否深色主题
    pub dark: bool,
    /// 自定义宽度输入框内容
    pub custom_w: String,
    /// 自定义高度输入框内容
    pub custom_h: String,
}

impl AppData {
    fn new() -> Self {
        Self {
            input: String::new(),
            output: String::new(),
            status: "就绪".to_string(),
            icons: Vec::new(),
            thumbs: Vec::new(),
            preview: None,
            preview_gen: 1,
            selected: 0,
            width: 64,
            height: 64,
            format: Format::Ico,
            quality: 95,
            filter_quality: true,
            dark: true,
            custom_w: String::new(),
            custom_h: String::new(),
        }
    }

    /// 当前选中的图标
    pub fn current(&self) -> Option<&IconEntry> {
        self.icons.get(self.selected)
    }

    /// 图标信息两行文本：原始尺寸 + 位深 / 输出尺寸
    pub fn info_text(&self) -> String {
        match self.current() {
            Some(icon) => format!(
                "原始: {}x{} | {}\n输出: {}x{}",
                icon.width, icon.height, icon.bit_label, self.width, self.height
            ),
            // 空列表也给两行占位，右侧面板高度不会跟着跳动
            None => "原始: -\n输出: -".to_string(),
        }
    }

    /// 移除全部图标并清空预览
    pub fn clear_icons(&mut self) {
        self.icons.clear();
        self.thumbs.clear();
        self.preview = None;
        self.selected = 0;
    }

    /// 用新提取的图标替换列表，并把输出尺寸重置为选中图标的原始尺寸。
    pub fn set_icons(&mut self, icons: Vec<IconEntry>) {
        self.icons = icons;
        self.thumbs = self
            .icons
            .iter()
            .enumerate()
            .filter_map(|(index, icon)| {
                let rgba = icon.rgba.as_raw().clone();
                ZsImageFrame::from_rgba8(
                    ZsImageFrameId::new(index as u64 + 1),
                    icon.width,
                    icon.height,
                    rgba,
                )
                .ok()
            })
            .collect();
        self.selected = 0;
        self.reset_size_to_source();
        self.refresh_preview();
    }

    /// 把输出尺寸重置为当前图标原始尺寸（对应旧版「默认」按钮）
    pub fn reset_size_to_source(&mut self) {
        self.custom_w.clear();
        self.custom_h.clear();
        let source = self.current().map(|icon| (icon.width, icon.height));
        match source {
            Some((width, height)) => {
                self.width = width.clamp(SIZE_MIN, SIZE_MAX);
                self.height = height.clamp(SIZE_MIN, SIZE_MAX);
            }
            None => {
                self.width = 64;
                self.height = 64;
            }
        }
        self.refresh_preview();
    }

    /// 按当前输出尺寸重新生成预览帧（等比放进 150 的框由视图负责）
    pub fn refresh_preview(&mut self) {
        self.preview_gen = self.preview_gen.wrapping_add(1);
        let Some(icon) = self.current() else {
            self.preview = None;
            return;
        };
        let (w, h) = self.effective_size();
        let scaled = engine::raster::resize_to(&icon.rgba, w, h);
        self.preview = ZsImageFrame::from_rgba8(
            ZsImageFrameId::new(self.preview_gen),
            w,
            h,
            scaled.as_raw().clone(),
        )
        .ok();
    }

    /// ICO 必须是方形；这里返回实际会写入的尺寸
    pub fn effective_size(&self) -> (u32, u32) {
        let w = self.width.clamp(SIZE_MIN, SIZE_MAX);
        let h = self.height.clamp(SIZE_MIN, SIZE_MAX);
        if self.format == Format::Ico && w != h {
            let side = w.max(h);
            (side, side)
        } else {
            (w, h)
        }
    }

    /// 预览帧快照
    pub fn preview_snapshot(&self) -> Option<ZsImagePreviewSnapshot> {
        Some(ZsImagePreviewSnapshot {
            generation: self.preview_gen,
            frame: self.preview.clone(),
            loading: false,
            last_error: None,
        })
    }

    /// 缩略图快照
    pub fn thumb_snapshot(&self, index: usize) -> Option<ZsImagePreviewSnapshot> {
        let frame = self.thumbs.get(index)?.clone();
        Some(ZsImagePreviewSnapshot {
            generation: index as u64 + 1,
            frame: Some(frame),
            loading: false,
            last_error: None,
        })
    }
}

/// 应用外壳：共享状态
///
/// 重建句柄不放在这里：`stateful_view` 在 `update` 返回后自动重建，
/// 只有拖放回调与启动引导线程需要句柄，由 `main` 自己持有。
pub struct App {
    /// 共享状态（拖放回调也会写）
    pub data: Arc<Mutex<AppData>>,
}

impl App {
    /// 创建应用状态
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(AppData::new())),
        }
    }

    /// 加锁读取状态（锁内只做拷贝，避免长时间持有）
    pub fn read(&self) -> std::sync::MutexGuard<'_, AppData> {
        self.data.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 一条界面消息
#[derive(Clone)]
pub enum Msg {
    /// 无操作（对话框回调用）
    Noop,
    /// 打开输入文件对话框
    BrowseInput,
    /// 打开输出文件对话框
    BrowseOutput,
    /// 输入路径被编辑
    InputEdited(String),
    /// 输出路径被编辑
    OutputEdited(String),
    /// 输入文件已选定
    InputPicked(Option<String>),
    /// 输出文件已选定
    OutputPicked(Option<String>),
    /// 重新提取
    Extract,
    /// 选中某个图标
    Select(usize),
    /// 选择预设尺寸
    Preset(u32),
    /// 自定义宽度变化
    CustomW(String),
    /// 自定义高度变化
    CustomH(String),
    /// 应用自定义尺寸
    ApplyCustom,
    /// 恢复默认尺寸
    ResetSize,
    /// 切换输出格式
    Format(Format),
    /// 调整压缩质量
    Quality(f32),
    /// 切换质量过滤
    Filter(bool),
    /// 保存图标
    Save,
    /// 复制图标到剪贴板
    Copy,
    /// 切换主题
    Theme(bool),
    /// 关于
    About,
    /// 打开仓库
    Github,
}

fn info_dialog(cx: &mut AppCx, title: &str, message: String) {
    cx.show_dialog(
        NativeDialogSpec::message(title, message)
            .level(DialogLevel::Info)
            .buttons(DialogButtons::Ok),
        |_| Msg::Noop,
    );
}

fn error_dialog(cx: &mut AppCx, title: &str, message: String) {
    cx.show_dialog(
        NativeDialogSpec::message(title, message)
            .level(DialogLevel::Error)
            .buttons(DialogButtons::Ok),
        |_| Msg::Noop,
    );
}

/// 载入输入文件并刷新图标列表（浏览与拖放共用）
pub fn load_input(data: &mut AppData, path: String) -> Result<(), String> {
    if !crate::model::is_supported_input(&path) {
        let message = format!("不支持的文件类型，请选择图像、EXE/DLL 或快捷方式: {path}");
        data.status = message.clone();
        return Err(message);
    }
    data.input = path.clone();
    data.output = default_output_path(&path, data.format);
    data.clear_icons();
    data.status = "提取中...".to_string();
    match engine::load_icons(&path, data.filter_quality) {
        Ok(icons) => {
            let count = icons.len();
            data.set_icons(icons);
            data.status = if count == 0 {
                // 过滤门槛挡光了：不报错，直接告诉用户怎么看到全部
                format!(
                    "没有 {}x{} 及以上的图标 | 可取消勾选「仅显示高质量图标」",
                    model::HIGH_QUALITY_MIN,
                    model::HIGH_QUALITY_MIN
                )
            } else {
                format!("{count}个图标 | 完成")
            };
            Ok(())
        }
        Err(err) => {
            data.status = format!("提取失败: {err}");
            Err(err)
        }
    }
}

/// 保存当前图标到输出路径，返回给用户看的说明
fn save_current(data: &mut AppData) -> Result<String, String> {
    let Some(icon) = data.current().cloned() else {
        return Err("没有可保存的图标".to_string());
    };
    let mut path = data.output.clone();
    if path.trim().is_empty() {
        return Err("请选择输出文件".to_string());
    }
    // 扩展名必须与当前格式一致
    path = retarget_extension(&path, data.format);
    data.output = path.clone();
    let (w, h) = data.effective_size();

    match data.format {
        Format::Ico => {
            let written = engine::ico::save_multisize(std::path::Path::new(&path), &icon.rgba, w)
                .map_err(|e| format!("ICO 保存失败: {e}"))?;
            let sizes = engine::ico::read_sizes(std::path::Path::new(&path)).unwrap_or(written);
            let mut detail = String::new();
            if !sizes.is_empty() {
                detail.push_str("\n包含尺寸: ");
                detail.push_str(
                    &sizes
                        .iter()
                        .map(|s| format!("{s}x{s}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
            if w > model::ICO_SIZE_LIMIT {
                detail.push_str(&format!(
                    "\n（ICO 单帧上限 {}，大图已按 {} 输出）",
                    model::ICO_SIZE_LIMIT,
                    model::ICO_SIZE_LIMIT
                ));
            }
            Ok(format!("图标已成功保存到:\n{path}{detail}"))
        }
        Format::Png => {
            let image = engine::raster::resize_to(&icon.rgba, w, h);
            engine::raster::save_png(std::path::Path::new(&path), &image)?;
            Ok(format!("图标已成功保存到:\n{path}"))
        }
        Format::Webp => {
            let image = engine::raster::resize_to(&icon.rgba, w, h);
            engine::raster::save_webp(std::path::Path::new(&path), &image, data.quality)?;
            Ok(format!(
                "图标已成功保存到:\n{path}\n压缩质量: {}%",
                data.quality
            ))
        }
    }
}

/// 复制当前图标到剪贴板（CF_DIB，可直接粘到画图/PS）
fn copy_current(data: &AppData) -> Result<(), String> {
    let Some(icon) = data.current() else {
        return Err("没有可复制的图标".to_string());
    };
    let (w, h) = data.effective_size();
    let image = engine::raster::resize_to(&icon.rgba, w, h);
    let dib = engine::raster::to_dib(&image);
    crate::engine::clipboard::set_dib(&dib)
}

/// 状态迁移
pub fn update(app: &mut App, message: Msg, cx: &mut AppCx) {
    let mut data = app.read();
    match message {
        Msg::Noop => {}
        Msg::BrowseInput => {
            let spec = FileDialogSpec::new("选择图像或可执行文件")
                .filter(
                    "所有支持的文件",
                    [
                        "*.png", "*.jpg", "*.jpeg", "*.webp", "*.svg", "*.exe", "*.dll", "*.lnk",
                    ],
                )
                .filter("图像文件", ["*.png", "*.jpg", "*.jpeg", "*.webp", "*.svg"])
                .filter("Windows 可执行文件", ["*.exe", "*.dll"])
                .filter("快捷方式", ["*.lnk"])
                .filter("所有文件", ["*.*"]);
            cx.open_file_dialog(spec, |result| {
                Msg::InputPicked(extract_first_path(result.ok().flatten()))
            });
        }
        Msg::BrowseOutput => {
            let mut spec = SaveFileDialogSpec::new("选择输出文件");
            if !data.output.is_empty() {
                spec = spec.current_path(data.output.clone());
            }
            spec = spec
                .filter("图标文件", [format!("*.{}", data.format.extension())])
                .filter("所有文件", ["*.*"]);
            cx.save_file_dialog(spec, |result| {
                Msg::OutputPicked(
                    result
                        .ok()
                        .flatten()
                        .map(|p| p.to_string_lossy().to_string()),
                )
            });
        }
        Msg::InputEdited(value) => {
            data.input = value;
        }
        Msg::OutputEdited(value) => {
            data.output = value;
        }
        Msg::InputPicked(Some(path)) => {
            if let Err(err) = load_input(&mut data, path) {
                drop(data);
                error_dialog(cx, "错误", format!("提取过程中出现错误:\n{err}"));
            }
        }
        Msg::InputPicked(None) => {}
        Msg::OutputPicked(Some(path)) => {
            data.output = path;
        }
        Msg::OutputPicked(None) => {}
        Msg::Extract => {
            let input = data.input.clone();
            if input.trim().is_empty() {
                drop(data);
                error_dialog(cx, "错误", "请选择输入文件".to_string());
                return;
            }
            if let Err(err) = load_input(&mut data, input) {
                drop(data);
                error_dialog(cx, "错误", format!("提取过程中出现错误:\n{err}"));
            }
        }
        Msg::Select(index) => {
            if index < data.icons.len() {
                data.selected = index;
                data.reset_size_to_source();
            }
        }
        Msg::Preset(size) => {
            data.custom_w.clear();
            data.custom_h.clear();
            data.width = size.clamp(SIZE_MIN, SIZE_MAX);
            data.height = size.clamp(SIZE_MIN, SIZE_MAX);
            data.refresh_preview();
        }
        Msg::CustomW(value) => {
            data.custom_w = value;
        }
        Msg::CustomH(value) => {
            data.custom_h = value;
        }
        Msg::ApplyCustom => {
            let w = data.custom_w.trim().parse::<u32>();
            let h = data.custom_h.trim().parse::<u32>();
            match (w, h) {
                (Ok(w), Ok(h))
                    if (SIZE_MIN..=SIZE_MAX).contains(&w) && (SIZE_MIN..=SIZE_MAX).contains(&h) =>
                {
                    data.width = w;
                    data.height = h;
                    data.refresh_preview();
                }
                (Ok(w), Ok(h)) if w > 0 && h > 0 => {
                    drop(data);
                    error_dialog(cx, "错误", format!("尺寸必须在 {SIZE_MIN}-{SIZE_MAX} 之间"));
                }
                _ => {
                    drop(data);
                    error_dialog(cx, "错误", "请输入有效的数字".to_string());
                }
            }
        }
        Msg::ResetSize => {
            data.reset_size_to_source();
        }
        Msg::Format(format) => {
            data.format = format;
            data.output = retarget_extension(&data.output, format);
            data.refresh_preview();
        }
        Msg::Quality(value) => {
            data.quality = value.round().clamp(1.0, 100.0) as u8;
        }
        Msg::Filter(checked) => {
            data.filter_quality = checked;
            let input = data.input.clone();
            if !input.is_empty() && std::path::Path::new(&input).exists() {
                if let Err(err) = load_input(&mut data, input) {
                    data.status = format!("提取失败: {err}");
                }
            }
        }
        Msg::Save => match save_current(&mut data) {
            Ok(message) => {
                data.status = format!("保存完成: {}", data.output);
                drop(data);
                info_dialog(cx, "成功", message);
            }
            Err(err) => {
                data.status = format!("保存失败: {err}");
                drop(data);
                error_dialog(cx, "错误", format!("保存过程中出现错误:\n{err}"));
            }
        },
        Msg::Copy => match copy_current(&data) {
            Ok(()) => {
                data.status = "已复制图像到剪贴板".to_string();
                drop(data);
                info_dialog(
                    cx,
                    "成功",
                    "图标图像已复制到剪贴板，可粘贴到画图等程序".to_string(),
                );
            }
            Err(err) => {
                data.status = format!("复制失败: {err}");
                drop(data);
                error_dialog(cx, "错误", format!("复制失败:\n{err}"));
            }
        },
        Msg::Theme(dark) => {
            data.dark = dark;
        }
        Msg::About => {
            drop(data);
            info_dialog(cx, "关于", about_text());
        }
        Msg::Github => {
            crate::host::open_url(model::GITHUB_REPO_URL);
            data.status = "已在浏览器中打开项目主页".to_string();
        }
    }
}

fn extract_first_path(paths: Option<Vec<std::path::PathBuf>>) -> Option<String> {
    paths
        .and_then(|list| list.into_iter().next())
        .map(|p| p.to_string_lossy().to_string())
}

fn about_text() -> String {
    format!(
        "图像/文件图标提取工具 v2.0（Rust + ZSUI）\n\
         功能特性：\n\
         • 支持图像格式 (PNG, JPG, WEBP, SVG)\n\
         • 从可执行文件提取图标 (EXE, DLL)\n\
         • 从快捷方式提取图标 (LNK)\n\
         • 支持文件拖放\n\
         • 智能图标质量排序与过滤\n\
         • 多尺寸 ICO（256/128/96/64/48/32/24/16）\n\
         • PNG / WEBP 输出，可调压缩质量\n\
         • 一键复制图像到剪贴板\n\
         • 明暗主题切换\n\n\
         项目地址：{}\n\n\
         界面基于 ZSUI（State → view → Msg → update）构建",
        model::GITHUB_REPO_URL
    )
}

/// 供视图使用的浅色/深色色板（避免视图层直接写死颜色）
#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> AppData {
        AppData::new()
    }

    fn sample_icon(size: u32) -> IconEntry {
        let image = image::RgbaImage::from_pixel(size, size, image::Rgba([9, 9, 9, 255]));
        crate::engine::entry_from_image(image, "32位RGBA".to_string())
    }

    #[test]
    fn set_icons_selects_first_and_adopts_source_size() {
        let mut data = blank();
        data.set_icons(vec![sample_icon(48), sample_icon(16)]);
        assert_eq!(data.selected, 0);
        assert_eq!((data.width, data.height), (48, 48));
        assert_eq!(data.thumbs.len(), 2);
        assert!(data.preview.is_some());
    }

    #[test]
    fn ico_forces_square_output() {
        let mut data = blank();
        data.format = Format::Ico;
        data.width = 256;
        data.height = 128;
        assert_eq!(data.effective_size(), (256, 256));
        data.format = Format::Png;
        assert_eq!(data.effective_size(), (256, 128));
    }

    #[test]
    fn format_switch_retargets_output_extension() {
        let mut data = blank();
        data.output = r"C:\icons\logo.ico".to_string();
        data.format = Format::Webp;
        data.output = retarget_extension(&data.output, data.format);
        assert!(data.output.ends_with(".webp"));
    }

    #[test]
    fn save_writes_multisize_ico_and_reports_sizes() {
        let dir = std::env::temp_dir().join("icon_tool_save_case.ico");
        let mut data = blank();
        data.set_icons(vec![sample_icon(256)]);
        data.output = dir.to_string_lossy().to_string();
        data.format = Format::Ico;
        let message = save_current(&mut data).expect("save ico");
        assert!(message.contains("256x256"));
        let sizes = crate::engine::ico::read_sizes(&dir).unwrap();
        assert_eq!(sizes, vec![256, 128, 96, 64, 48, 32, 24, 16]);
        let _ = std::fs::remove_file(&dir);
    }
}
