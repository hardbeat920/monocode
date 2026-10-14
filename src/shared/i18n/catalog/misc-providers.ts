import type { Translations } from "./types";

export const en = {
  // Provider account usage
  "providers.account.refreshUsage": "Refresh usage limits",
  "providers.account.extraBalance": "Extra usage balance",
  "providers.account.extra": "Extra",
  "providers.account.default": "Default account",
  "providers.account.personal": "Personal",
  "providers.meter.daily": "Daily",
  "providers.meter.fiveHours": "5h",
  "providers.meter.weekly": "Weekly",
  "providers.meter.monthly": "Monthly",
  "providers.meter.resetDue": "reset due",
  "providers.meter.resets": "Resets {date}",
  "providers.meter.left": "{percent} left",
  "providers.meter.full": "Full",
  "providers.meter.limitRemaining": "{title} limit remaining",
  "providers.meter.limitUsed": "{title} limit used",

  // Account status
  "providers.status.checking": "Checking…",
  "providers.status.notSignedIn": "Not signed in",
  "providers.status.usageUnavailable": "Usage unavailable",
  "providers.status.exhausted": "Exhausted",
  "providers.status.runningLow": "Running low",
  "providers.status.ready": "Ready",
  "providers.status.percentLeft": "{percent}% left",
  "providers.status.backIn": "back in {duration}",

  // Durations and window labels
  "providers.duration.now": "now",
  "providers.duration.minutes": "{minutes}m",
  "providers.duration.hours": "{hours}h",
  "providers.duration.hoursMinutes": "{hours}h {minutes}m",
  "providers.duration.days": "{days}d",
  "providers.duration.daysHours": "{days}d {hours}h",
  "providers.window.week": "wk",
  "providers.window.month": "mo",
  "providers.window.weeks": "{count}wk",
  "providers.tooltip.remaining": "{percent} remaining",
  "providers.tooltip.used": "{percent} used",
  "providers.tooltip.window": "{usage} · {window} window",
  "providers.tooltip.resetsNow": "Resets now",
  "providers.tooltip.resetsIn": "Resets in {duration}",

  // Usage fetch errors
  "providers.error.usageUnavailable": "{provider} usage unavailable",
  "providers.error.notSignedIn": "{provider} not signed in",
  "providers.error.cliNotFound": "{provider} CLI not found",
  "providers.error.claudeNotJson": "Claude usage response was not JSON",
  "providers.error.claudeEmpty": "Claude usage response was empty",
  "providers.error.opencodeNotJson": "OpenCode Go response was not JSON",
  "providers.error.opencodeUnexpected":
    "OpenCode Go usage response was unexpected",
  "providers.error.opencodeNotConnected": "OpenCode Go not connected",
  "providers.error.devinVersionUnknown": "Devin CLI version unknown",
  "providers.error.codexNoData": "No Codex usage data",
  "providers.error.codexUnknownReset": "Codex returned an unknown reset result",
  "providers.error.codexProbeExited": "Codex usage probe exited",
  "providers.error.codexProbeTimedOut": "Codex usage probe timed out",
  "providers.error.piUnexpected":
    "Pi usage response was unexpected. Try refreshing.",
  "providers.error.piFetchFailed": "Could not fetch Pi usage. Try refreshing.",

  // Harness update notice
  "providers.update.ariaLabel": "Harness updates",
  "providers.update.titleOne": "Harness update available",
  "providers.update.titleMany": "Harness updates available",
  "providers.update.updateAll": "Update all",
  "providers.update.dismiss": "Dismiss harness updates",
  "providers.update.refreshed":
    "Model picker refreshed with the new version’s models.",
  "providers.update.hint": "New models often need the latest version.",
  "providers.update.updatedTo": "Updated to {version}",
  "providers.update.updating": "Updating",
  "providers.update.retry": "Retry",
  "providers.update.update": "Update",
  "providers.update.stillOn": "Still on {version} after updating.",

  // Connections settings
  "connections.settings.title": "Your machines",
  "connections.settings.description":
    "Run agents on another computer and return to them from your laptop. The host keeps working when you close MonoCode here.",
  "connections.settings.addMachine": "Add machine",
  "connections.settings.empty":
    "Add your always-on Windows, Mac, or Linux machine to get started.",
  "connections.settings.sshTarget": "SSH · {target}",
  "connections.settings.sshTargetPort": "SSH · {target} · port {port}",
  "connections.settings.status.checking": "Checking connection…",
  "connections.settings.status.connected": "Connected",
  "connections.settings.status.noProvider":
    "Connected · install a supported provider on the host",
  "connections.settings.status.needsUpdate":
    "Connected · host update needed for Explorer and Changes",
  "connections.settings.status.offline": "Offline · reconnect to check access",
  "connections.settings.updateWarning":
    "Updating restarts the host and interrupts active agent turns.",
  "connections.settings.updateHost": "Update Host",
  "connections.settings.updateHostTitle":
    "Downloads the matching host package and restarts the host; active agent turns will be interrupted",
  "connections.settings.reconnect": "Reconnect",
  "connections.settings.removeAria": "Remove {name}",
  "connections.settings.removeTitle": "Remove connection…",
  "connections.settings.confirmRemoveAria": "Confirm removing {name}",
  "connections.settings.removeHeading": "Remove {name} from this desktop?",
  "connections.settings.removeBody1":
    "This closes this desktop’s connection to the machine. It does not stop the host, and its sessions keep running and stay on that machine. You can add it again later.",
  "connections.settings.removeBody2":
    "Removing alone leaves this desktop’s credential valid on the host. Revoke access to invalidate it first; the machine must be reachable.",
  "connections.settings.removeBody3":
    "To stop the host and turn off its background service, run {command} on that machine ({windowsCommand} on Windows). Its sessions and history are kept.",
  "connections.settings.revokeAndRemove": "Revoke access and remove",
  "connections.settings.removeOnly": "Remove from this desktop only",
  "connections.settings.cancel": "Cancel",
  "connections.settings.revokeFailed":
    "Could not revoke access, so {name} was not removed: {reason}. Reconnect and try again, or remove it from this desktop only and revoke it on the host with monocode-host devices and monocode-host revoke <device-id>.",
  "connections.settings.removedRevoked":
    "{name} was removed and this desktop's access was revoked. The host and its sessions keep running.",
  "connections.settings.removedOnly":
    "{name} was removed from this desktop. The host and its sessions keep running, and it still accepts this desktop's credential.",
  "connections.settings.updated": "{name} was updated and reconnected.",
  "connections.settings.connectedNotice":
    "{name} is connected. To work on it, click + next to Projects in the project rail and choose Open folder on a machine.",
  "connections.settings.connectedShort": "{name} is connected.",
  "connections.settings.sshHeading": "Connect through SSH",
  "connections.settings.sshAddress": "SSH address",
  "connections.settings.sshAddressPlaceholder":
    "user@my-mac-mini or an SSH alias",
  "connections.settings.name": "Name",
  "connections.settings.optional": "(optional)",
  "connections.settings.namePlaceholder": "Optional, e.g. Home Mac mini",
  "connections.settings.advanced": "Advanced",
  "connections.settings.sshPort": "SSH port",
  "connections.settings.sshPortPlaceholder": "From SSH config",
  "connections.settings.sshInfo":
    "MonoCode installs and starts its background host, then connects securely. Your SSH keys and config are used automatically. Enable SSH on the host and sign in to Codex or Claude Code there. On Windows and Mac, keep the host’s desktop account signed in and the machine awake. Locking the desktop is fine.",
  "connections.settings.linuxInfo":
    "On Linux, setup installs a systemd user service and turns on lingering for your account ({command}), so the host and your other user services keep running after you log out. The host keeps running until you stop it on that machine; removing it here only disconnects this desktop.",
  "connections.settings.connecting": "Connecting…",
  "connections.settings.connect": "Connect",
  "connections.settings.starting": "Starting connection…",
  "connections.settings.passwordAria": "SSH password or passphrase",
  "connections.settings.trustHost": "Trust host and continue",
  "connections.settings.continue": "Continue",
  "connections.settings.reject": "Reject",
  "connections.settings.cancelConnection": "Cancel connection",
  "connections.settings.byUrlSummary": "Connect to an existing host by URL",
  "connections.settings.hostUrl": "Host URL",
  "connections.settings.deviceToken": "Device token",
  "connections.settings.connectByUrl": "Connect by URL",

  // Open folder on a machine dialog
  "connections.addProject.title": "Open folder on a machine",
  "connections.addProject.description":
    "Sessions in this project run on that machine, using its checkout and its Codex or Claude Code sign-in. They keep running when you close MonoCode here.",
  "connections.addProject.noMachines":
    "No machines are connected yet. Add one in Settings, then open a folder on it here.",
  "connections.addProject.cancel": "Cancel",
  "connections.addProject.addMachine": "Add a machine",
  "connections.addProject.machine": "Machine",
  "connections.addProject.onMachine": "On {name}",
  "connections.addProject.pathAria": "Folder path on the machine",
  "connections.addProject.folders": "Folders",
  "connections.addProject.noSubfolders": "No subfolders",
  "connections.addProject.loadingFolders": "Loading folders…",
  "connections.addProject.opening": "Opening…",
  "connections.addProject.open": "Open",

  // Remote session
  "connections.session.missingDetails":
    "This project’s machine details are missing. Add the project again from the project rail.",
  "connections.session.notConnected":
    "The machine for this project isn’t connected on this computer.",
  "connections.session.connecting": "Connecting to the machine…",
  "connections.session.manageMachines": "Manage machines",
  "connections.session.hostIdentityChanged":
    "Host identity changed. Reconnect this machine before continuing.",
  "connections.session.wrongProject":
    "This session belongs to a different host project",
  "connections.session.loadingModels": "Loading models…",
  "connections.session.waiting":
    "Waiting for the host to confirm your request.",
  "connections.session.retry": "Retry",
  "connections.session.failedDraft": "Couldn’t save the draft on {machine}.",
  "connections.session.failedSend": "Couldn’t send the message on {machine}.",
  "connections.session.tryAgain": "Try again",
  "connections.session.dismiss": "Dismiss",
  "connections.session.catalogFailed": "Couldn’t load models from {machine}.",
  "connections.session.chooseWorktree": "Choose a worktree on this machine",
  "connections.session.worktreeFixed":
    "This session’s worktree is fixed. Start a new session to use another.",
  "connections.session.selectModel":
    "Select that model in the composer before building this remote plan.",

  // Connections model errors
  "connections.error.saveLocally":
    "Cannot save your request locally. Free up app storage before sending.",
  "connections.error.incompatibleHost":
    "This machine is running an incompatible MonoCode Host",
  "connections.error.tooManyAttachments": "Too many attachments",
  "connections.error.cannotRead": "Cannot read {name} for remote upload",
  "connections.error.uploadFailed": "Could not finish uploading {name}",
  "connections.error.unavailable":
    "This isn’t available for projects on another machine yet.",
  "connections.error.outdatedHost":
    "Update MonoCode Host in Connections settings to use this project’s files.",
  "connections.error.oneMachine":
    "Files can only be copied or moved within one machine.",
  "connections.error.machineNotConnected":
    "This project’s machine isn’t connected on this computer.",

  // Quick composer
  "quickComposer.git.loadFailed": "Couldn’t load branches for this project.",
  "quickComposer.git.loading": "Loading branches…",
  "quickComposer.git.close": "Close",
  "quickComposer.permissions.aria": "Permissions",
  "quickComposer.workspace.newWorktree": "New worktree",
  "quickComposer.workspace.currentCheckout": "Current checkout",
  "quickComposer.workspace.aria": "Workspace {label}",
  "quickComposer.workspace.from": "From {base}",
  "quickComposer.workspace.noRepo": "No repo",
  "quickComposer.workspace.loading": "Loading…",
  "quickComposer.workspace.createFrom": "Create worktree from {base}",
  "quickComposer.workspace.chooseBranch": "Choose branch",
  "quickComposer.workspace.worktreeGone":
    "This worktree is no longer available. Select another working copy.",
  "quickComposer.composer.loadingModel": "Loading model…",
  "quickComposer.composer.dragToMove": "Drag to move",
  "quickComposer.composer.closeAria": "Close composer",
  "quickComposer.composer.closeTitle": "Close (Esc)",
  "quickComposer.composer.dropToAttach": "Drop to attach",
  "quickComposer.composer.projectTitle": "Project (⌘P)",
  "quickComposer.composer.noProject": "No project",
  "quickComposer.composer.attachments": "Attachments",
  "quickComposer.composer.placeholder":
    "Start a {harness} session in {project}…",
  "quickComposer.composer.openProjectFirst": "Open a project in MonoCode first",
  "quickComposer.composer.promptAria": "Prompt",
  "quickComposer.composer.unsupportedAttachments":
    "Choose a provider that supports attachments, or remove the attached files.",
  "quickComposer.composer.addAttachment": "Add attachment",
  "quickComposer.composer.attachTitle": "Attach files or take a screenshot",
  "quickComposer.composer.attachUnsupported":
    "This provider does not support attachments",
  "quickComposer.composer.modelTitle": "Model (⌘.)",
  "quickComposer.composer.permissionsTitle": "Permissions",
  "quickComposer.composer.addingAttachment": "Adding attachment…",
  "quickComposer.composer.hintStart": "start",
  "quickComposer.composer.hintStartAndOpen": "start and open",
  "quickComposer.composer.saveDraft": "Save draft",
  "quickComposer.composer.start": "Start",
  "quickComposer.composer.chooseFiles": "Choose files…",
  "quickComposer.composer.takeScreenshot": "Take screenshot…",
  "quickComposer.composer.commands": "Commands",
  "quickComposer.composer.noCommands": "No matching commands",
  "quickComposer.composer.findProject": "Find a project",
  "quickComposer.composer.projects": "Projects",
  "quickComposer.composer.noMatches": "No matches",
  "quickComposer.attachments.tooMany": "You can attach up to {max} files.",
  "quickComposer.attachments.couldNotAttach": "Could not attach {name}.",
  "quickComposer.models.selector": "Model selector",
  "quickComposer.models.providers": "Providers",
  "quickComposer.models.favorites": "Favorites",
  "quickComposer.models.searchAria": "Search models",
  "quickComposer.models.searchPlaceholder": "Search models…",
  "quickComposer.models.list": "Models",
  "quickComposer.models.removeFavoriteTitle": "Remove from favorites",
  "quickComposer.models.addFavoriteTitle": "Add to favorites",
  "quickComposer.models.removeFavoriteAria": "Remove {name} from favorites",
  "quickComposer.models.addFavoriteAria": "Add {name} to favorites",
  "quickComposer.models.noMatching": "No matching models",
  "quickComposer.models.noFavorites": "No favorite models",
  "quickComposer.models.loading": "Loading models…",
  "quickComposer.models.fastMode": "Fast mode",
  "quickComposer.models.fastOff": "Turn off fast mode",
  "quickComposer.models.fastOn": "Turn on fast mode",
  "quickComposer.models.reset": "Reset to saved defaults",
} as const;

export const tr: Translations<typeof en> = {
  // Provider account usage
  "providers.account.refreshUsage": "Kullanım limitlerini yenile",
  "providers.account.extraBalance": "Ek kullanım bakiyesi",
  "providers.account.extra": "Ek",
  "providers.account.default": "Varsayılan hesap",
  "providers.account.personal": "Kişisel",
  "providers.meter.daily": "Günlük",
  "providers.meter.fiveHours": "5sa",
  "providers.meter.weekly": "Haftalık",
  "providers.meter.monthly": "Aylık",
  "providers.meter.resetDue": "sıfırlanma bekleniyor",
  "providers.meter.resets": "Sıfırlanma: {date}",
  "providers.meter.left": "{percent} kaldı",
  "providers.meter.full": "Dolu",
  "providers.meter.limitRemaining": "{title} limiti, kalan",
  "providers.meter.limitUsed": "{title} limiti, kullanılan",

  // Account status
  "providers.status.checking": "Denetleniyor…",
  "providers.status.notSignedIn": "Oturum açılmamış",
  "providers.status.usageUnavailable": "Kullanım bilgisi alınamıyor",
  "providers.status.exhausted": "Tükendi",
  "providers.status.runningLow": "Azalıyor",
  "providers.status.ready": "Hazır",
  "providers.status.percentLeft": "%{percent} kaldı",
  "providers.status.backIn": "{duration} sonra hazır",

  // Durations and window labels
  "providers.duration.now": "şimdi",
  "providers.duration.minutes": "{minutes}dk",
  "providers.duration.hours": "{hours}sa",
  "providers.duration.hoursMinutes": "{hours}sa {minutes}dk",
  "providers.duration.days": "{days}g",
  "providers.duration.daysHours": "{days}g {hours}sa",
  "providers.window.week": "hf",
  "providers.window.month": "ay",
  "providers.window.weeks": "{count}hf",
  "providers.tooltip.remaining": "{percent} kaldı",
  "providers.tooltip.used": "{percent} kullanıldı",
  "providers.tooltip.window": "{usage} · {window} penceresi",
  "providers.tooltip.resetsNow": "Şimdi sıfırlanıyor",
  "providers.tooltip.resetsIn": "{duration} içinde sıfırlanıyor",

  // Usage fetch errors
  "providers.error.usageUnavailable": "{provider} kullanım bilgisi alınamıyor",
  "providers.error.notSignedIn": "{provider} oturumu açılmamış",
  "providers.error.cliNotFound": "{provider} CLI bulunamadı",
  "providers.error.claudeNotJson": "Claude kullanım yanıtı JSON değildi",
  "providers.error.claudeEmpty": "Claude kullanım yanıtı boştu",
  "providers.error.opencodeNotJson": "OpenCode Go yanıtı JSON değildi",
  "providers.error.opencodeUnexpected":
    "OpenCode Go kullanım yanıtı beklenmedik biçimdeydi",
  "providers.error.opencodeNotConnected": "OpenCode Go bağlı değil",
  "providers.error.devinVersionUnknown": "Devin CLI sürümü bilinmiyor",
  "providers.error.codexNoData": "Codex kullanım verisi yok",
  "providers.error.codexUnknownReset":
    "Codex bilinmeyen bir sıfırlama sonucu döndürdü",
  "providers.error.codexProbeExited": "Codex kullanım sorgusu sonlandı",
  "providers.error.codexProbeTimedOut":
    "Codex kullanım sorgusu zaman aşımına uğradı",
  "providers.error.piUnexpected":
    "Pi kullanım yanıtı beklenmedik biçimdeydi. Yenilemeyi dene.",
  "providers.error.piFetchFailed": "Pi kullanımı alınamadı. Yenilemeyi dene.",

  // Harness update notice
  "providers.update.ariaLabel": "Harness güncellemeleri",
  "providers.update.titleOne": "Harness güncellemesi var",
  "providers.update.titleMany": "Harness güncellemeleri var",
  "providers.update.updateAll": "Tümünü güncelle",
  "providers.update.dismiss": "Harness güncellemelerini kapat",
  "providers.update.refreshed":
    "Model seçici, yeni sürümün modelleriyle yenilendi.",
  "providers.update.hint": "Yeni modeller çoğunlukla en son sürümü gerektirir.",
  "providers.update.updatedTo": "{version} sürümüne güncellendi",
  "providers.update.updating": "Güncelleniyor",
  "providers.update.retry": "Yeniden dene",
  "providers.update.update": "Güncelle",
  "providers.update.stillOn": "Güncellemeden sonra hâlâ {version} sürümünde.",

  // Connections settings
  "connections.settings.title": "Makinelerin",
  "connections.settings.description":
    "Ajanları başka bir bilgisayarda çalıştır, dizüstü bilgisayarından onlara geri dön. Burada MonoCode'u kapatsan da host çalışmaya devam eder.",
  "connections.settings.addMachine": "Makine ekle",
  "connections.settings.empty":
    "Başlamak için her zaman açık Windows, Mac veya Linux makineni ekle.",
  "connections.settings.sshTarget": "SSH · {target}",
  "connections.settings.sshTargetPort": "SSH · {target} · port {port}",
  "connections.settings.status.checking": "Bağlantı denetleniyor…",
  "connections.settings.status.connected": "Bağlı",
  "connections.settings.status.noProvider":
    "Bağlı · host'a desteklenen bir sağlayıcı kur",
  "connections.settings.status.needsUpdate":
    "Bağlı · Explorer ve Değişiklikler için host güncellemesi gerekli",
  "connections.settings.status.offline":
    "Çevrimdışı · erişimi denetlemek için yeniden bağlan",
  "connections.settings.updateWarning":
    "Güncelleme host'u yeniden başlatır ve etkin ajan turlarını yarıda keser.",
  "connections.settings.updateHost": "Host'u güncelle",
  "connections.settings.updateHostTitle":
    "Uyumlu host paketini indirir ve host'u yeniden başlatır; etkin ajan turları yarıda kesilir",
  "connections.settings.reconnect": "Yeniden bağlan",
  "connections.settings.removeAria": "{name} makinesini kaldır",
  "connections.settings.removeTitle": "Bağlantıyı kaldır…",
  "connections.settings.confirmRemoveAria":
    "{name} makinesini kaldırmayı onayla",
  "connections.settings.removeHeading":
    "{name} bu masaüstünden kaldırılsın mı?",
  "connections.settings.removeBody1":
    "Bu işlem, masaüstünün makineyle olan bağlantısını kapatır. Host'u durdurmaz; oturumlar çalışmaya devam eder ve o makinede kalır. Makineyi daha sonra yeniden ekleyebilirsin.",
  "connections.settings.removeBody2":
    "Yalnızca kaldırırsan bu masaüstünün kimlik bilgisi host'ta geçerli kalır. Geçersiz kılmak için önce erişimi iptal et; makineye ulaşılabiliyor olmalı.",
  "connections.settings.removeBody3":
    "Host'u durdurmak ve arka plan hizmetini kapatmak için o makinede {command} komutunu çalıştır (Windows'ta {windowsCommand}). Oturumlar ve geçmiş korunur.",
  "connections.settings.revokeAndRemove": "Erişimi iptal et ve kaldır",
  "connections.settings.removeOnly": "Yalnızca bu masaüstünden kaldır",
  "connections.settings.cancel": "İptal",
  "connections.settings.revokeFailed":
    "Erişim iptal edilemedi, bu yüzden {name} kaldırılmadı: {reason}. Yeniden bağlanıp tekrar dene ya da yalnızca bu masaüstünden kaldır ve erişimi host'ta monocode-host devices ve monocode-host revoke <device-id> komutlarıyla iptal et.",
  "connections.settings.removedRevoked":
    "{name} kaldırıldı ve bu masaüstünün erişimi iptal edildi. Host ve oturumları çalışmaya devam eder.",
  "connections.settings.removedOnly":
    "{name} bu masaüstünden kaldırıldı. Host ve oturumları çalışmaya devam eder, ayrıca bu masaüstünün kimlik bilgisini hâlâ kabul eder.",
  "connections.settings.updated": "{name} güncellendi ve yeniden bağlandı.",
  "connections.settings.connectedNotice":
    "{name} bağlandı. Üzerinde çalışmak için proje çubuğunda Projeler yanındaki + düğmesine tıkla ve Makinede klasör aç seçeneğini seç.",
  "connections.settings.connectedShort": "{name} bağlandı.",
  "connections.settings.sshHeading": "SSH ile bağlan",
  "connections.settings.sshAddress": "SSH adresi",
  "connections.settings.sshAddressPlaceholder":
    "user@my-mac-mini veya bir SSH takma adı",
  "connections.settings.name": "Ad",
  "connections.settings.optional": "(isteğe bağlı)",
  "connections.settings.namePlaceholder": "İsteğe bağlı, ör. Ev Mac mini",
  "connections.settings.advanced": "Gelişmiş",
  "connections.settings.sshPort": "SSH portu",
  "connections.settings.sshPortPlaceholder": "SSH yapılandırmasından",
  "connections.settings.sshInfo":
    "MonoCode arka plandaki host'u kurar ve başlatır, ardından güvenli biçimde bağlanır. SSH anahtarların ve yapılandırman otomatik kullanılır. Host'ta SSH'ı etkinleştir ve orada Codex veya Claude Code'a giriş yap. Windows ve Mac'te host'un masaüstü hesabını açık, makineyi uyanık tut. Masaüstünü kilitlemekte sakınca yok.",
  "connections.settings.linuxInfo":
    "Linux'ta kurulum bir systemd kullanıcı hizmeti yükler ve hesabın için lingering'i açar ({command}); böylece oturumu kapatsan da host ve diğer kullanıcı hizmetlerin çalışmaya devam eder. Host'u o makinede durdurana kadar çalışır; buradan kaldırmak yalnızca bu masaüstünün bağlantısını keser.",
  "connections.settings.connecting": "Bağlanıyor…",
  "connections.settings.connect": "Bağlan",
  "connections.settings.starting": "Bağlantı başlatılıyor…",
  "connections.settings.passwordAria": "SSH parolası veya parola ifadesi",
  "connections.settings.trustHost": "Host'a güven ve devam et",
  "connections.settings.continue": "Devam",
  "connections.settings.reject": "Reddet",
  "connections.settings.cancelConnection": "Bağlantıyı iptal et",
  "connections.settings.byUrlSummary": "Mevcut bir host'a URL ile bağlan",
  "connections.settings.hostUrl": "Host URL'si",
  "connections.settings.deviceToken": "Cihaz token'ı",
  "connections.settings.connectByUrl": "URL ile bağlan",

  // Open folder on a machine dialog
  "connections.addProject.title": "Makinede klasör aç",
  "connections.addProject.description":
    "Bu projedeki oturumlar o makinede, oradaki checkout ve Codex veya Claude Code girişiyle çalışır. Burada MonoCode'u kapatsan da çalışmaya devam eder.",
  "connections.addProject.noMachines":
    "Henüz bağlı makine yok. Ayarlar'dan bir tane ekle, sonra buradan üzerinde klasör aç.",
  "connections.addProject.cancel": "İptal",
  "connections.addProject.addMachine": "Makine ekle",
  "connections.addProject.machine": "Makine",
  "connections.addProject.onMachine": "{name} üzerinde",
  "connections.addProject.pathAria": "Makinedeki klasör yolu",
  "connections.addProject.folders": "Klasörler",
  "connections.addProject.noSubfolders": "Alt klasör yok",
  "connections.addProject.loadingFolders": "Klasörler yükleniyor…",
  "connections.addProject.opening": "Açılıyor…",
  "connections.addProject.open": "Aç",

  // Remote session
  "connections.session.missingDetails":
    "Bu projenin makine bilgileri eksik. Projeyi proje çubuğundan yeniden ekle.",
  "connections.session.notConnected":
    "Bu projenin makinesi bu bilgisayarda bağlı değil.",
  "connections.session.connecting": "Makineye bağlanılıyor…",
  "connections.session.manageMachines": "Makineleri yönet",
  "connections.session.hostIdentityChanged":
    "Host kimliği değişti. Devam etmeden önce bu makineyi yeniden bağla.",
  "connections.session.wrongProject": "Bu oturum başka bir host projesine ait",
  "connections.session.loadingModels": "Modeller yükleniyor…",
  "connections.session.waiting": "Host'un isteğini onaylaması bekleniyor.",
  "connections.session.retry": "Yeniden dene",
  "connections.session.failedDraft": "Taslak {machine} üzerinde kaydedilemedi.",
  "connections.session.failedSend": "Mesaj {machine} üzerinde gönderilemedi.",
  "connections.session.tryAgain": "Tekrar dene",
  "connections.session.dismiss": "Kapat",
  "connections.session.catalogFailed":
    "{machine} üzerinden modeller yüklenemedi.",
  "connections.session.chooseWorktree": "Bu makinede bir worktree seç",
  "connections.session.worktreeFixed":
    "Bu oturumun worktree'si sabit. Başka birini kullanmak için yeni oturum başlat.",
  "connections.session.selectModel":
    "Bu uzak planı uygulamadan önce mesaj alanında o modeli seç.",

  // Connections model errors
  "connections.error.saveLocally":
    "İsteğin yerel olarak kaydedilemiyor. Göndermeden önce uygulama depolama alanında yer aç.",
  "connections.error.incompatibleHost":
    "Bu makinede uyumsuz bir MonoCode Host çalışıyor",
  "connections.error.tooManyAttachments": "Çok fazla ek var",
  "connections.error.cannotRead": "{name} uzak yükleme için okunamıyor",
  "connections.error.uploadFailed": "{name} yüklemesi tamamlanamadı",
  "connections.error.unavailable":
    "Bu özellik başka bir makinedeki projeler için henüz kullanılamıyor.",
  "connections.error.outdatedHost":
    "Bu projenin dosyalarını kullanmak için Bağlantılar ayarlarından MonoCode Host'u güncelle.",
  "connections.error.oneMachine":
    "Dosyalar yalnızca tek bir makine içinde kopyalanabilir veya taşınabilir.",
  "connections.error.machineNotConnected":
    "Bu projenin makinesi bu bilgisayarda bağlı değil.",

  // Quick composer
  "quickComposer.git.loadFailed": "Bu proje için branch'ler yüklenemedi.",
  "quickComposer.git.loading": "Branch'ler yükleniyor…",
  "quickComposer.git.close": "Kapat",
  "quickComposer.permissions.aria": "İzinler",
  "quickComposer.workspace.newWorktree": "Yeni worktree",
  "quickComposer.workspace.currentCheckout": "Mevcut checkout",
  "quickComposer.workspace.aria": "Çalışma alanı: {label}",
  "quickComposer.workspace.from": "Kaynak: {base}",
  "quickComposer.workspace.noRepo": "Repo yok",
  "quickComposer.workspace.loading": "Yükleniyor…",
  "quickComposer.workspace.createFrom": "{base} üzerinden worktree oluştur",
  "quickComposer.workspace.chooseBranch": "Branch seç",
  "quickComposer.workspace.worktreeGone":
    "Bu worktree artık yok. Başka bir çalışma kopyası seç.",
  "quickComposer.composer.loadingModel": "Model yükleniyor…",
  "quickComposer.composer.dragToMove": "Taşımak için sürükle",
  "quickComposer.composer.closeAria": "Mesaj alanını kapat",
  "quickComposer.composer.closeTitle": "Kapat (Esc)",
  "quickComposer.composer.dropToAttach": "Eklemek için bırak",
  "quickComposer.composer.projectTitle": "Proje (⌘P)",
  "quickComposer.composer.noProject": "Proje yok",
  "quickComposer.composer.attachments": "Ekler",
  "quickComposer.composer.placeholder":
    "{project} içinde bir {harness} oturumu başlat…",
  "quickComposer.composer.openProjectFirst": "Önce MonoCode'da bir proje aç",
  "quickComposer.composer.promptAria": "İstem",
  "quickComposer.composer.unsupportedAttachments":
    "Ek destekleyen bir sağlayıcı seç veya eklenen dosyaları kaldır.",
  "quickComposer.composer.addAttachment": "Ek ekle",
  "quickComposer.composer.attachTitle": "Dosya ekle veya ekran görüntüsü al",
  "quickComposer.composer.attachUnsupported":
    "Bu sağlayıcı ekleri desteklemiyor",
  "quickComposer.composer.modelTitle": "Model (⌘.)",
  "quickComposer.composer.permissionsTitle": "İzinler",
  "quickComposer.composer.addingAttachment": "Ek ekleniyor…",
  "quickComposer.composer.hintStart": "başlat",
  "quickComposer.composer.hintStartAndOpen": "başlat ve aç",
  "quickComposer.composer.saveDraft": "Taslağı kaydet",
  "quickComposer.composer.start": "Başlat",
  "quickComposer.composer.chooseFiles": "Dosya seç…",
  "quickComposer.composer.takeScreenshot": "Ekran görüntüsü al…",
  "quickComposer.composer.commands": "Komutlar",
  "quickComposer.composer.noCommands": "Eşleşen komut yok",
  "quickComposer.composer.findProject": "Proje bul",
  "quickComposer.composer.projects": "Projeler",
  "quickComposer.composer.noMatches": "Eşleşme yok",
  "quickComposer.attachments.tooMany": "En fazla {max} dosya ekleyebilirsin.",
  "quickComposer.attachments.couldNotAttach": "{name} eklenemedi.",
  "quickComposer.models.selector": "Model seçici",
  "quickComposer.models.providers": "Sağlayıcılar",
  "quickComposer.models.favorites": "Favoriler",
  "quickComposer.models.searchAria": "Modellerde ara",
  "quickComposer.models.searchPlaceholder": "Modellerde ara…",
  "quickComposer.models.list": "Modeller",
  "quickComposer.models.removeFavoriteTitle": "Favorilerden çıkar",
  "quickComposer.models.addFavoriteTitle": "Favorilere ekle",
  "quickComposer.models.removeFavoriteAria":
    "{name} modelini favorilerden çıkar",
  "quickComposer.models.addFavoriteAria": "{name} modelini favorilere ekle",
  "quickComposer.models.noMatching": "Eşleşen model yok",
  "quickComposer.models.noFavorites": "Favori model yok",
  "quickComposer.models.loading": "Modeller yükleniyor…",
  "quickComposer.models.fastMode": "Hızlı mod",
  "quickComposer.models.fastOff": "Hızlı modu kapat",
  "quickComposer.models.fastOn": "Hızlı modu aç",
  "quickComposer.models.reset": "Kayıtlı varsayılanlara sıfırla",
};
