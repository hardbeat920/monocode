import CoreText
import UIKit

struct TranscriptLayoutFrame {
  let version: Int
  let rows: [TranscriptRowModel]
  let offsets: [CGFloat]
  let prepared: [String: TranscriptTextLayout]
  let groupWidths: [String: CGFloat]
  let status: [String: Any]
  let lastUserID: String?
  let older: Bool
  let firstTurnID: String?
  let watchFrom: String?
  var height: CGFloat { offsets.last ?? 0 }

  func index(at y: CGFloat) -> Int {
    var lo = 0
    var hi = rows.count
    while lo < hi {
      let mid = (lo + hi) / 2
      if offsets[mid + 1] <= y { lo = mid + 1 } else { hi = mid }
    }
    return min(lo, max(0, rows.count - 1))
  }
}

/// Confined to the transcript worker. UIKit only sees immutable frames.
final class TranscriptLayoutEngine {
  private var sources: [String: [String: Any]] = [:]
  private var models: [String: TranscriptRowModel] = [:]
  private var order: [String] = []
  private var metadata: [String: Any] = [:]
  private var style = TranscriptStyle()
  private var version = 0
  private var dirty = false
  private let layouts = NSCache<NSString, TranscriptTextLayout>()
  /// A Mono's "Thinking…" line waits this long before it shows, as Messages'
  /// typing dots do, so a quick reply or reaction lands without it flashing.
  static let thinkingDelay: TimeInterval = 1
  /// Since when a Mono's turn has had a live line, by look rather than ID, so
  /// the desktop's taking over from the phone's doesn't start the wait again.
  private var liveSince: Date?
  private var holding = false
  private var primed = false
  /// When the line held back is due, for the worker to build again then.
  private(set) var revealAt: Date?
  /// A reaction the Mono sends as its turn ends waits this long, so its
  /// "Thinking…" line fades and the chat settles before the message makes
  /// room for it: one motion after the other, not both at once.
  static let reactionDelay: TimeInterval = 0.4
  private var showedLive = false
  private var liveEndedAt: Date?
  /// The reaction each of the user's messages was last shown with.
  private var shownReactions: [String: String] = [:]
  /// Messages shown without the reaction they have yet, and as what.
  private var staged: [String: TranscriptRowModel] = [:]

  init() {
    layouts.countLimit = 500
    layouts.totalCostLimit = 12 * 1024 * 1024
  }
  func purge() { layouts.removeAllObjects() }

  // Consume every delta even when several arrive before a frame is published.
  func accept(_ update: [String: Any]) {
    metadata = update
    for row in update["rows"] as? [[String: Any]] ?? [] {
      guard let id = row["id"] as? String else { continue }
      sources[id] = row
      models.removeValue(forKey: id)
      staged.removeValue(forKey: id)
      layouts.removeObject(forKey: id as NSString)
      dirty = true
    }
    if let ids = update["order"] as? [String] {
      let removed = Set(order).subtracting(ids)
      for id in removed { layouts.removeObject(forKey: id as NSString) }
      order = ids
      let retained = Set(ids)
      sources = sources.filter { retained.contains($0.key) }
      models = models.filter { retained.contains($0.key) }
      dirty = true
    }
  }
  /// `rows` without a Mono's live line until it has been live for the delay.
  /// One already live when the chat opens shows straight away.
  private static func isLive(_ row: TranscriptRowModel) -> Bool {
    row.kind == "fold" && row.action["live"] as? Bool == true
  }
  /// `rows` with a reaction that lands as the shown "Thinking…" line goes
  /// held off its message until the chat has closed up under that line.
  private func stagingReactions(_ rows: [TranscriptRowModel]) -> [TranscriptRowModel] {
    guard !style.agent.isEmpty else { return rows }
    let now = Date()
    let live = rows.contains(where: Self.isLive)
    if showedLive && !live { liveEndedAt = now }
    showedLive = live
    let due = liveEndedAt.map { $0.addingTimeInterval(Self.reactionDelay) }
    let waiting = due.map { now < $0 } ?? false
    if !waiting { liveEndedAt = nil }
    var result = rows
    var held: [String: TranscriptRowModel] = [:]
    for (index, row) in rows.enumerated() where row.kind == "user" {
      guard let reaction = row.action["reaction"] as? String else {
        shownReactions[row.id] = nil
        continue
      }
      if shownReactions[row.id] == reaction { continue }
      if waiting, var source = sources[row.id], var action = source["action"] as? [String: Any] {
        let model =
          staged[row.id]
          ?? {
            action["reaction"] = nil
            source["action"] = action
            return TranscriptRowModel(source, style: style)
          }()
        result[index] = model
        held[row.id] = model
      } else {
        shownReactions[row.id] = reaction
      }
    }
    if Set(held.keys) != Set(staged.keys) {
      // Its text lays out lower with the reaction, so it's measured again.
      for id in Set(held.keys).symmetricDifference(staged.keys) {
        layouts.removeObject(forKey: id as NSString)
      }
      dirty = true
    }
    staged = held
    if !held.isEmpty, let due { revealAt = min(revealAt ?? due, due) }
    return result
  }
  private func holdingThinking(_ rows: [TranscriptRowModel]) -> [TranscriptRowModel] {
    let isLive = Self.isLive
    revealAt = nil
    defer { if !rows.isEmpty { primed = true } }
    guard !style.agent.isEmpty, rows.contains(where: isLive) else {
      liveSince = nil
      if holding { holding = false; dirty = true }
      return rows
    }
    let now = Date()
    let since = liveSince ?? (primed ? now : .distantPast)
    liveSince = since
    let due = since.addingTimeInterval(Self.thinkingDelay)
    let hold = now < due
    // Shown or hidden, it's a new frame.
    if hold != holding { holding = hold; dirty = true }
    guard hold else { return rows }
    revealAt = due
    return rows.filter { !isLive($0) }
  }
  func build(style next: TranscriptStyle, viewport: CGRect) -> TranscriptLayoutFrame {
    if style != next {
      style = next
      models.removeAll()
      staged.removeAll()
      layouts.removeAllObjects()
      dirty = true
    }
    let all = order.compactMap { id -> TranscriptRowModel? in
      if let cached = models[id] { return cached }
      guard let source = sources[id] else { return nil }
      let model = TranscriptRowModel(source, style: style)
      models[id] = model
      return model
    }
    let rows = stagingReactions(holdingThinking(all))
    var offsets: [CGFloat] = [0]
    var groupWidths: [String: CGFloat] = [:]
    offsets.reserveCapacity(rows.count + 1)
    for row in rows {
      offsets.append(offsets.last! + row.height)
      if let group = row.group {
        groupWidths[group] = max(groupWidths[group] ?? 0, row.horizontalWidth)
      }
    }
    if dirty {
      version += 1
      dirty = false
    }
    let shell = TranscriptLayoutFrame(
      version: version, rows: rows, offsets: offsets, prepared: [:], groupWidths: groupWidths,
      status: metadata["status"] as? [String: Any] ?? [:],
      lastUserID: metadata["lastUserId"] as? String,
      older: metadata["older"] as? Bool ?? false, firstTurnID: metadata["firstTurnId"] as? String,
      watchFrom: metadata["watchFrom"] as? String)
    // On first open prepare the tail; afterwards stay several screens ahead
    // of native deceleration. Scrolling never parses or measures text.
    let userIndex = rows.firstIndex { $0.id == shell.lastUserID }
    let opensAnchored =
      style.agent.isEmpty
      && (shell.status["busy"] as? Bool == true || shell.status["pendingId"] is String)
    let initialY = opensAnchored ? userIndex.map { offsets[$0] } ?? 0 : max(0, shell.height - 1800)
    let area =
      viewport.height > 0 ? viewport : CGRect(x: 0, y: initialY, width: style.width, height: 1800)
    var prepared: [String: TranscriptTextLayout] = [:]
    if !rows.isEmpty {
      let from = shell.index(at: max(0, area.minY - area.height * 2))
      let to = shell.index(at: area.maxY + area.height * 2)
      for index in from...max(from, to) {
        let row = rows[index]
        let key = row.id as NSString
        let layout = layouts.object(forKey: key) ?? TranscriptTextLayout(row)
        layouts.setObject(layout, forKey: key, cost: max(256, row.text.length * 32))
        prepared[row.id] = layout
      }
    }
    return TranscriptLayoutFrame(
      version: version, rows: rows, offsets: offsets, prepared: prepared, groupWidths: groupWidths,
      status: shell.status, lastUserID: shell.lastUserID, older: shell.older,
      firstTurnID: shell.firstTurnID, watchFrom: shell.watchFrom)
  }
}
