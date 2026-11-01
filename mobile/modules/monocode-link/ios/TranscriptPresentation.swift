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
  /// Text on the user's bubble, which a Mono's chat fills with the accent.
  var bubbleText = "#ebebeb"
  var surface = "#1c1c1c"
  var surfaceRaised = "#262626"
  var stroke = "rgba(235,235,235,0.08)"
  var error = "#f87171"
  var rail = "#2e2e2e"
  var agent = ""
  var agentRects: [TranscriptPixel] = []
  /// Reactions the user put on a Mono's replies, by the reply's first row.
  var reactions: [String: String] = [:]
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
  /// Liquid Glass where the system has it (iOS 26), else `color`.
  var glass = false
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
  /// A reaction's round bubble, and how far above its message's bubble it
  /// rises: just over half its height, so it overlaps the corner.
  static let reactionSize: CGFloat = 34
  static let reactionLift: CGFloat = 20
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
  /// A visual's dithered cells and lines, drawn by the row's canvas.
  private(set) var visualPaints: [TranscriptVisualPaint] = []
  private(set) var userExpandable = false
  private(set) var userExpanded = false
  private(set) var height: CGFloat = 0
  private(set) var cardRect = CGRect.zero
  private(set) var bubbleRadius: CGFloat = 0
  /// A Mono chat's time for the row, and the height it centres on, shown
  /// only while the transcript is slid aside, as in Messages.
  private(set) var time: (text: String, y: CGFloat)?
  /// Where a markdown row's text starts: inside its bubble in a Mono's chat.
  private(set) var contentBase: CGFloat = 20
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
    // A Mono's reply sits in a bubble, as a chat app's: one surface across
    // its rows, rounded at the reply's first and last, the text inset in it.
    // Its bubbles sit close, as a messaging app's.
    let bubbled = !style.agent.isEmpty && !folded && presentation["reply"] as? Bool == true
    let replyFirst = presentation["replyFirst"] as? Bool == true
    let replyLast = presentation["replyLast"] as? Bool == true
    // A reaction the user put on the reply sits over its top corner; the row
    // makes room for it.
    let reaction = bubbled && replyFirst ? style.reactions[id] : nil
    let top: CGFloat =
      bubbled && replyFirst
      ? 3 + (reaction == nil ? 0 : Self.reactionLift) : presentation["top"] as? CGFloat ?? 0
    let bottom: CGFloat = bubbled && replyLast ? 3 : presentation["bottom"] as? CGFloat ?? 0
    let base: CGFloat = folded ? 42 : 20
    let available = max(1, style.width - (folded ? 42 : 20) - 20)
    let muted = presentation["muted"] as? Bool == true
    let hairline: CGFloat = 1 / style.pixelRatio
    switch kind {
    case "user":
      let compact = action["compact"] as? Bool == true
      let reaction = action["reaction"] as? String
      // A reaction sits over the bubble's top corner; the row makes room for it.
      // A Mono's chat leaves as much room above a message as below it, unless
      // it stacks under another of the user's.
      let gap: CGFloat =
        style.agent.isEmpty
        ? (compact ? 4 : 6) : presentation["afterUser"] as? Bool == true ? 4 : 16
      // A reply to one of the Mono's messages opens with it quoted, shown as
      // an outlined pill over the bubble, one line.
      var message = plain
      var quoted: String?
      if !style.agent.isEmpty, message.hasPrefix("> "), let end = message.range(of: "\n\n") {
        quoted = String(message[message.index(message.startIndex, offsetBy: 2)..<end.lowerBound])
        message = String(message[end.upperBound...])
      }
      let quoteLine = style.line(20)
      let quoteHeight: CGFloat = quoted == nil ? 0 : quoteLine + 14 + 6
      let y = gap + quoteHeight + (reaction == nil ? 0 : Self.reactionLift)
      let emoji = !style.agent.isEmpty && action["emojiOnly"] as? Bool == true
      let maxWidth = max(1, style.width - 76 - (emoji ? 0 : 28))
      if let quoted {
        let value = string(
          quoted, size: 14, line: 20, color: contentColor.withAlphaComponent(0.55))
        let room = max(1, style.width - 76 - 32)
        let width = min(room, TranscriptTypography.measure(value, width: 1_000_000, line: quoteLine).width)
        let chip = CGRect(
          x: style.width - 20 - width - 32, y: gap, width: width + 32, height: quoteLine + 14)
        surfaces.append(
          TranscriptSurface(
            rect: chip, color: .clear, radius: chip.height / 2,
            border: contentColor.withAlphaComponent(0.18)))
        add(value, x: chip.minX + 16, y: chip.minY + 7, width: width, height: quoteLine, lines: 1)
      }
      let ink = emoji ? contentColor : UIColor.transcriptHex(style.bubbleText)
      let body = string(
        emoji ? message.trimmingCharacters(in: .whitespacesAndNewlines) : message,
        size: emoji ? 56 : 16, line: emoji ? 64 : 23, color: ink)
      let line = style.line(emoji ? 64 : 23)
      let measured = TranscriptTypography.measure(body, width: maxWidth, line: line)
      userExpandable = !emoji && measured.height > 4 * line + 0.5
      userExpanded = action["nativeExpanded"] as? Bool == true
      let shownHeight = userExpandable && !userExpanded ? 4 * line : measured.height
      let attachments = action["attachments"] as? Int ?? 0
      let label = string(
        "\(attachments) attachment\(attachments == 1 ? "" : "s")", size: 13, line: 16,
        color: ink.withAlphaComponent(0.65))
      let labelSize = TranscriptTypography.measure(label, width: maxWidth, line: style.line(16))
      let more = string(
        userExpanded ? "Show less" : "Show more", size: 13, line: 16,
        color: ink.withAlphaComponent(0.6))
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
      if !message.isEmpty {
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
      // A Mono's chat rounds both sides' bubbles alike: a capsule on one
      // line, else as its replies.
      let single = measured.height <= line + 0.5 && attachments == 0
      bubbleRadius =
        !style.agent.isEmpty ? (single ? cardRect.height / 2 : 20) : single ? 21 : 14
      if !emoji {
        surfaces.append(
          TranscriptSurface(
            rect: cardRect, color: UIColor.transcriptHex(style.bubble), radius: bubbleRadius))
      }
      if let reaction {
        // As a Messages tapback: the emoji in a round bubble over the top-left
        // corner, its tail two shrinking dots trailing off down and away.
        let value = string(reaction, size: 19, line: 24, color: contentColor)
        let measured = TranscriptTypography.measure(value, width: maxWidth, line: style.line(24))
        let size = Self.reactionSize
        let width = max(size, measured.width + 16)
        let badge = CGRect(
          x: cardRect.minX - size * 0.55, y: cardRect.minY - Self.reactionLift, width: width,
          height: size)
        let fill = UIColor.transcriptHex(style.surfaceRaised)
        let edge = UIColor.transcriptHex(style.stroke)
        let dot = CGRect(x: badge.minX + 2, y: badge.maxY - 5, width: 10, height: 10)
        let speck = CGRect(x: dot.minX - 5, y: dot.maxY + 1, width: 5, height: 5)
        for circle in [speck, dot] {
          surfaces.append(
            TranscriptSurface(rect: circle, color: fill, radius: circle.width / 2, border: edge))
        }
        surfaces.append(
          TranscriptSurface(rect: badge, color: fill, radius: size / 2, border: edge))
        add(
          value, x: badge.midX - measured.width / 2, y: badge.midY - measured.height / 2,
          width: measured.width, height: measured.height)
      }
      if !style.agent.isEmpty, let at = action["at"] as? Double {
        time = (TranscriptTypography.clock(at), cardRect.midY)
      }
      if style.agent.isEmpty && !compact && !emoji, let at = action["at"] as? Double {
        let time = string(
          TranscriptTypography.clock(at), size: 12, line: 15,
          color: contentColor.withAlphaComponent(0.4))
        let size = TranscriptTypography.measure(time, width: available, line: style.line(15))
        add(
          time, x: style.width - 32 - size.width, y: cursor + 4, width: size.width,
          height: size.height)
        cursor += 4 + size.height
      }
      // A Mono's chat leaves no room under a message another of the user's
      // stacks under.
      let stacked = !style.agent.isEmpty && presentation["beforeUser"] as? Bool == true
      height = cursor + (compact || stacked ? 0 : 16)
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
        // A Mono's replies carry no badge. While it works, its mascot and a
        // ticker of what it's doing trail the turn, at the messages' size.
        if live {
          let line = style.line(24)
          let mascot: CGFloat = 26
          let rowHeight = max(mascot, line)
          mascotRect = CGRect(x: 22, y: 8 + (rowHeight - mascot) / 2, width: mascot, height: mascot)
          liveLabel = TranscriptLiveLabel(
            rect: CGRect(
              x: 22 + mascot + 10, y: 8 + (rowHeight - line) / 2,
              width: max(1, style.width - 22 - mascot - 10 - 20), height: line),
            text: action["label"] as? String ?? "Thinking…", font: .systemFont(ofSize: 16 * s),
            color: contentColor.withAlphaComponent(0.5), shimmer: true, duration: 1.6)
          height = 8 + rowHeight + 8
        }
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
    case "divider":
      // A Mono's replies are bubbles of their own, so nothing breaks them.
      height = 0
    case "choices":
      // Suggested replies in one card, each behind a letter, as a quiz's
      // answers. Once a reply follows, the one sent stays and the rest step
      // back; only an unanswered card takes a tap.
      let options = action["options"] as? [String] ?? []
      let chosen = (action["chosen"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
      let answered = chosen != nil
      let pad: CGFloat = 16
      let badge: CGFloat = 28
      let gap: CGFloat = 12
      let rect = CGRect(x: base, y: top, width: max(1, style.width - base - 56), height: 0)
      let labelWidth = max(1, rect.width - pad * 2 - badge - gap)
      var cursor = top + pad - 2
      for (index, option) in options.enumerated() {
        let checked = option.trimmingCharacters(in: .whitespacesAndNewlines) == chosen
        let alpha: CGFloat = answered && !checked ? 0.4 : 1
        let label = string(option, size: 16, line: 24, color: contentColor.withAlphaComponent(alpha))
        let size = TranscriptTypography.measure(label, width: labelWidth, line: style.line(24))
        // The badge centres on the label's first line.
        let textY = cursor + max(0, (badge - style.line(24)) / 2)
        let mark = CGRect(
          x: rect.minX + pad, y: cursor + max(0, (style.line(24) - badge) / 2), width: badge,
          height: badge)
        let line = max(mark.maxY, textY + size.height) - cursor
        let letter = string(
          String(UnicodeScalar(UInt8(65 + index % 26))), size: 13, line: 16,
          color: checked ? UIColor.transcriptHex(style.bubbleText) : contentColor.withAlphaComponent(0.55 * alpha),
          medium: true)
        let letterSize = TranscriptTypography.measure(letter, width: badge, line: style.line(16))
        surfaces.append(
          TranscriptSurface(
            rect: mark, color: checked ? UIColor.transcriptHex(style.bubble) : .clear, radius: 8,
            border: checked ? nil : contentColor.withAlphaComponent(0.18 * alpha)))
        add(
          letter, x: mark.midX - letterSize.width / 2, y: mark.midY - letterSize.height / 2,
          width: letterSize.width, height: letterSize.height)
        add(label, x: mark.maxX + gap, y: textY, width: size.width, height: size.height)
        let row = CGRect(x: rect.minX, y: cursor - gap / 2, width: rect.width, height: line + gap)
        if !answered {
          hits.append(
            TranscriptHit(rect: row, action: ["type": "choice", "text": option], label: option))
        }
        cursor += line + (index + 1 < options.count ? gap : 0)
      }
      let card = CGRect(x: rect.minX, y: top, width: rect.width, height: cursor + pad - 2 - top)
      surfaces.insert(
        TranscriptSurface(
          rect: card, color: contentColor.withAlphaComponent(0.03), radius: 20,
          border: UIColor.transcriptHex(style.stroke)),
        at: 0)
      height = card.maxY + bottom
    case "visual":
      // A chart or dashboard the agent drew, in an outlined bubble, laid out
      // natively from the tree the desktop rendered.
      let pad: CGFloat = 16
      let width = max(1, style.width - base - 20)
      let inner = max(1, width - pad * 2)
      var cursor = top + pad
      let title = string(plain, size: 15, line: 20, color: contentColor, medium: true)
      add(title, x: base + pad, y: cursor, width: inner, height: style.line(20), lines: 1)
      cursor += style.line(20) + 12
      if let tree = action["tree"] as? [String: Any] {
        let layout = TranscriptVisualLayout(style: style)
        cursor += layout.layout(tree, x: base + pad, y: cursor, width: inner)
        for text in layout.texts {
          add(
            text.text, x: text.rect.minX, y: text.rect.minY, width: text.rect.width,
            height: text.rect.height, lines: text.lines)
        }
        visualPaints = layout.paints
      }
      let card = CGRect(x: base, y: top, width: width, height: cursor + pad - top)
      surfaces.append(
        TranscriptSurface(
          rect: card, color: .clear, radius: 20, border: contentColor.withAlphaComponent(0.16)))
      height = card.maxY + bottom
    case "document":
      // ArtifactCard: the document's mark and title; a tap opens the reader.
      let visual = ["html", "visual"].contains(action["kind"] as? String ?? "")
      // At the messages' size, outlined so it reads apart from the bubbles.
      let title = string(plain, size: 16, line: 24, color: contentColor)
      let titleWidth = min(
        max(1, style.width - base - 56 - 16 - 18 - 10 - 16),
        TranscriptTypography.measure(title, width: 1_000_000, line: style.line(24)).width)
      let rect = CGRect(
        x: base, y: top, width: 16 + 18 + 10 + titleWidth + 16, height: max(48, style.line(24) + 22))
      surfaces.append(
        TranscriptSurface(
          rect: rect, color: .clear, radius: 20,
          border: contentColor.withAlphaComponent(0.16)))
      mark(
        visual ? "icon:metrics" : "icon:edit", x: rect.minX + 16, y: rect.midY - 9, size: 18,
        alpha: 0.6)
      add(
        title, x: rect.minX + 44, y: rect.midY - style.line(24) / 2, width: titleWidth,
        height: style.line(24), lines: 1)
      hits.append(
        TranscriptHit(
          rect: rect, action: action, label: "Open \(visual ? "visual" : "document") " + plain))
      height = rect.maxY + bottom
    case "launch":
      // A session the Mono started, as the desktop's card under its reply:
      // how it stands, its title, and the project it runs in on a pill. A tap
      // opens it over the chat.
      let state = action["state"] as? String ?? "done"
      let icons: [String: (String, UIColor)] = [
        "working": ("terminal", contentColor.withAlphaComponent(0.55)),
        "input": ("chat", UIColor.transcriptHex("#fbbf24")),
        "draft": ("edit", contentColor.withAlphaComponent(0.45)),
        "failed": ("minus", UIColor.transcriptHex(style.error)),
        "stopped": ("minus", contentColor.withAlphaComponent(0.45)),
        "done": ("check", UIColor.transcriptHex("#34d399").withAlphaComponent(0.9)),
      ]
      let (icon, tint) = icons[state] ?? icons["done"]!
      // At the messages' size, outlined so it reads apart from the bubbles.
      let pad: CGFloat = 16
      let iconSize: CGFloat = 18
      let pillPad: CGFloat = 9
      let providerSize: CGFloat = 15
      let room = style.width - base - 56
      let project = string(
        action["project"] as? String ?? "", size: 13, line: 18,
        color: contentColor.withAlphaComponent(0.65))
      let projectSize = TranscriptTypography.measure(
        project, width: room * 0.45 - pillPad * 2 - providerSize - 4, line: style.line(18))
      let pillWidth = pillPad * 2 + providerSize + 4 + projectSize.width
      let title = string(plain, size: 16, line: 24, color: contentColor)
      let titleRoom = max(1, room - pad - iconSize - 10 - 10 - pillWidth - 8)
      let titleWidth = min(
        titleRoom,
        TranscriptTypography.measure(title, width: 1_000_000, line: style.line(24)).width)
      let h = max(48, style.line(24) + 22)
      let rect = CGRect(
        x: base, y: top, width: pad + iconSize + 10 + titleWidth + 10 + pillWidth + 8, height: h)
      surfaces.append(
        TranscriptSurface(
          rect: rect, color: .clear, radius: 20,
          border: contentColor.withAlphaComponent(0.16)))
      marks.append(
        TranscriptMark(
          rect: CGRect(
            x: rect.minX + pad, y: rect.midY - iconSize / 2, width: iconSize, height: iconSize),
          image: TranscriptArtwork.shared.image("icon:" + icon), tint: tint))
      let titleRect = CGRect(
        x: rect.minX + pad + iconSize + 10, y: rect.midY - style.line(24) / 2, width: titleWidth,
        height: style.line(24))
      if state == "working" {
        liveLabel = TranscriptLiveLabel(
          rect: titleRect, text: plain, font: .systemFont(ofSize: 16 * s), color: contentColor,
          shimmer: true, duration: 1.6)
      } else {
        add(
          title, x: titleRect.minX, y: titleRect.minY, width: titleRect.width,
          height: titleRect.height, lines: 1)
      }
      let pillHeight = max(26, style.line(18) + 8)
      let pill = CGRect(
        x: rect.maxX - 8 - pillWidth, y: rect.midY - pillHeight / 2, width: pillWidth,
        height: pillHeight)
      surfaces.append(
        TranscriptSurface(
          rect: pill, color: contentColor.withAlphaComponent(0.05), radius: pillHeight / 2,
          border: UIColor.transcriptHex(style.stroke)))
      mark(
        "provider:" + (action["harness"] as? String ?? ""), x: pill.minX + pillPad,
        y: pill.midY - providerSize / 2, size: providerSize, alpha: 0.65)
      add(
        project, x: pill.minX + pillPad + providerSize + 4, y: pill.midY - projectSize.height / 2,
        width: projectSize.width, height: projectSize.height, lines: 1)
      hits.append(TranscriptHit(rect: rect, action: action, label: "Open session " + plain))
      height = rect.maxY + bottom + 4
    case "footer":
      // A Mono's chat has no copy button or time under its replies.
      if !style.agent.isEmpty { break }
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
      let bubbleWidth = max(1, style.width - 20 - 56)
      let pad: (x: CGFloat, y: CGFloat) = (16, 10)
      contentBase = bubbled ? base + pad.x : base
      let x = contentBase + CGFloat(listDepth) * 22 + CGFloat(quoteDepth) * 14 + markerWidth - 14
      let width = max(1, style.width - x - (bubbled ? 56 + pad.x : 20))
      var y = top + (presentation["gap"] as? CGFloat ?? 0) + (bubbled && replyFirst ? pad.y : 0)
      var hug: CGFloat?
      var capsule = false
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
        // A one-line-of-thought reply hugs its text; on one line it's a capsule.
        if replyFirst && replyLast && marker == nil && quoteDepth == 0 {
          hug = measured.width + pad.x * 2
          capsule = measured.height <= style.line(line) + 0.5
        }
      }
      if bubbled {
        if replyLast { height += pad.y }
        let from: CGFloat = replyFirst ? top : 0
        let to = height - (replyLast ? bottom : 0)
        var corners: CACornerMask = []
        if replyFirst { corners.formUnion([.layerMinXMinYCorner, .layerMaxXMinYCorner]) }
        if replyLast { corners.formUnion([.layerMinXMaxYCorner, .layerMaxXMaxYCorner]) }
        surfaces.insert(
          TranscriptSurface(
            rect: CGRect(x: base, y: from, width: min(bubbleWidth, hug ?? bubbleWidth), height: to - from),
            color: UIColor.transcriptHex(style.surfaceRaised),
            radius: capsule ? (to - from) / 2 : 20, corners: corners),
          at: 0)
        if replyLast, let at = presentation["at"] as? Double {
          time = (TranscriptTypography.clock(at), max(from, to - pad.y - style.line(24) / 2))
        }
        if let reaction {
          // As on the user's messages, mirrored: over the top-right corner,
          // its two dots trailing off down and to the right.
          let bubble = CGRect(
            x: base, y: from, width: min(bubbleWidth, hug ?? bubbleWidth), height: to - from)
          let value = string(reaction, size: 19, line: 24, color: contentColor)
          let measured = TranscriptTypography.measure(value, width: bubbleWidth, line: style.line(24))
          let size = Self.reactionSize
          let width = max(size, measured.width + 16)
          let badge = CGRect(
            x: bubble.maxX - width + size * 0.55, y: bubble.minY - Self.reactionLift, width: width,
            height: size)
          let fill = UIColor.transcriptHex(style.surfaceRaised)
          let edge = UIColor.transcriptHex(style.stroke)
          let dot = CGRect(x: badge.maxX - 12, y: badge.maxY - 5, width: 10, height: 10)
          let speck = CGRect(x: dot.maxX, y: dot.maxY + 1, width: 5, height: 5)
          for circle in [speck, dot] {
            surfaces.append(
              TranscriptSurface(rect: circle, color: fill, radius: circle.width / 2, border: edge))
          }
          surfaces.append(TranscriptSurface(rect: badge, color: fill, radius: size / 2, border: edge))
          add(
            value, x: badge.midX - measured.width / 2, y: badge.midY - measured.height / 2,
            width: measured.width, height: measured.height)
        }
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
