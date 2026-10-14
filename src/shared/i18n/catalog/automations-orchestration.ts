import type { Translations } from "./types";

export const en = {
  // Automations: shell and list
  "automations.title": "Automations",
  "automations.filter": "Filter automations",
  "automations.new": "New automation",
  "automations.list.noMatch": "No matching automations",
  "automations.list.empty": "No automations yet",
  "automations.card.open": "Open {name}",
  "automations.card.pause": "Pause {name}",
  "automations.card.enable": "Enable {name}",
  "automations.confirmDelete": "Delete “{name}” and its run history?",

  // Automations: picker
  "automations.picker.subtitle": "Pick an example or start from scratch.",
  "automations.picker.scratch": "Start from scratch",
  "automations.picker.scratchHint":
    "Write your own instructions and choose a trigger.",
  "automations.category.popular": "Popular",
  "automations.category.review": "Code Review",
  "automations.category.security": "Security",
  "automations.category.incidents": "Incidents & Triage",
  "automations.category.research": "Data & Research",
  "automations.category.environment": "Environment",

  // Automations: templates
  "automations.template.findCriticalBugs.name": "Find critical bugs",
  "automations.template.findCriticalBugs.description":
    "Analyze recent commits for high-severity correctness bugs and submit safe fixes",
  "automations.template.scanVulnerabilities.name":
    "Scan codebase for vulnerabilities",
  "automations.template.scanVulnerabilities.description":
    "Review the full repository on a schedule and alert on validated high-impact security issues",
  "automations.template.generateDocs.name": "Generate docs",
  "automations.template.generateDocs.description":
    "Create and update developer documentation for recently changed or under-documented code",
  "automations.template.addTestCoverage.name": "Add test coverage",
  "automations.template.addTestCoverage.description":
    "Review recent changes and add tests for high-risk logic that lacks adequate coverage",
  "automations.template.reviewPullRequests.name": "Review pull requests",
  "automations.template.reviewPullRequests.description":
    "When a pull request is opened, review the diff for bugs, regressions, and missing tests",
  "automations.template.reviewDraftPrs.name": "Review draft PRs",
  "automations.template.reviewDraftPrs.description":
    "Give early feedback when a draft pull request is opened so issues are caught before review",
  "automations.template.dependencyAudit.name": "Audit dependencies",
  "automations.template.dependencyAudit.description":
    "Check lockfiles and manifests for vulnerable, abandoned, or unexpectedly upgraded packages",
  "automations.template.secretScan.name": "Scan for secrets",
  "automations.template.secretScan.description":
    "Search the working tree and recent history for committed credentials, tokens, and keys",
  "automations.template.triageGithubIssues.name": "Triage GitHub issues",
  "automations.template.triageGithubIssues.description":
    "When a GitHub issue is opened, inspect the repo and add a concrete reproduction or next step",
  "automations.template.triageNewIssues.name": "Triage new issues",
  "automations.template.triageNewIssues.description":
    "When a Linear issue is created, inspect the repo and add a concrete reproduction or next step",
  "automations.template.failingCiWatch.name": "Watch failing checks",
  "automations.template.failingCiWatch.description":
    "On a weekday morning, run the project's tests and diagnose anything that is already red",
  "automations.template.weeklyChangelog.name": "Weekly changelog",
  "automations.template.weeklyChangelog.description":
    "Summarize the week's commits into a changelog humans can actually read",
  "automations.template.repoHealth.name": "Repo health check",
  "automations.template.repoHealth.description":
    "Inspect the working tree, stale branches, and obvious project-setup drift on a schedule",
  "automations.template.installDoctor.name": "Environment doctor",
  "automations.template.installDoctor.description":
    "Verify the project still installs and boots from a clean working copy",

  // Automations: schedules, dates and durations
  "automations.weekday.sunday": "Sunday",
  "automations.weekday.monday": "Monday",
  "automations.weekday.tuesday": "Tuesday",
  "automations.weekday.wednesday": "Wednesday",
  "automations.weekday.thursday": "Thursday",
  "automations.weekday.friday": "Friday",
  "automations.weekday.saturday": "Saturday",
  "automations.schedule.hourlyAt": "Hourly at :{minute}",
  "automations.schedule.dailyAt": "Daily at {time}",
  "automations.schedule.weekdaysAt": "Weekdays at {time}",
  "automations.schedule.weeklyAt": "{day} at {time}",
  "automations.schedule.weeklyFallback": "Weekly at {time}",
  "automations.schedule.nextRun": "Next run {day}, {time} {zone}",
  "automations.duration.lessThanMinute": "< 1m",
  "automations.duration.minutes": "{minutes}m",
  "automations.duration.hours": "{hours}h",
  "automations.duration.hoursMinutes": "{hours}h {minutes}m",

  // Automations: run history
  "automations.run.openSession": "Open session",
  "automations.run.noSession": "This run has no session yet",
  "automations.run.testRun": "Test run",
  "automations.run.scheduled": "Scheduled",
  "automations.run.scheduledWith": "Scheduled · {schedule}",
  "automations.run.status.succeeded": "Succeeded",
  "automations.run.status.failed": "Failed",
  "automations.run.status.skipped": "Skipped",
  "automations.run.status.cancelled": "Cancelled",
  "automations.run.status.running": "Running",
  "automations.run.status.pending": "Pending",
  "automations.history.title": "Run history",
  "automations.history.trigger": "Trigger",
  "automations.history.triggered": "Triggered",
  "automations.history.status": "Status",
  "automations.history.duration": "Duration",
  "automations.history.empty": "This automation has not run yet.",

  // Automations: editor
  "automations.editor.name": "Automation name",
  "automations.editor.untitled": "Untitled",
  "automations.editor.reset": "Reset",
  "automations.editor.cancel": "Cancel",
  "automations.editor.runNow": "Run now",
  "automations.editor.save": "Save",
  "automations.editor.create": "Create",
  "automations.editor.pause": "Pause automation",
  "automations.editor.enable": "Enable automation",
  "automations.editor.active": "Active",
  "automations.editor.inactive": "Inactive",
  "automations.editor.actions": "Automation actions",
  "automations.editor.delete": "Delete automation",
  "automations.editor.view": "Automation view",
  "automations.editor.tabSettings": "Settings",
  "automations.editor.instructions": "Instructions",
  "automations.editor.instructionsHint":
    "Skills, @file references, and built-in commands work here.",
  "automations.editor.sessionSection": "Session",
  "automations.editor.workingCopy": "Working copy",
  "automations.editor.workingCopyHint": "This repo, or a fresh worktree",
  "automations.editor.conversation": "Conversation",
  "automations.editor.conversationHint": "New chat, or continue the last run",
  "automations.editor.sessionFolder": "Session folder",
  "automations.editor.sessionFolderHint": "Where runs appear in the sidebar",
  "automations.editor.folderNone": "None",
  "automations.editor.folderRemoved": "Removed folder",
  "automations.editor.advanced": "Advanced",
  "automations.editor.advancedHint": "Catch-up window for missed runs",
  "automations.editor.missedGrace": "Missed-run grace",
  "automations.editor.missedGraceHint":
    "Catch up if a scheduled run was missed",
  "automations.option.workspaceCurrent": "Current",
  "automations.option.workspaceWorktree": "Fresh worktree",
  "automations.option.conversationFresh": "Start fresh",
  "automations.option.conversationReuse": "Continue last",
  "automations.option.graceNone": "Do not catch up",
  "automations.option.grace30": "30 minutes",
  "automations.option.grace120": "2 hours",
  "automations.option.grace720": "12 hours",
  "automations.option.grace1440": "24 hours",

  // Automations: triggers
  "automations.trigger.section": "Triggers",
  "automations.trigger.add": "Add Trigger",
  "automations.trigger.choose": "Choose automation trigger",
  "automations.trigger.search": "Search triggers",
  "automations.trigger.connect": "Connect {provider} in Settings",
  "automations.trigger.notConnected": "Not connected",
  "automations.trigger.noMatch": "No matching triggers",
  "automations.trigger.events": "{trigger} events",
  "automations.trigger.none": "No trigger",
  "automations.trigger.remove": "Remove trigger",
  "automations.trigger.scheduled": "Scheduled",
  "automations.trigger.hourly": "Hourly",
  "automations.trigger.daily": "Daily",
  "automations.trigger.weekdays": "Weekdays",
  "automations.trigger.weekly": "Weekly",
  "automations.trigger.draftOpened": "Draft opened",
  "automations.trigger.pullRequestOpened": "Pull request opened",
  "automations.trigger.issueOpened": "Issue opened",
  "automations.trigger.issueCreated": "Issue created",
  "automations.trigger.issueAppeared": "Issue appeared",
  "automations.trigger.mergeRequestOpened": "Merge request opened",
  "automations.trigger.pullRequestAppeared": "Pull request appeared",
  "automations.trigger.workItemAppeared": "Work item appeared",
  "automations.trigger.everyHourAt": "Every hour at",
  "automations.trigger.everyDayAt": "Every day at",
  "automations.trigger.everyWeekdayAt": "Every weekday at",
  "automations.trigger.everyWeekOn": "Every week on",
  "automations.trigger.at": "at",
  "automations.trigger.day": "Day",
  "automations.trigger.minute": "Minute",
  "automations.trigger.time": "Time",
  "automations.trigger.push": "Push",
  "automations.trigger.on": "on",
  "automations.trigger.branch": "Branch",
  "automations.trigger.selectBranch": "Select a branch",
  "automations.trigger.noBranches": "No branches found",
  "automations.trigger.chooseProjectFirst": "Choose a project first",
  "automations.trigger.by": "by",
  "automations.trigger.actor": "Actor",
  "automations.trigger.anyone": "Anyone",

  // Orchestration: sidebar card
  "orchestration.sidebar.agents": {
    one: "{count} agent",
    other: "{count} agents",
  },
  "orchestration.sidebar.done": "{done}/{total} done",
  "orchestration.sidebar.list": "Orchestrated agents",
  "orchestration.sidebar.details": "Agent details: {title}",
  "orchestration.sidebar.openBeside": "Open this agent beside the orchestrator",
  "orchestration.sidebar.seeDetails": "See details",
  "orchestration.sidebar.cancelTask": "Cancel task",
  "orchestration.sidebar.stoppingInterrupted":
    "Stopping interrupted work before this run can resume.",
  "orchestration.sidebar.waitingLead":
    "Waiting for the lead's interrupted turn to finish before this run can resume.",
  "orchestration.sidebar.resumeInfo":
    "Resume continues interrupted workers from their retained checkouts and starts queued work. Policy-blocked tasks stay stopped for review.",
  "orchestration.sidebar.anotherConversation": "Another conversation",
  "orchestration.sidebar.blockerRunning":
    "{title} is still running in this project.",
  "orchestration.sidebar.openBlocker": "Open blocker",
  "orchestration.sidebar.waitAgents": "Wait for interrupted agents to stop",
  "orchestration.sidebar.waitLead":
    "Wait for the lead's interrupted turn to finish",
  "orchestration.sidebar.otherConversation": "the other conversation",
  "orchestration.sidebar.stopBefore": "Stop {title} before resuming",
  "orchestration.sidebar.continueWork": "Continue interrupted and queued work",
  "orchestration.sidebar.resume": "Resume",

  // Orchestration: task and run status
  "orchestration.task.needsInput": "Needs input",
  "orchestration.task.saved": "Saved",
  "orchestration.task.paused": "Paused",
  "orchestration.task.queued": "Queued",
  "orchestration.task.running": "Working",
  "orchestration.task.cancelling": "Stopping",
  "orchestration.task.completed": "Done",
  "orchestration.task.failed": "Failed",
  "orchestration.task.blocked": "Needs review",
  "orchestration.task.interrupted": "Interrupted",
  "orchestration.task.cancelled": "Cancelled",
  "orchestration.run.active": "active",
  "orchestration.run.paused": "paused",
  "orchestration.run.stopped": "stopped",
  "orchestration.run.finished": "finished",

  // Orchestration: proposal card
  "orchestration.preview.modelFor": "Model for {title}",
  "orchestration.preview.searchModelsLabel": "Search assignment models",
  "orchestration.preview.searchModelsPlaceholder":
    "Search models or harnesses…",
  "orchestration.preview.modelsList": "Assignment models",
  "orchestration.preview.noModels": "No matching models",
  "orchestration.preview.effort": "{name} effort",
  "orchestration.preview.workersHelpLabel": "What parallel workers means",
  "orchestration.preview.workersHelpTitle": "How many workers run at once",
  "orchestration.preview.workersHelpBody":
    "The rest of the tasks wait their turn, and a task that depends on another waits for it either way. Every worker edits this same project folder, so a lower number means fewer changes landing in it at the same time.",
  "orchestration.preview.proposal": "Orchestration proposal",
  "orchestration.preview.planning": "Planning assignments…",
  "orchestration.preview.lead": "Lead · {name}",
  "orchestration.preview.tasks": {
    one: "{count} task",
    other: "{count} tasks",
  },
  "orchestration.preview.tryAgain": "Try again",
  "orchestration.preview.starting": "Starting…",
  "orchestration.preview.confirm": "Confirm & start",
  "orchestration.preview.viewAgents": "View agents",
  "orchestration.preview.choosing":
    "Your lead is choosing tasks and worker models. Review the assignments here before starting.",
  "orchestration.preview.checking": "Checking available harnesses and models…",
  "orchestration.preview.details": "Details for {title}",
  "orchestration.preview.task": "Task",
  "orchestration.preview.taskTitle": "Title for task {number}",
  "orchestration.preview.instructions": "Instructions",
  "orchestration.preview.taskInstructions": "Instructions for task {number}",
  "orchestration.preview.after": "After · {tasks}",
  "orchestration.preview.showFewer": "Show fewer tasks",
  "orchestration.preview.showMore": {
    one: "Show {count} more task",
    other: "Show {count} more tasks",
  },
  "orchestration.preview.parallelWorkers": "Parallel workers",
  "orchestration.preview.parallel": "{workers} parallel",
  "orchestration.preview.approved": "Approved",
  "orchestration.preview.awaiting": "Awaiting confirmation",
  "orchestration.preview.sharedFolder": "Shared project folder",

  // Orchestration: errors shown to the user
  "orchestration.error.alreadyStarting": "The run is already starting",
  "orchestration.error.reviewFirst":
    "Review a completed proposal before starting",
  "orchestration.error.returnToCheckout":
    "Return to the proposal's checkout before starting",
  "orchestration.error.modelUnavailable":
    "An assigned model is no longer available. Change that assignment before starting.",
  "orchestration.error.leadUnavailable":
    "The orchestration lead is unavailable",
  "orchestration.error.waitLeadResume":
    "Wait for the lead's interrupted turn to finish before resuming orchestration",
  "orchestration.error.waitLeadTurn":
    "Wait for the lead's current turn to finish",
  "orchestration.error.chooseWorkers": "Choose 1 to 4 workers",
  "orchestration.error.chooseHarnesses": "Choose installed worker harnesses",
  "orchestration.error.stopCurrent":
    "Stop the current run before starting another proposal",
  "orchestration.error.waitAgentsResume":
    "Wait for interrupted agents to stop before resuming orchestration",
  "orchestration.error.stopWorkers":
    "Stop active workers before changing the run",
  "orchestration.error.returnLead":
    "Return the lead to its original checkout before resuming orchestration",
  "orchestration.error.blockerResume":
    '"{label}" is still running in this checkout. Stop it before resuming orchestration.',
  "orchestration.error.blockerStart":
    '"{label}" is still running in this checkout. Stop it before starting orchestration.',
  "orchestration.error.parallelRange": "Choose 1 to 4 parallel workers",
  "orchestration.error.noWorkerModels":
    "No worker models are available, or the model catalog is too large. Check your connected harnesses.",
  "orchestration.error.planningInterrupted":
    "Planning was interrupted. Generate the assignments again.",
} as const;

export const tr: Translations<typeof en> = {
  // Automations: shell and list
  "automations.title": "Otomasyonlar",
  "automations.filter": "Otomasyonları filtrele",
  "automations.new": "Yeni otomasyon",
  "automations.list.noMatch": "Eşleşen otomasyon yok",
  "automations.list.empty": "Henüz otomasyon yok",
  "automations.card.open": "{name} otomasyonunu aç",
  "automations.card.pause": "{name} otomasyonunu duraklat",
  "automations.card.enable": "{name} otomasyonunu etkinleştir",
  "automations.confirmDelete":
    "“{name}” otomasyonu ve çalıştırma geçmişi silinsin mi?",

  // Automations: picker
  "automations.picker.subtitle": "Bir örnek seç ya da sıfırdan başla.",
  "automations.picker.scratch": "Sıfırdan başla",
  "automations.picker.scratchHint":
    "Kendi talimatlarını yaz ve bir tetikleyici seç.",
  "automations.category.popular": "Popüler",
  "automations.category.review": "Kod incelemesi",
  "automations.category.security": "Güvenlik",
  "automations.category.incidents": "Olaylar ve sınıflandırma",
  "automations.category.research": "Veri ve araştırma",
  "automations.category.environment": "Ortam",

  // Automations: templates
  "automations.template.findCriticalBugs.name": "Kritik hataları bul",
  "automations.template.findCriticalBugs.description":
    "Son commit'leri yüksek önem derecesindeki doğruluk hataları için incele ve güvenli düzeltmeleri gönder",
  "automations.template.scanVulnerabilities.name":
    "Kod tabanını güvenlik açıkları için tara",
  "automations.template.scanVulnerabilities.description":
    "Tüm depoyu zamanlanmış olarak incele ve doğrulanmış yüksek etkili güvenlik sorunlarını bildir",
  "automations.template.generateDocs.name": "Dokümantasyon oluştur",
  "automations.template.generateDocs.description":
    "Yakın zamanda değişen veya yeterince belgelenmemiş kod için geliştirici dokümantasyonunu oluştur ve güncelle",
  "automations.template.addTestCoverage.name": "Test kapsamını artır",
  "automations.template.addTestCoverage.description":
    "Son değişiklikleri incele ve yeterince kapsanmayan yüksek riskli mantık için test ekle",
  "automations.template.reviewPullRequests.name": "Pull request'leri incele",
  "automations.template.reviewPullRequests.description":
    "Bir pull request açıldığında diff'i hatalar, gerilemeler ve eksik testler açısından incele",
  "automations.template.reviewDraftPrs.name": "Taslak PR'ları incele",
  "automations.template.reviewDraftPrs.description":
    "Taslak bir pull request açıldığında erken geri bildirim ver, böylece sorunlar incelemeden önce yakalanır",
  "automations.template.dependencyAudit.name": "Bağımlılıkları denetle",
  "automations.template.dependencyAudit.description":
    "Lockfile ve manifest dosyalarında savunmasız, terk edilmiş veya beklenmedik şekilde yükseltilmiş paketleri kontrol et",
  "automations.template.secretScan.name": "Gizli bilgileri tara",
  "automations.template.secretScan.description":
    "Çalışma ağacında ve yakın geçmişte commit edilmiş kimlik bilgilerini, token'ları ve anahtarları ara",
  "automations.template.triageGithubIssues.name":
    "GitHub issue'larını sınıflandır",
  "automations.template.triageGithubIssues.description":
    "Bir GitHub issue'su açıldığında depoyu incele ve somut bir yeniden üretim adımı ya da sonraki adım ekle",
  "automations.template.triageNewIssues.name": "Yeni issue'ları sınıflandır",
  "automations.template.triageNewIssues.description":
    "Bir Linear issue'su oluşturulduğunda depoyu incele ve somut bir yeniden üretim adımı ya da sonraki adım ekle",
  "automations.template.failingCiWatch.name": "Başarısız kontrolleri izle",
  "automations.template.failingCiWatch.description":
    "Hafta içi sabahları projenin testlerini çalıştır ve zaten başarısız olan her şeyi teşhis et",
  "automations.template.weeklyChangelog.name": "Haftalık changelog",
  "automations.template.weeklyChangelog.description":
    "Haftanın commit'lerini insanların gerçekten okuyabileceği bir changelog'a özetle",
  "automations.template.repoHealth.name": "Depo sağlık kontrolü",
  "automations.template.repoHealth.description":
    "Çalışma ağacını, eski branch'leri ve belirgin proje kurulumu sapmalarını zamanlanmış olarak incele",
  "automations.template.installDoctor.name": "Ortam doktoru",
  "automations.template.installDoctor.description":
    "Projenin temiz bir çalışma kopyasından hâlâ kurulup açıldığını doğrula",

  // Automations: schedules, dates and durations
  "automations.weekday.sunday": "Pazar",
  "automations.weekday.monday": "Pazartesi",
  "automations.weekday.tuesday": "Salı",
  "automations.weekday.wednesday": "Çarşamba",
  "automations.weekday.thursday": "Perşembe",
  "automations.weekday.friday": "Cuma",
  "automations.weekday.saturday": "Cumartesi",
  "automations.schedule.hourlyAt": "Saatlik, :{minute}",
  "automations.schedule.dailyAt": "Her gün {time}",
  "automations.schedule.weekdaysAt": "Hafta içi {time}",
  "automations.schedule.weeklyAt": "{day} {time}",
  "automations.schedule.weeklyFallback": "Haftalık {time}",
  "automations.schedule.nextRun": "Sonraki çalıştırma {day}, {time} {zone}",
  "automations.duration.lessThanMinute": "< 1 dk",
  "automations.duration.minutes": "{minutes} dk",
  "automations.duration.hours": "{hours} sa",
  "automations.duration.hoursMinutes": "{hours} sa {minutes} dk",

  // Automations: run history
  "automations.run.openSession": "Oturumu aç",
  "automations.run.noSession": "Bu çalıştırmanın henüz oturumu yok",
  "automations.run.testRun": "Test çalıştırması",
  "automations.run.scheduled": "Zamanlanmış",
  "automations.run.scheduledWith": "Zamanlanmış · {schedule}",
  "automations.run.status.succeeded": "Başarılı",
  "automations.run.status.failed": "Başarısız",
  "automations.run.status.skipped": "Atlandı",
  "automations.run.status.cancelled": "İptal edildi",
  "automations.run.status.running": "Çalışıyor",
  "automations.run.status.pending": "Bekliyor",
  "automations.history.title": "Çalıştırma geçmişi",
  "automations.history.trigger": "Tetikleyici",
  "automations.history.triggered": "Tetiklenme",
  "automations.history.status": "Durum",
  "automations.history.duration": "Süre",
  "automations.history.empty": "Bu otomasyon henüz çalışmadı.",

  // Automations: editor
  "automations.editor.name": "Otomasyon adı",
  "automations.editor.untitled": "Adsız",
  "automations.editor.reset": "Sıfırla",
  "automations.editor.cancel": "İptal",
  "automations.editor.runNow": "Şimdi çalıştır",
  "automations.editor.save": "Kaydet",
  "automations.editor.create": "Oluştur",
  "automations.editor.pause": "Otomasyonu duraklat",
  "automations.editor.enable": "Otomasyonu etkinleştir",
  "automations.editor.active": "Etkin",
  "automations.editor.inactive": "Devre dışı",
  "automations.editor.actions": "Otomasyon işlemleri",
  "automations.editor.delete": "Otomasyonu sil",
  "automations.editor.view": "Otomasyon görünümü",
  "automations.editor.tabSettings": "Ayarlar",
  "automations.editor.instructions": "Talimatlar",
  "automations.editor.instructionsHint":
    "Skill'ler, @file başvuruları ve yerleşik komutlar burada çalışır.",
  "automations.editor.sessionSection": "Oturum",
  "automations.editor.workingCopy": "Çalışma kopyası",
  "automations.editor.workingCopyHint": "Bu depo ya da yeni bir worktree",
  "automations.editor.conversation": "Konuşma",
  "automations.editor.conversationHint":
    "Yeni sohbet ya da son çalıştırmaya devam",
  "automations.editor.sessionFolder": "Oturum klasörü",
  "automations.editor.sessionFolderHint":
    "Çalıştırmaların kenar çubuğunda göründüğü yer",
  "automations.editor.folderNone": "Yok",
  "automations.editor.folderRemoved": "Kaldırılmış klasör",
  "automations.editor.advanced": "Gelişmiş",
  "automations.editor.advancedHint":
    "Kaçırılan çalıştırmalar için telafi süresi",
  "automations.editor.missedGrace": "Kaçırılan çalıştırma toleransı",
  "automations.editor.missedGraceHint":
    "Zamanlanmış bir çalıştırma kaçırılırsa telafi et",
  "automations.option.workspaceCurrent": "Geçerli",
  "automations.option.workspaceWorktree": "Yeni worktree",
  "automations.option.conversationFresh": "Yeni başla",
  "automations.option.conversationReuse": "Sonuncuya devam et",
  "automations.option.graceNone": "Telafi etme",
  "automations.option.grace30": "30 dakika",
  "automations.option.grace120": "2 saat",
  "automations.option.grace720": "12 saat",
  "automations.option.grace1440": "24 saat",

  // Automations: triggers
  "automations.trigger.section": "Tetikleyiciler",
  "automations.trigger.add": "Tetikleyici ekle",
  "automations.trigger.choose": "Otomasyon tetikleyicisi seç",
  "automations.trigger.search": "Tetikleyici ara",
  "automations.trigger.connect": "{provider} bağlantısını Ayarlar'dan kur",
  "automations.trigger.notConnected": "Bağlı değil",
  "automations.trigger.noMatch": "Eşleşen tetikleyici yok",
  "automations.trigger.events": "{trigger} olayları",
  "automations.trigger.none": "Tetikleyici yok",
  "automations.trigger.remove": "Tetikleyiciyi kaldır",
  "automations.trigger.scheduled": "Zamanlanmış",
  "automations.trigger.hourly": "Saatlik",
  "automations.trigger.daily": "Günlük",
  "automations.trigger.weekdays": "Hafta içi",
  "automations.trigger.weekly": "Haftalık",
  "automations.trigger.draftOpened": "Taslak açıldı",
  "automations.trigger.pullRequestOpened": "Pull request açıldı",
  "automations.trigger.issueOpened": "Issue açıldı",
  "automations.trigger.issueCreated": "Issue oluşturuldu",
  "automations.trigger.issueAppeared": "Issue göründü",
  "automations.trigger.mergeRequestOpened": "Merge request açıldı",
  "automations.trigger.pullRequestAppeared": "Pull request göründü",
  "automations.trigger.workItemAppeared": "İş öğesi göründü",
  "automations.trigger.everyHourAt": "Her saat, dakika",
  "automations.trigger.everyDayAt": "Her gün saat",
  "automations.trigger.everyWeekdayAt": "Hafta içi her gün saat",
  "automations.trigger.everyWeekOn": "Her hafta",
  "automations.trigger.at": "saat",
  "automations.trigger.day": "Gün",
  "automations.trigger.minute": "Dakika",
  "automations.trigger.time": "Saat",
  "automations.trigger.push": "Push",
  "automations.trigger.on": "branch:",
  "automations.trigger.branch": "Branch",
  "automations.trigger.selectBranch": "Bir branch seç",
  "automations.trigger.noBranches": "Branch bulunamadı",
  "automations.trigger.chooseProjectFirst": "Önce bir proje seç",
  "automations.trigger.by": "kullanıcı:",
  "automations.trigger.actor": "Kullanıcı",
  "automations.trigger.anyone": "Herkes",

  // Orchestration: sidebar card
  "orchestration.sidebar.agents": {
    one: "{count} ajan",
    other: "{count} ajan",
  },
  "orchestration.sidebar.done": "{done}/{total} tamamlandı",
  "orchestration.sidebar.list": "Orkestre edilen ajanlar",
  "orchestration.sidebar.details": "Ajan ayrıntıları: {title}",
  "orchestration.sidebar.openBeside": "Bu ajanı orkestratörün yanında aç",
  "orchestration.sidebar.seeDetails": "Ayrıntıları gör",
  "orchestration.sidebar.cancelTask": "Görevi iptal et",
  "orchestration.sidebar.stoppingInterrupted":
    "Bu çalıştırma devam edebilmeden önce yarıda kalan işler durduruluyor.",
  "orchestration.sidebar.waitingLead":
    "Bu çalıştırma devam edebilmeden önce lead'in yarıda kalan turunun bitmesi bekleniyor.",
  "orchestration.sidebar.resumeInfo":
    "Devam et, yarıda kalan çalışanları korunan checkout'larından sürdürür ve kuyruktaki işleri başlatır. Politika nedeniyle engellenen görevler incelenmek üzere durmuş kalır.",
  "orchestration.sidebar.anotherConversation": "Başka bir konuşma",
  "orchestration.sidebar.blockerRunning": "{title} bu projede hâlâ çalışıyor.",
  "orchestration.sidebar.openBlocker": "Engelleyeni aç",
  "orchestration.sidebar.waitAgents": "Yarıda kalan ajanların durmasını bekle",
  "orchestration.sidebar.waitLead":
    "Lead'in yarıda kalan turunun bitmesini bekle",
  "orchestration.sidebar.otherConversation": "diğer konuşma",
  "orchestration.sidebar.stopBefore": "Devam etmeden önce {title} durdurulmalı",
  "orchestration.sidebar.continueWork":
    "Yarıda kalan ve kuyruktaki işlere devam et",
  "orchestration.sidebar.resume": "Devam et",

  // Orchestration: task and run status
  "orchestration.task.needsInput": "Yanıt bekliyor",
  "orchestration.task.saved": "Kaydedildi",
  "orchestration.task.paused": "Duraklatıldı",
  "orchestration.task.queued": "Sırada",
  "orchestration.task.running": "Çalışıyor",
  "orchestration.task.cancelling": "Durduruluyor",
  "orchestration.task.completed": "Tamam",
  "orchestration.task.failed": "Başarısız",
  "orchestration.task.blocked": "İnceleme gerekli",
  "orchestration.task.interrupted": "Yarıda kaldı",
  "orchestration.task.cancelled": "İptal edildi",
  "orchestration.run.active": "etkin",
  "orchestration.run.paused": "duraklatıldı",
  "orchestration.run.stopped": "durduruldu",
  "orchestration.run.finished": "tamamlandı",

  // Orchestration: proposal card
  "orchestration.preview.modelFor": "{title} için model",
  "orchestration.preview.searchModelsLabel": "Görev modellerinde ara",
  "orchestration.preview.searchModelsPlaceholder": "Model veya harness ara…",
  "orchestration.preview.modelsList": "Görev modelleri",
  "orchestration.preview.noModels": "Eşleşen model yok",
  "orchestration.preview.effort": "{name} efor düzeyi",
  "orchestration.preview.workersHelpLabel": "Paralel çalışanlar ne demek",
  "orchestration.preview.workersHelpTitle": "Aynı anda kaç çalışan çalışır",
  "orchestration.preview.workersHelpBody":
    "Kalan görevler sırasını bekler; başka bir göreve bağlı olan görev ise her durumda onu bekler. Her çalışan aynı proje klasöründe düzenleme yapar, bu yüzden daha düşük bir sayı klasöre aynı anda inen değişiklik sayısını azaltır.",
  "orchestration.preview.proposal": "Orkestrasyon önerisi",
  "orchestration.preview.planning": "Görevler planlanıyor…",
  "orchestration.preview.lead": "Lider · {name}",
  "orchestration.preview.tasks": {
    one: "{count} görev",
    other: "{count} görev",
  },
  "orchestration.preview.tryAgain": "Yeniden dene",
  "orchestration.preview.starting": "Başlatılıyor…",
  "orchestration.preview.confirm": "Onayla ve başlat",
  "orchestration.preview.viewAgents": "Ajanları gör",
  "orchestration.preview.choosing":
    "Lead'in görevleri ve çalışan modellerini seçiyor. Başlatmadan önce görevleri burada gözden geçir.",
  "orchestration.preview.checking":
    "Kullanılabilir harness'lar ve modeller denetleniyor…",
  "orchestration.preview.details": "{title} ayrıntıları",
  "orchestration.preview.task": "Görev",
  "orchestration.preview.taskTitle": "{number}. görevin başlığı",
  "orchestration.preview.instructions": "Talimatlar",
  "orchestration.preview.taskInstructions": "{number}. görevin talimatları",
  "orchestration.preview.after": "Önkoşul · {tasks}",
  "orchestration.preview.showFewer": "Daha az görev göster",
  "orchestration.preview.showMore": {
    one: "{count} görev daha göster",
    other: "{count} görev daha göster",
  },
  "orchestration.preview.parallelWorkers": "Paralel çalışanlar",
  "orchestration.preview.parallel": "{workers} paralel",
  "orchestration.preview.approved": "Onaylandı",
  "orchestration.preview.awaiting": "Onay bekleniyor",
  "orchestration.preview.sharedFolder": "Ortak proje klasörü",

  // Orchestration: errors shown to the user
  "orchestration.error.alreadyStarting": "Çalıştırma zaten başlatılıyor",
  "orchestration.error.reviewFirst":
    "Başlatmadan önce tamamlanmış bir öneriyi gözden geçir",
  "orchestration.error.returnToCheckout":
    "Başlatmadan önce önerinin checkout'una geri dön",
  "orchestration.error.modelUnavailable":
    "Atanan bir model artık kullanılamıyor. Başlatmadan önce o atamayı değiştir.",
  "orchestration.error.leadUnavailable": "Orkestrasyon lead'i kullanılamıyor",
  "orchestration.error.waitLeadResume":
    "Orkestrasyona devam etmeden önce lead'in yarıda kalan turunun bitmesini bekle",
  "orchestration.error.waitLeadTurn": "Lead'in geçerli turunun bitmesini bekle",
  "orchestration.error.chooseWorkers": "1 ile 4 arasında çalışan seç",
  "orchestration.error.chooseHarnesses": "Kurulu çalışan harness'larını seç",
  "orchestration.error.stopCurrent":
    "Başka bir öneriyi başlatmadan önce geçerli çalıştırmayı durdur",
  "orchestration.error.waitAgentsResume":
    "Orkestrasyona devam etmeden önce yarıda kalan ajanların durmasını bekle",
  "orchestration.error.stopWorkers":
    "Çalıştırmayı değiştirmeden önce etkin çalışanları durdur",
  "orchestration.error.returnLead":
    "Orkestrasyona devam etmeden önce lead'i özgün checkout'una geri döndür",
  "orchestration.error.blockerResume":
    '"{label}" bu checkout\'ta hâlâ çalışıyor. Orkestrasyona devam etmeden önce durdur.',
  "orchestration.error.blockerStart":
    '"{label}" bu checkout\'ta hâlâ çalışıyor. Orkestrasyonu başlatmadan önce durdur.',
  "orchestration.error.parallelRange": "1 ile 4 arasında paralel çalışan seç",
  "orchestration.error.noWorkerModels":
    "Kullanılabilir çalışan modeli yok ya da model kataloğu çok büyük. Bağlı harness'larını kontrol et.",
  "orchestration.error.planningInterrupted":
    "Planlama yarıda kesildi. Görevleri yeniden oluştur.",
};
