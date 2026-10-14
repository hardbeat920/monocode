//! Language for native UI: the app menu, the tray, the Dock menu, the menu bar
//! Mono menu, and notification text that Rust writes itself.
//!
//! The webview owns the language setting and pushes it here through
//! `set_app_locale`. The choice is also written to disk so the next launch
//! builds its menus in the right language before any window loads.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

use tauri::{AppHandle, Manager};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Locale {
    En = 0,
    Tr = 1,
}

impl Locale {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "en" => Ok(Self::En),
            "tr" => Ok(Self::Tr),
            other => Err(format!(
                "Unsupported locale \"{other}\". Expected \"en\" or \"tr\"."
            )),
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Tr => "tr",
        }
    }

    fn from_u8(value: u8) -> Self {
        if value == Self::Tr as u8 {
            Self::Tr
        } else {
            Self::En
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(Locale::En as u8);

pub fn current() -> Locale {
    Locale::from_u8(CURRENT.load(Ordering::Relaxed))
}

/// Swaps in `locale` and reports whether it differs from the previous one.
fn replace(locale: Locale) -> bool {
    CURRENT.swap(locale as u8, Ordering::Relaxed) != locale as u8
}

fn locale_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("locale"))
}

/// Loads the persisted locale. Runs in setup, before any native menu exists.
/// A missing or unreadable file leaves English in place.
pub fn init(app: &AppHandle) {
    let stored = locale_file(app)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|value| Locale::parse(value.trim()).ok());
    if let Some(locale) = stored {
        replace(locale);
    }
}

fn persist(app: &AppHandle, locale: Locale) -> Result<(), String> {
    let path = locale_file(app).ok_or("No app config directory.")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    std::fs::write(&path, locale.code()).map_err(|error| error.to_string())
}

/// Sets the language for native menus, the tray, and notifications. Not
/// `async`, so Tauri runs it on the main thread, where AppKit menus live.
#[tauri::command]
pub fn set_app_locale(app: AppHandle, locale: String) -> Result<(), String> {
    let locale = Locale::parse(&locale)?;
    if replace(locale) {
        refresh_native(&app);
    }
    persist(&app, locale)
}

/// Rebuilds every native surface whose labels were captured at build time.
fn refresh_native(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        if let Err(error) = crate::menu::refresh(app) {
            eprintln!("monocode: rebuild app menu: {error}");
        }
        crate::macos::install_dock_menu(app);
        if let Err(error) = crate::mono_chat::refresh_menu(app) {
            eprintln!("monocode: rebuild Mono menu: {error}");
        }
        crate::notifications::refresh_actions();
    }
    #[cfg(target_os = "windows")]
    {
        if let Err(error) = crate::tray::refresh(app) {
            eprintln!("monocode: rebuild tray menu: {error}");
        }
    }
    // Linux has no native menu or tray to rebuild.
    let _ = app;
}

/// Text in the current locale.
pub fn t(key: Text) -> &'static str {
    text(key, current())
}

macro_rules! texts {
    ($($key:ident => ($en:expr, $tr:expr),)*) => {
        /// Every native string. Each platform uses only some of them.
        #[allow(dead_code)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Text {
            $($key,)*
        }

        #[cfg(test)]
        const ALL: &[Text] = &[$(Text::$key,)*];

        pub fn text(key: Text, locale: Locale) -> &'static str {
            match (key, locale) {
                $(
                    (Text::$key, Locale::En) => $en,
                    (Text::$key, Locale::Tr) => $tr,
                )*
            }
        }
    };
}

texts! {
    // App menu (macOS).
    AboutMonoCode => ("About MonoCode", "MonoCode Hakkında"),
    Settings => ("Settings…", "Ayarlar…"),
    CheckForUpdates => ("Check for Updates…", "Güncellemeleri Denetle…"),
    HideMonoCode => ("Hide MonoCode", "MonoCode'u Gizle"),
    HideOthers => ("Hide Others", "Diğerlerini Gizle"),
    ShowAll => ("Show All", "Tümünü Göster"),
    QuitMonoCode => ("Quit MonoCode", "MonoCode'dan Çık"),

    // Submenu titles.
    FileMenu => ("File", "Dosya"),
    EditMenu => ("Edit", "Düzen"),
    ViewMenu => ("View", "Görüntü"),
    WindowMenu => ("Window", "Pencere"),
    HelpMenu => ("Help", "Yardım"),

    // File.
    NewWindow => ("New Window", "Yeni Pencere"),
    OpenProject => ("Open Project…", "Proje Aç…"),
    Search => ("Search…", "Ara…"),
    GoToFile => ("Go to File…", "Dosyaya Git…"),
    CommandPalette => ("Command Palette…", "Komut Paleti…"),
    FindInFiles => ("Find in Files…", "Dosyalarda Bul…"),
    Autosave => ("Autosave", "Otomatik Kaydet"),
    NewTab => ("New Tab", "Yeni Sekme"),
    NewTerminal => ("New Terminal", "Yeni Terminal"),
    NewTerminalTab => ("New Terminal Tab", "Yeni Terminal Sekmesi"),
    SplitPaneRight => ("Split Pane Right", "Bölmeyi Sağa Ayır"),
    SplitPaneDown => ("Split Pane Down", "Bölmeyi Aşağı Ayır"),
    ClosePane => ("Close Pane", "Bölmeyi Kapat"),
    CloseOtherTabs => ("Close Other Tabs", "Diğer Sekmeleri Kapat"),
    CloseAllTabs => ("Close All Tabs", "Tüm Sekmeleri Kapat"),
    PreviousTab => ("Previous Tab", "Önceki Sekme"),
    NextTab => ("Next Tab", "Sonraki Sekme"),
    GoBack => ("Go Back", "Geri Git"),
    GoForward => ("Go Forward", "İleri Git"),

    // Edit.
    Undo => ("Undo", "Geri Al"),
    Redo => ("Redo", "Yinele"),
    Cut => ("Cut", "Kes"),
    Copy => ("Copy", "Kopyala"),
    Paste => ("Paste", "Yapıştır"),
    SelectAll => ("Select All", "Tümünü Seç"),
    Find => ("Find", "Bul"),

    // View.
    ToggleSidebar => ("Toggle Sidebar", "Kenar Çubuğunu Aç/Kapat"),
    ToggleSessionSidebar => ("Toggle Session Sidebar", "Oturum Kenar Çubuğunu Aç/Kapat"),
    Inbox => ("Inbox", "Gelen Kutusu"),
    Notes => ("Notes", "Notlar"),
    ToggleTerminal => ("Toggle Terminal", "Terminali Aç/Kapat"),
    SwitchModel => ("Switch Model…", "Model Değiştir…"),
    FocusPaneLeft => ("Focus Pane Left", "Soldaki Bölmeye Odaklan"),
    FocusPaneRight => ("Focus Pane Right", "Sağdaki Bölmeye Odaklan"),
    FocusPaneUp => ("Focus Pane Up", "Üstteki Bölmeye Odaklan"),
    FocusPaneDown => ("Focus Pane Down", "Alttaki Bölmeye Odaklan"),
    ZoomIn => ("Zoom In", "Yakınlaştır"),
    ZoomOut => ("Zoom Out", "Uzaklaştır"),
    ResetZoom => ("Reset Zoom", "Yakınlaştırmayı Sıfırla"),
    Reload => ("Reload", "Yeniden Yükle"),
    SidebarAppearance => ("Sidebar Appearance…", "Kenar Çubuğu Görünümü…"),

    // Window.
    Minimize => ("Minimize", "Küçült"),
    Zoom => ("Zoom", "Büyüt"),

    // Help.
    Website => ("MonoCode Website", "MonoCode Web Sitesi"),
    ViewOnGitHub => ("View on GitHub", "GitHub'da Görüntüle"),
    ReportBug => ("Report a Bug…", "Hata Bildir…"),
    RequestFeature => ("Request a Feature…", "Özellik İste…"),

    // Windows tray.
    ShowMonoCode => ("Show MonoCode", "MonoCode'u Göster"),

    // Menu bar Mono menu (macOS).
    MonoMenuTooltip => ("Chat with a Mono", "Bir Mono ile sohbet et"),
    MonoMenuHeader => ("Monos", "Monolar"),
    MonoMenuEmpty => (
        "Create a Mono in MonoCode to chat here",
        "Burada sohbet etmek için MonoCode'da bir Mono oluştur"
    ),
    QuickComposer => ("Quick Composer", "Hızlı Mesaj Alanı"),
    OpenFloatingChat => ("Open floating chat", "Kayan sohbeti aç"),
    MonoChatWindowTitle => ("Mono chat", "Mono sohbeti"),
    MonoUnavailable => ("That Mono is no longer available.", "Bu Mono artık kullanılamıyor."),

    // Notifications.
    NotificationShow => ("Show", "Göster"),
    ReminderBody => (
        "Reminder: continue this conversation.",
        "Hatırlatıcı: bu konuşmaya devam et."
    ),

    // Remote machine setup progress.
    RemoteChecking => ("Checking the remote machine…", "Uzak makine denetleniyor…"),
    RemoteUpdatingHost => (
        "Updating MonoCode Host on the machine…",
        "Makinedeki MonoCode Host güncelleniyor…"
    ),
    RemoteReconnecting => ("Reconnecting to the machine…", "Makineye yeniden bağlanılıyor…"),
    RemoteInstallingHost => (
        "Installing or starting MonoCode Host…",
        "MonoCode Host kuruluyor veya başlatılıyor…"
    ),
    RemotePairing => ("Pairing this desktop with the host…", "Bu masaüstü host ile eşleştiriliyor…"),
    RemoteOpeningTunnel => ("Opening the secure connection…", "Güvenli bağlantı açılıyor…"),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_locales_and_rejects_others() {
        assert_eq!(Locale::parse("en"), Ok(Locale::En));
        assert_eq!(Locale::parse("tr"), Ok(Locale::Tr));
        for bad in ["", "EN", "tr-TR", "de", " en"] {
            let error = Locale::parse(bad).unwrap_err();
            assert!(error.contains("\"en\" or \"tr\""), "{error}");
        }
    }

    #[test]
    fn locale_round_trips_through_its_code() {
        for locale in [Locale::En, Locale::Tr] {
            assert_eq!(Locale::parse(locale.code()), Ok(locale));
            assert_eq!(Locale::from_u8(locale as u8), locale);
        }
    }

    #[test]
    fn every_key_has_english_and_turkish_text() {
        for &key in ALL {
            let en = text(key, Locale::En);
            let tr = text(key, Locale::Tr);
            assert!(!en.trim().is_empty(), "{key:?} has no English text");
            assert!(!tr.trim().is_empty(), "{key:?} has no Turkish text");
            assert_eq!(en, en.trim(), "{key:?} English has stray whitespace");
            assert_eq!(tr, tr.trim(), "{key:?} Turkish has stray whitespace");
            assert_ne!(en, tr, "{key:?} is not translated");
            assert_eq!(
                en.ends_with('…'),
                tr.ends_with('…'),
                "{key:?} must keep the ellipsis in both languages"
            );
        }
    }

    #[test]
    fn product_names_stay_untranslated() {
        for &key in ALL {
            let en = text(key, Locale::En);
            let tr = text(key, Locale::Tr);
            for name in ["MonoCode", "GitHub", "Mono"] {
                assert_eq!(
                    en.matches(name).count(),
                    tr.matches(name).count(),
                    "{key:?} must keep {name}"
                );
            }
        }
    }
}
