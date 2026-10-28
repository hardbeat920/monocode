import CryptoKit
import Foundation
import MonocodeLinkCore
import UIKit

enum TranscriptNative {
  static func call(_ input: [String: Any]) throws -> Any {
    let data = try JSONSerialization.data(withJSONObject: input)
    let request = String(decoding: data, as: UTF8.self)
    let response: String = try request.withCString { pointer in
      guard let output = monocode_link_call(pointer) else {
        throw failure("Native connection failed")
      }
      defer { monocode_link_free(output) }
      return String(cString: output)
    }
    guard let bytes = response.data(using: .utf8),
      let envelope = try JSONSerialization.jsonObject(with: bytes) as? [String: Any]
    else { throw failure("Invalid native response") }
    guard envelope["ok"] as? Bool == true else {
      throw failure(envelope["error"] as? String ?? "Desktop connection failed")
    }
    return envelope["result"] ?? NSNull()
  }
  static func failure(_ message: String) -> NSError {
    NSError(domain: "MonocodeTranscript", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
  }
}

/// Slow connection/request calls must not occupy the layout worker.
private final class TranscriptRequest {
  private let lock = NSLock()
  private var result: Result<[String: Any], Error>?
  private let discard: (([String: Any]) -> Void)?
  init(
    discard: (([String: Any]) -> Void)? = nil, ready: @escaping () -> Void,
    _ work: @escaping () throws -> [String: Any]
  ) {
    self.discard = discard
    DispatchQueue.global(qos: .userInitiated).async {
      let result = autoreleasepool { Result { try work() } }
      self.lock.lock()
      self.result = result
      self.lock.unlock()
      ready()
    }
  }
  deinit { if case .success(let value) = result { discard?(value) } }
  func take() -> Result<[String: Any], Error>? {
    lock.lock()
    defer { lock.unlock() }
    let value = result
    result = nil
    return value
  }
}

/// One background worker owns the watch, Rust projection and CoreText layout.
/// Control messages are small; streaming text is never delivered to React.
final class TranscriptSession {
  var onFrame: ((TranscriptLayoutFrame) -> Void)?
  var onSelection: ((String) -> Void)?
  private let lock = NSCondition()
  private var wakeRevision: UInt64 = 0
  private var running = true
  private var active = true
  private var watchID: Int?
  private var commands: [[String: Any]] = []
  private var style = TranscriptStyle()
  private var viewport = CGRect.zero
  private var dirtyLayout = true
  private var purgeLayouts = false
  private let configuration: [String: Any]
  private let cacheURL: URL

  init(configuration: [String: Any], style: TranscriptStyle) {
    self.configuration = configuration
    self.style = style
    let directory = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
      .appendingPathComponent("native-transcripts", isDirectory: true)
    try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    // Fingerprints and IDs are encoded into a filename, never paths or credentials.
    let key =
      "\(configuration["pin"] as? String ?? "")/\(configuration["conversationId"] as? String ?? "")"
    let name = SHA256.hash(data: Data(key.utf8)).map { String(format: "%02x", $0) }.joined()
    cacheURL = directory.appendingPathComponent(name + ".json")
  }
  func start() {
    DispatchQueue(label: "monocode.transcript.worker", qos: .userInitiated).async { [self] in run()
    }
  }

  func configure(style: TranscriptStyle, viewport: CGRect) {
    lock.lock()
    if self.style != style || self.viewport != viewport {
      self.style = style
      self.viewport = viewport
      dirtyLayout = true
      signal()
    }
    lock.unlock()
  }
  func command(_ command: [String: Any]) {
    lock.lock()
    commands.append(command)
    signal()
    lock.unlock()
  }
  func setActive(_ value: Bool) {
    lock.lock()
    if active != value {
      active = value
      signal()
    }
    let id = watchID
    lock.unlock()
    if !value, let id { closeWatch(id) }
  }
  func purge() {
    lock.lock()
    purgeLayouts = true
    signal()
    lock.unlock()
  }
  func stop() {
    lock.lock()
    running = false
    signal()
    let id = watchID
    lock.unlock()
    if let id { closeWatch(id) }
  }
  private func closeWatch(_ id: Int) {
    DispatchQueue.global(qos: .userInitiated).async {
      _ = try? TranscriptNative.call(["operation": "unwatch", "watchId": id])
    }
  }

  /// Called under the condition lock. A revision prevents a completion or
  /// command arriving during layout from being lost just before we wait.
  private func signal() {
    wakeRevision &+= 1
    lock.broadcast()
  }
  private func wake() {
    lock.lock()
    signal()
    lock.unlock()
  }
  private func wait(after revision: UInt64, until deadline: Date? = nil) {
    lock.lock()
    while running && wakeRevision == revision {
      if let deadline {
        if !lock.wait(until: deadline) { break }
      } else {
        lock.wait()
      }
    }
    lock.unlock()
  }

  private func run() {
    var seed = configuration["turns"] as? [[String: Any]] ?? []
    if seed.isEmpty, let data = try? Data(contentsOf: cacheURL),
      let cached = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
    {
      seed = cached
    }
    guard
      let created = try? TranscriptNative.call([
        "operation": "transcriptCreate", "turns": seed,
        "collapsed": configuration["collapsed"] as? Bool ?? true,
      ]) as? [String: Any], let transcriptID = created["transcriptId"] as? Int
    else {
      let engine = TranscriptLayoutEngine()
      engine.accept([
        "order": [], "rows": [],
        "status": [
          "busy": false,
          "error": "Rebuild the iOS app to enable the native transcript.", "hasTurns": false,
          "userCount": 0, "pendingId": NSNull(), "ready": false,
        ],
      ])
      onFrame?(engine.build(style: style, viewport: .zero))
      return
    }
    let engine = TranscriptLayoutEngine()
    var latest: [String: Any]?
    var watch: Int?
    var firstWatch = true
    var from: String?
    var retryAt = Date.distantPast
    var saved = false
    var published = false
    var opening: TranscriptRequest?
    var reading: TranscriptRequest?
    var receiving: TranscriptRequest?
    var older: TranscriptRequest?
    let ready: () -> Void = { [weak self] in self?.wake() }
    func update(_ command: [String: Any]) throws -> [String: Any] {
      let output =
        try TranscriptNative.call([
          "operation": "transcriptUpdate", "transcriptId": transcriptID, "command": command,
        ]) as? [String: Any] ?? [:]
      if output["status"] != nil { engine.accept(output) }
      return output
    }
    func exchange(_ method: String, _ params: [String: Any]) throws -> [String: Any] {
      var input = connectionInput("exchange")
      input["request"] = request(method, params)
      return try TranscriptNative.call(input) as? [String: Any] ?? [:]
    }
    func save() {
      guard let snapshot = try? update(["type": "snapshot"]),
        let turns = snapshot["turns"] as? [[String: Any]], !turns.isEmpty,
        let data = try? JSONSerialization.data(withJSONObject: turns)
      else { return }
      try? data.write(to: cacheURL, options: .atomic)
      // Match the existing twelve-conversation cache bound.
      if let urls = try? FileManager.default.contentsOfDirectory(
        at: cacheURL.deletingLastPathComponent(),
        includingPropertiesForKeys: [.contentModificationDateKey])
      {
        let newest = urls.sorted {
          ((try? $0.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate)
            ?? .distantPast)
            > ((try? $1.resourceValues(forKeys: [.contentModificationDateKey])
              .contentModificationDate) ?? .distantPast)
        }
        for old in newest.dropFirst(12) { try? FileManager.default.removeItem(at: old) }
      }
    }
    defer {
      if let watch { closeWatch(watch) }
      save()
      _ = try? TranscriptNative.call(["operation": "transcriptDrop", "transcriptId": transcriptID])
    }
    latest = try? update(["type": "render"])
    if let pending = configuration["pending"] {
      latest = try? update(["type": "pending", "pending": pending])
    }
    func iterate() -> Bool {
      lock.lock()
      let alive = running
      let revision = wakeRevision
      let foreground = active
      let nextStyle = style
      let visible = viewport
      let relayout = dirtyLayout
      let purge = purgeLayouts
      let queued = commands
      commands.removeAll()
      dirtyLayout = false
      purgeLayouts = false
      lock.unlock()
      if !alive { return false }
      if purge { engine.purge() }
      var changed = false
      if let result = older?.take() {
        older = nil
        do {
          let page = try result.get()
          latest = try update([
            "type": "prepend", "turns": page["turns"] ?? [], "older": page["older"] ?? false,
          ])
        } catch { latest = try? update(["type": "error", "error": error.localizedDescription]) }
        changed = true
      }
      if let result = reading?.take() {
        reading = nil
        do {
          var conversation = try result.get()
          conversation["type"] = "replace"
          latest = try update(conversation)
        } catch { latest = try? update(["type": "error", "error": error.localizedDescription]) }
        retryAt = Date().addingTimeInterval(1)
        changed = true
      }
      for command in queued {
        do {
          if command["type"] as? String == "older" {
            if older == nil, let before = latest?["firstTurnId"] as? String {
              let params: [String: Any] = [
                "conversationId": configuration["conversationId"] ?? "", "before": before,
                "limit": 20,
              ]
              older = TranscriptRequest(ready: ready) {
                try exchange(
                  "conversation.older",
                  params)
              }
            }
          } else if command["type"] as? String == "selection" {
            let selected = try update(command)
            onSelection?(selected["text"] as? String ?? "")
          } else {
            latest = try update(command)
            changed = true
          }
        } catch {
          latest = try? update(["type": "error", "error": error.localizedDescription])
          changed = true
        }
      }
      if nextStyle.width > 0, changed || relayout || latest != nil && !published {
        onFrame?(engine.build(style: nextStyle, viewport: visible))
        published = true
      }
      if !foreground {
        if let result = opening?.take() {
          opening = nil
          if case .success(let value) = result, let id = value["watchId"] as? Int { closeWatch(id) }
        }
        if let id = watch {
          closeWatch(id)
          watch = nil
          setWatch(nil)
        }
        receiving = nil
        if !saved {
          save()
          saved = true
        }
        wait(after: revision)
        return true
      }
      saved = false
      do {
        if watch == nil {
          if retryAt > Date() || reading != nil {
            wait(after: revision, until: reading == nil ? retryAt : nil)
            return true
          }
          if opening == nil {
            var params: [String: Any] = [
              "conversationId": configuration["conversationId"] ?? "", "tail": 3,
              "incremental": true,
            ]
            // Cached turns are only a preview; never use their old cursor.
            if !firstWatch, let cursor = latest?["watchFrom"] as? String {
              params["from"] = cursor
              from = cursor
            }
            var input = connectionInput("watch")
            input["request"] = request("conversation.watch", params)
            let request = input
            opening = TranscriptRequest(
              discard: { [self] value in
                if let id = value["watchId"] as? Int { closeWatch(id) }
              }, ready: ready,
              { [self] in
                let opened = try TranscriptNative.call(request) as? [String: Any] ?? [:]
                lock.lock()
                let keep = running && active
                lock.unlock()
                if !keep, let id = opened["watchId"] as? Int {
                  closeWatch(id)
                  throw TranscriptNative.failure("Conversation paused")
                }
                return opened
              })
          }
          guard let result = opening?.take() else {
            wait(after: revision)
            return true
          }
          opening = nil
          let opened = try result.get()
          watch = opened["watchId"] as? Int
          setWatch(watch)
          firstWatch = false
        }
        guard let watch else { throw TranscriptNative.failure("Could not open conversation") }
        if let result = receiving?.take() {
          receiving = nil
          let available = try result.get()
          if available["ready"] as? Bool == true {
            let response =
              try TranscriptNative.call([
                "operation": "transcriptNext", "transcriptId": transcriptID, "watchId": watch,
                "timeoutMs": 0,
              ]) as? [String: Any]
            if response?["idle"] as? Bool != true {
              latest = response
              if let latest { engine.accept(latest) }
              if let cursor = from {
                latest = try update(["type": "cursor", "from": cursor])
                from = nil
              }
              if latest != nil, nextStyle.width > 0 {
                onFrame?(engine.build(style: nextStyle, viewport: visible))
              }
            }
          }
        }
        if receiving == nil {
          // Wait for arrival off the layout worker. Projection stays on this
          // worker so frame deltas and local commands cannot overtake each other.
          receiving = TranscriptRequest(ready: ready) {
            try TranscriptNative.call([
              "operation": "wait", "watchId": watch,
            ]) as? [String: Any] ?? [:]
          }
        }
      } catch {
        receiving = nil
        if let id = watch {
          closeWatch(id)
          watch = nil
          setWatch(nil)
        }
        // A dropped watch and an older desktop both keep working through reads.
        lock.lock()
        let stillActive = active && running
        lock.unlock()
        guard stillActive else { return true }
        if reading == nil {
          let params: [String: Any] = ["conversationId": configuration["conversationId"] ?? ""]
          reading = TranscriptRequest(ready: ready) { try exchange("conversation.read", params) }
        }
      }
      wait(after: revision)
      return true
    }
    // A long-lived DispatchQueue block needs a pool per iteration, otherwise
    // Foundation's temporary JSON/text objects survive for the entire watch.
    while autoreleasepool(invoking: iterate) {}
  }

  private func setWatch(_ id: Int?) {
    lock.lock()
    watchID = id
    lock.unlock()
  }
  private func connectionInput(_ operation: String) -> [String: Any] {
    [
      "operation": operation, "identity": configuration["identity"] ?? [:],
      "pin": configuration["pin"] ?? "", "addresses": configuration["addresses"] ?? [],
    ]
  }
  private func request(_ method: String, _ params: [String: Any]) -> [String: Any] {
    ["version": 1, "id": UUID().uuidString, "method": method, "params": params]
  }
}
