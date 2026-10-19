import ExpoModulesCore
import UIKit

private final class TranscriptDisplayTarget: NSObject {
  weak var view: NativeTranscriptView?
  @objc func tick(_ link: CADisplayLink) { view?.advanceFollow(link) }
}

final class NativeTranscriptView: ExpoView, UIScrollViewDelegate {
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
    if let rects = input["agentRects"],
      let data = try? JSONSerialization.data(withJSONObject: rects)
    {
      style.agentRects = (try? JSONDecoder().decode([TranscriptPixel].self, from: data)) ?? []
    }
    for key in ["content", "muted", "accent", "bubble", "surface", "error", "rail", "agent"] {
      guard let value = input[key] as? String else { continue }
      switch key {
      case "content": style.content = value
      case "muted": style.muted = value
      case "accent": style.accent = value
      case "bubble": style.bubble = value
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
      if frame.status["error"] is String { spinner.stopAnimating() }
      return
    }
    spinner.stopAnimating()
    let changed = old?.version != frame.version
    let oldIndex = old?.index(at: max(0, scroll.contentOffset.y - top)) ?? 0
    let anchorID = old.flatMap { $0.rows.isEmpty ? nil : $0.rows[oldIndex].id }
    let delta =
      old.map { scroll.contentOffset.y - top - $0.offsets[min(oldIndex, $0.rows.count)] } ?? 0
    let newPrompt = frame.lastUserID != old?.lastUserID && frame.lastUserID != nil
    let firstLive = frame.status["ready"] as? Bool == true && old?.status["ready"] as? Bool != true
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
      stopFollowingAnimation()
      startFollowingAnimation()
    } else if changed && !following, let anchorID,
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
    }
    publishViewport()
    updateAway()
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

  private func updateGeometry() {
    guard let frame = frameData else { return }
    let bottomInset = bottom + extra + max(0, keyboard - keyboardOffset)
    var height = top + frame.height + bottomInset
    if let anchor = anchorOffset(frame) {
      // Keyboard and composer growth absorb into this space rather than moving rows.
      height = max(height, anchor + scroll.bounds.height)
    }
    scroll.contentSize = CGSize(width: bounds.width, height: max(scroll.bounds.height, height))
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
    let end = max(0, scroll.contentSize.height - scroll.bounds.height)
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
    let distance = target - scroll.contentOffset.y
    if abs(distance) < 0.5 {
      scroll.contentOffset.y = target
      stopFollowingAnimation()
      return
    }
    scroll.contentOffset.y += distance * (1 - exp(-20 * dt))
  }
  private func stopFollowingAnimation() {
    displayLink?.invalidate()
    displayLink = nil
    lastTick = 0
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
        x: 0, y: row.superview === fold ? y - (fold?.frame.minY ?? 0) : y, width: bounds.width,
        height: model.height)
      row.configure(
        model, layout: prepared, animate: animate,
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
      row.onSelection = { [weak self] id in self?.session?.command(["type": "selection", "id": id])
      }
    }
    var held: [String: TranscriptRowView] = [:]
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
    // Only a Mono's chat follows its reply down, as Transcript.tsx.
    if initialized && scroll.isDragging && !style.agent.isEmpty
      && scroll.contentSize.height - bounds.height - scroll.contentOffset.y < 70
    {
      following = true
    }
    if initialized && scroll.contentOffset.y < top + 120, frameData?.older == true, !loadingOlder {
      loadingOlder = true
      session?.command(["type": "older"])
    }
  }
  func scrollViewDidEndDecelerating(_ scrollView: UIScrollView) {
    if following { startFollowingAnimation() }
  }
  func scrollViewDidEndDragging(_ scrollView: UIScrollView, willDecelerate decelerate: Bool) {
    if following && !decelerate { startFollowingAnimation() }
  }

  private func keyboardChanged(_ notification: Notification) {
    guard let info = notification.userInfo,
      let end = info[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect, let window
    else { return }
    let rect = convert(window.convert(end, from: nil), from: window)
    let atEnd = scroll.contentSize.height - scroll.bounds.height - scroll.contentOffset.y < 70
    keyboard = rect.maxY >= bounds.maxY ? max(0, bounds.maxY - rect.minY) : 0
    let duration = info[UIResponder.keyboardAnimationDurationUserInfoKey] as? Double ?? 0.25
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
  private func select(_ text: String) {
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
