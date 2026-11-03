import CoreText
import UIKit

/// Text a visual lays out, in its row's coordinates.
struct TranscriptVisualText {
  let text: NSAttributedString
  let rect: CGRect
  let lines: Int
}
/// A filled shape, or a stroked one when `stroke` is set, drawn by the row's canvas.
struct TranscriptVisualPaint {
  let path: CGPath
  let color: UIColor
  var stroke: CGFloat? = nil
}

/// A visual's component tree laid out for the transcript, as the desktop's
/// VisualView draws it (crates/monocode-gpui/src/visuals): Bayer-dithered 3pt
/// cells, the same scales, curves and tones, without hover or motion. Its
/// controls show their state only; the visual runs on the desktop.
final class TranscriptVisualLayout {
  static let cell: CGFloat = 3
  private static let bayer: [CGFloat] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5]
  private(set) var texts: [TranscriptVisualText] = []
  private(set) var paints: [TranscriptVisualPaint] = []
  private let style: TranscriptStyle
  private let content: UIColor
  private let dark: Bool

  init(style: TranscriptStyle) {
    self.style = style
    content = UIColor.transcriptHex(style.content)
    var white: CGFloat = 0
    content.getWhite(&white, alpha: nil)
    dark = white > 0.5
  }

  static func inked(_ col: Int, _ row: Int, _ tone: CGFloat) -> Bool {
    tone > (bayer[(row & 3) * 4 + (col & 3)] + 0.5) / 16
  }
  private static func step(_ tone: CGFloat) -> CGFloat { (tone * 16).rounded() / 16 }

  // MARK: Colours

  private func color(_ token: Any?) -> UIColor {
    let hues: [String: (String, String)] = [
      "positive": ("#4fd57f", "#0a924b"), "negative": ("#fc5855", "#d01d21"),
      "warning": ("#fea247", "#d96700"), "series2": ("#4fd57f", "#0a924b"),
      "series3": ("#b77df8", "#863ad2"), "series4": ("#fea247", "#d96700"),
      "series5": ("#fa78b2", "#d12c80"), "series6": ("#3faff3", "#0077d7"),
    ]
    switch token as? String ?? "accent" {
    case "ink": return content
    case "muted": return content.withAlphaComponent(0.6)
    case "faint": return content.withAlphaComponent(0.2)
    case let name:
      if let hue = hues[name] { return UIColor.transcriptHex(dark ? hue.0 : hue.1) }
      return UIColor.transcriptHex(style.accent)
    }
  }

  // MARK: Drawing primitives

  /// Text at (x, y) in `width`, wrapped unless `lines`; returns its size.
  @discardableResult
  private func text(
    _ value: String, size: CGFloat, weight: UIFont.Weight = .regular, color: UIColor,
    mono: Bool = false, x: CGFloat, y: CGFloat, width: CGFloat, lines: Int = 0,
    align: NSTextAlignment = .left
  ) -> CGSize {
    let pointSize = size * style.scale
    let font =
      mono
      ? UIFont.monospacedSystemFont(ofSize: pointSize, weight: weight)
      : UIFont.systemFont(ofSize: pointSize, weight: weight)
    let line = style.line((size * 1.45).rounded())
    let paragraph = NSMutableParagraphStyle()
    paragraph.minimumLineHeight = line
    paragraph.maximumLineHeight = line
    paragraph.alignment = align
    paragraph.lineBreakMode = lines == 1 ? .byTruncatingTail : .byWordWrapping
    let string = NSAttributedString(
      string: value,
      attributes: [.font: font, .foregroundColor: color, .paragraphStyle: paragraph])
    let measured = TranscriptTypography.measure(string, width: max(1, width), line: line)
    let height = lines > 0 ? min(measured.height, CGFloat(lines) * line) : measured.height
    texts.append(
      TranscriptVisualText(
        text: string, rect: CGRect(x: x, y: y, width: max(1, width), height: height), lines: lines))
    return CGSize(width: measured.width, height: height)
  }

  /// A grid of cells from (x, y), one path per colour, as the desktop paints a
  /// quad per run.
  private func cells(
    x: CGFloat, y: CGFloat, cols: Int, rows: Int, ink: (Int, Int) -> UIColor?
  ) {
    let c = Self.cell
    var paths: [(UIColor, CGMutablePath)] = []
    func path(for color: UIColor) -> CGMutablePath {
      if let found = paths.first(where: { $0.0 === color }) { return found.1 }
      let created = CGMutablePath()
      paths.append((color, created))
      return created
    }
    for row in 0..<max(0, rows) {
      var start = 0
      var run: UIColor?
      for col in 0...max(0, cols) {
        let next = col < cols ? ink(col, row) : nil
        if next === run { continue }
        if let run {
          path(for: run).addRect(
            CGRect(x: x + CGFloat(start) * c, y: y + CGFloat(row) * c, width: CGFloat(col - start) * c, height: c))
        }
        run = next
        start = col
      }
    }
    for (color, path) in paths { paints.append(TranscriptVisualPaint(path: path, color: color)) }
  }

  private func rect(_ rect: CGRect, _ color: UIColor) {
    paints.append(TranscriptVisualPaint(path: CGPath(rect: rect, transform: nil), color: color))
  }

  /// A line with a soft glow under it, as `glowing_line`.
  private func line(_ path: CGPath, _ color: UIColor, width: CGFloat) {
    let dim: CGFloat = dark ? 1 : 0.45
    paints.append(TranscriptVisualPaint(path: path, color: color.withAlphaComponent(0.07 * dim), stroke: width + 10))
    paints.append(TranscriptVisualPaint(path: path, color: color.withAlphaComponent(0.14 * dim), stroke: width + 5))
    paints.append(TranscriptVisualPaint(path: path, color: color, stroke: width))
  }

  /// A square end marker in a faint halo of its colour.
  private func marker(_ x: CGFloat, _ y: CGFloat, _ color: UIColor) {
    let side = 2 * Self.cell
    rect(CGRect(x: x - (side + 6) / 2, y: y - (side + 6) / 2, width: side + 6, height: side + 6), color.withAlphaComponent(0.25))
    rect(CGRect(x: x - side / 2, y: y - side / 2, width: side, height: side), color)
  }

  private func axis(
    _ value: String, x: CGFloat, y: CGFloat, align: NSTextAlignment = .left, width: CGFloat = 60
  ) {
    let left = align == .right ? x - width : align == .center ? x - width / 2 : x
    text(
      value, size: 10, color: content.withAlphaComponent(0.45), mono: true, x: left, y: y - 7,
      width: width, lines: 1, align: align)
  }

  // MARK: Layout

  /// Lays out `node` at (x, y) in `width`; returns its height.
  func layout(_ node: [String: Any], x: CGFloat, y: CGFloat, width: CGFloat) -> CGFloat {
    let gap = node["gap"] as? CGFloat ?? 0
    let children = node["children"] as? [[String: Any]] ?? []
    switch node["kind"] as? String ?? "" {
    case "column":
      return stack(children, x: x, y: y, width: width, gap: gap)
    case "card":
      let pad: CGFloat = 14
      let inner = stack(children, x: x + pad, y: y + pad, width: width - pad * 2, gap: gap)
      let frame = CGRect(x: x, y: y, width: width, height: inner + pad * 2)
      paints.insert(
        TranscriptVisualPaint(
          path: CGPath(roundedRect: frame, cornerWidth: 14, cornerHeight: 14, transform: nil),
          color: content.withAlphaComponent(0.03)), at: 0)
      paints.append(
        TranscriptVisualPaint(
          path: CGPath(roundedRect: frame.insetBy(dx: 0.5, dy: 0.5), cornerWidth: 14, cornerHeight: 14, transform: nil),
          color: content.withAlphaComponent(0.08), stroke: 1 / style.pixelRatio))
      return frame.height
    case "row":
      // As many side by side as fit at 120pt each, wrapping.
      let per = max(1, min(children.count, Int((width + gap) / (120 + gap))))
      return flow(children, per: per, x: x, y: y, width: width, gap: gap)
    case "grid":
      let per = max(1, min(node["columns"] as? Int ?? 1, Int(width / 180)))
      return flow(children, per: per, x: x, y: y, width: width, gap: gap)
    case "text":
      let sizes: [String: CGFloat] = [
        "caption": 11, "small": 12, "body": 14, "subtitle": 17, "title": 22, "hero": 34,
      ]
      let size = node["size"] as? String ?? "body"
      return text(
        node["text"] as? String ?? "", size: sizes[size] ?? 14,
        weight: ["subtitle", "title", "hero"].contains(size) ? .semibold : .regular,
        color: color(node["color"] ?? "ink"), x: x, y: y, width: width
      ).height
    case "stat": return stat(node, x: x, y: y, width: width)
    case "table": return table(node, x: x, y: y, width: width)
    case "chart":
      let height = node["height"] as? CGFloat ?? 220
      cartesian(node, x: x, y: y, width: width, height: height > 0 ? height : 220)
      return height > 0 ? height : 220
    case "gauge": return gauge(node, x: x, y: y, width: width)
    case "donut": return donut(node, x: x, y: y, width: width)
    case "heatmap": return heatmap(node, x: x, y: y, width: width)
    case "progress": return progress(node, x: x, y: y, width: width)
    case "canvas": return canvas(node, x: x, y: y, width: width)
    case "button", "toggle", "input", "select", "slider", "tabs":
      return control(node, x: x, y: y, width: width)
    case "scene":
      return text(
        "Open this 3D scene on your desktop to see it.", size: 12,
        color: content.withAlphaComponent(0.55), x: x, y: y, width: width
      ).height
    default:
      return 0
    }
  }

  private func stack(_ children: [[String: Any]], x: CGFloat, y: CGFloat, width: CGFloat, gap: CGFloat) -> CGFloat {
    var cursor = y
    for (index, child) in children.enumerated() {
      if index > 0 { cursor += gap }
      cursor += layout(child, x: x, y: cursor, width: width)
    }
    return cursor - y
  }

  private func flow(
    _ children: [[String: Any]], per: Int, x: CGFloat, y: CGFloat, width: CGFloat, gap: CGFloat
  ) -> CGFloat {
    let each = (width - gap * CGFloat(per - 1)) / CGFloat(per)
    var cursor = y
    var start = 0
    while start < children.count {
      var tallest: CGFloat = 0
      for column in 0..<per where start + column < children.count {
        tallest = max(
          tallest,
          layout(children[start + column], x: x + CGFloat(column) * (each + gap), y: cursor, width: each))
      }
      cursor += tallest
      start += per
      if start < children.count { cursor += gap }
    }
    return cursor - y
  }

  // MARK: Pieces

  private func stat(_ node: [String: Any], x: CGFloat, y: CGFloat, width: CGFloat) -> CGFloat {
    var cursor = y
    cursor += text(
      node["label"] as? String ?? "", size: 13, color: content.withAlphaComponent(0.6), x: x, y: cursor,
      width: width, lines: 1
    ).height + 4
    cursor += text(
      node["value"] as? String ?? "", size: 30, weight: .semibold, color: color(node["color"] ?? "ink"),
      x: x, y: cursor, width: width
    ).height
    var delta = node["delta"] as? String
    var detail = node["detail"] as? String
    // "+12.4% vs last month" lends its leading change to the delta.
    if delta == nil, let full = detail, let first = full.first, "+-−".contains(first),
      full.dropFirst().first?.isNumber == true
    {
      let parts = full.split(separator: " ", maxSplits: 1)
      delta = String(parts[0])
      detail = parts.count > 1 ? String(parts[1]) : nil
    }
    if delta != nil || detail != nil {
      cursor += 4
      var left = x
      var lineHeight: CGFloat = 0
      if let delta {
        let down = delta.hasPrefix("-") || delta.hasPrefix("−")
        let good = down == (node["lower_is_better"] as? Bool == true)
        let size = text(
          (down ? "▼ " : "▲ ") + delta, size: 12, color: color(good ? "positive" : "negative"), mono: true,
          x: left, y: cursor, width: width, lines: 1)
        left += size.width + 8
        lineHeight = size.height
      }
      if let detail {
        lineHeight = max(
          lineHeight,
          text(detail, size: 12, color: content.withAlphaComponent(0.55), x: left, y: cursor, width: width - (left - x), lines: 1).height)
      }
      cursor += lineHeight
    }
    if let trend = (node["trend"] as? [NSNumber])?.map({ CGFloat(truncating: $0) }), trend.count > 1 {
      cursor += 8
      let token = node["color"] as? String ?? "ink"
      self.trend(trend, tint: ["ink", "muted", "faint"].contains(token) ? UIColor.transcriptHex(style.accent) : color(token), x: x, y: cursor, width: width)
      cursor += 56
    }
    return cursor - y
  }

  private func table(_ node: [String: Any], x: CGFloat, y: CGFloat, width: CGFloat) -> CGFloat {
    let columns = node["columns"] as? [String] ?? []
    let rows = node["rows"] as? [[String]] ?? []
    let count = max(1, columns.count)
    let each = width / CGFloat(count)
    var cursor = y
    for (index, cells) in ([columns] + rows).enumerated() {
      var tallest: CGFloat = 0
      for (column, cell) in cells.prefix(count).enumerated() {
        tallest = max(
          tallest,
          text(
            cell, size: 12, weight: index == 0 ? .semibold : .regular,
            color: index == 0 ? content.withAlphaComponent(0.65) : content,
            x: x + CGFloat(column) * each + 6, y: cursor + 7, width: each - 12, lines: 2
          ).height)
      }
      cursor += tallest + 14
      rect(CGRect(x: x, y: cursor - 1 / style.pixelRatio, width: width, height: 1 / style.pixelRatio), content.withAlphaComponent(0.08))
    }
    return cursor - y
  }

  private func control(_ node: [String: Any], x: CGFloat, y: CGFloat, width: CGFloat) -> CGFloat {
    let label = node["label"] as? String ?? ""
    var value = ""
    switch node["kind"] as? String {
    case "toggle": value = node["value"] as? Bool == true ? "On" : "Off"
    case "input": value = node["value"] as? String ?? ""
    case "select":
      let options = node["options"] as? [String] ?? []
      let selected = node["selected"] as? Int ?? 0
      value = selected < options.count ? options[selected] : ""
    case "slider": value = "\(node["value"] as? Double ?? 0)"
    case "tabs":
      let labels = node["labels"] as? [String] ?? []
      let selected = node["selected"] as? Int ?? 0
      return text(
        labels.enumerated().map { $0.offset == selected ? "[\($0.element)]" : $0.element }.joined(separator: "  "),
        size: 12, color: content.withAlphaComponent(0.7), x: x, y: y, width: width, lines: 1
      ).height
    default: break
    }
    let shown = text(label, size: 13, color: content, x: x, y: y, width: width * 0.6, lines: 1)
    if !value.isEmpty {
      text(
        value, size: 12, color: content.withAlphaComponent(0.55), mono: true, x: x + width * 0.6, y: y,
        width: width * 0.4, lines: 1, align: .right)
    }
    return shown.height
  }

  // MARK: Charts

  private func cartesian(_ node: [String: Any], x: CGFloat, y: CGFloat, width: CGFloat, height: CGFloat) {
    let c = Self.cell
    let kind = node["chart_type"] as? String ?? "bar"
    let series = node["series"] as? [[String: Any]] ?? []
    let labels = node["labels"] as? [String] ?? []
    let values = series.map { ($0["values"] as? [NSNumber] ?? []).map { CGFloat(truncating: $0) } }
    let bars = kind == "bar"
    let count = max(labels.count, values.map(\.count).max() ?? 0)
    guard count > 0 else { return }
    let scale = TranscriptScale(values.flatMap { $0 }, zero: kind != "line")
    let ticks = scale.ticks
    let tickLabels = ticks.map { scale.label($0) }
    let longest = CGFloat(tickLabels.map(\.count).max() ?? 1)
    let left = (longest * 6.1).rounded(.up) + 10
    let top: CGFloat = bars ? 16 : 8
    let right = width - (bars ? 0 : 6)
    let cols = max(0, Int((right - left) / c))
    let rows = max(0, Int((height - top - 22) / c))
    let plotW = CGFloat(cols) * c
    let plotH = CGFloat(rows) * c
    let ox = x + left
    let oy = y + top
    let yOf = { (v: CGFloat) in scale.depth(v) * plotH }
    let band = plotW / CGFloat(count)
    let xOf = { (i: Int) -> CGFloat in
      bars ? band * (CGFloat(i) + 0.5) : count <= 1 ? plotW / 2 : CGFloat(i) * plotW / CGFloat(count - 1)
    }
    let tints = series.map { color($0["color"] ?? "series1") }
    for (tick, label) in zip(ticks, tickLabels) {
      let ty = oy + yOf(tick).rounded()
      rect(CGRect(x: ox, y: ty, width: plotW, height: 1), content.withAlphaComponent(0.07))
      axis(label, x: ox - 10, y: ty, align: .right)
    }
    let stride = max(1, Int((CGFloat(count) / max(1, (plotW / 72).rounded(.down))).rounded(.up)))
    for (i, label) in labels.enumerated() where i % stride == 0 {
      let shown = label.count > 12 ? String(label.prefix(11)) + "…" : label
      let align: NSTextAlignment = bars ? .center : i == 0 ? .left : i + 1 == count ? .right : .center
      axis(shown, x: ox + xOf(i), y: oy + plotH + 13, align: align, width: 80)
    }
    if bars {
      let n = max(1, series.count)
      let widthPx = min((band * 0.7 - c * CGFloat(n - 1)) / CGFloat(n), 36)
      let wide = max(1, Int((widthPx / c).rounded()))
      let group = wide * n + (n - 1)
      let rowOf = { (v: CGFloat) in Int((scale.depth(v) * plotH / c).rounded()) }
      let zero = rowOf(min(max(scale.min, 0), scale.max))
      let fade = max(1, CGFloat(rows) * 0.7)
      var columns = [(Int, Int, Int)?](repeating: nil, count: cols)
      var lastTop: Int?
      for i in 0..<count {
        let start = Int((band * (CGFloat(i) + 0.5) / c - CGFloat(group) / 2).rounded())
        for (j, list) in values.enumerated() where i < list.count {
          let end = rowOf(list[i])
          let first = start + j * (wide + 1)
          for col in first..<first + wide where col >= 0 && col < cols {
            columns[col] = (min(end, zero), max(end, zero), j)
          }
          if i == count - 1 { lastTop = min(end, zero) }
        }
      }
      cells(x: ox, y: oy, cols: cols, rows: rows) { col, row in
        guard let bar = columns[col], row >= bar.0, row < bar.1 else { return nil }
        let tone = 0.75 - (0.75 - 0.125) * min(CGFloat(row - bar.0) / fade, 1)
        return row == bar.0 || Self.inked(col, row, tone) ? tints[bar.2] : nil
      }
      if values.count == 1, let lastTop, let latest = values[0].last, values[0].count == count {
        text(
          TranscriptScale.compact(latest, step: scale.step / 10), size: 10, color: content.withAlphaComponent(0.9),
          mono: true, x: ox + band * (CGFloat(count) - 0.5) - 40, y: oy + CGFloat(lastTop) * c - 16,
          width: 80, lines: 1, align: .center)
      }
      return
    }
    let curves = values.map { list in
      TranscriptCurve(list.enumerated().map { CGPoint(x: xOf($0.offset), y: yOf($0.element)) })
    }
    if kind == "area" {
      let baseline = yOf(max(scale.min, 0))
      for (index, curve) in curves.enumerated() where curve.count > 0 {
        let wash = curve.path(step: 4, origin: CGPoint(x: ox, y: oy))
        wash.addLine(to: CGPoint(x: ox + curve.lastX, y: oy + baseline))
        wash.addLine(to: CGPoint(x: ox + curve.firstX, y: oy + baseline))
        wash.closeSubpath()
        paints.append(TranscriptVisualPaint(path: wash, color: tints[index].withAlphaComponent(dark ? 0.07 : 0.04)))
      }
      let bands: [(CGFloat, CGFloat)] = [(0.05, 1), (0.13, 0.8), (0.25, 0.65), (0.4, 0.5), (0.6, 0.35)]
      cells(x: ox, y: oy, cols: cols, rows: rows) { col, row in
        let px = (CGFloat(col) + 0.5) * c
        let py = (CGFloat(row) + 0.5) * c
        guard py <= baseline else { return nil }
        // Later series sit in front, as stacked paper would.
        for index in curves.indices.reversed() {
          let curve = curves[index]
          guard curve.count > 0, px >= curve.firstX, px <= curve.lastX, py >= curve.at(px) else { continue }
          let depth = (py - curve.at(px)) / max(plotH, 1)
          let lead = max(2 * c / max(plotH, 1), bands[0].0)
          let share = depth < lead ? 1 : (bands.first { depth < $0.0 }?.1 ?? 0.2)
          return Self.inked(col, row, Self.step(0.625 * share)) ? tints[index] : nil
        }
        return nil
      }
    }
    for (index, curve) in curves.enumerated() where curve.count > 0 {
      line(curve.path(step: 2, origin: CGPoint(x: ox, y: oy)), tints[index], width: 2)
      marker(ox + curve.lastX, oy + curve.at(curve.lastX), tints[index])
    }
  }

  private func trend(_ values: [CGFloat], tint: UIColor, x: CGFloat, y: CGFloat, width: CGFloat) {
    let c = Self.cell
    let cols = Int(width / c)
    let rows = Int(56 / c)
    let w = CGFloat(cols) * c
    let h = CGFloat(rows) * c
    let low = values.min() ?? 0
    let high = values.max() ?? 1
    let range = high > low ? high - low : 1
    let curve = TranscriptCurve(
      values.enumerated().map {
        CGPoint(
          x: CGFloat($0.offset) / CGFloat(values.count - 1) * w,
          y: 6 + (1 - ($0.element - low) / range) * (h - 6 - h * 0.3))
      })
    let levels: [CGFloat] = [10 / 16, 6 / 16, 3 / 16, 1 / 16]
    cells(x: x, y: y, cols: cols, rows: rows) { col, row in
      let px = (CGFloat(col) + 0.5) * c
      let depth = (CGFloat(row) + 0.5) * c - curve.at(px)
      guard depth >= 0 else { return nil }
      return Self.inked(col, row, levels[min(Int(depth / 7), levels.count - 1)]) ? tint : nil
    }
    line(curve.path(step: 2, origin: CGPoint(x: x, y: y)), tint, width: 1.5)
    marker(x + w - c, y + curve.at(w), tint)
  }

  private func gauge(_ node: [String: Any], x: CGFloat, y: CGFloat, width: CGFloat) -> CGFloat {
    let c = Self.cell
    let value = node["value"] as? CGFloat ?? 0
    let low = node["min"] as? CGFloat ?? 0
    let high = node["max"] as? CGFloat ?? 1
    let unit = node["unit"] as? String
    let tint = color(node["color"])
    let side = min(220, width)
    let ratio = min(max((value - low) / max(high - low, 1e-9), 0), 1)
    let outer = side / 2 - 1 - c
    let height = side / 2 + (outer + c) * CGFloat(0.5).squareRoot() + 18
    let offset = ((width - side) / 2).rounded()
    let center = side / 2
    let thickness = max(2, (side * 0.12 / c).rounded()) * c
    let inner = outer - thickness
    let segments = 36
    let span: CGFloat = 270
    let segment = span / CGFloat(segments)
    let middle = outer - thickness / 2
    let gap = 2 / middle * 180 / .pi
    let filled = ratio * CGFloat(segments)
    let active = max(0, Int((filled - 1e-6).rounded(.up)))
    let track = tint.withAlphaComponent(0.14)
    cells(x: x + offset, y: y, cols: Int(side / c), rows: Int(height / c)) { col, row in
      let dx = (CGFloat(col) + 0.5) * c - center
      let dy = (CGFloat(row) + 0.5) * c - center
      let r = (dx * dx + dy * dy).squareRoot()
      guard r >= inner, r <= outer + c else { return nil }
      let along = atan2(dx, -dy) * 180 / .pi + 135
      guard along >= 0, along < span else { return nil }
      let index = Int(along / segment)
      let within = along - CGFloat(index) * segment
      guard within >= gap / 2, within <= segment - gap / 2 else { return nil }
      let head = active > 0 && index == active - 1
      if r > outer && !head { return nil }
      if index >= active { return track }
      if head {
        let part = min(max(filled - CGFloat(index), 0), 1)
        if within > gap / 2 + (segment - gap) * max(part, 0.15) { return r <= outer ? track : nil }
        return tint
      }
      return Self.inked(col, row, max(1 - CGFloat(active - 1 - index) / 16, 9 / 16)) ? tint : nil
    }
    let reading =
      unit == "%"
      ? TranscriptScale.compact(value, step: 1) + "%"
      : TranscriptScale.compact(value, step: 0.1) + (unit.map { " " + $0 } ?? "")
    text(
      reading, size: (side * 0.15).rounded(), weight: .semibold, color: content, x: x, y: y + center - side * 0.12,
      width: width, lines: 1, align: .center)
    text(
      node["label"] as? String ?? "", size: 12, color: content.withAlphaComponent(0.6), x: x,
      y: y + center + side * 0.07, width: width, lines: 1, align: .center)
    let ends = middle * CGFloat(0.5).squareRoot()
    axis(TranscriptScale.compact(low, step: 1), x: x + offset + center - ends, y: y + center + ends + thickness / 2 + 12, align: .center)
    axis(TranscriptScale.compact(high, step: 1), x: x + offset + center + ends, y: y + center + ends + thickness / 2 + 12, align: .center)
    return height
  }

  private func donut(_ node: [String: Any], x: CGFloat, y: CGFloat, width: CGFloat) -> CGFloat {
    let c = Self.cell
    let slices = node["slices"] as? [[String: Any]] ?? []
    let values = slices.map { max(0, $0["value"] as? CGFloat ?? 0) }
    let total = values.reduce(0, +)
    let tints = slices.map { color($0["color"] ?? "series1") }
    let size: CGFloat = 184
    let grid = Int(size / c)
    let half = CGFloat(grid) * c / 2
    let outer = half - 2 * c
    let inner = outer - (size * 0.17 / c).rounded() * c
    let tau = CGFloat.pi * 2
    var edges: [CGFloat] = []
    var running: CGFloat = 0
    for value in values {
      edges.append(running / max(total, 1e-9) * tau)
      running += value
    }
    let shown = values.filter { $0 > 0 }.count
    cells(x: x, y: y, cols: grid, rows: grid) { col, row in
      let dx = (CGFloat(col) + 0.5) * c - half
      let dy = (CGFloat(row) + 0.5) * c - half
      let r = (dx * dx + dy * dy).squareRoot()
      guard r >= inner, r <= outer else { return nil }
      var angle = atan2(dx, -dy)
      if angle < 0 { angle += tau }
      guard let index = edges.lastIndex(where: { angle >= $0 }) else { return nil }
      var outline = r > outer - c || r < inner + c
      if shown > 1 {
        var near = CGFloat.greatestFiniteMagnitude
        for (i, edge) in edges.enumerated() where values[i] > 0 {
          var d = abs(angle - edge)
          d = min(d, tau - d)
          if d <= .pi / 2 { near = min(near, r * sin(d) / c) }
        }
        if near < 0.75 { return nil }
        outline = outline || near < 1.75
      }
      let depth = (outer - r) / (outer - inner)
      let tone = 3 / 16 + (13 / 16) * pow(1 - depth, 1.4)
      return outline || Self.inked(col, row, tone) ? tints[index] : nil
    }
    let figure =
      node["value"] as? String
      ?? (total >= 1e4 ? NumberFormatter.localizedString(from: NSNumber(value: Double(total.rounded())), number: .decimal) : TranscriptScale.compact(total, step: 0.1))
    if let label = node["label"] as? String {
      axis(String(label.uppercased().prefix(18)), x: x + half, y: y + half - 14, align: .center, width: inner * 2)
    }
    text(
      figure, size: min(26, ((inner * 2 - 16) / (CGFloat(max(figure.count, 1)) * 0.62)).rounded(.down)),
      weight: .semibold, color: content, x: x + half - inner, y: y + half - 12, width: inner * 2, lines: 1,
      align: .center)
    // The legend beside the ring when there's room, else under it.
    let beside = width - size >= 160
    let lx = beside ? x + size + 20 : x
    var ly = beside ? y + max(0, (size - CGFloat(slices.count) * 22) / 2) : y + size + 12
    let lw = beside ? width - size - 20 : width
    for (index, slice) in slices.enumerated() {
      rect(CGRect(x: lx, y: ly + 5, width: 9, height: 9), tints[index])
      text(slice["label"] as? String ?? "", size: 12, color: content.withAlphaComponent(0.8), x: lx + 17, y: ly, width: lw * 0.55, lines: 1)
      let share = total > 0 ? values[index] / total * 100 : 0
      text(
        "\(TranscriptScale.compact(values[index], step: 0.1)) · \(Int(share.rounded()))%", size: 12,
        color: content.withAlphaComponent(0.5), mono: true, x: lx + 17 + lw * 0.55, y: ly, width: lw * 0.45 - 17,
        lines: 1, align: .right)
      ly += 22
    }
    return beside ? size : size + 12 + CGFloat(slices.count) * 22
  }

  private func heatmap(_ node: [String: Any], x: CGFloat, y: CGFloat, width: CGFloat) -> CGFloat {
    let c = Self.cell
    let values = (node["values"] as? [[NSNumber]] ?? []).map { $0.map { CGFloat(truncating: $0) } }
    let rowLabels = node["rows"] as? [String] ?? []
    let columnLabels = node["columns"] as? [String] ?? []
    let tint = color(node["color"])
    let track = content.withAlphaComponent(0.07)
    let gutter: CGFloat = rowLabels.isEmpty ? 0 : 34
    let top: CGFloat = columnLabels.isEmpty ? 0 : 18
    let pitch = 5 * c
    let cols = Int((width - gutter) / c)
    let fits = max(1, (cols + 1) / 5)
    let skip = max(0, (values.first?.count ?? 0) - fits)
    let high = max(1e-9, values.flatMap { $0 }.max() ?? 0)
    cells(x: x + gutter, y: y + top, cols: fits * 5, rows: values.count * 5) { col, row in
      let (cx, cy) = (col % 5, row % 5)
      guard cx < 4, cy < 4 else { return nil }
      let r = row / 5
      let column = col / 5 + skip
      guard r < values.count, column < values[r].count else { return nil }
      let v = values[r][column]
      if v <= 0 { return track }
      return Self.inked(cx, cy, Self.step(0.25 + 0.75 * v / high)) ? tint : track
    }
    for (i, label) in rowLabels.enumerated() where !(rowLabels.count > 4 && i % 2 == 1) {
      axis(label, x: x, y: y + top + CGFloat(i) * pitch + 6, width: gutter)
    }
    var previous: (String, CGFloat)?
    for (i, label) in columnLabels.enumerated().dropFirst(skip) {
      let lx = CGFloat(i - skip) * pitch
      if let previous, previous.0 == label || lx - previous.1 < 36 { continue }
      previous = (label, lx)
      axis(label, x: x + gutter + lx, y: y + top / 2)
    }
    return top + CGFloat(values.count) * pitch
  }

  private func progress(_ node: [String: Any], x: CGFloat, y: CGFloat, width: CGFloat) -> CGFloat {
    let c = Self.cell
    let ratio = min(max((node["value"] as? CGFloat ?? 0) / max(node["max"] as? CGFloat ?? 1, 1e-9), 0), 1)
    let tint = color(node["color"])
    let head = text(node["label"] as? String ?? "", size: 13, color: content, x: x, y: y, width: width - 50, lines: 1)
    text("\(Int((ratio * 100).rounded()))%", size: 12, color: content.withAlphaComponent(0.55), mono: true, x: x + width - 50, y: y, width: 50, lines: 1, align: .right)
    var cursor = y + head.height + 8
    let cols = Int(width / c)
    let filled = Int((CGFloat(cols) * ratio).rounded())
    let track = tint.withAlphaComponent(0.14)
    cells(x: x, y: cursor, cols: cols, rows: 3) { col, row in
      if col >= filled { return track }
      let tone = 0.3 + 0.7 * pow(CGFloat(col) / CGFloat(max(filled, 1)), 1.5)
      return col >= filled - 3 || Self.inked(col, row, tone) ? tint : track
    }
    cursor += 3 * c
    if let detail = node["detail"] as? String {
      cursor += 8 + text(detail, size: 12, color: content.withAlphaComponent(0.55), x: x, y: cursor + 8, width: width).height
    }
    return cursor - y
  }

  private func canvas(_ node: [String: Any], x: CGFloat, y: CGFloat, width: CGFloat) -> CGFloat {
    let w = max(1, node["width"] as? CGFloat ?? 1)
    let h = node["height"] as? CGFloat ?? 1
    let k = width / w
    for shape in node["shapes"] as? [[String: Any]] ?? [] {
      let tint = color(shape["color"])
      let n = { (key: String) in (shape[key] as? CGFloat ?? 0) * k }
      switch shape["kind"] as? String {
      case "rect": rect(CGRect(x: x + n("x"), y: y + n("y"), width: n("width"), height: n("height")), tint)
      case "circle":
        let r = n("radius")
        paints.append(TranscriptVisualPaint(path: CGPath(ellipseIn: CGRect(x: x + n("x") - r, y: y + n("y") - r, width: r * 2, height: r * 2), transform: nil), color: tint))
      case "line":
        let path = CGMutablePath()
        path.move(to: CGPoint(x: x + n("x1"), y: y + n("y1")))
        path.addLine(to: CGPoint(x: x + n("x2"), y: y + n("y2")))
        paints.append(TranscriptVisualPaint(path: path, color: tint, stroke: max(1, n("width"))))
      case "polyline":
        let points = (shape["points"] as? [[NSNumber]] ?? []).map {
          CGPoint(x: x + CGFloat(truncating: $0.first ?? 0) * k, y: y + CGFloat(truncating: $0.last ?? 0) * k)
        }
        guard let first = points.first else { continue }
        let path = CGMutablePath()
        path.move(to: first)
        for point in points.dropFirst() { path.addLine(to: point) }
        paints.append(TranscriptVisualPaint(path: path, color: tint, stroke: max(1, n("width"))))
      case "label":
        text(shape["text"] as? String ?? "", size: 12, color: tint, x: x + n("x"), y: y + n("y") - 12, width: width - n("x"), lines: 1)
      default: break
      }
    }
    return h * k
  }
}

/// The desktop charts' axis scale (visuals/dither.rs `Scale`).
struct TranscriptScale {
  let min: CGFloat
  let max: CGFloat
  let step: CGFloat

  init(_ values: [CGFloat], zero: Bool) {
    var low: CGFloat = zero ? 0 : .greatestFiniteMagnitude
    var high: CGFloat = zero ? 0 : -.greatestFiniteMagnitude
    for v in values where v.isFinite {
      low = Swift.min(low, v)
      high = Swift.max(high, v)
    }
    if low > high { (low, high) = (0, 1) }
    let span = high - low > 0 ? high - low : abs(high) > 0 ? abs(high) : 1
    let raw = span / 4
    let magnitude = pow(10, floor(log10(raw)))
    let step = ([1, 2, 2.5, 5, 10].first { $0 * magnitude >= raw } ?? 10) * magnitude
    min = floor(low / step) * step
    let top = ceil(high / step) * step
    max = top <= min ? min + step : top
    self.step = step
  }

  var ticks: [CGFloat] {
    let count = Swift.min(12, Int(((max - min) / step).rounded()))
    return (0...count).map { min + step * CGFloat($0) }
  }
  /// Where `value` falls, 0 at `max` and 1 at `min`.
  func depth(_ value: CGFloat) -> CGFloat { (max - value) / (max - min) }
  func label(_ value: CGFloat) -> String { Self.compact(value, step: step) }

  /// 1.2K, 48K, 3.4M; plain numbers keep `step`'s decimals.
  static func compact(_ value: CGFloat, step: CGFloat) -> String {
    func trim(_ s: String) -> String {
      guard s.contains(".") else { return s }
      var t = s
      while t.hasSuffix("0") { t.removeLast() }
      if t.hasSuffix(".") { t.removeLast() }
      return t
    }
    let a = abs(value)
    if a >= 1e9 { return trim(String(format: "%.1f", value / 1e9)) + "B" }
    if a >= 1e6 { return trim(String(format: "%.1f", value / 1e6)) + "M" }
    if a >= 1e4 { return trim(String(format: "%.1f", value / 1e3)) + "K" }
    let decimals =
      (0..<4).first { d in
        let scaled = abs(step) * pow(10, CGFloat(d))
        return abs(scaled - scaled.rounded()) < 1e-6
      } ?? 4
    return trim(String(format: "%.\(decimals)f", value))
  }
}

/// A smooth curve that never overshoots its points (monotone cubic,
/// Fritsch–Carlson), as the desktop's `Curve`.
struct TranscriptCurve {
  private let xs: [CGFloat]
  private let ys: [CGFloat]
  private let ms: [CGFloat]
  var count: Int { xs.count }
  var firstX: CGFloat { xs.first ?? 0 }
  var lastX: CGFloat { xs.last ?? 0 }

  init(_ points: [CGPoint]) {
    // Local copies: the slopes are worked out before every property is set.
    let xs = points.map(\.x)
    let ys = points.map(\.y)
    self.xs = xs
    self.ys = ys
    let n = points.count
    var ms = [CGFloat](repeating: 0, count: n)
    if n >= 2 {
      // Typed out: inferring the closure alone times out the type checker.
      let d: [CGFloat] = (0..<n - 1).map { k -> CGFloat in
        let rise: CGFloat = ys[k + 1] - ys[k]
        let run: CGFloat = Swift.max(xs[k + 1] - xs[k], 1e-6)
        return rise / run
      }
      ms[0] = d[0]
      ms[n - 1] = d[n - 2]
      for k in 1..<n - 1 { ms[k] = d[k - 1] * d[k] <= 0 ? 0 : (d[k - 1] + d[k]) / 2 }
      for k in 0..<n - 1 {
        if d[k] == 0 {
          ms[k] = 0
          ms[k + 1] = 0
          continue
        }
        let a: CGFloat = ms[k] / d[k]
        let b: CGFloat = ms[k + 1] / d[k]
        let s: CGFloat = a * a + b * b
        if s > 9 {
          let t: CGFloat = 3 / s.squareRoot()
          ms[k] = t * a * d[k]
          ms[k + 1] = t * b * d[k]
        }
      }
    }
    self.ms = ms
  }

  /// The curve's y at `x`, clamped to its ends.
  func at(_ x: CGFloat) -> CGFloat {
    let n = xs.count
    if n == 0 { return 0 }
    if n == 1 || x <= xs[0] { return ys[0] }
    if x >= xs[n - 1] { return ys[n - 1] }
    var k = 0
    while k < n - 2 && xs[k + 1] <= x { k += 1 }
    let h = Swift.max(xs[k + 1] - xs[k], 1e-6)
    let t = (x - xs[k]) / h
    let t2 = t * t
    let t3 = t2 * t
    return (2 * t3 - 3 * t2 + 1) * ys[k] + (t3 - 2 * t2 + t) * h * ms[k]
      + (-2 * t3 + 3 * t2) * ys[k + 1] + (t3 - t2) * h * ms[k + 1]
  }

  /// Points along the curve every `step`, as a path from `origin`.
  func path(step: CGFloat, origin: CGPoint) -> CGMutablePath {
    let path = CGMutablePath()
    guard let first = xs.first else { return path }
    if xs.count < 2 {
      path.move(to: CGPoint(x: origin.x + first, y: origin.y + ys[0]))
      return path
    }
    let count = Swift.max(1, Int(((lastX - firstX) / step).rounded(.up)))
    for i in 0...count {
      let x = firstX + (lastX - firstX) * CGFloat(i) / CGFloat(count)
      let point = CGPoint(x: origin.x + x, y: origin.y + at(x))
      if i == 0 { path.move(to: point) } else { path.addLine(to: point) }
    }
    return path
  }
}
