import ExpoModulesCore
import UIKit

private final class TranscriptDisplayTarget: NSObject {
  weak var view: NativeTranscriptView?
  @objc func tick(_ link: CADisplayLink) { view?.advanceFollow(link) }
}

final class NativeTranscriptView: ExpoView, UIScrollViewDelegate, UIGestureRecognizerDelegate {
  /// How far above the top inset the anchored message rests, as Transcript.tsx.
  private static let anchorLift: CGFloat = 36
  let onStatus = EventDispatcher()
  let onAction = EventDispatcher()
  let onScrollState = EventDispatcher()
  private let scroll = UIScrollView()
  private let spinner = UIActivityIndicatorView(style: .medium)
  private var session: TranscriptSession?
  private var configuration = ""
  private var startedConfiguration = ""
  private var pending = "null"
  private var sentPending = ""
  private var style = TranscriptStyle()
  private var collapsed = true
  private var frameData: TranscriptLayoutFrame?
  private var visible: [String: TranscriptRowView] = [:]
  private var pool: [TranscriptRowView] = []
  private var horizontalOffsets: [String: CGFloat] = [:]
  private var top: CGFloat = 0
  private var bottom: CGFloat = 0
  private var extra: CGFloat = 0
  private var keyboardOffset: CGFloat = 0
  private var keyboard: CGFloat = 0
  private var active = true
  private var foreground = true
  private var initialized = false
  /// Blank space under the latest message so it can rest under the header.
  private var anchoring = false
  /// Whether following scrolls to that message rather than to the end.
  private var pinned = false
  private var following = true
  private let cadence = TranscriptCadence()
  /// A fold or "Show more" tapped, until the frame showing it arrives.
  private var toggle: (id: String, open: Bool)?
  /// Clips the rows a fold reveals or hides, as Collapsible.tsx does.
  private var fold: UIView?
  private var foldAnimator: UIViewPropertyAnimator?
  private var foldLeaving: [TranscriptRowView] = []
  private var loadingOlder = false
  private var lastStatus: NSDictionary?
  private var lastAway: Bool?
  private var observations: [NSObjectProtocol] = []
  private var displayLink: CADisplayLink?
  private let displayTarget = TranscriptDisplayTarget()
  private var lastTick: CFTimeInterval = 0
  /// The follow's speed, kept as its target moves (a message sent, a line
  /// streamed in) so the transcript glides on rather than lurching again.
  private var followVelocity: CGFloat = 0
  /// How far a leftward drag has slid a Mono chat aside to show its times, as
  /// Messages does: the whole transcript moves, the times come in with it and
  /// then hold at the edge.
  private var reveal: CGFloat = 0
  private static let revealWidth: CGFloat = 64
  private let slide = UIPanGestureRecognizer()
  private var times: [String: UILabel] = [:]
  /// The row last long-pressed, its kind and ID, until its text comes back.
  private var held: (kind: String, id: String)?
  #if DEBUG
    /// When the last message was sent, for the dev-only scroll trace.
    private var traceSent: CFTimeInterval = 0
    private var traceInset: CGFloat = 0
  #endif
  // Newest complete worker frame wins if the main thread is occupied.
  private let relayLock = NSLock()
  private var relayFrame: TranscriptLayoutFrame?
  private var relayScheduled = false
  private var relayGeneration = UUID()

  required init(appContext: AppContext? = nil) {
    super.init(appContext: appContext)
    clipsToBounds = true
    scroll.delegate = self
    scroll.contentInsetAdjustmentBehavior = .never
    scroll.keyboardDismissMode = .interactive
    scroll.alwaysBounceVertical = true
    scroll.showsVerticalScrollIndicator = false
    // Rows handle their own taps; one between or below them lets go of the keyboard.
    let tap = UITapGestureRecognizer(target: self, action: #selector(tappedOutside(_:)))
    tap.cancelsTouchesInView = false
    scroll.addGestureRecognizer(tap)
    slide.addTarget(self, action: #selector(slid(_:)))
    slide.delegate = self
    scroll.addGestureRecognizer(slide)
    addSubview(scroll)
    spinner.hidesWhenStopped = true
    addSubview(spinner)
    displayTarget.view = self
    let center = NotificationCenter.default
    observations.append(
      center.addObserver(
        forName: UIResponder.keyboardWillChangeFrameNotification, object: nil, queue: .main
      ) { [weak self] note in self?.keyboardChanged(note) })
    observations.append(
      center.addObserver(
        forName: UIApplication.didEnterBackgroundNotification, object: nil, queue: .main
      ) { [weak self] _ in self?.setForeground(false) })
    observations.append(
      center.addObserver(
        forName: UIApplication.willEnterForegroundNotification, object: nil, queue: .main
      ) { [weak self] _ in self?.setForeground(true) })
    observations.append(
      center.addObserver(
        forName: UIApplication.didReceiveMemoryWarningNotification, object: nil, queue: .main
      ) { [weak self] _ in
        self?.session?.purge()
        self?.pool.removeAll()
      })
  }
  deinit {
    session?.stop()
    displayLink?.invalidate()
    for observer in observations { NotificationCenter.default.removeObserver(observer) }
  }

  @objc private func tappedOutside(_ gesture: UITapGestureRecognizer) {
    let point = gesture.location(in: scroll)
    var hit = scroll.hitTest(point, with: nil)
    while let view = hit, view !== scroll {
      if view is TranscriptRowView { return }
      hit = view.superview
    }
    window?.endEditing(true)
  }

  override func gestureRecognizerShouldBegin(_ recognizer: UIGestureRecognizer) -> Bool {
    guard recognizer === slide else { return super.gestureRecognizerShouldBegin(recognizer) }
    let velocity = slide.velocity(in: scroll)
    guard !style.agent.isEmpty, velocity.x < 0, abs(velocity.x) > abs(velocity.y) * 1.5 else {
      return false
    }
    // A code block or table that scrolls sideways keeps its own swipe.
    var hit = scroll.hitTest(slide.location(in: scroll), with: nil)
    while let view = hit, view !== scroll {
      if let inner = view as? UIScrollView, inner.isScrollEnabled { return false }
      hit = view.superview
    }
    return true
  }
  func gestureRecognizer(
    _ recognizer: UIGestureRecognizer,
    shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
  ) -> Bool {
    recognizer === slide && other === scroll.panGestureRecognizer
  }
  @objc private func slid(_ pan: UIPanGestureRecognizer) {
    switch pan.state {
    case .began:
      // Sideways now: the transcript stops scrolling under the finger.
      scroll.panGestureRecognizer.isEnabled = false
      scroll.panGestureRecognizer.isEnabled = true
    case .changed:
      let drag = max(0, -pan.translation(in: scroll).x)
      let limit = Self.revealWidth
      // Past the times it gives like a rubber band, and only so far.
      let over = drag - limit
      reveal = over <= 0 ? drag : limit + 24 * (1 - 1 / (over / 60 + 1))
      layoutReveal()
    default:
      reveal = 0
      UIView.animate(
        withDuration: 0.35, delay: 0, usingSpringWithDamping: 0.9, initialSpringVelocity: 0,
        options: [.allowUserInteraction, .beginFromCurrentState]
      ) { self.layoutReveal() } completion: { _ in
        guard self.reveal == 0 else { return }
        for label in self.times.values { label.removeFromSuperview() }
        self.times.removeAll()
      }
    }
  }
  /// Slides every row by `reveal` and lays each visible time at the edge: it
  /// comes in with the rows, then holds once fully out.
  private func layoutReveal() {
    for (id, row) in visible {
      guard let model = row.model else { continue }
      row.frame.origin.x = -reveal
      guard let time = model.time, row.superview === scroll else { continue }
      let label =
        times[id]
        ?? {
          let label = UILabel()
          label.font = .systemFont(ofSize: 12 * style.scale)
          label.textColor = .transcriptHex(style.muted)
          label.text = time.text
          label.sizeToFit()
          label.frame.origin = CGPoint(x: bounds.width + 8, y: row.frame.minY + time.y - label.bounds.height / 2)
          label.alpha = 0
          scroll.addSubview(label)
          times[id] = label
          return label
        }()
      label.frame.origin = CGPoint(
        x: bounds.width - min(reveal, Self.revealWidth) + 8,
        y: row.frame.minY + time.y - label.bounds.height / 2)
      label.alpha = min(1, reveal / Self.revealWidth)
    }
    for (id, label) in times where visible[id] == nil {
      label.removeFromSuperview()
      times[id] = nil
    }
  }

  func setConfiguration(_ value: String) { configuration = value }
  func setPending(_ value: String) { pending = value }
  func setActive(_ value: Bool) {
    active = value
    session?.setActive(value && foreground)
    for row in visible.values { row.setPresentationActive(value && foreground) }
    if !value { stopFollowingAnimation() }
  }
  func setInsets(
    top: Double? = nil, bottom: Double? = nil, extra: Double? = nil, keyboardOffset: Double? = nil
  ) {
    if let top { self.top = top }
    if let bottom { self.bottom = bottom }
    if let extra { self.extra = extra }
    if let keyboardOffset { self.keyboardOffset = keyboardOffset }
    updateGeometry()
  }
  func setAppearance(_ value: String) {
    guard let data = value.data(using: .utf8),
      let input = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    else { return }
    style.scale = max(0.7, input["scale"] as? CGFloat ?? 1)
    style.pixelRatio = max(1, input["pixelRatio"] as? CGFloat ?? 3)
    style.faint = input["faint"] as? String ?? style.faint
    style.fileRef = input["fileRef"] as? String ?? style.fileRef
    style.fileLinks = input["fileLinks"] as? Bool ?? false
    style.stroke = input["stroke"] as? String ?? style.stroke
    style.surfaceRaised = input["surfaceRaised"] as? String ?? style.surfaceRaised
    style.reactions = input["reactions"] as? [String: String] ?? [:]
    if let rects = input["agentRects"],
      let data = try? JSONSerialization.data(withJSONObject: rects)
    {
      style.agentRects = (try? JSONDecoder().decode([TranscriptPixel].self, from: data)) ?? []
    }
    for key in [
      "content", "muted", "accent", "bubble", "bubbleText", "surface", "error", "rail", "agent",
    ] {
      guard let value = input[key] as? String else { continue }
      switch key {
      case "content": style.content = value
      case "muted": style.muted = value
      case "accent": style.accent = value
      case "bubble": style.bubble = value
      case "bubbleText": style.bubbleText = value
      case "surface": style.surface = value
      case "error": style.error = value
      case "rail": style.rail = value
      case "agent": style.agent = value
      default: break
      }
    }
    if let collapsed = input["collapsed"] as? Bool, collapsed != self.collapsed {
      self.collapsed = collapsed
      session?.command(["type": "collapsed", "value": collapsed])
    }
    publishViewport()
  }
  func updateProps() {
    guard bounds.width > 0, !configuration.isEmpty else { return }
    if startedConfiguration != configuration {
      session?.stop()
      frameData = nil
      lastStatus = nil
      lastAway = nil
      initialized = false
      for row in visible.values { row.reuse() }
      visible.removeAll()
      pool.removeAll()
      horizontalOffsets.removeAll()
      guard let data = configuration.data(using: .utf8),
        var input = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
      else { return }
      input["collapsed"] = collapsed
      if let data = pending.data(using: .utf8),
        let value = try? JSONSerialization.jsonObject(with: data, options: .fragmentsAllowed)
      {
        input["pending"] = value
      }
      style.width = bounds.width
      let created = TranscriptSession(configuration: input, style: style)
      let generation = UUID()
      relayLock.lock()
      relayGeneration = generation
      relayFrame = nil
      relayLock.unlock()
      created.onFrame = { [weak self] frame in self?.enqueue(frame, generation: generation) }
      created.onSelection = { [weak self] text in DispatchQueue.main.async { self?.select(text) } }
      created.setActive(active && foreground)
      session = created
      startedConfiguration = configuration
      sentPending = pending
      created.start()
      spinner.startAnimating()
    }
    if pending != sentPending, let data = pending.data(using: .utf8),
      let value = try? JSONSerialization.jsonObject(with: data, options: .fragmentsAllowed)
    {
      session?.command(["type": "pending", "pending": value])
      sentPending = pending
    }
    publishViewport()
  }

  override func layoutSubviews() {
    super.layoutSubviews()
    scroll.frame = bounds
    spinner.center = CGPoint(x: bounds.midX, y: top + max(0, bounds.height - top - bottom) / 2)
    style.width = bounds.width
    updateProps()
    updateGeometry()
    displayRows()
  }

  private func setForeground(_ value: Bool) {
    foreground = value
    session?.setActive(value && active)
    for row in visible.values { row.setPresentationActive(value && active) }
    if !value { stopFollowingAnimation() }
  }
  private func enqueue(_ frame: TranscriptLayoutFrame, generation: UUID) {
    relayLock.lock()
    guard generation == relayGeneration else {
      relayLock.unlock()
      return
    }
    relayFrame = frame
    if relayScheduled {
      relayLock.unlock()
      return
    }
    relayScheduled = true
    relayLock.unlock()
    DispatchQueue.main.async { [weak self] in
      guard let self else { return }
      self.relayLock.lock()
      let newest = self.relayFrame
      self.relayFrame = nil
      self.relayScheduled = false
      self.relayLock.unlock()
      if let newest { self.apply(newest) }
    }
  }

  private func apply(_ frame: TranscriptLayoutFrame) {
    let old = frameData
    let wasInitialized = initialized
    let status = NSDictionary(dictionary: frame.status)
    if lastStatus?.isEqual(status) != true {
      lastStatus = status
      onStatus(frame.status)
    }
    if frame.rows.isEmpty {
      frameData = frame
      updateGeometry()
      displayRows()
      // Loaded with nothing in it, or failed: there's nothing left to wait on.
      if frame.status["error"] is String || frame.status["ready"] as? Bool == true {
        spinner.stopAnimating()
      }
      return
    }
    spinner.stopAnimating()
    let changed = old?.version != frame.version
    let oldIndex = old?.index(at: max(0, scroll.contentOffset.y - top)) ?? 0
    let anchorID = old.flatMap { $0.rows.isEmpty ? nil : $0.rows[oldIndex].id }
    let delta =
      old.map { scroll.contentOffset.y - top - $0.offsets[min(oldIndex, $0.rows.count)] } ?? 0
    let newPrompt = frame.lastUserID != old?.lastUserID && frame.lastUserID != nil
    #if DEBUG
      if newPrompt && frame.lastUserID?.hasPrefix("pending:") == true { traceSent = CACurrentMediaTime() }
      if changed || newPrompt {
        trace("frame\(newPrompt ? " prompt" : "") following=\(following) \(traceRows(old, frame))")
      }
    #endif
    let firstLive = frame.status["ready"] as? Bool == true && old?.status["ready"] as? Bool != true
    // Older turns came in above: what's on screen keeps its place exactly.
    let olderArrived = old != nil && frame.firstTurnID != old?.firstTurnID
    frameData = frame
    if changed { horizontalOffsets = horizontalOffsets.filter { frame.groupWidths[$0.key] != nil } }
    if loadingOlder,
      frame.firstTurnID != old?.firstTurnID || frame.status["error"] is String || !frame.older
    {
      loadingOlder = false
    }
    // Every message sent anchors, as Transcript.tsx; a Mono's chat goes to the end.
    if newPrompt {
      following = true
      anchoring = style.agent.isEmpty
      pinned = anchoring
    }
    if firstLive && frame.status["busy"] as? Bool == true && following && style.agent.isEmpty {
      anchoring = true
      pinned = true
    }
    var folding = false
    if let toggle, let index = frame.rows.lastIndex(where: { $0.id == toggle.id }) {
      let action = frame.rows[index].action
      let open = action["nativeExpanded"] as? Bool ?? action["open"] as? Bool ?? false
      if open != toggle.open {
        folding = old != nil
        self.toggle = nil
      }
    }
    if wasInitialized && changed && !folding { cadence.burst() }
    updateGeometry()
    if !initialized {
      initialized = true
      if frame.status["busy"] as? Bool != true && pending == "null" {
        anchoring = false
        pinned = false
        updateGeometry()
      }
      scroll.contentOffset.y = targetOffset()
      scroll.alpha = 0
      UIView.animate(withDuration: 0.25) { self.scroll.alpha = 1 }
    } else if newPrompt {
      startFollowingAnimation()
    } else if changed && (!following || olderArrived), let anchorID,
      let index = frame.rows.firstIndex(where: { $0.id == anchorID })
    {
      // Bounds preserves UIKit's deceleration while content above moves.
      var bounds = scroll.bounds
      bounds.origin.y = top + frame.offsets[index] + delta
      scroll.bounds = bounds
    } else if following {
      startFollowingAnimation()
    }
    if folding, let old {
      foldRows(from: old)
    } else {
      displayRows(animate: wasInitialized && changed && !loadingOlder)
      if wasInitialized && changed && !olderArrived, let old {
        for id in entering(from: old, to: frame) { visible[id].map(enter) }
      }
    }
    publishViewport()
    updateAway()
    // A chat that doesn't fill the screen yet reads further back on its own,
    // so opening one never waits on a scroll to show the rest.
    if initialized && frame.older && !loadingOlder && frame.height < bounds.height * 1.5 {
      loadingOlder = true
      session?.command(["type": "older"])
    }
  }

  /// A Mono chat's rows that are new in `frame`: not ones taking over what
  /// left with the same look, as a sent message over its local echo or the
  /// desktop's status line over the phone's.
  private func entering(from old: TranscriptLayoutFrame, to frame: TranscriptLayoutFrame) -> [String] {
    guard !style.agent.isEmpty else { return [] }
    let look = { (row: TranscriptRowModel) in
      row.kind == "fold" ? "fold" : row.kind + "\u{0}" + row.text.string
    }
    let now = Set(frame.rows.map(\.id))
    let before = Set(old.rows.map(\.id))
    var gone: [String: Int] = [:]
    for row in old.rows where !now.contains(row.id) { gone[look(row), default: 0] += 1 }
    var entering: [String] = []
    for row in frame.rows where !before.contains(row.id) && row.height > 0 {
      if let left = gone[look(row)], left > 0 {
        gone[look(row)] = left - 1
      } else {
        entering.append(row.id)
      }
    }
    return entering
  }
  /// A new message comes in as Messages' do: it rises a little and grows from
  /// its bubble's tail corner on a spring as it fades in. Only on the layer,
  /// so rows keep being laid out by frame meanwhile.
  private func enter(_ row: TranscriptRowView) {
    guard let model = row.model, !UIAccessibility.isReduceMotionEnabled else { return }
    let bubble = model.kind == "user" ? model.cardRect : model.surfaces.first?.rect
    let corner =
      bubble.map { model.kind == "user" ? CGPoint(x: $0.maxX, y: $0.maxY) : CGPoint(x: $0.minX, y: $0.maxY) }
      ?? CGPoint(x: 20, y: row.bounds.maxY)
    let dx = corner.x - row.bounds.midX
    let dy = corner.y - row.bounds.midY
    // A reply over several rows is one bubble in pieces: they rise together
    // without growing, so no seam opens between them.
    let p = model.presentation
    let piece = p["reply"] as? Bool == true
      && !(p["replyFirst"] as? Bool == true && p["replyLast"] as? Bool == true)
    let scale: CGFloat = piece ? 1 : 0.9
    var from = CATransform3DMakeTranslation(0, 14, 0)
    from = CATransform3DTranslate(from, dx, dy, 0)
    from = CATransform3DScale(from, scale, scale, 1)
    from = CATransform3DTranslate(from, -dx, -dy, 0)
    let grow = CASpringAnimation(keyPath: "transform")
    grow.fromValue = from
    grow.toValue = CATransform3DIdentity
    grow.mass = 1
    grow.stiffness = 300
    grow.damping = 28
    grow.duration = grow.settlingDuration
    let fade = CABasicAnimation(keyPath: "opacity")
    fade.fromValue = 0
    fade.toValue = 1
    fade.duration = 0.18
    fade.timingFunction = CAMediaTimingFunction(name: .easeOut)
    row.layer.add(grow, forKey: "enter")
    row.layer.add(fade, forKey: "enterFade")
  }

  /// Rows under a fold slide while what it holds is clipped open or shut, and
  /// rows already on screen keep their text as is. Core Animation runs it all.
  private func foldRows(from old: TranscriptLayoutFrame) {
    finishFold()
    guard let frame = frameData else { return }
    var before: [String: CGFloat] = [:]
    before.reserveCapacity(old.rows.count)
    for (index, row) in old.rows.enumerated() { before[row.id] = top + old.offsets[index] }
    var after: [String: Int] = [:]
    after.reserveCapacity(frame.rows.count)
    for (index, row) in frame.rows.enumerated() { after[row.id] = index }
    let shown = visible.mapValues(\.frame)
    let leaving = displayRows(holding: true)
    // The first run of rows that came or went is what the fold opened or shut.
    var block: (y: CGFloat, height: CGFloat, ids: Set<String>, opening: Bool)?
    if let start = frame.rows.firstIndex(where: { before[$0.id] == nil }) {
      var end = start
      while end + 1 < frame.rows.count, before[frame.rows[end + 1].id] == nil { end += 1 }
      block = (
        top + frame.offsets[start], frame.offsets[end + 1] - frame.offsets[start],
        Set(frame.rows[start...end].map(\.id)), true
      )
    } else if let start = old.rows.firstIndex(where: { after[$0.id] == nil }) {
      var end = start
      while end + 1 < old.rows.count, after[old.rows[end + 1].id] == nil { end += 1 }
      block = (
        top + old.offsets[start], old.offsets[end + 1] - old.offsets[start],
        Set(old.rows[start...end].map(\.id)), false
      )
    }
    var moves: [(TranscriptRowView, CGRect)] = []
    if let block {
      let clip = UIView(
        frame: CGRect(
          x: 0, y: block.y, width: bounds.width, height: block.opening ? 0 : block.height))
      clip.clipsToBounds = true
      clip.isUserInteractionEnabled = block.opening
      scroll.addSubview(clip)
      fold = clip
      let inside = block.opening ? Array(visible.values) : Array(leaving.values)
      for row in inside where block.ids.contains(row.model?.id ?? "") {
        row.frame.origin.y -= block.y
        clip.addSubview(row)
      }
    }
    for (id, row) in visible where row.superview === scroll {
      let target = row.frame
      guard let y = shown[id]?.minY ?? before[id], abs(y - target.minY) > 0.1 else { continue }
      row.frame.origin.y = y
      moves.append((row, target))
    }
    for (id, row) in leaving {
      foldLeaving.append(row)
      if row.superview === scroll, let index = after[id] {
        moves.append((row, CGRect(origin: CGPoint(x: 0, y: top + frame.offsets[index]), size: row.frame.size)))
      }
    }
    let animator = UIViewPropertyAnimator(
      duration: UIAccessibility.isReduceMotionEnabled ? 0 : 0.28,
      controlPoint1: CGPoint(x: 0.4, y: 0), controlPoint2: CGPoint(x: 0.2, y: 1)
    ) { [fold] in
      for (row, target) in moves { row.frame = target }
      if let fold, let block { fold.frame.size.height = block.opening ? block.height : 0 }
    }
    animator.isUserInteractionEnabled = true
    animator.addCompletion { [weak self] _ in self?.finishFold() }
    foldAnimator = animator
    animator.startAnimation()
  }
  private func finishFold() {
    let animator = foldAnimator
    foldAnimator = nil
    if animator?.state == .active {
      animator?.stopAnimation(false)
      animator?.finishAnimation(at: .end)
    }
    for row in foldLeaving {
      row.reuse()
      if pool.count < 40 { pool.append(row) }
    }
    foldLeaving.removeAll()
    guard let clip = fold else { return }
    fold = nil
    for case let row as TranscriptRowView in clip.subviews where row.model != nil {
      row.frame.origin.y += clip.frame.minY
      scroll.addSubview(row)
    }
    clip.removeFromSuperview()
    displayRows()
  }

  /// Dev-only: what a Mono's chat does around a send, logged to Metro as
  /// `[transcript]`, so its scroll can be measured rather than guessed.
  private func trace(_ message: @autoclosure () -> String) {
    #if DEBUG
      guard !style.agent.isEmpty else { return }
      let ms = traceSent == 0 ? 0 : Int((CACurrentMediaTime() - traceSent) * 1000)
      let y = Int(scroll.contentOffset.y)
      let end = Int(scroll.contentSize.height - scroll.bounds.height)
      onScrollState(["log": "+\(ms)ms y=\(y) end=\(end) \(message())"])
    #endif
  }
  #if DEBUG
    private func traceRows(_ old: TranscriptLayoutFrame?, _ frame: TranscriptLayoutFrame) -> String {
      guard !style.agent.isEmpty else { return "" }
      var before: [String: CGFloat] = [:]
      for row in old?.rows ?? [] { before[row.id] = row.height }
      var changes: [String] = []
      for row in frame.rows {
        let name = "\(row.kind)#\(row.id.suffix(6))"
        if let height = before.removeValue(forKey: row.id) {
          if abs(height - row.height) > 0.1 { changes.append("~\(name) \(Int(height))→\(Int(row.height))") }
        } else {
          changes.append("+\(name) \(Int(row.height))")
        }
      }
      for (id, height) in before { changes.append("-#\(id.suffix(6)) \(Int(height))") }
      return "h \(Int(old?.height ?? 0))→\(Int(frame.height)) " + changes.prefix(8).joined(separator: ", ")
    }
  #endif

  private func updateGeometry() {
    guard let frame = frameData else { return }
    let bottomInset = bottom + extra + max(0, keyboard - keyboardOffset)
    #if DEBUG
      if abs(bottomInset - traceInset) > 0.5 {
        trace("inset \(Int(traceInset))→\(Int(bottomInset)) bottom=\(Int(bottom)) extra=\(Int(extra)) keyboard=\(Int(keyboard))")
        traceInset = bottomInset
      }
    #endif
    var height = top + frame.height + bottomInset
    if let anchor = anchorOffset(frame) {
      // Keyboard and composer growth absorb into this space rather than moving rows.
      height = max(height, anchor + scroll.bounds.height)
    }
    // A Mono's chat rests a short conversation on the composer, as a messaging
    // app's, and grows it upward: the room above is an inset, so the follow
    // slides the rows up as messages come or the keyboard opens.
    let lead =
      style.agent.isEmpty ? 0 : max(0, scroll.bounds.height - top - frame.height - bottomInset)
    if abs(scroll.contentInset.top - lead) > 0.5 { scroll.contentInset.top = lead }
    scroll.contentSize = CGSize(
      width: bounds.width, height: lead > 0 ? height : max(scroll.bounds.height, height))
    scroll.verticalScrollIndicatorInsets = UIEdgeInsets(
      top: top, left: 0, bottom: bottomInset, right: 0)
    if initialized && following && !scroll.isDragging && !scroll.isDecelerating {
      startFollowingAnimation()
    }
  }
  /// The offset that rests the latest message (and its day stamp) under the header.
  private func anchorOffset(_ frame: TranscriptLayoutFrame) -> CGFloat? {
    guard anchoring, let id = frame.lastUserID,
      var index = frame.rows.lastIndex(where: { $0.id == id })
    else { return nil }
    if index > 0 && frame.rows[index - 1].kind == "stamp" { index -= 1 }
    return frame.offsets[index] + Self.anchorLift
  }
  private func targetOffset() -> CGFloat {
    let end = max(-scroll.contentInset.top, scroll.contentSize.height - scroll.bounds.height)
    if pinned, let frame = frameData, let anchor = anchorOffset(frame) { return min(anchor, end) }
    return end
  }
  func jumpToEnd() {
    pinned = false
    following = true
    updateGeometry()
    startFollowingAnimation()
  }
  private func startFollowingAnimation() {
    guard
      active && foreground && initialized && following && !scroll.isDragging
        && !scroll.isDecelerating,
      abs(targetOffset() - scroll.contentOffset.y) > 0.5, displayLink == nil
    else { return }
    let link = CADisplayLink(
      target: displayTarget, selector: #selector(TranscriptDisplayTarget.tick(_:)))
    link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
    link.add(to: .main, forMode: .common)
    displayLink = link
    trace("follow start → \(Int(targetOffset()))")
    lastTick = 0
  }
  fileprivate func advanceFollow(_ link: CADisplayLink) {
    guard following && !scroll.isDragging && !scroll.isDecelerating else {
      stopFollowingAnimation()
      return
    }
    let target = targetOffset()
    let dt = min(0.05, lastTick == 0 ? link.duration : link.timestamp - lastTick)
    lastTick = link.timestamp
    // A critically damped spring, stepped exactly: it eases in from rest,
    // settles without overshoot, and keeps its speed when the target moves.
    let omega: CGFloat = 18
    let x = scroll.contentOffset.y - target
    let v = followVelocity
    let decay = exp(-omega * dt)
    let next = (x + (v + omega * x) * dt) * decay
    followVelocity = (v - omega * (v + omega * x) * dt) * decay
    if abs(next) < 0.5 && abs(followVelocity) < 20 {
      scroll.contentOffset.y = target
      trace("follow done")
      stopFollowingAnimation()
      return
    }
    scroll.contentOffset.y = target + next
  }
  private func stopFollowingAnimation() {
    displayLink?.invalidate()
    displayLink = nil
    lastTick = 0
    followVelocity = 0
  }

  /// Returns, rather than reuses, rows that left the screen when `holding`.
  @discardableResult
  private func displayRows(animate: Bool = false, holding: Bool = false) -> [String:
    TranscriptRowView]
  {
    guard let frame = frameData, !frame.rows.isEmpty else {
      for row in visible.values { row.reuse() }
      visible.removeAll()
      return [:]
    }
    let from = frame.index(at: max(0, scroll.contentOffset.y - top - 120))
    let to = frame.index(at: scroll.contentOffset.y + bounds.height - top + 120)
    // Only the answer under the latest message writes itself in; anything
    // above it was shown already and comes back as it was.
    let answer =
      frame.lastUserID.flatMap { id in frame.rows.lastIndex { $0.id == id } }.map { $0 + 1 } ?? 0
    var keep = Set<String>()
    for index in from...max(from, to) {
      let model = frame.rows[index]
      keep.insert(model.id)
      guard let prepared = frame.prepared[model.id] else { continue }
      let row = visible[model.id] ?? pool.popLast() ?? TranscriptRowView(frame: .zero)
      if visible[model.id] == nil {
        scroll.addSubview(row)
        visible[model.id] = row
      }
      let y = top + frame.offsets[index]
      row.frame = CGRect(
        x: -reveal,
        y: row.superview === fold ? y - (fold?.frame.minY ?? 0) : y, width: bounds.width,
        height: model.height)
      row.configure(
        model, layout: prepared, animate: animate && index >= answer,
        horizontalWidth: model.group.flatMap { frame.groupWidths[$0] } ?? 0,
        horizontalOffset: model.group.flatMap { horizontalOffsets[$0] } ?? 0)
      row.cadence = cadence
      row.setPresentationActive(active && foreground)
      row.onHorizontalScroll = { [weak self, weak row] group, offset in
        guard let self, let row else { return }
        self.horizontalOffsets[group] = offset
        for peer in self.visible.values where peer !== row && peer.model?.group == group {
          peer.setHorizontalOffset(offset)
        }
      }
      row.onAction = { [weak self] action in
        if action["type"] as? String == "toggle" {
          if let id = action["id"] as? String {
            self?.toggle = (id, action["open"] as? Bool ?? false)
          }
          self?.session?.command(action)
        } else {
          self?.onAction(action)
        }
      }
      row.onSelection = { [weak self] id in
        guard let self else { return }
        // A reply over several rows answers to its first, where a reaction sits.
        let rows = self.frameData?.rows ?? []
        var at = rows.firstIndex { $0.id == id }
        while let index = at, index > 0, rows[index].presentation["reply"] as? Bool == true,
          rows[index].presentation["replyFirst"] as? Bool != true
        { at = index - 1 }
        self.held = at.map { (rows[$0].kind, rows[$0].id) }
        self.session?.command(["type": "selection", "id": id])
      }
    }
    var held: [String: TranscriptRowView] = [:]
    defer { if !times.isEmpty { layoutReveal() } }
    for id in Array(visible.keys) where !keep.contains(id) {
      guard let row = visible.removeValue(forKey: id) else { continue }
      if holding {
        held[id] = row
      } else {
        row.reuse()
        if pool.count < 40 { pool.append(row) }
      }
    }
    return held
  }
  private func publishViewport() {
    session?.configure(
      style: style,
      viewport: initialized
        ? CGRect(
          x: 0, y: max(0, scroll.contentOffset.y - top), width: bounds.width, height: bounds.height)
        : .zero)
  }
  private func updateAway() {
    let away =
      initialized
      && (scroll.contentSize.height - scroll.bounds.height - scroll.contentOffset.y > 80)
    if away != lastAway {
      lastAway = away
      onScrollState(["away": away])
    }
  }
  func scrollViewWillBeginDragging(_ scrollView: UIScrollView) {
    // The blank space stays until the next message, so nothing jumps under the finger.
    following = false
    pinned = false
    stopFollowingAnimation()
  }
  func scrollViewDidScroll(_ scrollView: UIScrollView) {
    displayRows()
    publishViewport()
    updateAway()
    if initialized && scroll.contentOffset.y < top + 120, frameData?.older == true, !loadingOlder {
      loadingOlder = true
      session?.command(["type": "older"])
    }
  }
  /// Only a Mono's chat follows its reply down, as Transcript.tsx, and only
  /// once the reader comes to rest at the end. A drag that merely starts
  /// there is reading up, so older turns landing above keep its place.
  private func settled() {
    if !style.agent.isEmpty
      && scroll.contentSize.height - bounds.height - scroll.contentOffset.y < 70
    {
      following = true
    }
    if following { startFollowingAnimation() }
  }
  func scrollViewDidEndDecelerating(_ scrollView: UIScrollView) {
    settled()
  }
  func scrollViewDidEndDragging(_ scrollView: UIScrollView, willDecelerate decelerate: Bool) {
    if !decelerate { settled() }
  }

  private func keyboardChanged(_ notification: Notification) {
    guard let info = notification.userInfo,
      let end = info[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect, let window
    else { return }
    let rect = convert(window.convert(end, from: nil), from: window)
    let atEnd = scroll.contentSize.height - scroll.bounds.height - scroll.contentOffset.y < 70
    keyboard = rect.maxY >= bounds.maxY ? max(0, bounds.maxY - rect.minY) : 0
    let duration = info[UIResponder.keyboardAnimationDurationUserInfoKey] as? Double ?? 0.25
    trace("keyboard \(Int(keyboard)) atEnd=\(atEnd) following=\(following)")
    let curve = info[UIResponder.keyboardAnimationCurveUserInfoKey] as? UInt ?? 7
    UIView.animate(
      withDuration: duration, delay: 0,
      options: UIView.AnimationOptions(rawValue: curve << 16).union(.beginFromCurrentState)
    ) {
      self.updateGeometry()
      // Lift only when already at the end, like KeyboardChatScrollView's whenAtEnd.
      if self.following || atEnd { self.scroll.contentOffset.y = self.targetOffset() }
    }
  }
  /// A long-pressed message's text: a Mono's chat offers what to do with it
  /// in a sheet, as Messages does; elsewhere it opens to be selected.
  private func select(_ text: String) {
    guard !text.isEmpty, active else { return }
    let kind = held
    held = nil
    if !style.agent.isEmpty {
      UIImpactFeedbackGenerator(style: .medium).impactOccurred()
      onAction([
        "type": "message", "text": text, "role": kind?.kind == "user" ? "user" : "agent",
        "id": kind?.id ?? "",
      ])
      return
    }
    selectText(text)
  }
  /// The text in a sheet to select from.
  func selectText(_ text: String) {
    guard !text.isEmpty, active else { return }
    var responder: UIResponder? = self
    while let current = responder, !(current is UIViewController) { responder = current.next }
    guard let parent = responder as? UIViewController, parent.presentedViewController == nil else {
      return
    }
    let controller = UIViewController()
    let textView = UITextView()
    textView.text = text
    textView.font = .systemFont(ofSize: 16 * style.scale)
    textView.isEditable = false
    textView.isSelectable = true
    textView.backgroundColor = .transcriptHex(style.surface)
    textView.textColor = .transcriptHex(style.content)
    textView.textContainerInset = UIEdgeInsets(top: 24, left: 20, bottom: 30, right: 20)
    controller.view = textView
    controller.sheetPresentationController?.detents = [.medium(), .large()]
    controller.sheetPresentationController?.prefersGrabberVisible = true
    parent.present(controller, animated: true)
  }
  override func accessibilityScroll(_ direction: UIAccessibilityScrollDirection) -> Bool {
    let delta: CGFloat = direction == .up ? -1 : direction == .down ? 1 : 0
    guard delta != 0 else { return false }
    following = false
    pinned = false
    scroll.setContentOffset(
      CGPoint(
        x: 0,
        y: min(
          max(0, scroll.contentOffset.y + delta * bounds.height * 0.8),
          max(0, scroll.contentSize.height - bounds.height))), animated: false)
    UIAccessibility.post(notification: .pageScrolled, argument: nil)
    return true
  }
}
