import type { Translations } from "./types";

export const en = {
  // Generic words shared across the app.
  "common.back": "Back",
  "common.cancel": "Cancel",
  "common.clear": "Clear",
  "common.close": "Close",
  "common.copy": "Copy",
  "common.delete": "Delete",
  "common.done": "Done",
  "common.forward": "Forward",
  "common.loading": "Loading…",
  "common.noResults": "No results",
  "common.open": "Open",
  "common.refresh": "Refresh",
  "common.reset": "Reset",
  "common.retry": "Retry",
  "common.save": "Save",
  "common.search": "Search",

  "shared.privateEmail.hide": "Hide email",
  "shared.privateEmail.reveal": "Reveal email",

  "shared.select.placeholder": "Choose an option…",
  "shared.select.searchPlaceholder": "Search options…",
  "shared.select.empty": "No matching options",
  "shared.select.optionsLabel": "{label} options",

  "shared.emoji.search": "Search emoji",
  "shared.emoji.recent": "Recent",
  "shared.emoji.empty": "No emoji found",
  "shared.emoji.pick": "Pick an emoji",
  "shared.emoji.jumpToCategory": "Jump to category {index}",
  "shared.emoji.skinTone": "Skin tone",
  "shared.emoji.changeSkinTone": "Change skin tone",

  "shared.color.numbered": "Color {index}",
  "shared.color.custom": "Custom color",
  "shared.color.saturation": "Saturation and brightness",
  "shared.color.hue": "Hue",
  "shared.color.hex": "Hex color",

  "shared.dateTime.previousMonth": "Previous month",
  "shared.dateTime.nextMonth": "Next month",
  "shared.dateTime.weekday.mo": "Mo",
  "shared.dateTime.weekday.tu": "Tu",
  "shared.dateTime.weekday.we": "We",
  "shared.dateTime.weekday.th": "Th",
  "shared.dateTime.weekday.fr": "Fr",
  "shared.dateTime.weekday.sa": "Sa",
  "shared.dateTime.weekday.su": "Su",
  "shared.dateTime.time": "Time",
  "shared.dateTime.timeHint": "Local time, 24-hour",

  "shared.lightbox.preview": "Image preview: {alt}",
  "shared.lightbox.close": "Close image preview",

  "platform.fs.connectMachine":
    "Connect this project’s machine to open its files.",
  "platform.fs.openProjects": "Open projects",
  "platform.fs.attachFiles": "Attach files",

  "platform.clipboard.contentUnavailable":
    "File content is no longer available.",
  "platform.clipboard.copyFailed": "Could not copy {name}: {reason}",
  "platform.clipboard.nothingToCopy": "No copyable content is available.",
  "platform.clipboard.copyFailedShort": "copy failed",
  "platform.clipboard.nothingToAttach": {
    one: "Nothing to attach from that path — the file may have been moved, renamed, or deleted.",
    other:
      "Nothing to attach from those paths — the file may have been moved, renamed, or deleted.",
  },
  "platform.clipboard.partialAttach":
    "Attached {attached} of {total} copied files. A turn carries up to {max}.",
  "platform.clipboard.noImage": "The clipboard does not contain an image.",
  "platform.clipboard.unreadable": "The clipboard could not be read.",
} as const;

export const tr: Translations<typeof en> = {
  "common.back": "Geri",
  "common.cancel": "İptal",
  "common.clear": "Temizle",
  "common.close": "Kapat",
  "common.copy": "Kopyala",
  "common.delete": "Sil",
  "common.done": "Tamam",
  "common.forward": "İleri",
  "common.loading": "Yükleniyor…",
  "common.noResults": "Sonuç yok",
  "common.open": "Aç",
  "common.refresh": "Yenile",
  "common.reset": "Sıfırla",
  "common.retry": "Yeniden dene",
  "common.save": "Kaydet",
  "common.search": "Ara",

  "shared.privateEmail.hide": "E-postayı gizle",
  "shared.privateEmail.reveal": "E-postayı göster",

  "shared.select.placeholder": "Bir seçenek seç…",
  "shared.select.searchPlaceholder": "Seçeneklerde ara…",
  "shared.select.empty": "Eşleşen seçenek yok",
  "shared.select.optionsLabel": "{label} seçenekleri",

  "shared.emoji.search": "Emoji ara",
  "shared.emoji.recent": "Son kullanılanlar",
  "shared.emoji.empty": "Emoji bulunamadı",
  "shared.emoji.pick": "Bir emoji seç",
  "shared.emoji.jumpToCategory": "{index}. kategoriye git",
  "shared.emoji.skinTone": "Ten rengi",
  "shared.emoji.changeSkinTone": "Ten rengini değiştir",

  "shared.color.numbered": "Renk {index}",
  "shared.color.custom": "Özel renk",
  "shared.color.saturation": "Doygunluk ve parlaklık",
  "shared.color.hue": "Ton",
  "shared.color.hex": "Hex rengi",

  "shared.dateTime.previousMonth": "Önceki ay",
  "shared.dateTime.nextMonth": "Sonraki ay",
  "shared.dateTime.weekday.mo": "Pt",
  "shared.dateTime.weekday.tu": "Sa",
  "shared.dateTime.weekday.we": "Ça",
  "shared.dateTime.weekday.th": "Pe",
  "shared.dateTime.weekday.fr": "Cu",
  "shared.dateTime.weekday.sa": "Ct",
  "shared.dateTime.weekday.su": "Pz",
  "shared.dateTime.time": "Saat",
  "shared.dateTime.timeHint": "Yerel saat, 24 saat biçimi",

  "shared.lightbox.preview": "Görsel önizlemesi: {alt}",
  "shared.lightbox.close": "Görsel önizlemesini kapat",

  "platform.fs.connectMachine":
    "Dosyalarını açmak için bu projenin makinesine bağlan.",
  "platform.fs.openProjects": "Projeleri aç",
  "platform.fs.attachFiles": "Dosya ekle",

  "platform.clipboard.contentUnavailable": "Dosya içeriği artık mevcut değil.",
  "platform.clipboard.copyFailed": "{name} kopyalanamadı: {reason}",
  "platform.clipboard.nothingToCopy": "Kopyalanabilir içerik yok.",
  "platform.clipboard.copyFailedShort": "kopyalama başarısız",
  "platform.clipboard.nothingToAttach": {
    one: "Bu yoldan eklenecek bir şey yok — dosya taşınmış, yeniden adlandırılmış veya silinmiş olabilir.",
    other:
      "Bu yollardan eklenecek bir şey yok — dosyalar taşınmış, yeniden adlandırılmış veya silinmiş olabilir.",
  },
  "platform.clipboard.partialAttach":
    "Kopyalanan {total} dosyadan {attached} tanesi eklendi. Bir mesaj en fazla {max} dosya taşıyabilir.",
  "platform.clipboard.noImage": "Panoda görsel yok.",
  "platform.clipboard.unreadable": "Pano okunamadı.",
};
