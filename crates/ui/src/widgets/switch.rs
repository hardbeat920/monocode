//! The settings switch (SettingsView.tsx `Switch`): `h-5 w-9 rounded-full`,
//! accent when on, `content/20` when off, a white 16px thumb.

use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::{Theme, u};

type ChangeHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

pub fn switch(id: impl Into<ElementId>, on: bool) -> Switch {
    Switch {
        id: id.into(),
        on,
        disabled: false,
        on_change: None,
    }
}

impl Switch {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called with the new value.
    pub fn on_change(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let track = if self.on {
            theme.colors.accent
        } else {
            theme.content(0.20)
        };
        let on = self.on;
        let mut el = div()
            .id(self.id)
            .relative()
            .flex_none()
            .w(u(36.))
            .h(u(20.))
            .rounded_full()
            .bg(track)
            .child(
                div()
                    .absolute()
                    .top(u(2.))
                    .left(u(if on { 18. } else { 2. }))
                    .size(u(16.))
                    .rounded_full()
                    .bg(crate::color::hex(0xffffff)),
            );
        if self.disabled {
            el = el.opacity(0.4);
        } else if let Some(handler) = self.on_change {
            el = el.on_click(move |_, window, cx| handler(!on, window, cx));
        }
        el
    }
}
