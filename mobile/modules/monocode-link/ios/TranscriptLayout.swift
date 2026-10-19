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
  func build(style next: TranscriptStyle, viewport: CGRect) -> TranscriptLayoutFrame {
    if style != next {
      style = next
      models.removeAll()
      layouts.removeAllObjects()
      dirty = true
    }
    let rows = order.compactMap { id -> TranscriptRowModel? in
      if let cached = models[id] { return cached }
      guard let source = sources[id] else { return nil }
      let model = TranscriptRowModel(source, style: style)
      models[id] = model
      return model
    }
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
