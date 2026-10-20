import CoreText
import UIKit

struct TranscriptStyle: Equatable {
  var width: CGFloat = 0
  var scale: CGFloat = 1
  var pixelRatio: CGFloat = 3
  var content = "#ebebeb"
  var muted = "#8c8c8c"
  var faint = "#5c5c5c"
  var accent = "#4c9ef5"
  var fileRef = "#c4b5fd"
  var fileLinks = false
  var bubble = "#252525"
  var surface = "#1c1c1c"
  var surfaceRaised = "#262626"
  var stroke = "rgba(235,235,235,0.08)"
  var error = "#f87171"
  var rail = "#2e2e2e"
  var agent = ""
  var agentRects: [TranscriptPixel] = []
  /// CoreText rounds fixed line heights up to whole points, so a fractional
  /// Dynamic Type scale must be rounded the same way before measuring.
  func line(_ value: CGFloat) -> CGFloat { ceil(value * scale - 0.001) }
}

extension UIColor {
  static func transcriptHex(_ text: String) -> UIColor {
    if text.hasPrefix("#") {
      var hex = String(text.dropFirst())
      if hex.count == 3 { hex = hex.map { "\($0)\($0)" }.joined() }
      let value = UInt64(hex, radix: 16) ?? 0
      let alpha: CGFloat = hex.count == 8 ? CGFloat(value & 255) / 255 : 1
      let rgb = hex.count == 8 ? value >> 8 : value
      return UIColor(
        red: CGFloat((rgb >> 16) & 255) / 255,
        green: CGFloat((rgb >> 8) & 255) / 255, blue: CGFloat(rgb & 255) / 255, alpha: alpha)
    }
    let values = text.components(separatedBy: CharacterSet(charactersIn: "0123456789.-").inverted)
      .compactMap(Double.init).map { CGFloat($0) }
    guard values.count >= 3 else { return .clear }
    let alpha = values.count > 3 ? values[3] : 1
    if text.hasPrefix("hsl") {
      let h = values[0] / 360
      let s = values[1] / 100
      let l = values[2] / 100
      let b = l + s * min(l, 1 - l)
      return UIColor(hue: h, saturation: b == 0 ? 0 : 2 * (1 - l / b), brightness: b, alpha: alpha)
    }
    return UIColor(
      red: values[0] / 255, green: values[1] / 255, blue: values[2] / 255, alpha: alpha)
  }
}

struct TranscriptFragment {
  let text: NSAttributedString
  let rect: CGRect
  var maxLines = 0
}
struct TranscriptSurface {
  let rect: CGRect
  let color: UIColor
  let radius: CGFloat
  var border: UIColor? = nil
  var corners: CACornerMask = [
    .layerMinXMinYCorner, .layerMaxXMinYCorner, .layerMinXMaxYCorner, .layerMaxXMaxYCorner,
  ]
}
struct TranscriptMark {
  let rect: CGRect
  let image: UIImage?
  let tint: UIColor
}
struct TranscriptHit {
  let rect: CGRect
  let action: [String: Any]
  let label: String
}
struct TranscriptLiveLabel {
  let rect: CGRect
  let text: String
  let font: UIFont
  let color: UIColor
  let shimmer: Bool
  let duration: Double
}

enum TranscriptTypography {
  static func runs(
    _ runs: [[String: Any]], style: TranscriptStyle, size: CGFloat, line: CGFloat,
    color: UIColor, bold: Bool = false, mono: Bool = false
  ) -> NSAttributedString {
    let result = NSMutableAttributedString(string: "")
    let paragraph = NSMutableParagraphStyle()
    paragraph.minimumLineHeight = style.line(line)
    paragraph.maximumLineHeight = style.line(line)
    paragraph.lineBreakMode = mono ? .byClipping : .byWordWrapping
    for run in runs {
      let code = run["code"] as? Bool == true
      let weight: UIFont.Weight = bold || run["bold"] as? Bool == true ? .semibold : .regular
      let pointSize = (code ? 15 : size) * style.scale
      var font =
        mono || code
        ? UIFont(name: weight == .semibold ? "Menlo-Bold" : "Menlo", size: pointSize)!
        : UIFont.systemFont(ofSize: pointSize, weight: weight)
      if run["italic"] as? Bool == true,
        let descriptor = font.fontDescriptor.withSymbolicTraits(
          font.fontDescriptor.symbolicTraits.union(.traitItalic))
      {
        font = UIFont(descriptor: descriptor, size: pointSize)
      }
      var attributes: [NSAttributedString.Key: Any] = [
        .font: font, .foregroundColor: color, .paragraphStyle: paragraph,
      ]
      if run["strike"] as? Bool == true {
        attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
        attributes[.foregroundColor] = UIColor.transcriptHex(style.muted)
      }
      let value = run["text"] as? String ?? ""
      if let link = run["link"] as? String {
        attributes[.link] = link
        attributes[.foregroundColor] = UIColor.transcriptHex(style.accent)
      } else if code {
        if let name = TranscriptArtwork.inlineFileName(value) {
          if style.fileLinks {
            attributes[.link] =
              "monocode-file:" + value.trimmingCharacters(in: .whitespacesAndNewlines)
            attributes[.foregroundColor] = UIColor.transcriptHex(style.fileRef)
          }
          let icon = TranscriptInlineIcon(
            TranscriptArtwork.shared.image(TranscriptArtwork.shared.file(name)),
            size: 14 * style.scale)
          var attachment = attributes
          attachment[NSAttributedString.Key(kCTRunDelegateAttributeName as String)] =
            icon.delegate()
          attachment[.transcriptIcon] = icon
          result.append(NSAttributedString(string: "\u{200B}", attributes: attributes))
          result.append(NSAttributedString(string: "\u{FFFC}", attributes: attachment))
          result.append(NSAttributedString(string: "\u{2060}", attributes: attributes))
        } else {
          attributes[.backgroundColor] = UIColor.transcriptHex(style.content).withAlphaComponent(
            0.07)
        }
      }
      result.append(NSAttributedString(string: value, attributes: attributes))
    }
    return result
  }
  static func measure(_ text: NSAttributedString, width: CGFloat, line: CGFloat) -> CGSize {
    let measured = CTFramesetterSuggestFrameSizeWithConstraints(
      CTFramesetterCreateWithAttributedString(text),
      CFRange(location: 0, length: text.length), nil,
      CGSize(width: max(1, width), height: .greatestFiniteMagnitude), nil)
    return CGSize(
      width: min(width, ceil(measured.width)),
      height: max(line, ceil((measured.height - 0.1) / line) * line))
  }
  static func clock(_ at: Double) -> String {
    let formatter = DateFormatter()
    formatter.setLocalizedDateFormatFromTemplate("jmm")
    return formatter.string(from: Date(timeIntervalSince1970: at / 1000))
  }
  static func working(_ action: [String: Any], now: Double = Date().timeIntervalSince1970 * 1000)
    -> String
  {
    if let label = action["label"] as? String { return label }
    let live = action["live"] as? Bool == true
    if live, let waiting = action["waiting"] as? String { return waiting }
    let who = (action["model"] as? String ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
    let verb = live ? (who.isEmpty ? "Working" : "working") : (who.isEmpty ? "Worked" : "worked")
    let prefix = who.isEmpty ? verb : "\(who) \(verb)"
    let elapsed =
      live
      ? (action["startedAt"] as? Double).map { max(0, now - $0) } : action["durationMs"] as? Double
    guard let elapsed else { return prefix + (live ? "…" : "") }
    let seconds = max(1, Int((elapsed / 1000).rounded()))
    let duration =
      seconds < 60
      ? "\(seconds)s" : "\(seconds / 60)m" + (seconds % 60 > 0 ? " \(seconds % 60)s" : "")
    return prefix + " for " + duration
  }
}

/// Immutable once constructed on the worker. Values mirror TranscriptTurn and
/// Markdown; scrolling only positions already measured rows.
final class TranscriptRowModel {
  let id: String
  let kind: String
  let action: [String: Any]
  let presentation: [String: Any]
  let style: TranscriptStyle
  private(set) var text = NSAttributedString(string: "")
  private(set) var fragments: [TranscriptFragment] = []
  private(set) var surfaces: [TranscriptSurface] = []
  private(set) var marks: [TranscriptMark] = []
  private(set) var hits: [TranscriptHit] = []
  private(set) var liveLabel: TranscriptLiveLabel?
  private(set) var mascotRect: CGRect?
  private(set) var userExpandable = false
  private(set) var userExpanded = false
  private(set) var height: CGFloat = 0
  private(set) var cardRect = CGRect.zero
  private(set) var bubbleRadius: CGFloat = 0
  private(set) var tailRadius: CGFloat = 0
  private(set) var horizontalRect = CGRect.zero
  private(set) var horizontalWidth: CGFloat = 0
  var group: String? { presentation["group"] as? String }
  var isFirst: Bool { presentation["first"] as? Bool ?? true }
  var isLast: Bool { presentation["last"] as? Bool ?? true }
  var folded: Bool { presentation["foldRail"] as? Bool == true }
  var contentColor: UIColor { UIColor.transcriptHex(style.content) }
  var textRect: CGRect { fragments.first?.rect ?? .zero }

  private func string(
    _ value: String, size: CGFloat, line: CGFloat, color: UIColor, bold: Bool = false,
    medium: Bool = false
  ) -> NSAttributedString {
    let result = NSMutableAttributedString(
      attributedString: TranscriptTypography.runs(
        [["text": value]], style: style, size: size, line: line, color: color, bold: bold))
    if medium {
      result.addAttribute(
        .font, value: UIFont.systemFont(ofSize: size * style.scale, weight: .medium),
        range: NSRange(location: 0, length: result.length))
    }
    return result
  }
  private func add(
    _ value: NSAttributedString, x: CGFloat, y: CGFloat, width: CGFloat, height: CGFloat,
    lines: Int = 0
  ) {
    fragments.append(
      TranscriptFragment(
        text: value, rect: CGRect(x: x, y: y, width: max(1, width), height: height), maxLines: lines
      ))
  }
  private func mark(_ name: String, x: CGFloat, y: CGFloat, size: CGFloat, alpha: CGFloat = 0.45) {
    marks.append(
      TranscriptMark(
        rect: CGRect(x: x, y: y, width: size, height: size),
        image: TranscriptArtwork.shared.image(name), tint: contentColor.withAlphaComponent(alpha)))
  }

  init(_ row: [String: Any], style: TranscriptStyle) {
    id = row["id"] as? String ?? ""
    kind = row["kind"] as? String ?? "text"
    action = row["action"] as? [String: Any] ?? [:]
    presentation = row["presentation"] as? [String: Any] ?? [:]
    self.style = style
    let s = style.scale
    let runs = row["runs"] as? [[String: Any]] ?? []
    let plain = runs.compactMap { $0["text"] as? String }.joined()
    let top = presentation["top"] as? CGFloat ?? 0
    let bottom = presentation["bottom"] as? CGFloat ?? 0
    let base: CGFloat = folded ? 42 : 20
    let available = max(1, style.width - (folded ? 42 : 20) - 20)
    let muted = presentation["muted"] as? Bool == true
    let hairline: CGFloat = 1 / style.pixelRatio
    switch kind {
    case "user":
      let compact = action["compact"] as? Bool == true
      let y: CGFloat = compact ? 4 : 6
      let emoji = !style.agent.isEmpty && action["emojiOnly"] as? Bool == true
      let maxWidth = max(1, style.width - 76 - (emoji ? 0 : 28))
      let body = string(
        emoji ? plain.trimmingCharacters(in: .whitespacesAndNewlines) : plain,
        size: emoji ? 56 : 16, line: emoji ? 64 : 23, color: contentColor)
      let line = style.line(emoji ? 64 : 23)
      let measured = TranscriptTypography.measure(body, width: maxWidth, line: line)
      userExpandable = !emoji && measured.height > 4 * line + 0.5
      userExpanded = action["nativeExpanded"] as? Bool == true
      let shownHeight = userExpandable && !userExpanded ? 4 * line : measured.height
      let attachments = action["attachments"] as? Int ?? 0
      let label = string(
        "\(attachments) attachment\(attachments == 1 ? "" : "s")", size: 13, line: 16,
        color: UIColor.transcriptHex(style.muted))
      let labelSize = TranscriptTypography.measure(label, width: maxWidth, line: style.line(16))
      let more = string(
        userExpanded ? "Show less" : "Show more", size: 13, line: 16,
        color: contentColor.withAlphaComponent(0.6))
      let moreSize = TranscriptTypography.measure(more, width: maxWidth, line: style.line(16))
      let textWidth = max(
        measured.width, attachments > 0 ? labelSize.width : 0, userExpandable ? moreSize.width : 0)
      let inset: CGFloat = emoji ? 0 : 14
      let x = style.width - 20 - textWidth - (emoji ? 0 : 14)
      var cursor = y + (emoji ? 0 : 9)
      if attachments > 0 && !emoji {
        add(label, x: x, y: cursor, width: textWidth, height: labelSize.height)
        cursor += labelSize.height + 4
      }
      if !plain.isEmpty {
        add(
          body, x: x, y: cursor, width: max(1, textWidth), height: shownHeight,
          lines: userExpandable && !userExpanded ? 4 : 0)
        cursor += shownHeight
      }
      if userExpandable {
        cursor += 4
        let rect = CGRect(x: x, y: cursor, width: textWidth, height: moreSize.height)
        add(more, x: x, y: cursor, width: textWidth, height: moreSize.height)
        hits.append(
          TranscriptHit(
            rect: rect.insetBy(dx: -8, dy: -4),
            action: ["type": "toggle", "id": id, "open": userExpanded],
            label: userExpanded ? "Show less" : "Show more"))
        cursor += moreSize.height
      }
      cursor += emoji ? 0 : 9
      cardRect = CGRect(x: x - inset, y: y, width: textWidth + inset * 2, height: cursor - y)
      bubbleRadius = measured.height <= line + 0.5 && attachments == 0 ? 21 : 14
      if !emoji {
        surfaces.append(
          TranscriptSurface(
            rect: cardRect, color: UIColor.transcriptHex(style.bubble), radius: bubbleRadius))
        if !style.agent.isEmpty && !compact { tailRadius = bubbleRadius == 21 ? 18 : 14 }
      }
      if let reaction = action["reaction"] as? String {
        let value = string(reaction, size: 11, line: 14, color: contentColor)
        let measured = TranscriptTypography.measure(value, width: maxWidth, line: style.line(14))
        let badge = CGRect(
          x: cardRect.minX - 10, y: cardRect.minY - 10, width: measured.width + 14,
          height: max(22, measured.height + 8))
        surfaces.append(
          TranscriptSurface(
            rect: badge, color: UIColor.transcriptHex(style.surfaceRaised), radius: 8,
            border: UIColor.transcriptHex(style.stroke)))
        add(
          value, x: badge.minX + 7, y: badge.midY - measured.height / 2, width: measured.width,
          height: measured.height)
      }
      if !compact && !emoji, let at = action["at"] as? Double {
        let time = string(
          TranscriptTypography.clock(at), size: 12, line: 15,
          color: contentColor.withAlphaComponent(0.4))
        let size = TranscriptTypography.measure(time, width: available, line: style.line(15))
        add(
          time, x: style.width - 32 - size.width, y: cursor + 4, width: size.width,
          height: size.height)
        cursor += 4 + size.height
      }
      height = cursor + (compact ? 0 : 16)
    case "stamp":
      let at = row["action"] as? Double ?? 0
      let date = Date(timeIntervalSince1970: (row["action"] as? Double ?? 0) / 1000)
      let formatter = DateFormatter()
      formatter.setLocalizedDateFormatFromTemplate(
        Calendar.current.component(.year, from: date)
          == Calendar.current.component(.year, from: Date()) ? "EEE MMM d" : "EEE MMM d yyyy")
      let day =
        Calendar.current.isDateInToday(date)
        ? "Today"
        : Calendar.current.isDateInYesterday(date) ? "Yesterday" : formatter.string(from: date)
      let value = NSMutableAttributedString(
        attributedString: string(
          day, size: 12, line: 16, color: contentColor.withAlphaComponent(0.75), bold: true))
      value.append(
        string(
          " " + TranscriptTypography.clock(at), size: 12, line: 16,
          color: contentColor.withAlphaComponent(0.45)))
      let size = TranscriptTypography.measure(value, width: style.width - 64, line: style.line(16))
      let rect = CGRect(
        x: (style.width - size.width - 24) / 2, y: 24, width: size.width + 24,
        height: size.height + 8)
      surfaces.append(
        TranscriptSurface(
          rect: rect, color: contentColor.withAlphaComponent(0.07), radius: rect.height / 2))
      add(value, x: rect.minX + 12, y: 28, width: size.width, height: size.height)
      height = rect.maxY + 12
    case "fold":
      let live = action["live"] as? Bool == true
      if !style.agent.isEmpty {
        let name = live ? style.agent : action["label"] as? String ?? style.agent
        let value = string(
          name, size: 14, line: 20, color: contentColor.withAlphaComponent(0.8), medium: true)
        let size = TranscriptTypography.measure(
          value, width: max(1, (live ? available * 0.45 : available) - 40), line: style.line(20))
        let rect = CGRect(x: 20, y: 4, width: size.width + 40, height: size.height + 4)
        surfaces.append(
          TranscriptSurface(
            rect: rect, color: contentColor.withAlphaComponent(0.07), radius: rect.height / 2))
        mascotRect = CGRect(x: 28, y: rect.midY - 7, width: 14, height: 14)
        add(value, x: 48, y: 6, width: size.width, height: size.height, lines: 1)
        if live {
          add(
            string("·", size: 14, line: 20, color: contentColor.withAlphaComponent(0.25)),
            x: rect.maxX + 6, y: 6, width: 5, height: style.line(20))
          liveLabel = TranscriptLiveLabel(
            rect: CGRect(
              x: rect.maxX + 17, y: 6, width: max(1, style.width - rect.maxX - 37), height: style.line(20)),
            text: action["label"] as? String ?? "Thinking…", font: .systemFont(ofSize: 14 * s),
            color: contentColor.withAlphaComponent(0.5), shimmer: true, duration: 1.6)
        }
        height = rect.maxY + 4
      } else {
        var x: CGFloat = 20
        if action["open"] as? Bool == true {
          mark("icon:chevronDown", x: x, y: 4 + (style.line(24) - 16) / 2, size: 16)
          x += 22
        } else if let harness = action["harness"] as? String {
          mark("provider:" + harness, x: x, y: 4 + (style.line(24) - 16) / 2, size: 16)
          x += 22
        }
        liveLabel = TranscriptLiveLabel(
          rect: CGRect(x: x, y: 4, width: max(1, style.width - x - 20), height: style.line(24)),
          text: TranscriptTypography.working(action), font: .systemFont(ofSize: 16 * s),
          color: contentColor.withAlphaComponent(0.5), shimmer: live, duration: 1)
        height = 8 + style.line(24)
        if action["expandable"] as? Bool == true {
          hits.append(
            TranscriptHit(
              rect: CGRect(x: 20, y: 0, width: available, height: height), action: action,
              label: action["open"] as? Bool == true ? "Hide the work" : "Show the work"))
        }
      }
    case "phase", "step", "errorStep":
      let step = kind != "phase"
      let icons = [
        "edit": "edit", "research": "search", "run": "terminal", "agent": "robot",
        "other": "wrench", "note": "minus",
      ]
      let icon = icons[action["kind"] as? String ?? ""]
      let x = base + (step || icons[action["kind"] as? String ?? ""] != nil ? 22 : 0)
      if !step, let icon { mark("icon:" + icon, x: base, y: top + 4 + (style.line(24) - 16) / 2, size: 16) }
      let color =
        kind == "errorStep"
        ? UIColor.transcriptHex(style.error).withAlphaComponent(0.8)
        : contentColor.withAlphaComponent(step ? 0.45 : 0.5)
      let value = string(plain, size: 16, line: 24, color: color)
      let size = TranscriptTypography.measure(value, width: style.width - x - 20, line: style.line(24))
      let textHeight = min(size.height, style.line(24) * (step ? 2 : 1))
      if !step && action["live"] as? Bool == true {
        liveLabel = TranscriptLiveLabel(
          rect: CGRect(x: x, y: top + 4, width: style.width - x - 20, height: textHeight),
          text: plain, font: .systemFont(ofSize: 16 * s), color: color, shimmer: true, duration: 1.6
        )
      } else {
        add(
          value, x: x, y: top + 4, width: style.width - x - 20, height: textHeight,
          lines: step ? 2 : 1)
      }
      height = top + 8 + textHeight + bottom
      if !step && action["expandable"] as? Bool == true {
        hits.append(
          TranscriptHit(
            rect: CGRect(x: base, y: top, width: available, height: height - bottom),
            action: action, label: plain))
      }
    case "notice":
      let value = string(plain, size: 14, line: 20, color: contentColor.withAlphaComponent(0.5))
      let size = TranscriptTypography.measure(value, width: available, line: style.line(20))
      add(value, x: base, y: 8, width: available, height: size.height)
      height = size.height + 16
    case "error":
      let recovery = plain.range(of: "removed provider account", options: .caseInsensitive) != nil
      let title = recovery ? "Account unavailable" : "Couldn’t continue"
      let titleText = string(
        title, size: 14, line: 20, color: UIColor.transcriptHex(style.muted), medium: true)
      let hint = string(
        "View details", size: 12, line: 17, color: UIColor.transcriptHex(style.faint))
      let textWidth = max(1, style.width - 126)
      let titleSize = TranscriptTypography.measure(
        titleText, width: max(1, style.width - 126), line: style.line(20))
      let bodyHeight = titleSize.height + style.line(17)
      let h = max(64, titleSize.height + style.line(17) + 28)
      let rect = CGRect(x: 20, y: 8, width: style.width - 40, height: h)
      surfaces.append(
        TranscriptSurface(
          rect: rect, color: contentColor.withAlphaComponent(0.04), radius: 16,
          border: UIColor.transcriptHex(style.stroke)))
      mark("icon:" + (recovery ? "lock" : "chat"), x: 34, y: rect.midY - 9, size: 18)
      add(
        titleText, x: 64, y: rect.midY - bodyHeight / 2, width: textWidth, height: titleSize.height)
      add(
        hint, x: 64, y: rect.midY - bodyHeight / 2 + titleSize.height, width: textWidth,
        height: style.line(17))
      marks.append(
        TranscriptMark(
          rect: CGRect(x: rect.maxX - 30, y: rect.midY - 8, width: 16, height: 16),
          image: TranscriptArtwork.shared.image("icon:chevronRight"),
          tint: UIColor.transcriptHex(style.faint)))
      hits.append(
        TranscriptHit(
          rect: rect, action: ["type": "error", "notice": action], label: title + ". View details"))
      height = h + 16
    case "footer":
      let metrics = action["metrics"] as? [String: Any] ?? [:]
      let hasMetrics =
        metrics["cacheHitPercent"] is NSNumber
        || ["inputTokens", "outputTokens", "cacheReadTokens", "cacheWriteTokens"].contains {
          (metrics[$0] as? Double ?? 0) > 0
        }
      var x: CGFloat = 16
      for (name, enabled) in [
        ("copy", !(action["copyText"] as? String ?? "").isEmpty), ("metrics", hasMetrics),
      ] where enabled {
        mark("icon:" + name, x: x + 4, y: 8, size: 16, alpha: 0.4)
        var event = action
        event["type"] = name
        hits.append(
          TranscriptHit(
            rect: CGRect(x: x, y: 4, width: 24, height: 24), action: event,
            label: name == "copy" ? "Copy response" : "Token metrics"))
        x += 28
      }
      if let at = action["completedAt"] as? Double {
        if x > 16 {
          let dot = CGRect(x: x + 6, y: 14.5, width: 3, height: 3)
          surfaces.append(
            TranscriptSurface(rect: dot, color: contentColor.withAlphaComponent(0.25), radius: 2))
          x = dot.maxX + 10
        }
        add(
          string(
            TranscriptTypography.clock(at), size: 14, line: 18,
            color: contentColor.withAlphaComponent(0.35)), x: x, y: 7, width: style.width - x - 20,
          height: style.line(18))
      }
      height = 4 + max(24, style.line(18)) + 12
    default:
      let listDepth = presentation["listDepth"] as? Int ?? 0
      let quoteDepth = presentation["quoteDepth"] as? Int ?? 0
      let marker = (presentation["marker"] as? String).map {
        string($0, size: 16, line: 24, color: UIColor.transcriptHex(style.muted))
      }
      let markerWidth =
        marker.map {
          max(14, TranscriptTypography.measure($0, width: available, line: style.line(24)).width)
        } ?? 14
      let x = base + CGFloat(listDepth) * 22 + CGFloat(quoteDepth) * 14 + markerWidth - 14
      let width = max(1, style.width - x - 20)
      var y = top + (presentation["gap"] as? CGFloat ?? 0)
      let first = isFirst
      let last = isLast
      if kind == "heading" { y += 4 }
      if kind == "code" {
        let value = TranscriptTypography.runs(
          runs, style: style, size: 13, line: 19, color: contentColor.withAlphaComponent(0.85),
          mono: true)
        let measured = TranscriptTypography.measure(value, width: 1_000_000, line: style.line(19))
        let codeTop: CGFloat = first ? 12 : 0
        let codeBottom: CGFloat = last ? 12 : 0
        let h = codeTop + style.line(19) + codeBottom
        horizontalRect = CGRect(x: x, y: y, width: width, height: h)
        horizontalWidth = max(width, measured.width + 24)
        add(value, x: x + 12, y: y + codeTop, width: max(1, measured.width + 1), height: style.line(19))
        height = y + h + bottom
      } else if kind == "table" {
        let cells = row["cells"] as? [[[String: Any]]] ?? []
        let columns = max(
          1, presentation["columns"] as? Int ?? (row["cells"] as? [Any])?.count ?? 1)
        let cellWidth = max(140, floor((width - 2 * hairline) / CGFloat(columns)))
        let values = cells.map {
          TranscriptTypography.runs(
            $0, style: style, size: 14, line: 20, color: contentColor,
            bold: presentation["header"] as? Bool == true)
        }
        let sizes = values.map {
          TranscriptTypography.measure($0, width: cellWidth - 20, line: style.line(20))
        }
        let h = max(style.line(20), sizes.map(\.height).max() ?? 0) + 12 + hairline + (last ? hairline : 0)
        horizontalRect = CGRect(x: x, y: y, width: width, height: h)
        horizontalWidth = CGFloat(columns) * cellWidth + 2 * hairline
        for (index, value) in values.enumerated() {
          add(
            value, x: x + hairline + CGFloat(index) * cellWidth + 10,
            y: y + 6 + hairline, width: cellWidth - 20, height: sizes[index].height)
        }
        height = y + h + bottom
      } else if kind == "rule" {
        surfaces.append(
          TranscriptSurface(
            rect: CGRect(x: x, y: y, width: width, height: hairline),
            color: UIColor.transcriptHex(style.stroke), radius: 0))
        height = y + hairline + bottom
      } else {
        let heading = kind == "heading"
        let large = kind == "heading" && !muted && (row["level"] as? Int ?? 0) <= 2
        let size: CGFloat = heading && muted ? 14 : large ? 18 : 16
        let line: CGFloat = large ? 26 : 24
        let value = TranscriptTypography.runs(
          runs, style: style, size: size, line: line,
          color: contentColor.withAlphaComponent(muted ? (heading ? 0.8 : 0.65) : 1), bold: heading)
        let measured = TranscriptTypography.measure(value, width: width, line: style.line(line))
        if let marker {
          add(marker, x: x - markerWidth - 8, y: y, width: markerWidth, height: style.line(24))
        }
        add(value, x: x, y: y, width: width, height: measured.height)
        height = y + measured.height + bottom
      }
    }
    let result = NSMutableAttributedString(string: "")
    for fragment in fragments { result.append(fragment.text) }
    text = result
    height = ceil(height * style.pixelRatio) / style.pixelRatio
  }
}

struct TranscriptPreparedLine {
  let line: CTLine
  let origin: CGPoint
  let range: CFRange
}
struct TranscriptPreparedFragment {
  let fragment: TranscriptFragment
  let offset: Int
  let lines: [TranscriptPreparedLine]
}
final class TranscriptTextLayout {
  let model: TranscriptRowModel
  let parts: [TranscriptPreparedFragment]
  init(_ model: TranscriptRowModel) {
    self.model = model
    var parts: [TranscriptPreparedFragment] = []
    var offset = 0
    for fragment in model.fragments {
      let frame = CTFramesetterCreateFrame(
        CTFramesetterCreateWithAttributedString(fragment.text), CFRange(location: 0, length: 0),
        CGPath(rect: CGRect(origin: .zero, size: fragment.rect.size), transform: nil), nil)
      let lines = CTFrameGetLines(frame) as! [CTLine]
      var origins = [CGPoint](repeating: .zero, count: lines.count)
      CTFrameGetLineOrigins(frame, CFRange(location: 0, length: 0), &origins)
      let limit = fragment.maxLines > 0 ? min(lines.count, fragment.maxLines) : lines.count
      var prepared: [TranscriptPreparedLine] = []
      for index in 0..<limit {
        var line = lines[index]
        let range = CTLineGetStringRange(line)
        if fragment.maxLines > 0 && index + 1 == limit
          && range.location + range.length < fragment.text.length
        {
          let rest = fragment.text.attributedSubstring(
            from: NSRange(location: range.location, length: fragment.text.length - range.location))
          let token = NSAttributedString(
            string: "…",
            attributes: fragment.text.attributes(at: range.location, effectiveRange: nil))
          line =
            CTLineCreateTruncatedLine(
              CTLineCreateWithAttributedString(rest), Double(fragment.rect.width), .end,
              CTLineCreateWithAttributedString(token)) ?? line
        }
        prepared.append(TranscriptPreparedLine(line: line, origin: origins[index], range: range))
      }
      parts.append(TranscriptPreparedFragment(fragment: fragment, offset: offset, lines: prepared))
      offset += fragment.text.length
    }
    self.parts = parts
  }
}
