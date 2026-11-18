//! 图像/文件图标提取工具 —— Rust + ZSUI 原生实现。
//!
//! 架构沿用 ZSUI 的 `State → view → Msg → update`：
//! * `app.rs`  —— 状态与状态迁移（含提取、保存、复制等用例）
//! * `view.rs` —— 纯函数界面声明
//! * `engine/` —— 与界面无关的图像、ICO、PE 资源、SVG、剪贴板能力
//! * `host.rs` —— ZSUI 未覆盖的 Win32 宿主能力（文件拖放、窗口按钮、打开链接）

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod engine;
mod host;
mod model;
mod view;

use zsui::{native_window, UiInvalidationHandle, ZsuiResult};

/// 窗口标题（同时用于定位本进程主窗口）
pub(crate) const WINDOW_TITLE: &str = "图像/文件图标提取工具";
/// 固定窗口宽度
const WINDOW_WIDTH: u32 = 1000;
/// 固定窗口高度
const WINDOW_HEIGHT: u32 = 700;

/// 编译进二进制的应用图标：单文件分发时没有外部 `logo.ico` 可用
const EMBEDDED_LOGO: &[u8] = include_bytes!("../logo.ico");

fn resolve_logo() -> String {
    // 1) 便携版：图标与 exe 同目录
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("logo.ico");
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }
    // 2) 开发期：直接读工程根目录
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("logo.ico");
    if source.exists() {
        return source.to_string_lossy().to_string();
    }
    // 3) 单文件分发：把编译期嵌入的图标释放到临时目录，用完不删（下次直接复用）
    let fallback = std::env::temp_dir().join("2icon-logo.ico");
    if !fallback.exists() {
        let _ = std::fs::write(&fallback, EMBEDDED_LOGO);
    }
    fallback.to_string_lossy().to_string()
}

fn main() -> ZsuiResult<()> {
    let handle = UiInvalidationHandle::new();
    let app = app::App::new();

    // 文件拖放：回调运行在 Win32 消息线程上，同步完成载入后再请求重建。
    // 真实样本的提取耗时在 10ms 量级，不必为它引入线程。
    let drop_handle = handle.clone();
    let drop_data = app.data.clone();
    host::set_drop_handler(move |path| {
        {
            let mut data = drop_data.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(err) = app::load_input(&mut data, path) {
                data.status = format!("提取失败: {err}");
            }
        }
        drop_handle.request_rebuild();
    });

    // 启动参数：直接把文件拖到 exe 上或命令行传入时，开箱即用
    if let Some(path) = std::env::args().nth(1) {
        if std::path::Path::new(&path).exists() {
            let mut data = app.read();
            let _ = app::load_input(&mut data, path);
        }
    }

    // 窗口创建后才会出现真正的 HWND：请求一次重建，让视图里的宿主安装得以执行
    let nudge = handle.clone();
    std::thread::spawn(move || {
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(300));
            if host::is_installed() {
                return;
            }
            nudge.request_rebuild();
        }
    });

    native_window(WINDOW_TITLE)
        .app_name("icon-tool")
        .size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .min_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .resizable(false)
        .icon_path(resolve_logo())
        .invalidation_handle(handle)
        .stateful_view(app, view::view, app::update)
        .run()?;
    Ok(())
}
