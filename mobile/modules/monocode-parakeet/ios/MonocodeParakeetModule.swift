import ExpoModulesCore
import FluidAudio
import Foundation
import UIKit

/// NVIDIA Parakeet TDT 0.6B and its post-trainings, run on the phone by
/// FluidAudio's Core ML port. Each model is downloaded on request, not
/// bundled, and is only held in memory while dictation uses it.
public class MonocodeParakeetModule: Module {
  public func definition() -> ModuleDefinition {
    Name("MonocodeParakeet")
    Events("onDownloadProgress")

    OnCreate {
      // Nothing reaches the network unless a download is asked for.
      ModelHub.offlineMode = true
      ParakeetEngine.shared.observeMemoryWarnings()
    }

    /// Whether this phone can run Parakeet at all.
    Function("isSupported") { () -> Bool in ParakeetEngine.isSupported }

    /// Every model's state on this phone, by id: whether it can run here,
    /// is downloaded (and its size), downloading or loaded.
    /// `withSizes` walks each downloaded model's files, which only Settings
    /// shows; dictation leaves it off.
    AsyncFunction("statuses") { (withSizes: Bool?) async -> [String: [String: Any]] in
      await ParakeetEngine.shared.statuses(withSizes: withSizes ?? true)
    }

    /// Downloads and compiles a model, reporting progress as
    /// `onDownloadProgress` events. Joins the download already running for it.
    AsyncFunction("download") { [weak self] (model: String) async throws in
      try await ParakeetEngine.shared.download(try ParakeetModel.named(model)) { progress in
        self?.sendEvent("onDownloadProgress", progress)
      }
    }

    AsyncFunction("cancelDownload") { () async in
      await ParakeetEngine.shared.cancelDownload()
    }

    /// Deletes a model from the phone.
    AsyncFunction("remove") { (model: String) async throws in
      try await ParakeetEngine.shared.remove(try ParakeetModel.named(model))
    }

    /// The words in a recording. A language ("ru") only keeps the output to
    /// its alphabet; without one the model recognizes the language itself.
    AsyncFunction("transcribe") {
      (uri: String, language: String?, model: String) async throws -> String in
      try await ParakeetEngine.shared.transcribe(
        fileURL: try recordingURL(uri), language: language, model: try ParakeetModel.named(model))
    }

    /// Frees the model's memory now, rather than after the idle delay.
    AsyncFunction("unload") { () async in
      await ParakeetEngine.shared.unload()
    }
  }
}

func recordingURL(_ uri: String) throws -> URL {
  if let url = URL(string: uri), url.isFileURL { return url }
  if uri.hasPrefix("/") { return URL(fileURLWithPath: uri) }
  throw ParakeetException("The recording couldn't be read.")
}

final class ParakeetException: GenericException<String>, @unchecked Sendable {
  override var reason: String { param }
}

/// A download stopped on request: the app shows no error for it.
final class ParakeetCancelledException: Exception, @unchecked Sendable {
  override var code: String { "ERR_PARAKEET_CANCELLED" }
  override var reason: String { "The download was cancelled." }
}

/// The Parakeet models offered, by the id the app stores.
enum ParakeetModel: String, CaseIterable, Sendable {
  /// moondream's post-training of v3: same 25 languages, more accurate.
  case ultra
  /// NVIDIA's multilingual original.
  case v3
  /// moondream's 2-bit re-training of v3: a third of the download, iOS 18+.
  case redux
  /// NVIDIA's English-only model.
  case v2

  var version: AsrModelVersion {
    switch self {
    case .ultra: .ultra
    case .v3: .v3
    case .redux: .redux
    case .v2: .v2
    }
  }

  /// Redux's 2-bit encoder needs iOS 18 Core ML ops.
  var runsHere: Bool {
    switch self {
    case .redux:
      if #available(iOS 18, *) { return true }
      return false
    default: return true
    }
  }

  var directory: URL { AsrModels.defaultCacheDirectory(for: version) }
  var installed: Bool { AsrModels.modelsExist(at: directory, version: version) }

  static func named(_ id: String) throws -> ParakeetModel {
    guard let model = ParakeetModel(rawValue: id) else {
      throw ParakeetException("Unknown Parakeet model “\(id)”.")
    }
    return model
  }
}

/// Keeps reported progress from going back, and on compiling once there.
final class ProgressFloor: @unchecked Sendable {
  private let lock = NSLock()
  private var fraction = 0.0
  private var compiling = false

  func advance(_ next: Double, _ phase: String) -> (Double, String)? {
    lock.lock()
    defer { lock.unlock() }
    if phase == "compiling" { compiling = true }
    guard next >= fraction || phase == "compiling" else { return nil }
    fraction = max(fraction, min(next, 1))
    return (fraction, compiling ? "compiling" : phase)
  }
}

actor ParakeetEngine {
  static let shared = ParakeetEngine()

  /// Phones with less memory than this can't hold a model beside the app.
  private static let minimumMemory: UInt64 = 3_500_000_000
  /// How long a model stays loaded after a dictation, for the next one.
  private static let idleUnload: Duration = .seconds(90)

  static var isSupported: Bool {
    ProcessInfo.processInfo.physicalMemory >= minimumMemory
  }

  private var manager: AsrManager?
  /// The model `manager` (or the load under way) is for.
  private var managerModel: ParakeetModel?
  private var loading: Task<AsrManager, Error>?
  /// Bumped by unload(), so a load that finishes after it isn't kept.
  private var generation = 0
  /// Transcriptions running on each manager, and managers unloaded while
  /// one ran: those are cleaned up once it finishes.
  private var activeUses: [ObjectIdentifier: Int] = [:]
  private var retired: [ObjectIdentifier: AsrManager] = [:]
  private var downloading: Task<Void, Error>?
  private var downloadingModel: ParakeetModel?
  /// Set by cancelDownload(): FluidAudio's downloader reports a cancel as a
  /// URL error as often as a CancellationError.
  private var downloadCancelled = false
  /// The previous model's cleanup, awaited before another model loads, so
  /// two are never in memory at once.
  private var cleaning: Task<Void, Never>?
  private var cleaningToken = 0
  private var idle: Task<Void, Never>?
  private var memoryObserver: NSObjectProtocol?

  nonisolated func observeMemoryWarnings() {
    Task { await self.startObserving() }
  }

  private func startObserving() {
    guard memoryObserver == nil else { return }
    memoryObserver = NotificationCenter.default.addObserver(
      forName: UIApplication.didReceiveMemoryWarningNotification, object: nil, queue: nil
    ) { [weak self] _ in
      Task { await self?.unload() }
    }
  }

  func statuses(withSizes: Bool) -> [String: [String: Any]] {
    var all: [String: [String: Any]] = [:]
    for model in ParakeetModel.allCases {
      let installed = model.installed
      all[model.rawValue] = [
        "supported": Self.isSupported && model.runsHere,
        "installed": installed,
        "downloading": downloadingModel == model,
        "loaded": manager != nil && managerModel == model,
        "sizeBytes": installed && withSizes ? Self.size(of: model.directory) : 0,
      ]
    }
    return all
  }

  func download(
    _ model: ParakeetModel, progress: @escaping @Sendable ([String: Any]) -> Void
  ) async throws {
    guard Self.isSupported else { throw ParakeetException(Self.unsupportedReason) }
    guard model.runsHere else { throw ParakeetException(Self.needsIOS18) }
    let task: Task<Void, Error>
    if let downloading {
      guard downloadingModel == model else {
        throw ParakeetException("Another Parakeet model is downloading. Wait for it to finish.")
      }
      // Joined: same progress events, same errors as the caller that started it.
      task = downloading
    } else {
      task = Task {
        // Cleared by the task itself, before anyone waiting on it resumes,
        // so every caller then reads the download as over.
        defer { self.finishDownload() }
        // Hundreds of megabytes that can be downloaded again: kept out of
        // iCloud and device backups, partial downloads included.
        let root = model.directory.deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        Self.excludeFromBackup(root)
        ModelHub.offlineMode = false
        defer { ModelHub.offlineMode = true }
        // FluidAudio fetches the model in one pass, then compiles it part by
        // part, each part reporting from 0 again: progress only moves forward,
        // and stays on compiling once compiling starts.
        let report = ProgressFloor()
        do {
          _ = try await AsrModels.download(version: model.version) { update in
            let phase: String
            switch update.phase {
            case .listing: phase = "listing"
            case .downloading: phase = "downloading"
            case .compiling: phase = "compiling"
            }
            if let (fraction, phase) = report.advance(update.fractionCompleted, phase) {
              progress(["model": model.rawValue, "fraction": fraction, "phase": phase])
            }
          }
        } catch {
          // Settled here, before the defer lets another download reset the
          // flag: FluidAudio reports a cancel as a URL error as often as not.
          if self.downloadCancelled || error is CancellationError { throw CancellationError() }
          throw error
        }
        Self.excludeFromBackup(root)
      }
      downloading = task
      downloadingModel = model
      downloadCancelled = false
    }
    do {
      try await task.value
    } catch {
      if error is CancellationError { throw ParakeetCancelledException() }
      throw ParakeetException(
        "The Parakeet model couldn't be downloaded. Check your connection and try again. (\(error.localizedDescription))"
      )
    }
  }

  private func finishDownload() {
    downloading = nil
    downloadingModel = nil
  }

  func cancelDownload() {
    guard let downloading else { return }
    downloadCancelled = true
    downloading.cancel()
  }

  func remove(_ model: ParakeetModel) async throws {
    // A cancelled download can still write until it stops: wait for it, so
    // nothing lands in the folder after it is deleted.
    if downloadingModel == model, let downloading {
      downloadCancelled = true
      downloading.cancel()
      _ = try? await downloading.value
    }
    if managerModel == model { unload() }
    if FileManager.default.fileExists(atPath: model.directory.path) {
      try FileManager.default.removeItem(at: model.directory)
    }
  }

  func transcribe(fileURL: URL, language: String?, model: ParakeetModel) async throws -> String {
    idle?.cancel()
    defer { scheduleUnload() }
    let manager = try await loaded(model)
    let id = ObjectIdentifier(manager)
    activeUses[id, default: 0] += 1
    defer { finishUse(id) }
    var state = TdtDecoderState.make()
    let hint = language.flatMap { Language(rawValue: String($0.prefix(2)).lowercased()) }
    do {
      let result = try await manager.transcribe(fileURL, decoderState: &state, language: hint)
      return result.text.trimmingCharacters(in: .whitespacesAndNewlines)
    } catch ASRError.invalidAudioData {
      // Shorter than the model's smallest input: nothing was said.
      return ""
    }
  }

  func unload() {
    idle?.cancel()
    idle = nil
    generation += 1
    loading?.cancel()
    loading = nil
    if let manager { retire(manager) }
    manager = nil
    managerModel = nil
  }

  /// Frees a manager's models, at once or after the transcription using it.
  private func retire(_ manager: AsrManager) {
    let id = ObjectIdentifier(manager)
    if activeUses[id, default: 0] > 0 {
      retired[id] = manager
    } else {
      startCleanup(manager)
    }
  }

  private func startCleanup(_ manager: AsrManager) {
    let previous = cleaning
    cleaningToken += 1
    let token = cleaningToken
    cleaning = Task {
      await previous?.value
      await manager.cleanup()
      self.cleaningFinished(token)
    }
  }

  private func cleaningFinished(_ token: Int) {
    if token == cleaningToken { cleaning = nil }
  }

  private func finishUse(_ id: ObjectIdentifier) {
    let remaining = activeUses[id, default: 1] - 1
    activeUses[id] = remaining > 0 ? remaining : nil
    if remaining <= 0, let manager = retired.removeValue(forKey: id) {
      startCleanup(manager)
    }
  }

  private func loaded(_ model: ParakeetModel) async throws -> AsrManager {
    // One model in memory at a time: another one picked replaces it, and
    // leaves memory before this one loads. Checked again after every wait,
    // as another call may have run meanwhile; from here to the load starting
    // there is no suspension, so what follows holds for `model`.
    while true {
      if let current = managerModel, current != model { unload() }
      guard let pending = cleaning else { break }
      await pending.value
    }
    if let manager { return manager }
    guard Self.isSupported else { throw ParakeetException(Self.unsupportedReason) }
    guard model.runsHere else { throw ParakeetException(Self.needsIOS18) }
    guard model.installed else {
      throw ParakeetException(
        "This Parakeet model isn't downloaded. Download it in Settings → Dictation.")
    }
    let owner = generation
    let task: Task<AsrManager, Error>
    if let loading {
      task = loading
    } else {
      let directory = model.directory
      let version = model.version
      task = Task { () throws -> AsrManager in
        // Straight from the folder: no repository lookup and no re-download,
        // whatever another model's download is doing with the network flag.
        let models = try AsrModels.loadLocal(from: directory, version: version)
        let manager = AsrManager(config: .default)
        try await manager.loadModels(models)
        return manager
      }
      loading = task
      managerModel = model
    }
    let loaded: AsrManager
    do {
      loaded = try await task.value
    } catch {
      guard owner == generation else {
        // Cancelled by unload(): the model is fine.
        throw ParakeetException("Parakeet was unloaded to free memory. Try again.")
      }
      loading = nil
      if manager == nil { managerModel = nil }
      throw ParakeetException(
        "The Parakeet model couldn't be loaded. Try deleting and downloading it again in Settings → Dictation. (\(error.localizedDescription))"
      )
    }
    // Unloaded while it loaded, as on a memory warning: not kept.
    guard owner == generation else {
      retire(loaded)
      throw ParakeetException("Parakeet was unloaded to free memory. Try again.")
    }
    loading = nil
    if let manager { return manager }
    manager = loaded
    return loaded
  }

  private func scheduleUnload() {
    idle?.cancel()
    idle = Task { [weak self] in
      try? await Task.sleep(for: Self.idleUnload)
      guard !Task.isCancelled else { return }
      await self?.unload()
    }
  }

  private static let unsupportedReason =
    "NVIDIA Parakeet needs an iPhone with at least 4 GB of memory."
  private static let needsIOS18 = "Parakeet Redux needs iOS 18 or later."

  private static func excludeFromBackup(_ url: URL) {
    var url = url
    var values = URLResourceValues()
    values.isExcludedFromBackup = true
    try? url.setResourceValues(values)
  }

  private static func size(of directory: URL) -> Int {
    guard
      let files = FileManager.default.enumerator(
        at: directory, includingPropertiesForKeys: [.totalFileAllocatedSizeKey])
    else { return 0 }
    var total = 0
    for case let file as URL in files {
      total += (try? file.resourceValues(forKeys: [.totalFileAllocatedSizeKey]))?
        .totalFileAllocatedSize ?? 0
    }
    return total
  }
}
