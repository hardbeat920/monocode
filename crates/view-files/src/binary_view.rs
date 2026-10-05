//! Port of src/features/files/ui/BinaryFileView.tsx: the read-only surface
//! for files the editor does not open. Images and PDFs render in
//! `monocode_editor::ImageView` and `PdfView`; other bytes get a card that
//! points at the file on disk.

use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, InteractiveElement,
    IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Subscription, Task,
    WeakEntity, Window, div,
};
use monocode_editor::viewer::{format_file_size, is_pdf_bytes, sniff_image_mime};
use monocode_editor::{ImageView, PdfView};
use monocode_ui::{IconName, Theme, UiStyled as _, file_type_icon, icon, u};

use crate::data::FilesData;
use crate::file_editor::DISK_RELOAD_DELAY;
use crate::paths::{basename, display_path};

/// `LoadState`.
pub enum BinaryState {
    Loading,
    Image(Entity<ImageView>),
    Pdf(Entity<PdfView>),
    Unsupported { size: u64 },
    Error(String),
}

/// The image, PDF, or file card of one tab.
pub struct BinaryFileSurface {
    data: Rc<dyn FilesData>,
    path: String,
    cwd: String,
    state: BinaryState,
    generation: u64,
    load: Option<Task<()>>,
    reload_timer: Option<Task<()>>,
    _watch: Subscription,
    _appearance: Subscription,
}

impl BinaryFileSurface {
    pub fn new(
        data: Rc<dyn FilesData>,
        path: impl Into<String>,
        cwd: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let path = path.into();
        let weak: WeakEntity<Self> = cx.entity().downgrade();
        let watch = data.watch_file(
            &path,
            Box::new(move |cx| {
                weak.update(cx, |this, cx| this.schedule_reload(cx)).ok();
            }),
            cx,
        );
        let appearance = cx.observe_global::<Theme>(|this, cx| {
            let theme = crate::editor_theme(cx);
            match &this.state {
                BinaryState::Image(image) => {
                    image.update(cx, |image, cx| image.set_theme(theme, cx));
                }
                BinaryState::Pdf(pdf) => {
                    pdf.update(cx, |pdf, cx| pdf.set_theme(theme, cx));
                }
                _ => {}
            }
            cx.notify();
        });
        let mut this = Self {
            data,
            path,
            cwd: cwd.into(),
            state: BinaryState::Loading,
            generation: 0,
            load: None,
            reload_timer: None,
            _watch: watch,
            _appearance: appearance,
        };
        this.reload(cx);
        this
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn state(&self) -> &BinaryState {
        &self.state
    }

    /// Read the file again (`reloadKey`).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.state = BinaryState::Loading;
        let read = self.data.read_binary_file(&self.path, cx);
        self.load = Some(cx.spawn(async move |this, cx| {
            let result = read.await;
            this.update(cx, |this, cx| {
                if generation != this.generation {
                    return;
                }
                let theme = crate::editor_theme(cx);
                this.state = match result {
                    Err(message) => BinaryState::Error(message),
                    Ok(bytes) if is_pdf_bytes(&bytes) => {
                        BinaryState::Pdf(cx.new(|cx| PdfView::new(bytes, theme, cx)))
                    }
                    // The type comes from the bytes, never the extension.
                    Ok(bytes) if sniff_image_mime(&bytes).is_some() => {
                        let path = this.path.clone();
                        BinaryState::Image(cx.new(|_| ImageView::new(path, bytes, theme)))
                    }
                    Ok(bytes) => BinaryState::Unsupported {
                        size: bytes.len() as u64,
                    },
                };
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn schedule_reload(&mut self, cx: &mut Context<Self>) {
        let timer = cx.background_executor().timer(DISK_RELOAD_DELAY);
        self.reload_timer = Some(cx.spawn(async move |this, cx| {
            timer.await;
            this.update(cx, |this, cx| this.reload(cx)).ok();
        }));
    }

    /// `FileCard`.
    fn render_card(
        &self,
        title: String,
        detail: String,
        card_icon: AnyElement,
        retry: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let button = |id: &'static str| {
            div()
                .id(id)
                .flex()
                .h(u(28.))
                .items_center()
                .gap(u(6.))
                .rounded(u(theme.radius.md))
                .bg(theme.content(0.10))
                .px(u(10.))
                .text_px(theme.text.label)
                .text_color(theme.colors.content)
                .hover(|style| style.bg(theme.content(0.15)))
        };
        let path = self.path.clone();
        let reveal_path = self.path.clone();
        let mut buttons = div()
            .mt(u(16.))
            .flex()
            .items_center()
            .justify_center()
            .gap(u(8.));
        if retry {
            buttons = buttons.child(
                button("retry")
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx)))
                    .child(
                        icon(IconName::RotateCcw)
                            .size(u(12.))
                            .text_color(theme.colors.content),
                    )
                    .child("Retry"),
            );
        }
        buttons = buttons
            .child(
                button("reveal")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.data.reveal_path(&reveal_path, cx).detach();
                    }))
                    .child(
                        icon(IconName::Folder)
                            .size(u(12.))
                            .text_color(theme.colors.content),
                    )
                    .child("Reveal"),
            )
            .child(
                button("copy-path")
                    .on_click(move |_, _, cx: &mut App| {
                        cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
                    })
                    .child("Copy path"),
            );
        div()
            .flex()
            .size_full()
            .items_center()
            .justify_center()
            .p(u(24.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .max_w(u(448.))
                    .child(card_icon)
                    .child(
                        div()
                            .text_px(theme.text.body)
                            .text_color(theme.colors.content)
                            .child(title),
                    )
                    .child(
                        div()
                            .mt(u(4.))
                            .text_px(theme.text.label)
                            .line_height(u(20.))
                            .text_color(theme.content(0.50))
                            .child(detail),
                    )
                    .child(
                        div()
                            .mt(u(4.))
                            .max_w_full()
                            .truncate()
                            .font_family(theme.fonts.mono.clone())
                            .text_px(theme.text.caption)
                            .text_color(theme.content(0.35))
                            .child(display_path(&self.path, Some(&self.cwd))),
                    )
                    .child(buttons),
            )
            .into_any_element()
    }
}

impl Render for BinaryFileSurface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let name = basename(&self.path);
        let error_icon = || {
            icon(IconName::AlertCircle)
                .size(u(20.))
                .mb(u(12.))
                .text_color(theme.colors.danger)
                .into_any_element()
        };
        let body = match &self.state {
            BinaryState::Loading => div()
                .flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_px(theme.text.label)
                .text_color(theme.content(0.45))
                .child(format!("Opening {name}…"))
                .into_any_element(),
            BinaryState::Error(message) => {
                let message = message.clone();
                self.render_card(
                    format!("Couldn’t open {name}"),
                    message,
                    error_icon(),
                    true,
                    cx,
                )
            }
            BinaryState::Unsupported { size } => {
                let detail = format!("{} · not a readable image or PDF", format_file_size(*size));
                let card_icon = div()
                    .mb(u(12.))
                    .flex()
                    .justify_center()
                    .child(file_type_icon(name.clone()).size(28.))
                    .into_any_element();
                self.render_card(name.clone(), detail, card_icon, false, cx)
            }
            BinaryState::Pdf(pdf) => match pdf.read(cx).error().map(str::to_string) {
                Some(message) => self.render_card(
                    format!("Couldn’t open {name}"),
                    message,
                    error_icon(),
                    true,
                    cx,
                ),
                None => pdf.clone().into_any_element(),
            },
            BinaryState::Image(image) => image.clone().into_any_element(),
        };
        div().size_full().min_h_0().child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeFiles;
    use gpui::TestAppContext;
    use monocode_editor::viewer::Zoom;
    use monocode_ui::{AppearanceSettings, ThemePreference, set_appearance};

    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0,
        0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0,
    ];

    fn mount(
        path: &str,
        bytes: Option<Vec<u8>>,
        cx: &mut TestAppContext,
    ) -> (Rc<FakeFiles>, Entity<BinaryFileSurface>) {
        cx.update(crate::test_support::init);
        cx.update(|cx| {
            set_appearance(
                AppearanceSettings {
                    theme_preference: ThemePreference::Dark,
                    ..Default::default()
                },
                cx,
            );
        });
        let fs = FakeFiles::new();
        if let Some(bytes) = bytes {
            fs.state.borrow_mut().binary.insert(path.into(), bytes);
        }
        let data: Rc<dyn FilesData> = fs.clone();
        let path = path.to_string();
        let surface = cx.new(|cx| BinaryFileSurface::new(data, path, "/repo", cx));
        cx.run_until_parked();
        (fs, surface)
    }

    fn switch_to_light(cx: &mut TestAppContext) {
        cx.update(|cx| {
            set_appearance(
                AppearanceSettings {
                    theme_preference: ThemePreference::Light,
                    accent_color: Some("#cc5500".into()),
                    ..Default::default()
                },
                cx,
            );
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    fn cached_image_follows_appearance_without_resetting_zoom(cx: &mut TestAppContext) {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            2,
            1,
            image::Rgba([255, 0, 0, 255]),
        ))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
        let bytes = png.into_inner();
        let (fs, surface) = mount("/repo/logo.png", Some(bytes.clone()), cx);
        let image = surface.read_with(cx, |surface, _| match surface.state() {
            BinaryState::Image(image) => image.clone(),
            _ => panic!("the PNG must open in an image viewer"),
        });
        image.update(cx, |image, cx| image.set_zoom(Zoom::Scale(2.5), cx));
        let original = image.read_with(cx, |image, _| image.theme().clone());
        switch_to_light(cx);
        surface.read_with(cx, |surface, cx| {
            let BinaryState::Image(cached) = surface.state() else {
                panic!("an appearance change must retain the loaded image");
            };
            assert_eq!(cached.entity_id(), image.entity_id());
            assert_eq!(cached.read(cx).theme(), &crate::editor_theme(cx));
            assert_ne!(cached.read(cx).theme(), &original);
            assert_eq!(cached.read(cx).zoom(), Zoom::Scale(2.5));
        });
        assert_eq!(fs.state.borrow().binary["/repo/logo.png"], bytes);
    }

    fn two_page_pdf() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Resources << >> >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Resources << >> >>",
        ];
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = bytes.len();
        bytes.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in offsets {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        bytes
    }

    #[gpui::test]
    fn cached_pdf_follows_appearance_without_reopening_the_document(cx: &mut TestAppContext) {
        let bytes = two_page_pdf();
        let (fs, surface) = mount("/repo/document.pdf", Some(bytes.clone()), cx);
        let pdf = surface.read_with(cx, |surface, _| match surface.state() {
            BinaryState::Pdf(pdf) => pdf.clone(),
            _ => panic!("the PDF must open in a PDF viewer"),
        });
        pdf.update(cx, |pdf, cx| pdf.set_zoom(Zoom::Scale(1.75), cx));
        let (original, current_page) = pdf.read_with(cx, |pdf, _| {
            assert!(pdf.is_ready(), "the controlled PDF must load");
            assert_eq!(pdf.page_count(), 2);
            (pdf.theme().clone(), pdf.current_page())
        });
        switch_to_light(cx);
        surface.read_with(cx, |surface, cx| {
            let BinaryState::Pdf(cached) = surface.state() else {
                panic!("an appearance change must retain the loaded PDF");
            };
            assert_eq!(cached.entity_id(), pdf.entity_id());
            let cached = cached.read(cx);
            assert_eq!(cached.theme(), &crate::editor_theme(cx));
            assert_ne!(cached.theme(), &original);
            assert!(cached.is_ready());
            assert_eq!(cached.page_count(), 2);
            assert_eq!(cached.current_page(), current_page);
            assert_eq!(cached.scale(), 1.75);
        });
        assert_eq!(fs.state.borrow().binary["/repo/document.pdf"], bytes);
    }

    #[gpui::test]
    fn picks_the_viewer_from_the_bytes(cx: &mut TestAppContext) {
        let (_, image) = mount("/repo/logo.pdf", Some(PNG.to_vec()), cx);
        assert!(image.read_with(cx, |s, _| matches!(s.state(), BinaryState::Image(_))));
        let (_, pdf) = mount("/repo/doc.png", Some(b"%PDF-1.7\n".to_vec()), cx);
        assert!(pdf.read_with(cx, |s, _| matches!(s.state(), BinaryState::Pdf(_))));
        let (_, other) = mount("/repo/data.png", Some(b"<svg/>".to_vec()), cx);
        assert!(other.read_with(cx, |s, _| matches!(
            s.state(),
            BinaryState::Unsupported { size: 6 }
        )));
    }

    #[gpui::test]
    fn shows_read_errors_and_reloads_after_a_disk_change(cx: &mut TestAppContext) {
        let (fs, surface) = mount("/repo/missing.png", None, cx);
        assert!(surface.read_with(cx, |s, _| matches!(s.state(), BinaryState::Error(_))));
        fs.state
            .borrow_mut()
            .binary
            .insert("/repo/missing.png".into(), PNG.to_vec());
        cx.update(|cx| fs.touch("/repo/missing.png", cx));
        cx.executor().advance_clock(DISK_RELOAD_DELAY);
        cx.run_until_parked();
        assert!(surface.read_with(cx, |s, _| matches!(s.state(), BinaryState::Image(_))));
    }
}
