//! A tooltip: what a control is, or the whole of what is cut short, shown by the pointer after a
//! moment (the browser's `title` in the Tauri version). Drawn inside the window it belongs to.

use gpui::{
    AnyView, App, AppContext, BoxShadow, Context, IntoElement, ParentElement, Render, SharedString,
    Styled, Window, div, point, px, rgba,
};

use crate::Theme;

pub struct Tip {
    text: SharedString,
}

impl Tip {
    /// The tooltip's view, for GPUI's `tooltip`.
    pub fn view(text: impl Into<SharedString>, cx: &mut App) -> AnyView {
        let text = text.into();
        cx.new(|_| Tip { text }).into()
    }
}

impl Render for Tip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx);
        div()
            .max_w(px(320.0))
            .px(px(8.0))
            .py(px(4.0))
            .rounded(theme.radius_small)
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .text_size(px(12.0))
            .shadow(vec![BoxShadow {
                color: rgba(0x0000_0040).into(),
                offset: point(px(0.0), px(2.0)),
                blur_radius: px(6.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .child(self.text.clone())
    }
}
