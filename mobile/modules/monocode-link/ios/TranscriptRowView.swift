import CoreText
import UIKit

/// Viewport-sized canvases keep a very long code line from allocating a wide
/// bitmap. CoreText lines and inline attachment positions are prepared off-main.
private final class TranscriptTextCanvas: UIView {
  var layout: TranscriptTextLayout?
  var origin = CGPoint.zero
  var range = NSRange(location: 0, length: 0)
  override func draw(_ rect: CGRect) {
    guard let layout, range.length > 0, let context = UIGraphicsGetCurrentContext() else { return }
    for part in layout.parts {
      let fragment = part.fragment
      let localRange = NSIntersectionRange(
        range, NSRange(location: part.offset, length: fragment.text.length))
      guard localRange.length > 0 else { continue }
      context.saveGState()
      context.translateBy(x: fragment.rect.minX - origin.x, y: fragment.rect.maxY - origin.y)
      context.scaleBy(x: 1, y: -1)
      context.textMatrix = .identity
      for prepared in part.lines {
        let line = prepared.line
        let lineRange = prepared.range
        let start = max(localRange.location - part.offset, lineRange.location)
        let finish = min(
          NSMaxRange(localRange) - part.offset, lineRange.location + lineRange.length)
        guard finish > start else { continue }
        context.saveGState()
        if start > lineRange.location || finish < lineRange.location + lineRange.length {
          var ascent: CGFloat = 0
          var descent: CGFloat = 0
          CTLineGetTypographicBounds(line, &ascent, &descent, nil)
          let x1 = CTLineGetOffsetForStringIndex(line, start, nil)
          let x2 = CTLineGetOffsetForStringIndex(line, finish, nil)
          context.clip(
            to: CGRect(
              x: prepared.origin.x + min(x1, x2), y: prepared.origin.y - descent - 2,
              width: max(1, abs(x2 - x1)), height: ascent + descent + 4))
        }
        let runs = CTLineGetGlyphRuns(line) as! [CTRun]
        for run in runs {
          let attributes = CTRunGetAttributes(run) as! [NSAttributedString.Key: Any]
          var position = CGPoint.zero
          var ascent: CGFloat = 0
          var descent: CGFloat = 0
          guard CTRunGetGlyphCount(run) > 0 else { continue }
          CTRunGetPositions(run, CFRange(location: 0, length: 1), &position)
          let width = CGFloat(
            CTRunGetTypographicBounds(run, CFRange(location: 0, length: 0), &ascent, &descent, nil))
          if let color = attributes[.backgroundColor] as? UIColor {
            context.setFillColor(color.cgColor)
            context.fill(
              CGRect(
                x: prepared.origin.x + position.x, y: prepared.origin.y - descent, width: width,
                height: ascent + descent))
          }
        }
        context.textPosition = prepared.origin
        CTLineDraw(line, context)
        for run in runs {
          guard CTRunGetGlyphCount(run) > 0 else { continue }
          let attributes = CTRunGetAttributes(run) as! [NSAttributedString.Key: Any]
          var position = CGPoint.zero
          CTRunGetPositions(run, CFRange(location: 0, length: 1), &position)
          if let icon = attributes[.transcriptIcon] as? TranscriptInlineIcon,
            let image = icon.image?.cgImage
          {
            context.draw(
              image,
              in: CGRect(
                x: prepared.origin.x + position.x, y: prepared.origin.y - 2, width: icon.size,
                height: icon.size))
          }
          if attributes[.strikethroughStyle] != nil {
            let width = CGFloat(
              CTRunGetTypographicBounds(run, CFRange(location: 0, length: 0), nil, nil, nil))
            let font = attributes[.font] as? UIFont
            context.setFillColor((attributes[.foregroundColor] as? UIColor ?? .label).cgColor)
            context.fill(
              CGRect(
                x: prepared.origin.x + position.x, y: prepared.origin.y + (font?.xHeight ?? 8) / 2,
                width: width, height: 0.75))
          }
        }
        context.restoreGState()
      }
      context.restoreGState()
    }
  }
}

private final class TranscriptDecorationCanvas: UIView {
  var model: TranscriptRowModel?
  var horizontalOffset: CGFloat = 0
  var horizontalWidth: CGFloat = 0
  var copied = false
  override func draw(_ rect: CGRect) {
    guard let model, let context = UIGraphicsGetCurrentContext() else { return }
    let p = model.presentation
    let rail = UIColor.transcriptHex(model.style.rail)
    if model.kind == "code" {
      var corners: UIRectCorner = []
      if model.isFirst { corners.formUnion([.topLeft, .topRight]) }
      if model.isLast { corners.formUnion([.bottomLeft, .bottomRight]) }
      model.contentColor.withAlphaComponent(0.05).setFill()
      UIBezierPath(
        roundedRect: model.horizontalRect, byRoundingCorners: corners,
        cornerRadii: CGSize(width: 10, height: 10)
      ).fill()
    }
    func spine(_ x: CGFloat, branch: Bool, tail: Bool, y: CGFloat = 0, end: CGFloat? = nil) {
      rail.setFill()
      context.fill(
        CGRect(
          x: x, y: y, width: 1, height: tail ? (branch ? 9 : 0) : max(0, (end ?? bounds.height) - y)
        ))
      if branch {
        let path = UIBezierPath()
        path.move(to: CGPoint(x: x + 0.5, y: y + 8))
        path.addQuadCurve(
          to: CGPoint(x: x + 8, y: y + 15.5), controlPoint: CGPoint(x: x + 0.5, y: y + 15.5))
        rail.setStroke()
        path.lineWidth = 1
        path.stroke()
      }
    }
    if model.folded {
      spine(28, branch: p["foldBranch"] as? Bool == true, tail: p["foldTail"] as? Bool == true)
    }
    if model.kind == "fold" && model.style.agent.isEmpty && model.action["open"] as? Bool == true {
      rail.setFill()
      context.fill(
        CGRect(
          x: 28, y: 4 + (model.style.line(24) + 16) / 2, width: 1,
          height: max(0, model.height - 4 - (model.style.line(24) + 16) / 2)))
    }
    if model.kind == "step" || model.kind == "errorStep" {
      spine(
        (model.folded ? 42 : 20) + 7, branch: true, tail: p["stepLast"] as? Bool == true,
        end: model.height - (p["bottom"] as? CGFloat ?? 0))
    }
    let quoteDepth = p["quoteDepth"] as? Int ?? 0
    if quoteDepth > 0 {
      model.contentColor.withAlphaComponent(0.2).setFill()
      let top =
        (p["top"] as? CGFloat ?? 0)
        + (p["quoteFirst"] as? Bool == true ? (p["gap"] as? CGFloat ?? 0) : 0)
      for index in 0..<quoteDepth {
        context.fill(
          CGRect(
            x: (model.folded ? 42 : 20) + CGFloat(p["listDepth"] as? Int ?? 0) * 22 + CGFloat(index)
              * 14,
            y: top, width: 2, height: model.height - top - (p["bottom"] as? CGFloat ?? 0)))
      }
    }
    if model.tailRadius > 0 {
      let r = model.tailRadius
      let size = r * 4 / 3
      context.saveGState()
      context.translateBy(x: model.cardRect.maxX, y: model.cardRect.maxY + r / 3 - size)
      context.scaleBy(x: -size / 24, y: size / 24)
      let path = UIBezierPath()
      path.move(to: CGPoint(x: 9, y: 15.6))
      path.addCurve(
        to: CGPoint(x: 9.05, y: 18.6), controlPoint1: CGPoint(x: 9.05, y: 16.3),
        controlPoint2: CGPoint(x: 9.05, y: 17.4))
      path.addCurve(
        to: CGPoint(x: 7.4, y: 22.6), controlPoint1: CGPoint(x: 9.05, y: 20),
        controlPoint2: CGPoint(x: 8.3, y: 21.4))
      path.addQuadCurve(to: CGPoint(x: 7.8, y: 23.5), controlPoint: CGPoint(x: 6.9, y: 23.4))
      path.addCurve(
        to: CGPoint(x: 13.2, y: 21.5), controlPoint1: CGPoint(x: 9.6, y: 23.3),
        controlPoint2: CGPoint(x: 11.6, y: 22.3))
      path.addCurve(
        to: CGPoint(x: 17.2, y: 18.8), controlPoint1: CGPoint(x: 15, y: 20.5),
        controlPoint2: CGPoint(x: 16.2, y: 19.6))
      path.addCurve(
        to: CGPoint(x: 19.6, y: 18), controlPoint1: CGPoint(x: 17.9, y: 18.25),
        controlPoint2: CGPoint(x: 18.6, y: 18))
      path.addLine(to: CGPoint(x: 18, y: 18))
      path.addQuadCurve(to: CGPoint(x: 9, y: 15.6), controlPoint: CGPoint(x: 13.1, y: 18))
      path.close()
      UIColor.transcriptHex(model.style.bubble).setFill()
      path.fill()
      context.restoreGState()
    }
    if model.kind == "table" {
      let rect = model.horizontalRect
      let hairline: CGFloat = 1 / (window?.screen.scale ?? 3)
      context.saveGState()
      context.clip(to: rect)
      let logical = CGRect(
        x: rect.minX - horizontalOffset + hairline / 2, y: rect.minY + hairline / 2,
        width: horizontalWidth - hairline, height: rect.height - hairline)
      let color = UIColor.transcriptHex(model.style.stroke)
      color.setStroke()
      let path = UIBezierPath()
      // Only the first/last row rounds a corner; middle rows share dividers.
      let r: CGFloat = 8
      path.move(to: CGPoint(x: logical.minX, y: logical.maxY - (model.isLast ? r : 0)))
      path.addLine(to: CGPoint(x: logical.minX, y: logical.minY + (model.isFirst ? r : 0)))
      if model.isFirst {
        path.addQuadCurve(
          to: CGPoint(x: logical.minX + r, y: logical.minY),
          controlPoint: CGPoint(x: logical.minX, y: logical.minY))
        path.addLine(to: CGPoint(x: logical.maxX - r, y: logical.minY))
        path.addQuadCurve(
          to: CGPoint(x: logical.maxX, y: logical.minY + r),
          controlPoint: CGPoint(x: logical.maxX, y: logical.minY))
      } else {
        path.move(to: CGPoint(x: logical.minX, y: logical.minY))
        path.addLine(to: CGPoint(x: logical.maxX, y: logical.minY))
      }
      path.move(to: CGPoint(x: logical.maxX, y: logical.minY + (model.isFirst ? r : 0)))
      path.addLine(to: CGPoint(x: logical.maxX, y: logical.maxY - (model.isLast ? r : 0)))
      if model.isLast {
        path.addQuadCurve(
          to: CGPoint(x: logical.maxX - r, y: logical.maxY),
          controlPoint: CGPoint(x: logical.maxX, y: logical.maxY))
        path.addLine(to: CGPoint(x: logical.minX + r, y: logical.maxY))
        path.addQuadCurve(
          to: CGPoint(x: logical.minX, y: logical.maxY - r),
          controlPoint: CGPoint(x: logical.minX, y: logical.maxY))
      }
      path.lineWidth = hairline
      path.stroke()
      context.restoreGState()
    }
    for mark in model.marks {
      let isCopy =
        model.kind == "footer" && mark.rect.minX == 20
        && model.hits.first?.action["type"] as? String == "copy"
      if copied && isCopy {
        model.contentColor.withAlphaComponent(0.08).setFill()
        UIBezierPath(roundedRect: mark.rect.insetBy(dx: -4, dy: -4), cornerRadius: 6).fill()
      }
      let image = copied && isCopy ? TranscriptArtwork.shared.image("icon:check") : mark.image
      (image?.renderingMode == .alwaysTemplate
        ? image?.withTintColor(mark.tint, renderingMode: .alwaysOriginal) : image)?.draw(
          in: mark.rect)
    }
    if let rect = model.mascotRect {
      for pixel in model.style.agentRects {
        UIColor.transcriptHex(pixel.fill).setFill()
        context.fill(
          CGRect(
            x: rect.minX + pixel.x * rect.width / 16, y: rect.minY + pixel.y * rect.height / 16,
            width: pixel.w * rect.width / 16, height: pixel.h * rect.height / 16))
      }
    }
  }
}

/// A streamed burst still fading in. Later bursts get their own canvas, so an
/// earlier one keeps fading instead of snapping to full strength.
private struct TranscriptBurst {
  let canvas: TranscriptTextCanvas
  let start: Int
  var end: Int
  let settles: CFTimeInterval
}

/// The streaming cadence across bursts, as the RN `Cadence` in wordReveal.ts:
/// a fade three gaps long, so bursts overlap into one rolling edge.
final class TranscriptCadence {
  private var average: Double = 0.16
  private var last: CFTimeInterval?
  func burst(_ now: CFTimeInterval = CACurrentMediaTime()) {
    if let last { average = average * 0.7 + min(now - last, 1) * 0.3 }
    last = now
  }
  var fade: Double { min(0.4, max(0.12, average * 3)) }
}

final class TranscriptRowView: UIView, UIScrollViewDelegate {
  private static let streamed: Set<String> = ["text", "heading", "code", "table", "list", "quote"]
  /// How long a word still being written is held back before it shows as is.
  private static let hold: Double = 0.15
  private let stable = TranscriptTextCanvas()
  private var bursts: [TranscriptBurst] = []
  /// End of the text on screen; a trailing half-written word waits past it.
  private var revealed = 0
  private var holdGeneration = 0
  var cadence: TranscriptCadence?
  private let decoration = TranscriptDecorationCanvas()
  private let horizontal = UIScrollView()
  private var surfaces: [UIView] = []
  private let label = UILabel()
  /// Shimmer.tsx: a band of light slides through the text, masked by it.
  private let shimmer = CALayer()
  private let shimmerBand = CAGradientLayer()
  private let shimmerMask = UILabel()
  /// The label drawn once into an image; an offscreen label's own layer is not
  /// reliably drawn when used as a mask.
  private let shimmerMaskLayer = CALayer()
  private var clock: Timer?
  private var copiedUntil: Date?
  private(set) var model: TranscriptRowModel?
  private var prepared: TranscriptTextLayout?
  var onAction: (([String: Any]) -> Void)?
  var onSelection: ((String) -> Void)?
  var onHorizontalScroll: ((String, CGFloat) -> Void)?
  private var synchronizing = false
  private var presentationActive = true

  override init(frame: CGRect) {
    super.init(frame: frame)
    isOpaque = false
    stable.backgroundColor = .clear
    stable.isOpaque = false
    stable.isUserInteractionEnabled = false
    decoration.backgroundColor = .clear
    decoration.isOpaque = false
    decoration.isUserInteractionEnabled = false
    addSubview(decoration)
    addSubview(horizontal)
    addSubview(stable)
    addSubview(label)
    horizontal.delegate = self
    horizontal.showsHorizontalScrollIndicator = false
    horizontal.alwaysBounceHorizontal = false
    horizontal.isDirectionalLockEnabled = true
    label.isUserInteractionEnabled = false
    label.numberOfLines = 1
    label.lineBreakMode = .byTruncatingTail
    layer.addSublayer(shimmer)
    shimmer.addSublayer(shimmerBand)
    shimmer.mask = shimmerMaskLayer
    shimmerBand.startPoint = CGPoint(x: 0, y: 0.5)
    shimmerBand.endPoint = CGPoint(x: 1, y: 0.5)
    shimmerBand.anchorPoint = .zero
    addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(tapped(_:))))
    addGestureRecognizer(UILongPressGestureRecognizer(target: self, action: #selector(held(_:))))
  }
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
  deinit { clock?.invalidate() }

  func configure(
    _ model: TranscriptRowModel, layout: TranscriptTextLayout, animate: Bool,
    horizontalWidth: CGFloat = 0, horizontalOffset: CGFloat = 0
  ) {
    let width = max(model.horizontalWidth, horizontalWidth)
    if self.model === model, prepared === layout {
      if abs(horizontal.contentSize.width - width) > 0.1 {
        horizontal.contentSize.width = width
        horizontal.isScrollEnabled = width > horizontal.bounds.width + 0.5
        decoration.horizontalWidth = width
        positionCanvases()
      }
      setHorizontalOffset(horizontalOffset)
      return
    }
    decoration.horizontalWidth = width
    let old = self.model
    if old?.id != model.id { copiedUntil = nil }
    self.model = model
    prepared = layout
    clock?.invalidate()
    clock = nil
    for (index, surface) in model.surfaces.enumerated() {
      if index >= surfaces.count {
        let view = UIView()
        view.isUserInteractionEnabled = false
        view.layer.cornerCurve = .continuous
        insertSubview(view, belowSubview: decoration)
        surfaces.append(view)
      }
      let view = surfaces[index]
      view.isHidden = false
      view.frame = surface.rect
      view.backgroundColor = surface.color
      view.layer.cornerRadius = surface.radius
      view.layer.maskedCorners = surface.corners
      view.layer.borderColor = surface.border?.cgColor
      view.layer.borderWidth = surface.border == nil ? 0 : 1 / (window?.screen.scale ?? 3)
    }
    for index in model.surfaces.count..<surfaces.count { surfaces[index].isHidden = true }
    decoration.frame = bounds
    decoration.model = model
    decoration.copied = copiedUntil.map { $0 > Date() } ?? false
    decoration.setNeedsDisplay()
    horizontal.isHidden = model.group == nil
    if model.group != nil {
      horizontal.frame = model.horizontalRect
      horizontal.contentSize = CGSize(
        width: max(model.horizontalWidth, horizontalWidth), height: model.horizontalRect.height)
      horizontal.isScrollEnabled = horizontal.contentSize.width > horizontal.bounds.width + 0.5
      if stable.superview !== horizontal { horizontal.addSubview(stable) }
    } else if stable.superview !== self {
      insertSubview(stable, belowSubview: label)
    }
    let length = model.text.length
    let streams = animate && Self.streamed.contains(model.kind) && presentationActive
      && !UIAccessibility.isReduceMotionEnabled
    if old?.id != model.id || !streams {
      clearBursts()
      revealed = streams && old?.id != model.id ? 0 : length
    } else {
      // Text past the shared prefix changed; it reveals again from there.
      let before = old!.text.string as NSString
      let after = model.text.string as NSString
      var index = 0
      while index < min(before.length, after.length, revealed),
        before.character(at: index) == after.character(at: index)
      { index += 1 }
      let prefix =
        index < after.length ? after.rangeOfComposedCharacterSequence(at: index).location : index
      revealed = min(revealed, prefix)
      for burst in bursts where burst.start >= revealed { burst.canvas.removeFromSuperview() }
      bursts = bursts.filter { $0.start < revealed }.map {
        var burst = $0
        burst.end = min(burst.end, revealed)
        return burst
      }
    }
    stable.layout = layout
    for burst in bursts { burst.canvas.layout = layout }
    if streams {
      // A word still being written waits until it is whole, or the stream pauses.
      reveal(to: Self.wordEnd(model.text.string as NSString, from: revealed))
      if revealed < length { scheduleHold() }
    }
    updateRanges()
    positionCanvases()
    setHorizontalOffset(horizontalOffset)
    label.isHidden = model.liveLabel == nil
    shimmer.isHidden = true
    shimmerBand.removeAllAnimations()
    if let live = model.liveLabel {
      label.frame = live.rect
      label.font = live.font
      label.textColor = live.color
      label.text = live.text
      if presentationActive && live.shimmer {
        startShimmer(live)
      }
      if presentationActive && model.kind == "fold" && model.style.agent.isEmpty
        && model.action["live"] as? Bool == true
        && model.action["startedAt"] is NSNumber
      {
        clock = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
          guard let self, let model = self.model else { return }
          self.label.text = TranscriptTypography.working(model.action)
          self.drawShimmerMask(self.label.text ?? "")
        }
      }
    }
    configureAccessibility()
  }
  func setPresentationActive(_ active: Bool) {
    guard active != presentationActive else { return }
    presentationActive = active
    if !active, let model {
      clearBursts()
      revealed = model.text.length
      updateRanges()
    }
    clock?.invalidate()
    clock = nil
    shimmerBand.removeAllAnimations()
    shimmer.isHidden = true
    guard active, let model, let live = model.liveLabel else { return }
    if live.shimmer { startShimmer(live) }
    if model.kind == "fold" && model.style.agent.isEmpty && model.action["live"] as? Bool == true
      && model.action["startedAt"] is NSNumber
    {
      label.text = TranscriptTypography.working(model.action)
      clock = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in
        guard let self, let model = self.model else { return }
        self.label.text = TranscriptTypography.working(model.action)
        self.drawShimmerMask(self.label.text ?? "")
      }
    }
  }
  private func startShimmer(_ live: TranscriptLiveLabel) {
    CATransaction.begin()
    CATransaction.setDisableActions(true)
    defer { CATransaction.commit() }
    shimmer.isHidden = false
    shimmer.frame = live.rect
    let color = model!.contentColor
    shimmerBand.colors = [
      color.withAlphaComponent(0).cgColor, color.cgColor, color.withAlphaComponent(0).cgColor,
    ]
    let width = min(
      live.rect.width, ceil((live.text as NSString).size(withAttributes: [.font: live.font]).width))
    // Each character widens the light, as the desktop's `--spread` does.
    let spread = max(1, CGFloat(live.text.count) * 2)
    shimmerBand.bounds = CGRect(x: 0, y: 0, width: spread * 2, height: live.rect.height)
    shimmerBand.position = CGPoint(x: -spread * 2, y: 0)
    shimmerMask.frame = CGRect(origin: .zero, size: live.rect.size)
    shimmerMask.font = live.font
    shimmerMask.textColor = .white
    shimmerMask.lineBreakMode = .byTruncatingTail
    drawShimmerMask(live.text)
    // From a quarter of the text's width before it to a quarter after.
    let animation = CABasicAnimation(keyPath: "position.x")
    animation.fromValue = -width * 0.25 - spread
    animation.toValue = width * 1.25 - spread
    animation.duration = live.duration
    animation.repeatCount = .infinity
    animation.isRemovedOnCompletion = false
    // Continue mid-sweep when a streaming row is reconfigured, rather than
    // restarting before the light reaches the text.
    animation.timeOffset = CACurrentMediaTime().truncatingRemainder(dividingBy: live.duration)
    shimmerBand.add(animation, forKey: "sweep")
  }
  private func drawShimmerMask(_ text: String) {
    shimmerMask.text = text
    let bounds = shimmerMask.bounds
    guard bounds.width > 0, bounds.height > 0 else { return }
    let image = UIGraphicsImageRenderer(bounds: bounds).image { _ in
      shimmerMask.drawText(in: bounds)
    }
    CATransaction.begin()
    CATransaction.setDisableActions(true)
    shimmerMaskLayer.frame = bounds
    shimmerMaskLayer.contentsScale = image.scale
    shimmerMaskLayer.contents = image.cgImage
    CATransaction.commit()
  }
  /// Where to stop showing `text` while it streams: after its last whole word.
  private static func wordEnd(_ text: NSString, from: Int) -> Int {
    var end = text.length
    while end > from, !isSpace(text.character(at: end - 1)) { end -= 1 }
    return end
  }
  private static func isSpace(_ code: unichar) -> Bool {
    code == 32 || code == 10 || code == 9 || code == 13
  }
  private func reveal(to end: Int) {
    guard end > revealed, let model else { return }
    let canvas = TranscriptTextCanvas()
    canvas.backgroundColor = .clear
    canvas.isOpaque = false
    canvas.isUserInteractionEnabled = false
    canvas.layout = prepared
    if model.group != nil {
      horizontal.addSubview(canvas)
    } else {
      insertSubview(canvas, belowSubview: label)
    }
    let fade = cadence?.fade ?? 0.22
    let animation = CABasicAnimation(keyPath: "opacity")
    animation.fromValue = 0
    animation.toValue = 1
    animation.duration = fade
    // The desktop's fade curve.
    animation.timingFunction = CAMediaTimingFunction(controlPoints: 0.33, 0.53, 0.67, 1)
    canvas.layer.add(animation, forKey: "reveal")
    bursts.append(
      TranscriptBurst(
        canvas: canvas, start: revealed, end: end, settles: CACurrentMediaTime() + fade))
    revealed = end
    let id = model.id
    DispatchQueue.main.asyncAfter(deadline: .now() + fade + 0.02) { [weak self] in
      guard let self, self.model?.id == id else { return }
      self.settle()
    }
  }
  private func scheduleHold() {
    holdGeneration += 1
    let generation = holdGeneration
    DispatchQueue.main.asyncAfter(deadline: .now() + Self.hold) { [weak self] in
      guard let self, self.holdGeneration == generation, let model = self.model else { return }
      self.reveal(to: model.text.length)
      self.updateRanges()
      self.positionCanvases()
    }
  }
  /// Bursts that finished fading go back into the one stable canvas.
  private func settle() {
    let now = CACurrentMediaTime()
    guard bursts.first.map({ $0.settles <= now + 0.01 }) == true else { return }
    while let first = bursts.first, first.settles <= now + 0.01 {
      first.canvas.removeFromSuperview()
      bursts.removeFirst()
    }
    updateRanges()
  }
  private func clearBursts() {
    holdGeneration += 1
    for burst in bursts { burst.canvas.removeFromSuperview() }
    bursts.removeAll()
  }
  private func updateRanges() {
    stable.range = NSRange(location: 0, length: bursts.first?.start ?? revealed)
    stable.setNeedsDisplay()
    for burst in bursts {
      burst.canvas.range = NSRange(location: burst.start, length: burst.end - burst.start)
      burst.canvas.setNeedsDisplay()
    }
  }
  func setHorizontalOffset(_ offset: CGFloat) {
    guard let model else { return }
    if model.group == nil { return }
    let x = min(max(0, offset), max(0, horizontal.contentSize.width - horizontal.bounds.width))
    if abs(horizontal.contentOffset.x - x) < 0.1 && stable.frame.size == horizontal.bounds.size
      && stable.origin == CGPoint(x: model.horizontalRect.minX + x, y: model.horizontalRect.minY)
    {
      return
    }
    synchronizing = true
    if abs(horizontal.contentOffset.x - x) > 0.1 { horizontal.contentOffset.x = x }
    positionCanvases()
    synchronizing = false
  }
  private func positionCanvases() {
    guard let model else { return }
    for canvas in [stable] + bursts.map(\.canvas) {
      if model.group != nil {
        canvas.frame = CGRect(
          x: horizontal.contentOffset.x, y: 0, width: horizontal.bounds.width,
          height: horizontal.bounds.height)
        canvas.origin = CGPoint(
          x: model.horizontalRect.minX + horizontal.contentOffset.x, y: model.horizontalRect.minY)
      } else {
        let top = min(0, model.fragments.map { $0.rect.minY }.min() ?? 0)
        canvas.frame = CGRect(x: 0, y: top, width: bounds.width, height: bounds.height - top)
        canvas.origin = CGPoint(x: 0, y: top)
      }
      canvas.setNeedsDisplay()
    }
    decoration.horizontalOffset = model.group == nil ? 0 : horizontal.contentOffset.x
    decoration.setNeedsDisplay()
  }
  func scrollViewDidScroll(_ scrollView: UIScrollView) {
    positionCanvases()
    if !synchronizing, let group = model?.group {
      onHorizontalScroll?(group, horizontal.contentOffset.x)
    }
  }
  func reuse() {
    clock?.invalidate()
    clock = nil
    horizontal.setContentOffset(horizontal.contentOffset, animated: false)
    model = nil
    prepared = nil
    layer.removeAllAnimations()
    stable.layout = nil
    clearBursts()
    decoration.model = nil
    shimmerBand.removeAllAnimations()
    onAction = nil
    onSelection = nil
    onHorizontalScroll = nil
    accessibilityElements = nil
    removeFromSuperview()
  }
  @objc private func held(_ gesture: UILongPressGestureRecognizer) {
    if gesture.state == .began, let model { onSelection?(model.id) }
  }
  private func perform(_ action: [String: Any]) {
    if action["type"] as? String == "toggle" { UISelectionFeedbackGenerator().selectionChanged() }
    if action["type"] as? String == "copy" {
      copiedUntil = Date().addingTimeInterval(2)
      decoration.copied = true
      decoration.setNeedsDisplay()
      let id = model?.id
      DispatchQueue.main.asyncAfter(deadline: .now() + 2) { [weak self] in
        guard let self, self.model?.id == id, self.copiedUntil.map({ $0 <= Date() }) == true else {
          return
        }
        self.decoration.copied = false
        self.decoration.setNeedsDisplay()
      }
    }
    onAction?(action)
  }
  @objc private func tapped(_ gesture: UITapGestureRecognizer) {
    guard let model, let layout = prepared else { return }
    let point = gesture.location(in: self)
    for hit in model.hits where hit.rect.contains(point) {
      perform(hit.action)
      return
    }
    var translated = point
    if model.group != nil { translated.x += horizontal.contentOffset.x }
    for part in layout.parts where part.fragment.rect.contains(translated) {
      let local = CGPoint(
        x: translated.x - part.fragment.rect.minX, y: part.fragment.rect.maxY - translated.y)
      for line in part.lines {
        var ascent: CGFloat = 0
        var descent: CGFloat = 0
        let width = CGFloat(CTLineGetTypographicBounds(line.line, &ascent, &descent, nil))
        guard local.y >= line.origin.y - descent - 3 && local.y <= line.origin.y + ascent + 3,
          local.x >= line.origin.x, local.x <= line.origin.x + width
        else { continue }
        let index = CTLineGetStringIndexForPosition(
          line.line, CGPoint(x: local.x - line.origin.x, y: 0))
        if index >= 0 && index < part.fragment.text.length,
          let link = part.fragment.text.attribute(.link, at: index, effectiveRange: nil) as? String
        {
          perform(["type": "link", "url": link])
          return
        }
      }
    }
    // Nothing to act on: a tap on the transcript lets go of the keyboard.
    window?.endEditing(true)
  }
  private func configureAccessibility() {
    guard let model else { return }
    isAccessibilityElement = model.hits.count <= 1
    accessibilityLabel = model.liveLabel?.text ?? model.text.string
    accessibilityTraits = model.hits.isEmpty ? .staticText : .button
    accessibilityHint = model.hits.first?.label ?? "Long press to select text"
    if model.hits.count > 1 {
      accessibilityElements = model.hits.map { hit in
        let element = TranscriptAccessibilityAction(accessibilityContainer: self)
        element.accessibilityLabel = hit.label
        element.accessibilityTraits = .button
        element.accessibilityFrameInContainerSpace = hit.rect
        element.activate = { [weak self] in self?.perform(hit.action) }
        return element
      }
    } else {
      accessibilityElements = nil
    }
  }
  override func accessibilityActivate() -> Bool {
    if let hit = model?.hits.first {
      perform(hit.action)
    } else if let model {
      onSelection?(model.id)
    }
    return true
  }
}

private final class TranscriptAccessibilityAction: UIAccessibilityElement {
  var activate: (() -> Void)?
  override func accessibilityActivate() -> Bool {
    activate?()
    return true
  }
}
