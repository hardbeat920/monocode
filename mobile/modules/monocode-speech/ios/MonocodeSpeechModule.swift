import AVFoundation
import ExpoModulesCore
import Speech

/// Apple's on-device transcribers, as dictation needs them beyond what
/// `@react-native-ai/apple` offers: the languages each one takes on this
/// phone, and `DictationTranscriber` for the languages `SpeechTranscriber`
/// doesn't cover (Russian and Ukrainian among them on iOS 26).
public class MonocodeSpeechModule: Module {
  public func definition() -> ModuleDefinition {
    Name("MonocodeSpeech")

    /// Whether either transcriber can run here: iOS 26 and up.
    Function("appleAvailable") { () -> Bool in
      if #available(iOS 26, *) { return true }
      return false
    }

    /// Every language either transcriber takes, by BCP-47 identifier, with
    /// which one does and whether its speech assets are on the phone.
    AsyncFunction("appleLocales") { () async -> [[String: Any]] in
      guard #available(iOS 26, *) else { return [] }
      return await AppleLocales.list()
    }

    /// Resolves a language to one a transcriber takes and fetches its assets
    /// if they aren't on the phone yet. `SpeechTranscriber` is preferred where
    /// both take it.
    AsyncFunction("applePrepare") { (language: String) async throws -> [String: Any] in
      guard #available(iOS 26, *) else { throw SpeechUnavailableException() }
      let resolved = try await AppleLocales.resolve(language)
      try await AppleLocales.install(resolved)
      return [
        "locale": resolved.locale.identifier(.bcp47),
        "transcriber": resolved.transcriber.rawValue,
      ]
    }

    /// The words in a recording, by `DictationTranscriber`. `SpeechTranscriber`
    /// languages go through `@react-native-ai/apple` as before.
    AsyncFunction("appleDictate") { (uri: String, language: String) async throws -> String in
      guard #available(iOS 26, *) else { throw SpeechUnavailableException() }
      return try await AppleDictation.transcribe(fileURL: try fileURL(uri), language: language)
    }
  }
}

/// A `file://` URI, or a bare path, as a file URL.
func fileURL(_ uri: String) throws -> URL {
  if let url = URL(string: uri), url.isFileURL { return url }
  if uri.hasPrefix("/") { return URL(fileURLWithPath: uri) }
  throw InvalidRecordingException()
}

enum AppleTranscriberKind: String {
  case speech
  case dictation
}

@available(iOS 26, *)
struct ResolvedLocale {
  let locale: Locale
  let transcriber: AppleTranscriberKind

  var module: any SpeechModule {
    switch transcriber {
    case .speech: SpeechTranscriber(locale: locale, preset: .transcription)
    case .dictation: DictationTranscriber(locale: locale, preset: .longDictation)
    }
  }
}

@available(iOS 26, *)
enum AppleLocales {
  static func list() async -> [[String: Any]] {
    // SpeechTranscriber isn't on every iOS 26 device; DictationTranscriber is.
    let speech = SpeechTranscriber.isAvailable ? await SpeechTranscriber.supportedLocales : []
    let dictation = await DictationTranscriber.supportedLocales
    let installed = Set(
      (SpeechTranscriber.isAvailable ? await SpeechTranscriber.installedLocales : [])
        .map { $0.identifier(.bcp47) }
        + (await DictationTranscriber.installedLocales).map { $0.identifier(.bcp47) })
    let speechIds = Set(speech.map { $0.identifier(.bcp47) })
    let dictationIds = Set(dictation.map { $0.identifier(.bcp47) })
    return speechIds.union(dictationIds).sorted().map { id in
      [
        "id": id,
        "name": Locale.current.localizedString(forIdentifier: id) ?? id,
        "nativeName": Locale(identifier: id).localizedString(forIdentifier: id) ?? id,
        "transcriber": speechIds.contains(id)
          ? AppleTranscriberKind.speech.rawValue : AppleTranscriberKind.dictation.rawValue,
        "installed": installed.contains(id),
      ]
    }
  }

  /// The language as a transcriber takes it. The one that takes it exactly
  /// wins (SpeechTranscriber first), so a region only DictationTranscriber
  /// has, such as de-AT, isn't swapped for SpeechTranscriber's de-DE; an
  /// equivalent in another region is the fallback.
  static func resolve(_ language: String) async throws -> ResolvedLocale {
    let requested = Locale(identifier: language)
    let wanted = requested.identifier(.bcp47)
    let speech =
      SpeechTranscriber.isAvailable
      ? await SpeechTranscriber.supportedLocale(equivalentTo: requested) : nil
    if let speech, speech.identifier(.bcp47) == wanted {
      return ResolvedLocale(locale: speech, transcriber: .speech)
    }
    let dictation = await DictationTranscriber.supportedLocale(equivalentTo: requested)
    if let dictation, dictation.identifier(.bcp47) == wanted {
      return ResolvedLocale(locale: dictation, transcriber: .dictation)
    }
    if let speech { return ResolvedLocale(locale: speech, transcriber: .speech) }
    if let dictation { return ResolvedLocale(locale: dictation, transcriber: .dictation) }
    throw UnsupportedLanguageException(language)
  }

  static func install(_ resolved: ResolvedLocale) async throws {
    let modules = [resolved.module]
    switch await AssetInventory.status(forModules: modules) {
    case .installed:
      return
    case .unsupported:
      throw UnsupportedLanguageException(resolved.locale.identifier(.bcp47))
    default:
      // An installation request reserves the locale, and the system allows
      // only so many. Dictation uses one language at a time, so the oldest
      // other reservations make room; a failed install gives back its own.
      let target = resolved.locale.identifier(.bcp47)
      let reserved = await AssetInventory.reservedLocales
      let alreadyReserved = reserved.contains { $0.identifier(.bcp47) == target }
      if !alreadyReserved {
        let others = reserved.filter { $0.identifier(.bcp47) != target }
        let excess = others.count - (AssetInventory.maximumReservedLocales - 1)
        for locale in others.prefix(max(0, excess)) {
          await AssetInventory.release(reservedLocale: locale)
        }
      }
      do {
        if let request = try await AssetInventory.assetInstallationRequest(supporting: modules) {
          try await request.downloadAndInstall()
        }
      } catch {
        if !alreadyReserved { await AssetInventory.release(reservedLocale: resolved.locale) }
        throw error
      }
    }
  }
}

@available(iOS 26, *)
enum AppleDictation {
  static func transcribe(fileURL: URL, language: String) async throws -> String {
    let resolved = try await AppleLocales.resolve(language)
    let audio: AVAudioFile
    do {
      audio = try AVAudioFile(forReading: fileURL)
    } catch {
      throw InvalidRecordingException()
    }
    let transcriber = DictationTranscriber(locale: resolved.locale, preset: .longDictation)
    // Results are read alongside the analysis and awaited after it, so the
    // last ones, finalized as the analyzer finishes, aren't lost.
    let collected = Task { () throws -> [String] in
      var parts: [String] = []
      for try await result in transcriber.results where result.isFinal {
        parts.append(String(result.text.characters))
      }
      return parts
    }
    let analyzer = SpeechAnalyzer(modules: [transcriber])
    do {
      if let end = try await analyzer.analyzeSequence(from: audio) {
        try await analyzer.finalizeAndFinish(through: end)
      } else {
        await analyzer.cancelAndFinishNow()
      }
    } catch {
      collected.cancel()
      throw error
    }
    return try await collected.value
      .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
      .filter { !$0.isEmpty }
      .joined(separator: " ")
  }
}

final class SpeechUnavailableException: Exception, @unchecked Sendable {
  override var reason: String { "Apple Speech needs iOS 26 or later." }
}

final class InvalidRecordingException: Exception, @unchecked Sendable {
  override var reason: String { "The recording couldn't be read." }
}

final class UnsupportedLanguageException: GenericException<String>, @unchecked Sendable {
  override var code: String { "ERR_DICTATION_LANGUAGE" }
  override var reason: String {
    let name = Locale.current.localizedString(forIdentifier: param) ?? param
    return "\(name) isn't available for Apple Speech on this device."
  }
}
