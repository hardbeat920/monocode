//! Owns the native quick composer and git popup windows.

use std::rc::Rc;

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, Subscription, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, px, size,
};
use gpui_component::Root;
#[cfg(target_os = "macos")]
use monocode_platform::panel_geometry::{
    GIT_POPUP_MAX_HEIGHT, PanelAnchor, QUICK_COMPOSER_MAX_HEIGHT, QUICK_COMPOSER_TOP_FRACTION,
};
use monocode_platform::panel_geometry::{
    GIT_POPUP_WIDTH, QUICK_COMPOSER_INITIAL_HEIGHT, QUICK_COMPOSER_WIDTH,
};
#[cfg(target_os = "macos")]
use monocode_ui::Theme;

use crate::{
    QuickComposer, QuickComposerEvent, QuickComposerHost, QuickGitHost, QuickGitPopup,
    QuickGitPopupEvent, QuickGitRequest, QuickGitResult, QuickWorkspace,
};

/// Keep this entity alive for the application's lifetime. The windows start
/// hidden and never activate the workspace when presented.
pub struct QuickPanels {
    composer: Entity<QuickComposer>,
    popup: Entity<QuickGitPopup>,
    composer_window: AnyWindowHandle,
    popup_window: AnyWindowHandle,
    request: Option<QuickGitRequest>,
    presented: Option<String>,
    capturing: bool,
    _subscriptions: Vec<Subscription>,
}

impl QuickPanels {
    pub fn new(host: Rc<dyn QuickComposerHost>, cx: &mut App) -> Result<Entity<Self>, String> {
        if !cfg!(target_os = "macos") {
            return Err("The quick composer requires macOS floating panels.".into());
        }
        let composer_host = host.clone();
        let mut composer = None;
        let composer_window: AnyWindowHandle = cx
            .open_window(
                options(QUICK_COMPOSER_WIDTH, QUICK_COMPOSER_INITIAL_HEIGHT, cx),
                |window, cx| {
                    monocode_ui::sync_window(window, cx);
                    window.set_background_appearance(WindowBackgroundAppearance::Transparent);
                    let view = cx.new(|cx| QuickComposer::new(composer_host, window, cx));
                    composer = Some(view.clone());
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .map_err(|error| error.to_string())?
            .into();
        let mut popup = None;
        let git_host: Rc<dyn QuickGitHost> = host;
        let popup_window: AnyWindowHandle = cx
            .open_window(options(GIT_POPUP_WIDTH, 1., cx), |window, cx| {
                monocode_ui::sync_window(window, cx);
                window.set_background_appearance(WindowBackgroundAppearance::Transparent);
                let view = cx.new(|cx| QuickGitPopup::new(git_host, window, cx));
                popup = Some(view.clone());
                cx.new(|cx| Root::new(view, window, cx))
            })
            .map_err(|error| error.to_string())?
            .into();
        let composer = composer.expect("composer was built");
        let popup = popup.expect("popup was built");
        #[cfg(target_os = "macos")]
        {
            use monocode_platform::macos_panel::{PanelStyle, make_panel};
            if let Err(error) = composer_window
                .update(cx, |_, window, _| {
                    make_panel(window, &PanelStyle::quick_composer())
                })
                .map_err(|error| error.to_string())?
            {
                eprintln!("monocode: quick composer uses an ordinary window: {error}");
            }
            if let Err(error) = popup_window
                .update(cx, |_, window, _| {
                    make_panel(window, &PanelStyle::git_popup())
                })
                .map_err(|error| error.to_string())?
            {
                eprintln!("monocode: quick git popup uses an ordinary window: {error}");
            }
        }
        let panels = cx.new(|cx| {
            let composer_events = cx.subscribe(
                &composer,
                |this: &mut Self, _, event: &QuickComposerEvent, cx| {
                    let result = match event {
                        QuickComposerEvent::Dismiss => this.dismiss(cx),
                        QuickComposerEvent::Fit(height) => this.fit_composer(*height, cx),
                        QuickComposerEvent::OpenGit(request) => {
                            this.open_git(request.as_ref().clone(), cx)
                        }
                        QuickComposerEvent::CancelGit(id) => {
                            this.complete_git(id, None, false, None, cx)
                        }
                    };
                    if let Err(error) = result {
                        eprintln!("monocode: quick panel failed: {error}");
                    }
                },
            );
            let popup_events = cx.subscribe(
                &popup,
                |this: &mut Self, _, event: &QuickGitPopupEvent, cx| {
                    let result = match event {
                        QuickGitPopupEvent::Finish { id, choice } => {
                            this.complete_git(id, choice.clone(), true, None, cx)
                        }
                        QuickGitPopupEvent::Fit { id, height } => this.fit_git(id, *height, cx),
                        QuickGitPopupEvent::Shown => Ok(()),
                    };
                    if let Err(error) = result {
                        this.popup
                            .update(cx, |popup, cx| popup.finish_failed(error, cx));
                    }
                },
            );
            Self {
                composer,
                popup,
                composer_window,
                popup_window,
                request: None,
                presented: None,
                capturing: false,
                _subscriptions: vec![composer_events, popup_events],
            }
        });
        composer_window
            .update(cx, |_, window, cx| {
                panels.update(cx, |this, cx| {
                    let subscription = cx.observe_window_activation(window, |_, window, cx| {
                        if !window.is_window_active() {
                            let weak = cx.weak_entity();
                            cx.defer(move |cx| {
                                weak.update(cx, |this, cx| {
                                    if !this.capturing
                                        && this.request.is_none()
                                        && !this.composer_key(cx)
                                    {
                                        let _ = this.dismiss(cx);
                                    }
                                })
                                .ok();
                            });
                        }
                    });
                    this._subscriptions.push(subscription);
                });
            })
            .map_err(|error| error.to_string())?;
        popup_window
            .update(cx, |_, window, cx| {
                panels.update(cx, |this, cx| {
                    let subscription = cx.observe_window_activation(window, |this, window, cx| {
                        if !window.is_window_active() {
                            let request = this.request.clone();
                            let trigger_kind = request
                                .as_ref()
                                .filter(|request| this.blurred_by_trigger(request, cx))
                                .map(|request| request.kind);
                            let weak = cx.weak_entity();
                            cx.defer(move |cx| {
                                weak.update(cx, |this, cx| {
                                    if let Some(request) = request {
                                        let _ = this.complete_git(
                                            &request.id,
                                            None,
                                            false,
                                            trigger_kind,
                                            cx,
                                        );
                                    }
                                })
                                .ok();
                            });
                        }
                    });
                    this._subscriptions.push(subscription);
                });
            })
            .map_err(|error| error.to_string())?;
        Ok(panels)
    }

    pub fn composer(&self) -> &Entity<QuickComposer> {
        &self.composer
    }
    pub fn git_popup(&self) -> &Entity<QuickGitPopup> {
        &self.popup
    }
    pub fn composer_window(&self) -> AnyWindowHandle {
        self.composer_window
    }
    pub fn git_window(&self) -> AnyWindowHandle {
        self.popup_window
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        if self.capturing {
            return Ok(());
        }
        if self.visible(cx) && self.composer_key(cx) {
            self.dismiss(cx)
        } else {
            self.show(cx)
        }
    }

    pub fn show(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        if self.capturing {
            return Ok(());
        }
        let view = self.composer.clone();
        self.composer_window
            .update(cx, |_, window, cx| {
                monocode_ui::sync_window(window, cx);
                view.update(cx, |view, cx| view.show(window, cx));
                #[cfg(target_os = "macos")]
                {
                    use monocode_platform::macos_panel as native;
                    native::set_dark(window, Theme::of(cx).is_dark())?;
                    native::place_on_pointer_screen(
                        window,
                        QUICK_COMPOSER_WIDTH,
                        QUICK_COMPOSER_TOP_FRACTION,
                    )?;
                    native::present_with_fallback(window)?;
                }
                Ok::<_, String>(())
            })
            .map_err(|error| error.to_string())?
    }

    pub fn dismiss(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        if let Some(id) = self.request.as_ref().map(|request| request.id.clone()) {
            self.complete_git(&id, None, false, None, cx)?;
        }
        self.composer_window
            .update(cx, |_, window, _| hide(window))
            .map_err(|error| error.to_string())?
    }

    /// The screenshot host hides the panel while macOS captures, then
    /// restores the same draft and frame. Blur during capture is ignored.
    pub fn set_capturing(&mut self, capturing: bool, cx: &mut Context<Self>) -> Result<(), String> {
        self.capturing = capturing;
        self.composer_window
            .update(cx, |_, window, cx| {
                if capturing {
                    hide(window)
                } else {
                    #[cfg(target_os = "macos")]
                    monocode_platform::macos_panel::present_with_fallback(window)?;
                    let view = self.composer.clone();
                    view.update(cx, |view, cx| view.focus_prompt(window, cx));
                    Ok(())
                }
            })
            .map_err(|error| error.to_string())?
    }

    pub fn visible(&self, cx: &mut App) -> bool {
        self.composer_window
            .update(cx, |_, window, _| {
                #[cfg(target_os = "macos")]
                {
                    monocode_platform::macos_panel::is_visible(window)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = window;
                    false
                }
            })
            .unwrap_or(false)
    }

    fn composer_key(&self, cx: &mut App) -> bool {
        self.composer_window
            .update(cx, |_, window, _| {
                #[cfg(target_os = "macos")]
                {
                    monocode_platform::macos_panel::is_key(window)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = window;
                    false
                }
            })
            .unwrap_or(false)
    }

    fn fit_composer(&self, height: u32, cx: &mut App) -> Result<(), String> {
        self.composer_window
            .update(cx, |_, window, _| {
                #[cfg(not(target_os = "macos"))]
                let _ = (window, height);
                #[cfg(target_os = "macos")]
                monocode_platform::macos_panel::fit_height(
                    window,
                    QUICK_COMPOSER_WIDTH,
                    height as f64,
                    QUICK_COMPOSER_MAX_HEIGHT,
                )?;
                Ok::<_, String>(())
            })
            .map_err(|error| error.to_string())?
    }

    fn open_git(&mut self, request: QuickGitRequest, cx: &mut Context<Self>) -> Result<(), String> {
        let result = request.validate().and_then(|_| {
            if self.visible(cx) {
                Ok(())
            } else {
                Err("The composer is no longer visible.".into())
            }
        });
        if let Err(error) = result {
            self.composer.update(cx, |view, cx| {
                view.git_open_failed(&request.id, error.clone(), cx)
            });
            return Err(error);
        }
        self.request = Some(request.clone());
        self.presented = None;
        let popup = self.popup.clone();
        self.popup_window
            .update(cx, |_, window, cx| {
                monocode_ui::sync_window(window, cx);
                #[cfg(target_os = "macos")]
                monocode_platform::macos_panel::set_dark(window, Theme::of(cx).is_dark())?;
                popup.update(cx, |view, cx| view.open(request, window, cx));
                Ok::<_, String>(())
            })
            .map_err(|error| error.to_string())?
    }

    fn fit_git(&mut self, id: &str, height: u32, cx: &mut Context<Self>) -> Result<(), String> {
        let Some(request) = self.request.clone().filter(|request| request.id == id) else {
            return Ok(());
        };
        if !self.visible(cx) {
            return self.complete_git(id, None, false, None, cx);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (request, height);
        #[cfg(target_os = "macos")]
        {
            let parent = self.composer_window;
            let popup = self.popup_window;
            let anchor = anchor(&request);
            parent
                .update(cx, |_, parent, cx| {
                    popup.update(cx, |_, popup, _| {
                        monocode_platform::macos_panel::place_popup(
                            parent,
                            popup,
                            &anchor,
                            height as f64,
                            GIT_POPUP_WIDTH,
                            GIT_POPUP_MAX_HEIGHT,
                        )
                    })
                })
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())??;
            if self.presented.as_deref() != Some(id) {
                popup
                    .update(cx, |_, window, _| {
                        monocode_platform::macos_panel::present_with_fallback(window)
                    })
                    .map_err(|error| error.to_string())??;
                self.presented = Some(id.to_string());
            }
        }
        Ok(())
    }

    fn complete_git(
        &mut self,
        id: &str,
        choice: Option<QuickWorkspace>,
        restore_focus: bool,
        trigger_kind: Option<crate::QuickGitKind>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.request.as_ref().is_none_or(|request| request.id != id) {
            return Ok(());
        }
        self.request = None;
        self.presented = None;
        self.popup_window
            .update(cx, |_, window, _| hide(window))
            .map_err(|error| error.to_string())??;
        let composer = self.composer.clone();
        let result = QuickGitResult {
            id: id.to_string(),
            choice,
            restore_focus,
            trigger_kind,
        };
        self.composer_window
            .update(cx, |_, window, cx| {
                #[cfg(target_os = "macos")]
                if restore_focus && monocode_platform::macos_panel::is_visible(window) {
                    monocode_platform::macos_panel::present_with_fallback(window)?;
                }
                composer.update(cx, |view, cx| view.apply_git_result(result, window, cx));
                Ok::<_, String>(())
            })
            .map_err(|error| error.to_string())?
    }

    fn blurred_by_trigger(&self, request: &QuickGitRequest, cx: &mut App) -> bool {
        self.composer_window
            .update(cx, |_, window, _| {
                #[cfg(target_os = "macos")]
                {
                    monocode_platform::macos_panel::blurred_by_trigger(window, &anchor(request))
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = (window, request);
                    false
                }
            })
            .unwrap_or(false)
    }
}

fn options(width: f64, height: f64, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(width as f32), px(height as f32)),
            cx,
        ))),
        kind: WindowKind::PopUp,
        titlebar: None,
        window_background: WindowBackgroundAppearance::Transparent,
        focus: false,
        show: false,
        is_resizable: false,
        ..Default::default()
    }
}

#[cfg(target_os = "macos")]
fn anchor(request: &QuickGitRequest) -> PanelAnchor {
    PanelAnchor {
        x: request.anchor.x,
        y: request.anchor.y,
        width: request.anchor.width,
        height: request.anchor.height,
    }
}

fn hide(window: &mut Window) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        monocode_platform::macos_panel::hide(window)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("Floating panels require macOS.".into())
    }
}
