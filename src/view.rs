//! 界面声明（ZSUI 的 `view` 纯函数：只读状态，返回声明树）。

use zsui::{
    button, canvas, checkbox, column, image_preview, radio_button, row, scroll, slider, text,
    textbox, toggle, ColorRole, Dp, HorizontalAlign, NativeDrawFill, SemanticTextStyle,
    SliderRange, ThemeColorToken, ViewAlign, ViewJustify, ViewNode, ZsCanvasPrimitive,
    ZsCanvasRect, ZsCanvasScene, ZsImageFit, ZsuiThemeMode,
};

use crate::app::{App, Msg};
use crate::model::{Format, PRESET_SIZES, THUMB_SIZE};

/// 网格列数（窗口宽度固定，4 列可在左栏完整排布）
const GRID_COLUMNS: usize = 4;
/// 单个格子宽度
const TILE_WIDTH: f32 = 96.0;
/// 单个格子高度（缩略图 + 两行文字 + 内边距）
const TILE_HEIGHT: f32 = 128.0;
/// 右侧面板的目标宽度。
///
/// 实际绘制宽度取「目标宽度」与「行内控件最小宽度」的较大值（当前约 364dp），
/// 想进一步收窄得先把行内的最小宽度继续压小。
const PANEL_WIDTH: f32 = 344.0;
/// 自定义尺寸输入框宽度（剩余宽度留给两个按钮）
const CUSTOM_BOX_WIDTH: f32 = 54.0;
/// 预览图上下留白：预览框本身是方的，紧贴相邻行会显得挤
const PREVIEW_PADDING: f32 = 14.0;

fn preset_row(sizes: &[u32], current: u32) -> ViewNode<Msg> {
    let cells = sizes
        .iter()
        .map(|size| {
            radio_button(format!("{size}x{size}"), current == *size)
                // radio 自带 120dp 最小宽度，不清掉四个就会把整栏顶宽；
                // 清掉后交给 flex 均分，正好铺满收窄后的面板
                .min_width(Dp::new(0.0))
                .flex(1.0)
                .on_choose(Msg::Preset(*size))
        })
        .collect::<Vec<_>>();
    row(cells).gap(Dp::new(6.0)).height(Dp::new(26.0))
}

fn tile(app: &App, index: usize) -> ViewNode<Msg> {
    let data = app.read();
    let Some(icon) = data.icons.get(index) else {
        return column(Vec::<ViewNode<Msg>>::new());
    };
    let selected = data.selected == index;
    let snapshot = data.thumb_snapshot(index);
    let size_text = icon.size_text();
    let bit_label = icon.bit_label.clone();
    drop(data);

    // 说明文字直接画进 scene：`text` 节点会拿到自动 WidgetId 并抢走命中，
    // 点文字就选不中，所以格子内部只保留 image_preview 这一个子节点。
    let mut size_style = SemanticTextStyle::body();
    size_style.horizontal_align = HorizontalAlign::Center;
    let mut bit_style = size_style;
    bit_style.color = ColorRole::SecondaryText;
    let caption_width = TILE_WIDTH - 4.0;

    let mut scene = ZsCanvasScene::new();
    if selected {
        // 选中态用强调色薄片打底，深浅两套主题下都能看清
        scene.push(ZsCanvasPrimitive::round_fill(
            ZsCanvasRect::new(
                Dp::new(0.0),
                Dp::new(0.0),
                Dp::new(TILE_WIDTH),
                Dp::new(TILE_HEIGHT),
            ),
            NativeDrawFill::RoleWithAlpha {
                role: ColorRole::Accent,
                alpha: 48,
            },
            Dp::new(8.0),
        ));
    }
    let scene = scene
        .with(ZsCanvasPrimitive::text(
            size_text,
            ZsCanvasRect::new(
                Dp::new(2.0),
                Dp::new(82.0),
                Dp::new(caption_width),
                Dp::new(16.0),
            ),
            size_style,
        ))
        .with(ZsCanvasPrimitive::text(
            bit_label,
            ZsCanvasRect::new(
                Dp::new(2.0),
                Dp::new(99.0),
                Dp::new(caption_width),
                Dp::new(16.0),
            ),
            bit_style,
        ));

    let mut content: Vec<ViewNode<Msg>> = Vec::new();
    if let Some(snapshot) = snapshot {
        content.push(
            image_preview(&snapshot)
                .width(Dp::new(THUMB_SIZE as f32))
                .height(Dp::new(THUMB_SIZE as f32))
                .image_fit(ZsImageFit::Contain),
        );
    }

    // 整块可点：ZSUI 的 `on_click` 只对 Button 与 Canvas 生效，而 Button 自带
    // 平台 bezel 会盖住缩略图。这里用 Canvas 当命中层，缩略图作为它的子节点
    // 叠在上层，于是点图标或点文字都能选中。
    canvas(scene)
        .on_click(Msg::Select(index))
        .width(Dp::new(TILE_WIDTH))
        .height(Dp::new(TILE_HEIGHT))
        // 底色与页面一致，保持“无格子背景”；选中态由 scene 自己画
        .bg(ThemeColorToken::Surface)
        .children([column(content)
            .padding(Dp::new(6.0))
            .align(ViewAlign::Center)
            .justify(ViewJustify::Start)])
}

fn icon_grid(app: &App) -> ViewNode<Msg> {
    let count = {
        let data = app.read();
        data.icons.len()
    };

    if count == 0 {
        return column([text("尚未加载图标：可选择文件或把文件拖进窗口")])
            .padding(Dp::new(12.0))
            .align(ViewAlign::Center)
            .justify(ViewJustify::Center)
            .flex(1.0);
    }

    let mut rows: Vec<ViewNode<Msg>> = Vec::new();
    let mut index = 0usize;
    while index < count {
        let end = (index + GRID_COLUMNS).min(count);
        let cells = (index..end).map(|i| tile(app, i)).collect::<Vec<_>>();
        rows.push(
            row(cells)
                .gap(Dp::new(6.0))
                .height(Dp::new(TILE_HEIGHT))
                .justify(ViewJustify::Center),
        );
        index = end;
    }
    let row_count = rows.len();
    let content_height = (row_count as f32) * (TILE_HEIGHT + 6.0) + 8.0;

    scroll(column(rows).gap(Dp::new(6.0)))
        .content_height(Dp::new(content_height))
        .flex(1.0)
}

/// 右侧控制面板
fn control_panel(app: &App) -> ViewNode<Msg> {
    let data = app.read();

    // 尺寸单选按参考布局排成 4 列 2 行
    let size_rows: Vec<ViewNode<Msg>> = vec![
        preset_row(&PRESET_SIZES[..4], data.width),
        preset_row(&PRESET_SIZES[4..], data.width),
    ];

    // 标签、输入框、应用、默认收在同一行；输入框用固定窄宽，
    // 否则文本框的默认最小宽度会把整栏顶开
    let custom_row = row([
        text("自定义尺寸:").width(Dp::new(96.0)),
        textbox(&data.custom_w)
            .width(Dp::new(CUSTOM_BOX_WIDTH))
            .min_width(Dp::new(0.0))
            .on_change(Msg::CustomW),
        text("x"),
        textbox(&data.custom_h)
            .width(Dp::new(CUSTOM_BOX_WIDTH))
            .min_width(Dp::new(0.0))
            .on_change(Msg::CustomH),
        button("应用")
            .width(Dp::new(48.0))
            .min_width(Dp::new(0.0))
            .on_click(Msg::ApplyCustom),
        button("默认")
            .width(Dp::new(48.0))
            .min_width(Dp::new(0.0))
            .on_click(Msg::ResetSize),
    ])
    .gap(Dp::new(6.0))
    .height(Dp::new(28.0));

    let preview_size = 140.0;
    let preview = match data.preview_snapshot() {
        Some(snapshot) => image_preview(&snapshot)
            .width(Dp::new(preview_size))
            .height(Dp::new(preview_size))
            .image_fit(ZsImageFit::Contain),
        None => column([text("（无预览）")])
            .height(Dp::new(preview_size))
            .width(Dp::new(preview_size))
            .align(ViewAlign::Center)
            .justify(ViewJustify::Center),
    };
    // 预览框是固定方框，外面包一层 row 才能在这一栏里水平居中
    let preview_row = row([preview])
        .justify(ViewJustify::Center)
        .height(Dp::new(preview_size));
    // 上下各垫一段空白：预览框紧贴相邻行会显得挤
    let preview_gap = || canvas(ZsCanvasScene::new()).height(Dp::new(PREVIEW_PADDING));

    let format_row = row(Format::ALL
        .iter()
        .map(|format| {
            radio_button(format.label(), data.format == *format)
                // 同预设：不给固定宽度就会被 120dp 的最小宽度撑开
                .min_width(Dp::new(0.0))
                .flex(1.0)
                .on_choose(Msg::Format(*format))
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
    let filter_label = format!("仅显示高质量图标（≥{}px）", crate::model::HIGH_QUALITY_MIN);

    column([
        row([checkbox(filter_label, filter).on_toggle(Msg::Filter)]).height(Dp::new(24.0)),
        text("输出尺寸"),
        size_rows[0].clone(),
        size_rows[1].clone(),
        custom_row,
        preview_gap(),
        preview_row,
        preview_gap(),
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
    .width(Dp::new(PANEL_WIDTH))
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
