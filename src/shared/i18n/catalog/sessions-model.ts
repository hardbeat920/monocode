import type { Translations } from "./types";

export const en = {
  // Runtime (access) modes
  "sessions.mode.supervised.label": "Supervised",
  "sessions.mode.autoAcceptEdits.label": "Auto-accept edits",
  "sessions.mode.auto.label": "Auto",
  "sessions.mode.fullAccess.label": "Full access",
  "sessions.mode.supervised.hint": "Ask before commands and file changes.",
  "sessions.mode.autoAcceptEdits.hint":
    "Auto-approve edits, ask before other actions.",
  "sessions.mode.auto.hint": "An AI reviewer can approve or deny actions.",
  "sessions.mode.fullAccess.hint":
    "Allow commands, edits, and supported MCP confirmations in non-plan turns without prompts.",

  // Session titles
  "sessions.session.newTitle": "New session",
  "sessions.folder.newName": "New folder",
  "sessions.question.defaultTitle": "Question",

  // Slash command descriptions
  "sessions.command.draft.description":
    "Save this message without starting the agent.",
  "sessions.command.mcp.description": "Find an MCP server for this message.",
  "sessions.command.operator.description":
    "Give this thread access to MonoCode sessions, folders, and notes.",
  "sessions.command.orchestrator.description":
    "Plan and coordinate agent work.",
  "sessions.command.addToFolder.description":
    "Place this session in an existing or new sidebar folder.",
  "sessions.command.plan.description":
    "Create a reviewable implementation plan before changing files.",
  "sessions.command.btw.description":
    "Ask a read-only side question about the current turn.",
  "sessions.command.compact.description":
    "Summarize older conversation context to free space.",

  // Usage limit
  "sessions.usageLimit.reset": "{when} · in {duration}",

  // Context meter
  "sessions.context.used": "Context used",
  "sessions.context.usedPercent": "{percent}% context used",
  "sessions.context.tokensOfWindow": "{used} / {window} tokens",
  "sessions.context.tokens": "{used} tokens",

  // Quit / interrupted turns
  "sessions.inFlight.interrupted": "Turn interrupted when MonoCode quit.",
  "sessions.inFlight.quitWhileBusy": {
    one: "{count} chat is still running. Quit anyway? It will resume when you reopen MonoCode.",
    other:
      "{count} chats are still running. Quit anyway? They will resume when you reopen MonoCode.",
  },

  // Work summaries
  "sessions.activity.working": "Working",
  "sessions.activity.thinking": "Thinking",
  "sessions.activity.thought": "Thought",
  "sessions.activity.statusUpdate": "Status update",
  "sessions.activity.filesCount": "{count} files",
  "sessions.activity.editing": "Editing {file}",
  "sessions.activity.edited": "Edited {file}",
  "sessions.activity.reading": "Reading {file}",
  "sessions.activity.read": "Read {file}",
  "sessions.activity.searching": "Searching the project",
  "sessions.activity.searched": "Searched the project",
  "sessions.activity.exploring": "Exploring the project",
  "sessions.activity.explored": "Explored the project",
  "sessions.activity.runningInBackground": "Running in background",
  "sessions.activity.finishedInBackground": "Finished in background",
  "sessions.activity.runningCommand": "Running a command",
  "sessions.activity.ranCommand": "Ran a command",
  "sessions.activity.runningCommands": "Running {count} commands",
  "sessions.activity.ranCommands": "Ran {count} commands",
  "sessions.activity.runningSubagent": "Running a subagent",
  "sessions.activity.ranSubagent": "Ran a subagent",
  "sessions.activity.runningSubagents": "Running {count} subagents",
  "sessions.activity.ranSubagents": "Ran {count} subagents",
  "sessions.activity.runningTool": "Running a tool",
  "sessions.activity.ranTool": "Ran a tool",
  "sessions.activity.runningTools": "Running {count} tools",
  "sessions.activity.ranTools": "Ran {count} tools",
  "sessions.activity.oneNote": "1 note",
  "sessions.activity.notes": "{count} notes",
  "sessions.activity.usingMonoCode": "Using MonoCode",
  "sessions.activity.usedMonoCode": "Used MonoCode",
  "sessions.activity.subagent": "Subagent",
  "sessions.activity.subagentFailed": "Subagent failed",
  "sessions.activity.subagentsFailed": "{count} subagents failed",
  "sessions.activity.handoffPreparing": "Preparing a handoff",
  "sessions.activity.done": "Done",

  // Mono work status
  "sessions.monoStatus.thinking1": "Thinking…",
  "sessions.monoStatus.thinking2": "Pondering…",
  "sessions.monoStatus.thinking3": "Considering…",
  "sessions.monoStatus.thinking4": "Working it through…",
  "sessions.monoStatus.waitingApproval": "Waiting for approval…",
  "sessions.monoStatus.runCommand": "Running command…",
  "sessions.monoStatus.editFile": "Editing file…",
  "sessions.monoStatus.readFile": "Reading file…",
  "sessions.monoStatus.exploreProject": "Exploring project…",
  "sessions.monoStatus.runAgent": "Running agent…",
  "sessions.monoStatus.useTool": "Using tool…",

  // MCP picker
  "sessions.mcp.detail.differentProvider": "Different provider",
  "sessions.mcp.detail.disabled": "Disabled in provider configuration",
  "sessions.mcp.detail.unavailable": "Connection unavailable",
  "sessions.mcp.detail.configured": "Configured for this provider",

  // Task list and plan
  "sessions.taskList.complete": "Complete",
  "sessions.taskList.progress": "{completed} of {total}",
  "sessions.plan.title": "Plan",
  "sessions.plan.sections": "{count} sections",
  "sessions.plan.diagram": "diagram",
  "sessions.plan.words": "{count} words",

  // Prompt outline
  "sessions.prompt.handoff": "Handoff",
  "sessions.prompt.secondOpinion": "Second opinion",
  "sessions.prompt.secondOpinionWith": "Second opinion: {request}",
  "sessions.prompt.empty": "Empty message",

  // MonoCode CLI tool call labels
  "sessions.cliAction.modelsList": "List models",
  "sessions.cliAction.sessionsList": "List sessions",
  "sessions.cliAction.sessionsRead": "Read a session",
  "sessions.cliAction.sessionsSend": "Continue a session",
  "sessions.cliAction.sessionsDraft": "Save a draft",
  "sessions.cliAction.sessionsStart": "Start a session",
  "sessions.cliAction.sessionsStop": "Stop a session",
  "sessions.cliAction.sessionsArchive": "Archive a session",
  "sessions.cliAction.sessionsDelete": "Delete a session",
  "sessions.cliAction.foldersList": "List folders",
  "sessions.cliAction.foldersMove": "Move a session",
  "sessions.cliAction.notesList": "List notes",
  "sessions.cliAction.notesRead": "Read a note",
  "sessions.cliAction.notesWrite": "Write a note",
  "sessions.cliAction.artifactsList": "List artifacts",
  "sessions.cliAction.artifactsRead": "Read an artifact",
  "sessions.cliAction.artifactsWrite": "Write an artifact",
  "sessions.cliAction.help": "View CLI commands",

  // Errors
  "sessions.removal.saveFailed": "The conversation could not be saved.",
  "sessions.btw.turnUnavailable": "The completed turn is no longer available.",
  "sessions.btw.interruptedBeforeReload":
    "This by-the-way request was interrupted before reload.",
  "sessions.fileDrop.noFile":
    "This drag did not provide a file. Save the image, then drag the saved file here.",
} as const;

export const tr: Translations<typeof en> = {
  // Runtime (access) modes
  "sessions.mode.supervised.label": "Denetimli",
  "sessions.mode.autoAcceptEdits.label": "Düzenlemeleri otomatik kabul et",
  "sessions.mode.auto.label": "Otomatik",
  "sessions.mode.fullAccess.label": "Tam erişim",
  "sessions.mode.supervised.hint":
    "Komutlardan ve dosya değişikliklerinden önce sorar.",
  "sessions.mode.autoAcceptEdits.hint":
    "Düzenlemeleri otomatik onaylar, diğer işlemlerden önce sorar.",
  "sessions.mode.auto.hint":
    "Bir yapay zeka denetçisi işlemleri onaylayabilir veya reddedebilir.",
  "sessions.mode.fullAccess.hint":
    "Plan dışındaki turlarda komutlara, düzenlemelere ve desteklenen MCP onaylarına sormadan izin verir.",

  // Session titles
  "sessions.session.newTitle": "Yeni oturum",
  "sessions.folder.newName": "Yeni klasör",
  "sessions.question.defaultTitle": "Soru",

  // Slash command descriptions
  "sessions.command.draft.description": "Bu mesajı ajanı başlatmadan kaydeder.",
  "sessions.command.mcp.description": "Bu mesaj için bir MCP sunucusu bulur.",
  "sessions.command.operator.description":
    "Bu konuşmaya MonoCode oturumlarına, klasörlerine ve notlarına erişim verir.",
  "sessions.command.orchestrator.description":
    "Ajan çalışmalarını planlar ve koordine eder.",
  "sessions.command.addToFolder.description":
    "Bu oturumu mevcut veya yeni bir kenar çubuğu klasörüne yerleştirir.",
  "sessions.command.plan.description":
    "Dosyaları değiştirmeden önce gözden geçirilebilir bir uygulama planı oluşturur.",
  "sessions.command.btw.description":
    "Mevcut tur hakkında salt okunur bir yan soru sorar.",
  "sessions.command.compact.description":
    "Yer açmak için eski konuşma bağlamını özetler.",

  // Usage limit
  "sessions.usageLimit.reset": "{when} · {duration} içinde",

  // Context meter
  "sessions.context.used": "Kullanılan bağlam",
  "sessions.context.usedPercent": "Bağlamın %{percent} kadarı kullanıldı",
  "sessions.context.tokensOfWindow": "{used} / {window} token",
  "sessions.context.tokens": "{used} token",

  // Quit / interrupted turns
  "sessions.inFlight.interrupted": "MonoCode kapandığında tur yarıda kesildi.",
  "sessions.inFlight.quitWhileBusy": {
    one: "{count} sohbet hâlâ çalışıyor. Yine de çıkılsın mı? MonoCode'u yeniden açtığında devam edecek.",
    other:
      "{count} sohbet hâlâ çalışıyor. Yine de çıkılsın mı? MonoCode'u yeniden açtığında devam edecekler.",
  },

  // Work summaries
  "sessions.activity.working": "Çalışıyor",
  "sessions.activity.thinking": "Düşünüyor",
  "sessions.activity.thought": "Düşündü",
  "sessions.activity.statusUpdate": "Durum güncellemesi",
  "sessions.activity.filesCount": "{count} dosya",
  "sessions.activity.editing": "{file} düzenleniyor",
  "sessions.activity.edited": "{file} düzenlendi",
  "sessions.activity.reading": "{file} okunuyor",
  "sessions.activity.read": "{file} okundu",
  "sessions.activity.searching": "Proje aranıyor",
  "sessions.activity.searched": "Proje arandı",
  "sessions.activity.exploring": "Proje inceleniyor",
  "sessions.activity.explored": "Proje incelendi",
  "sessions.activity.runningInBackground": "Arka planda çalışıyor",
  "sessions.activity.finishedInBackground": "Arka planda tamamlandı",
  "sessions.activity.runningCommand": "Komut çalıştırılıyor",
  "sessions.activity.ranCommand": "Komut çalıştırıldı",
  "sessions.activity.runningCommands": "{count} komut çalıştırılıyor",
  "sessions.activity.ranCommands": "{count} komut çalıştırıldı",
  "sessions.activity.runningSubagent": "Alt ajan çalıştırılıyor",
  "sessions.activity.ranSubagent": "Alt ajan çalıştırıldı",
  "sessions.activity.runningSubagents": "{count} alt ajan çalıştırılıyor",
  "sessions.activity.ranSubagents": "{count} alt ajan çalıştırıldı",
  "sessions.activity.runningTool": "Araç çalıştırılıyor",
  "sessions.activity.ranTool": "Araç çalıştırıldı",
  "sessions.activity.runningTools": "{count} araç çalıştırılıyor",
  "sessions.activity.ranTools": "{count} araç çalıştırıldı",
  "sessions.activity.oneNote": "1 not",
  "sessions.activity.notes": "{count} not",
  "sessions.activity.usingMonoCode": "MonoCode kullanılıyor",
  "sessions.activity.usedMonoCode": "MonoCode kullanıldı",
  "sessions.activity.subagent": "Alt ajan",
  "sessions.activity.subagentFailed": "Alt ajan başarısız oldu",
  "sessions.activity.subagentsFailed": "{count} alt ajan başarısız oldu",
  "sessions.activity.handoffPreparing": "Devir hazırlanıyor",
  "sessions.activity.done": "Tamamlandı",

  // Mono work status
  "sessions.monoStatus.thinking1": "Düşünüyor…",
  "sessions.monoStatus.thinking2": "Derin düşünüyor…",
  "sessions.monoStatus.thinking3": "Değerlendiriyor…",
  "sessions.monoStatus.thinking4": "Üzerinde çalışıyor…",
  "sessions.monoStatus.waitingApproval": "Onay bekleniyor…",
  "sessions.monoStatus.runCommand": "Komut çalıştırılıyor…",
  "sessions.monoStatus.editFile": "Dosya düzenleniyor…",
  "sessions.monoStatus.readFile": "Dosya okunuyor…",
  "sessions.monoStatus.exploreProject": "Proje inceleniyor…",
  "sessions.monoStatus.runAgent": "Ajan çalıştırılıyor…",
  "sessions.monoStatus.useTool": "Araç kullanılıyor…",

  // MCP picker
  "sessions.mcp.detail.differentProvider": "Farklı sağlayıcı",
  "sessions.mcp.detail.disabled": "Sağlayıcı yapılandırmasında devre dışı",
  "sessions.mcp.detail.unavailable": "Bağlantı kullanılamıyor",
  "sessions.mcp.detail.configured": "Bu sağlayıcı için yapılandırıldı",

  // Task list and plan
  "sessions.taskList.complete": "Tamamlandı",
  "sessions.taskList.progress": "{completed} / {total}",
  "sessions.plan.title": "Plan",
  "sessions.plan.sections": "{count} bölüm",
  "sessions.plan.diagram": "diyagram",
  "sessions.plan.words": "{count} kelime",

  // Prompt outline
  "sessions.prompt.handoff": "Devir",
  "sessions.prompt.secondOpinion": "İkinci görüş",
  "sessions.prompt.secondOpinionWith": "İkinci görüş: {request}",
  "sessions.prompt.empty": "Boş mesaj",

  // MonoCode CLI tool call labels
  "sessions.cliAction.modelsList": "Modelleri listele",
  "sessions.cliAction.sessionsList": "Oturumları listele",
  "sessions.cliAction.sessionsRead": "Bir oturumu oku",
  "sessions.cliAction.sessionsSend": "Bir oturuma devam et",
  "sessions.cliAction.sessionsDraft": "Taslak kaydet",
  "sessions.cliAction.sessionsStart": "Oturum başlat",
  "sessions.cliAction.sessionsStop": "Bir oturumu durdur",
  "sessions.cliAction.sessionsArchive": "Bir oturumu arşivle",
  "sessions.cliAction.sessionsDelete": "Bir oturumu sil",
  "sessions.cliAction.foldersList": "Klasörleri listele",
  "sessions.cliAction.foldersMove": "Bir oturumu taşı",
  "sessions.cliAction.notesList": "Notları listele",
  "sessions.cliAction.notesRead": "Bir notu oku",
  "sessions.cliAction.notesWrite": "Bir not yaz",
  "sessions.cliAction.artifactsList": "Artifact'leri listele",
  "sessions.cliAction.artifactsRead": "Bir artifact oku",
  "sessions.cliAction.artifactsWrite": "Bir artifact yaz",
  "sessions.cliAction.help": "CLI komutlarını görüntüle",

  // Errors
  "sessions.removal.saveFailed": "Konuşma kaydedilemedi.",
  "sessions.btw.turnUnavailable": "Tamamlanan tur artık mevcut değil.",
  "sessions.btw.interruptedBeforeReload":
    "Bu yan istek yeniden yüklenmeden önce yarıda kesildi.",
  "sessions.fileDrop.noFile":
    "Bu sürükleme bir dosya sağlamadı. Görseli kaydedin, ardından kaydedilen dosyayı buraya sürükleyin.",
};
