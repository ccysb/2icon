//! 界面声明（ZSUI 的 `view` 纯函数：只读状态，返回声明树）。

use zsui::{
    button, checkbox, column, image_preview, radio_button, row, scroll, slider, text, textbox,
    toggle, Dp, SliderRange, ThemeColorToken, ViewAlign, ViewNode, ZsImageFit, ZsuiThemeMode,
};

use crate::app::{App, Msg};
use crate::model::{Format, PRESET_SIZES, THUMB_SIZE};

/// 网格列数（窗口宽度固定，5 列可在左栏完整排布）
const GRID_COLUMNS: usize = 4;
/// 单个格子宽度
const TILE_WIDTH: f32 = 96.0;
/// 单个格子高度（缩略图 + 两行文字 + 内边距）
const TILE_HEIGHT: f32 = 128.0;

fn preset_row(sizes: &[u32], current: u32) -> ViewNode<Msg> {
    let cells = sizes
        .iter()
        .map(|size| {
            radio_button(format!("{size}x{size}"), current == *size).on_choose(Msg::Preset(*size))
        })
        .collect::<Vec<_>>();
    row(cells).gap(Dp::new(10.0)).height(Dp::new(24.0))
}

fn tile(app: &App, index: usize) -> ViewNode<Msg> {
    let data = app.read();
    let Some(icon) = data.icons.get(index) else {
        return column(Vec::<ViewNode<Msg>>::new());
    };
    let selected = data.selected == index;
    let snapshot = data.thumb_snapshot(index);
    let caption = if selected {
        format!("● {}", icon.caption())
    } else {
        icon.caption()
    };
    drop(data);

    let mut children: Vec<ViewNode<Msg>> = Vec::new();
    if let Some(snapshot) = snapshot {
        children.push(
            image_preview(&snapshot)
                .width(Dp::new(THUMB_SIZE as f32))
                .height(Dp::new(THUMB_SIZE as f32))
                .image_fit(ZsImageFit::Contain),
        );
    }
    children.push(
        button(caption)
            .on_click(Msg::Select(index))
            .width(Dp::new(TILE_WIDTH - 12.0)),
    );

    column(children)
        .gap(Dp::new(2.0))
        .padding(Dp::new(4.0))
        .width(Dp::new(TILE_WIDTH))
        .height(Dp::new(TILE_HEIGHT))
        // 选中项用抬升色块表达，其余与页面同色（保持“无格子背景”）
        .bg(if selected {
            ThemeColorToken::SurfaceRaised
        } else {
            ThemeColorToken::Surface
        })
}

fn icon_grid(app: &App) -> ViewNode<Msg> {
    let (count, selected_index) = {
        let data = app.read();
        (data.icons.len(), data.selected)
    };

    if count == 0 {
        return column([text("尚未加载图标：可选择文件或把文件拖进窗口")])
            .padding(Dp::new(12.0))
            .flex(1.0);
    }

    let mut rows: Vec<ViewNode<Msg>> = Vec::new();
    let mut index = 0usize;
    while index < count {
        let end = (index + GRID_COLUMNS).min(count);
        let cells = (index..end).map(|i| tile(app, i)).collect::<Vec<_>>();
        rows.push(row(cells).gap(Dp::new(4.0)).height(Dp::new(TILE_HEIGHT)));
        index = end;
    }
    let row_count = rows.len();
    let content_height = (row_count as f32) * (TILE_HEIGHT + 4.0) + 8.0;

    let _ = selected_index;
    scroll(column(rows).gap(Dp::new(4.0)))
        .content_height(Dp::new(content_height))
        .flex(1.0)
}

/// 右侧控制面板
fn control_panel(app: &App) -> ViewNode<Msg> {
    let data = app.read();

    let size_rows: Vec<ViewNode<Msg>> = vec![
        preset_row(&PRESET_SIZES[..3], data.width),
        preset_row(&PRESET_SIZES[3..6], data.width),
        preset_row(&PRESET_SIZES[6..], data.width),
    ];

    let custom_input_row = row([
        text("自定义:"),
        textbox(&data.custom_w)
            .width(Dp::new(52.0))
            .on_change(Msg::CustomW),
        text("x"),
        textbox(&data.custom_h)
            .width(Dp::new(52.0))
            .on_change(Msg::CustomH),
    ])
    .gap(Dp::new(6.0))
    .height(Dp::new(28.0));

    let custom_action_row = row([
        button("应用").flex(1.0).on_click(Msg::ApplyCustom),
        button("默认").flex(1.0).on_click(Msg::ResetSize),
    ])
    .gap(Dp::new(6.0))
    .height(Dp::new(28.0));

    let preview = match data.preview_snapshot() {
        Some(snapshot) => image_preview(&snapshot)
            .width(Dp::new(120.0))
            .height(Dp::new(120.0))
            .image_fit(ZsImageFit::Contain),
        None => column([text("（无预览）")])
            .height(Dp::new(120.0))
            .width(Dp::new(120.0)),
    };

    let format_row = row(Format::ALL
        .iter()
        .map(|format| {
            radio_button(format.label(), data.format == *format).on_choose(Msg::Format(*format))
        })
        .collect::<Vec<_>>())
    .gap(Dp::new(12.0))
    .height(Dp::new(24.0));

    let quality_row = row([
        text("压缩质量"),
        slider(data.quality as f32, SliderRange::new(10.0, 100.0).step(1.0))
            .flex(1.0)
            .on_slide(Msg::Quality),
        text(format!("{}%", data.quality)),
    ])
    .gap(Dp::new(8.0))
    .height(Dp::new(24.0))
    // 提示只挂在质量这一行，避免整栏悬停都弹同一个气泡
    .tooltip(if data.format.uses_quality() {
        "有损压缩质量，数值越大越清晰"
    } else {
        "压缩质量仅对 WEBP 生效"
    });

    let info = text(data.info_text()).height(Dp::new(30.0));
    let filter = data.filter_quality;

    column([
        row([checkbox("仅显示高质量图标", filter).on_toggle(Msg::Filter)]).height(Dp::new(24.0)),
        text("输出尺寸"),
        size_rows[0].clone(),
        size_rows[1].clone(),
        size_rows[2].clone(),
        custom_input_row,
        custom_action_row,
        preview,
        text("输出格式"),
        format_row,
        quality_row,
        info,
    ])
    .gap(Dp::new(4.0))
}

/// 状态栏
fn status_bar(app: &App) -> ViewNode<Msg> {
    let data = app.read();
    let status = data.status.clone();
    let dark = data.dark;
    drop(data);

    row([
        text(status).flex(1.0),
        text("© 2025 图标提取工具"),
        text(if dark { "深色" } else { "浅色" }),
        toggle(dark).on_toggle(Msg::Theme),
        button("关于").on_click(Msg::About),
        button("GitHub ↗").on_click(Msg::Github),
    ])
    .gap(Dp::new(8.0))
    .height(Dp::new(30.0))
}

/// 顶层视图
pub fn view(app: &App) -> ViewNode<Msg> {
    // 宿主增强（接收文件拖放、去掉最小化/最大化按钮）只能在本线程安装：
    // 视图由运行时在 UI 线程调用，这里做一次幂等安装，失败则下次重试。
    crate::host::try_install(crate::WINDOW_TITLE);

    let dark = app.read().dark;

    let (input, output) = {
        let data = app.read();
        (data.input.clone(), data.output.clone())
    };

    let input_row = row([
        text("输入文件:").width(Dp::new(62.0)),
        textbox(input).flex(1.0).on_change(Msg::InputEdited),
        button("浏览")
            .width(Dp::new(68.0))
            .on_click(Msg::BrowseInput),
        button("提取").width(Dp::new(68.0)).on_click(Msg::Extract),
    ])
    .gap(Dp::new(8.0))
    .height(Dp::new(30.0));

    let output_row = row([
        text("输出文件:").width(Dp::new(62.0)),
        textbox(output).flex(1.0).on_change(Msg::OutputEdited),
        button("浏览")
            .width(Dp::new(68.0))
            .on_click(Msg::BrowseOutput),
    ])
    .gap(Dp::new(8.0))
    .height(Dp::new(30.0));

    let left = column([text("图标列表"), icon_grid(app)])
        .gap(Dp::new(4.0))
        .flex(1.0)
        .align(ViewAlign::Stretch);

    let right = column([
        control_panel(app),
        row([
            button("保存图标")
                .flex(1.0)
                .height(Dp::new(38.0))
                .on_click(Msg::Save),
            button("复制")
                .flex(1.0)
                .height(Dp::new(38.0))
                .on_click(Msg::Copy),
        ])
        .gap(Dp::new(8.0)),
    ])
    .width(Dp::new(380.0))
    .align(ViewAlign::Stretch)
    .gap(Dp::new(8.0));

    column([
        input_row,
        output_row,
        row([left, right])
            .gap(Dp::new(10.0))
            .flex(1.0)
            .align(ViewAlign::Stretch),
        status_bar(app),
    ])
    .padding(Dp::new(10.0))
    .gap(Dp::new(8.0))
    .align(ViewAlign::Stretch)
    .theme_mode(if dark {
        ZsuiThemeMode::Dark
    } else {
        ZsuiThemeMode::Light
    })
    .bg(ThemeColorToken::Surface)
}
