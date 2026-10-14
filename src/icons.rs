// BeeK 图标资源 — 编译期内嵌
//
// 为什么内嵌而不是运行时读文件：
// 1. gpui 0.2.2 里 img("字符串") 会被 is_uri() 判成 URI 走 HTTP，img(PathBuf) 又走异步加载，
//    首帧空白、要靠 notify 补第二帧；Image::from_bytes → to_image_data 是同步的，首帧就出图。
// 2. 内嵌后开发态与 .app 打包态行为完全一致，不依赖 Contents/Resources 路径解析。

use gpui::*;
use std::sync::{Arc, OnceLock};

/// 菜单栏 icon（36x36 @2x）。素材是纯黑 + alpha 的 template 图，
/// 交给 AppKit 按菜单栏明暗自动着色，因此不需要深浅两套。
pub const MENUBAR_TEMPLATE_PNG: &[u8] =
    include_bytes!("../resources/icons/BeeKMenuBarTemplate@2x.png");

/// 菜单栏图标的逻辑尺寸（pt）。素材 36px，按 18pt 绘制正好 2x。
pub const MENUBAR_ICON_PT: f64 = 18.0;

/// 悬浮按钮素材的设计尺寸（pt）：素材 64px，原本按 32pt 绘制（2x 屏 1:1）。
/// 现在的实际绘制尺寸由悬浮窗口决定（见 main.rs 的 ICON_SIZE / ICON_ZOOM）。
pub const SELECTION_ICON_PT: f32 = 32.0;

/// 悬浮按钮的四种状态，素材均为 64x64（@2x），按 32pt 逻辑尺寸绘制。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconKind {
    /// 默认：金色圆形徽标
    Default,
    /// 悬停
    Hover,
    /// 按下
    Pressed,
    /// 翻译中（不可再点）
    Disabled,
}

impl IconKind {
    fn bytes(self) -> &'static [u8] {
        match self {
            IconKind::Default => {
                include_bytes!("../resources/icons/beek-selection-default-32@2x.png")
            }
            IconKind::Hover => include_bytes!("../resources/icons/beek-selection-hover-32@2x.png"),
            IconKind::Pressed => {
                include_bytes!("../resources/icons/beek-selection-pressed-32@2x.png")
            }
            IconKind::Disabled => {
                include_bytes!("../resources/icons/beek-selection-disabled-32@2x.png")
            }
        }
    }

    fn slot(self) -> &'static OnceLock<Arc<RenderImage>> {
        static SLOTS: [OnceLock<Arc<RenderImage>>; 4] = [
            OnceLock::new(),
            OnceLock::new(),
            OnceLock::new(),
            OnceLock::new(),
        ];
        &SLOTS[match self {
            IconKind::Default => 0,
            IconKind::Hover => 1,
            IconKind::Pressed => 2,
            IconKind::Disabled => 3,
        }]
    }

    /// 同步解码成位图，每个状态只解一次。返回 ImageSource::Render 变体，
    /// 避免异步加载导致的首帧空白。
    pub fn render_image(self, cx: &App) -> Arc<RenderImage> {
        self.slot()
            .get_or_init(|| {
                let image = Image::from_bytes(ImageFormat::Png, self.bytes().to_vec())
                    .to_image_data(cx.svg_renderer())
                    .expect("内嵌划词图标解码失败");
                let sz = image.size(0);
                eprintln!(
                    "[beek] 内嵌图标解码完成：{:?} = {}x{} px（素材按 {}pt 设计）",
                    self, sz.width.0, sz.height.0, SELECTION_ICON_PT
                );
                image
            })
            .clone()
    }
}
