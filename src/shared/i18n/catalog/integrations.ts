import type { Translations } from "./types";

export const en = {
  "integrations.availability.notFound":
    "{name} not found{install}. Install it, or restart MonoCode if it is already installed.",
  "integrations.availability.hermesInstall":
    "Install from hermes-agent.nousresearch.com, then run hermes model",
  "integrations.auth.noBrowserSignIn":
    "{provider} does not offer a single browser sign-in flow.",
  "integrations.auth.exitedUnexpectedly":
    "{provider} sign-in exited unexpectedly.",
  "integrations.auth.exitedWithCode":
    "{provider} sign-in exited with code {code}.",
  "integrations.auth.timedOut":
    "{provider} sign-in timed out. Please try again.",
  "integrations.auth.startFailed":
    "Could not start {provider} sign-in: {error}",
  "integrations.orchestration.planningInterrupted":
    "Planning was interrupted. Generate the assignments again.",
  "integrations.status.compactedContext": "Compacted context",
  "integrations.subagent.failed": "Subagent failed.",
  "integrations.subagent.interrupted": "Subagent interrupted.",
  "integrations.subagent.operationFailed": "Subagent operation failed.",
  "integrations.git.commitMessageReply":
    "Could not generate a commit message. Model replied: {reply}",
  "integrations.git.commitMessageEmpty":
    "Could not generate a commit message. {provider} returned no text.",
  "integrations.turn.failed": "{provider} turn failed.",
  "integrations.turn.ended": "{provider} ended the turn ({reason}).",
  "integrations.turn.stopBeforeEdit":
    "Stop the current turn before editing the last message",
  "integrations.turn.noMessageToEdit": "No user message to edit",
  "integrations.provider.didNotStart": "{provider} did not start. {detail}",
  "integrations.provider.restoreFallback":
    "{provider} could not restore the previous conversation — starting a new session.",
  "integrations.codex.compactNeedsMessage":
    "Send a message before compacting this chat",
  "integrations.codex.answerSendFailed":
    "Could not send your answer to Codex: {error}",
  "integrations.codex.monoContextResumeFailed":
    "The saved Mono Codex context could not be resumed. Retry to keep its saved context.",
  "integrations.codex.imageSaveFailed":
    "Could not save generated image: {error}",
  "integrations.codex.mcpSignInUnsupported":
    "This MCP server requested a form or browser sign-in that MonoCode does not support yet. Complete it in the server's own interface.",
  "integrations.codex.unsupportedRequest":
    "Unsupported Codex request: {method}",
  "integrations.codex.notAuthenticated":
    "Codex CLI is not authenticated. Run `codex login` and try again.",
  "integrations.codex.secretInput":
    "Codex requested secret input. MonoCode cannot collect secret answers securely.",
  "integrations.devin.authHelp":
    "Run `devin auth login` in a terminal, then retry.",
  "integrations.devin.notLoggedIn": "Devin is not logged in. {help}",
  "integrations.devin.stoppedResponding":
    "Devin stopped responding before the turn finished.",
  "integrations.devin.restoreTimedOut":
    "Devin timed out restoring the previous conversation. Send again to retry.",
  "integrations.devin.restoreFailed":
    "Devin could not restore the previous conversation. {detail}",
  "integrations.devin.modelUnavailable":
    'Devin no longer offers the model "{model}"; using its current model. Pick another model to change it.',
  "integrations.antigravity.authHelp": "Run `agy` once in Terminal to sign in.",
  "integrations.antigravity.stalled":
    "Antigravity has been quiet for two minutes — its post-turn work may be stuck. Stop and resend to recover.",
  "integrations.grok.authHelp":
    "Grok Build is not signed in. Run `grok login` in a terminal, or set XAI_API_KEY.",
  "integrations.hermes.authHelp":
    "Configure Hermes with `hermes model`, then verify it with `hermes acp --check`.",
  "integrations.fx.authHelp":
    "fx has no Vercel AI Gateway credential it can read from here. Run `fx login` (or `fx setup`) in a terminal, or export AI_GATEWAY_API_KEY so it does not depend on the macOS Keychain.",
  "integrations.fx.initTimeout":
    "fx did not answer initialize within {seconds}s. {help}",
  "integrations.opencode.modelIdRequired":
    "OpenCode models use provider/model ids. Wait for the catalog to load, then pick a model.",
  "integrations.opencode.streamEnded":
    "OpenCode event stream ended unexpectedly.",
  "integrations.opencode.compactionInterrupted":
    "OpenCode compaction was interrupted",
  "integrations.opencode.serverExitedEarly":
    "OpenCode server exited before startup completed (code: {code}).",
  "integrations.opencode.sessionFailed": "OpenCode session failed.",
  "integrations.opencode.versionUnknown":
    "Unable to determine OpenCode version. MonoCode requires v{minimum} or newer.",
  "integrations.opencode.versionTooOld":
    "OpenCode v{version} is too old. Upgrade to v{minimum} or newer.",
  "integrations.opencode.versionUnsupported":
    "OpenCode v{version} is not supported. MonoCode supports OpenCode v1 and v2.",
  "integrations.opencode.serviceStartFailed":
    "Could not start the OpenCode background service: {error}",
  "integrations.pi.advisorReviewed": "Advisor reviewed this turn",
  "integrations.pi.turnFailed": "{provider} turn failed",
  "integrations.pi.fastUnavailable":
    "Fast mode is unavailable for the current model.",
  "integrations.pi.chooseOption": "Choose an option",
} as const;

export const tr: Translations<typeof en> = {
  "integrations.availability.notFound":
    "{name} bulunamadı{install}. Yükleyin ya da zaten yüklüyse MonoCode'u yeniden başlatın.",
  "integrations.availability.hermesInstall":
    "hermes-agent.nousresearch.com adresinden yükleyin, ardından hermes model komutunu çalıştırın",
  "integrations.auth.noBrowserSignIn":
    "{provider} tek adımlı bir tarayıcı oturum açma akışı sunmuyor.",
  "integrations.auth.exitedUnexpectedly":
    "{provider} oturum açma işlemi beklenmedik şekilde sonlandı.",
  "integrations.auth.exitedWithCode":
    "{provider} oturum açma işlemi {code} koduyla sonlandı.",
  "integrations.auth.timedOut":
    "{provider} oturum açma işlemi zaman aşımına uğradı. Lütfen yeniden deneyin.",
  "integrations.auth.startFailed":
    "{provider} oturum açma işlemi başlatılamadı: {error}",
  "integrations.orchestration.planningInterrupted":
    "Planlama yarıda kesildi. Atamaları yeniden oluşturun.",
  "integrations.status.compactedContext": "Bağlam sıkıştırıldı",
  "integrations.subagent.failed": "Alt ajan başarısız oldu.",
  "integrations.subagent.interrupted": "Alt ajan yarıda kesildi.",
  "integrations.subagent.operationFailed": "Alt ajan işlemi başarısız oldu.",
  "integrations.git.commitMessageReply":
    "Commit mesajı oluşturulamadı. Modelin yanıtı: {reply}",
  "integrations.git.commitMessageEmpty":
    "Commit mesajı oluşturulamadı. {provider} metin döndürmedi.",
  "integrations.turn.failed": "{provider} turu başarısız oldu.",
  "integrations.turn.ended": "{provider} turu sonlandırdı ({reason}).",
  "integrations.turn.stopBeforeEdit":
    "Son mesajı düzenlemeden önce mevcut turu durdurun",
  "integrations.turn.noMessageToEdit": "Düzenlenecek kullanıcı mesajı yok",
  "integrations.provider.didNotStart": "{provider} başlatılamadı. {detail}",
  "integrations.provider.restoreFallback":
    "{provider} önceki konuşmayı geri yükleyemedi; yeni bir oturum başlatılıyor.",
  "integrations.codex.compactNeedsMessage":
    "Bu sohbeti sıkıştırmadan önce bir mesaj gönderin",
  "integrations.codex.answerSendFailed":
    "Yanıtınız Codex'e gönderilemedi: {error}",
  "integrations.codex.monoContextResumeFailed":
    "Kayıtlı Mono Codex bağlamı sürdürülemedi. Kayıtlı bağlamı korumak için yeniden deneyin.",
  "integrations.codex.imageSaveFailed":
    "Oluşturulan görsel kaydedilemedi: {error}",
  "integrations.codex.mcpSignInUnsupported":
    "Bu MCP sunucusu, MonoCode'un henüz desteklemediği bir form ya da tarayıcı girişi istedi. İşlemi sunucunun kendi arayüzünde tamamlayın.",
  "integrations.codex.unsupportedRequest":
    "Desteklenmeyen Codex isteği: {method}",
  "integrations.codex.notAuthenticated":
    "Codex CLI'da oturum açılmamış. `codex login` komutunu çalıştırın ve yeniden deneyin.",
  "integrations.codex.secretInput":
    "Codex gizli bir giriş istedi. MonoCode gizli yanıtları güvenli şekilde toplayamaz.",
  "integrations.devin.authHelp":
    "Bir terminalde `devin auth login` komutunu çalıştırın, ardından yeniden deneyin.",
  "integrations.devin.notLoggedIn": "Devin'de oturum açılmamış. {help}",
  "integrations.devin.stoppedResponding":
    "Devin, tur bitmeden yanıt vermeyi bıraktı.",
  "integrations.devin.restoreTimedOut":
    "Devin önceki konuşmayı geri yüklerken zaman aşımına uğradı. Yeniden denemek için tekrar gönderin.",
  "integrations.devin.restoreFailed":
    "Devin önceki konuşmayı geri yükleyemedi. {detail}",
  "integrations.devin.modelUnavailable":
    'Devin artık "{model}" modelini sunmuyor; mevcut modeli kullanılıyor. Değiştirmek için başka bir model seçin.',
  "integrations.antigravity.authHelp":
    "Giriş yapmak için Terminal'de `agy` komutunu bir kez çalıştırın.",
  "integrations.antigravity.stalled":
    "Antigravity iki dakikadır sessiz; tur sonrası işi takılmış olabilir. Kurtarmak için durdurup yeniden gönderin.",
  "integrations.grok.authHelp":
    "Grok Build'de oturum açılmamış. Bir terminalde `grok login` komutunu çalıştırın ya da XAI_API_KEY değişkenini ayarlayın.",
  "integrations.hermes.authHelp":
    "Hermes'i `hermes model` ile yapılandırın, ardından `hermes acp --check` ile doğrulayın.",
  "integrations.fx.authHelp":
    "fx'in buradan okuyabileceği bir Vercel AI Gateway kimlik bilgisi yok. Bir terminalde `fx login` (ya da `fx setup`) komutunu çalıştırın veya macOS Keychain'e bağlı kalmaması için AI_GATEWAY_API_KEY değişkenini export edin.",
  "integrations.fx.initTimeout":
    "fx, {seconds} sn içinde initialize isteğine yanıt vermedi. {help}",
  "integrations.opencode.modelIdRequired":
    "OpenCode modelleri provider/model kimlikleri kullanır. Kataloğun yüklenmesini bekleyin, ardından bir model seçin.",
  "integrations.opencode.streamEnded":
    "OpenCode olay akışı beklenmedik şekilde sona erdi.",
  "integrations.opencode.compactionInterrupted":
    "OpenCode sıkıştırması yarıda kesildi",
  "integrations.opencode.serverExitedEarly":
    "OpenCode sunucusu başlatma tamamlanmadan kapandı (kod: {code}).",
  "integrations.opencode.sessionFailed": "OpenCode oturumu başarısız oldu.",
  "integrations.opencode.versionUnknown":
    "OpenCode sürümü belirlenemedi. MonoCode için v{minimum} veya daha yeni bir sürüm gerekir.",
  "integrations.opencode.versionTooOld":
    "OpenCode v{version} çok eski. v{minimum} veya daha yeni bir sürüme yükseltin.",
  "integrations.opencode.versionUnsupported":
    "OpenCode v{version} desteklenmiyor. MonoCode, OpenCode v1 ve v2'yi destekler.",
  "integrations.opencode.serviceStartFailed":
    "OpenCode arka plan hizmeti başlatılamadı: {error}",
  "integrations.pi.advisorReviewed": "Danışman bu turu inceledi",
  "integrations.pi.turnFailed": "{provider} turu başarısız oldu",
  "integrations.pi.fastUnavailable":
    "Hızlı mod mevcut model için kullanılamıyor.",
  "integrations.pi.chooseOption": "Bir seçenek belirleyin",
};
