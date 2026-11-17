import UIKit
import UIKit.UIGestureRecognizerSubclass
import WebKit

/// The page's reports. Kept apart from the card, since a web view's script
/// handlers hold on to whatever they are given.
private final class DraftPageMessages: NSObject, WKScriptMessageHandler {
  weak var card: TranscriptDraftCard?
  func userContentController(
    _ controller: WKUserContentController, didReceive message: WKScriptMessage
  ) {
    // Handlers reach every frame; the page's own sandboxed frame is not the app.
    guard message.frameInfo.isMainFrame, let text = message.body as? String else { return }
    card?.pageSaid(text)
  }
}

/// Sees each touch on the page without taking it, so a report from the page
/// can be checked against the person's own hand.
private final class DraftTouchWatcher: UIGestureRecognizer, UIGestureRecognizerDelegate {
  var onTouch: ((_ began: Bool, _ y: CGFloat) -> Void)?
  private var heldPans: [UIGestureRecognizer] = []
  override init(target: Any?, action: Selector?) {
    super.init(target: target, action: action)
    cancelsTouchesInView = false
    delaysTouchesBegan = false
    delaysTouchesEnded = false
    delegate = self
  }
  deinit { releasePans() }
  override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
    if let touch = touches.first { holdPans(over: touch) }
    onTouch?(true, touches.first?.location(in: view).y ?? 0)
  }
  override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
    onTouch?(false, touches.first?.location(in: view).y ?? 0)
  }
  override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
    onTouch?(false, touches.first?.location(in: view).y ?? 0)
    state = .failed
  }
  override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) {
    state = .failed
  }
  override func reset() {
    super.reset()
    releasePans()
  }
  func gestureRecognizer(
    _ gesture: UIGestureRecognizer,
    shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer
  ) -> Bool { true }

  /// Since iOS 26 a screen swipes back from anywhere on it: that pan sits above
  /// the transcript and doesn't know a slider from the thread, so dragging one
  /// left the thread. It sits out this touch; the scroller's own pans keep it,
  /// so the thread still scrolls under the finger.
  private func holdPans(over touch: UITouch) {
    guard heldPans.isEmpty else { return }
    var ancestor = view?.superview
    while let current = ancestor, !(current is TranscriptScrollView) {
      ancestor = current.superview
    }
    guard let scroller = ancestor else { return }
    heldPans = (touch.gestureRecognizers ?? []).filter { gesture in
      guard gesture is UIPanGestureRecognizer, gesture.isEnabled,
        let owner = gesture.view, owner !== scroller
      else { return false }
      return scroller.isDescendant(of: owner)
    }
    heldPans.forEach { $0.isEnabled = false }
  }

  private func releasePans() {
    heldPans.forEach { $0.isEnabled = true }
    heldPans = []
  }
}

/// The transcript's scroller. A sideways drag on a page belongs to the page,
/// where a slider or a carousel is waiting for it; anything else scrolls.
final class TranscriptScrollView: UIScrollView {
  override func gestureRecognizerShouldBegin(_ recognizer: UIGestureRecognizer) -> Bool {
    if recognizer === panGestureRecognizer {
      let velocity = panGestureRecognizer.velocity(in: self)
      if abs(velocity.x) > abs(velocity.y) {
        var hit = hitTest(panGestureRecognizer.location(in: self), with: nil)
        while let view = hit, view !== self {
          if let card = view as? TranscriptDraftCard, card.holdsPage(at: panGestureRecognizer) {
            return false
          }
          hit = view.superview
        }
      }
    }
    return super.gestureRecognizerShouldBegin(recognizer)
  }
}

/// A draft in the thread: the agent's page, live at its place in the
/// conversation, with the desktop card's controls. The transcript owns these,
/// because its rows are recycled and a page must outlive the row showing it.
///
/// What the page may do and what crosses to the desktop is decided in
/// `DraftController.ts`, which drives this view; here is only the view.
final class TranscriptDraftCard: UIView, WKNavigationDelegate {
  /// How tall a card is before anything says otherwise.
  static let estimate: CGFloat = 280
  /// The page's height is followed up to this, as on the desktop.
  private static let tallest: CGFloat = 12000
  private static let headerHeight: CGFloat = 44
  private static let footerHeight: CGFloat = 48
  private static let pausedHeight: CGFloat = 120
  private static let closedHeight: CGFloat = 56

  let id: String
  var action: [String: Any]
  var onEvent: (([String: Any]) -> Void)?
  var onHeight: ((CGFloat) -> Void)?
  /// Whether the thread is being dragged, which is no one using the page.
  var isThreadDragging: () -> Bool = { false }

  private var style: TranscriptStyle?
  private var chrome: [String: Any] = ["mode": "loading"]
  private let header = UIView()
  private let title = UILabel()
  private let comment = UIButton(type: .system)
  private let expand = UIButton(type: .system)
  private let close = UIButton(type: .system)
  private let body = UIView()
  private let cover = UIImageView()
  private let spinner = UIActivityIndicatorView(style: .medium)
  private let footer = UIView()
  private let status = UILabel()
  private let dot = UIView()
  private let send = UIButton(type: .system)
  private let handBack = UIButton(type: .system)
  private let open = UIButton(type: .system)
  private var web: WKWebView?
  private let messages = DraftPageMessages()
  private var html: String?
  private var generation = 0
  private var pageHeight: CGFloat?
  private var pictureRatio: CGFloat?
  /// Holding the previous look until the new page has its height.
  private var revealing = false
  private var fullView: DraftFullViewController?
  private var touchedAt: Double = 0
  private var touchBefore: Double = 0
  private(set) var lastTouchY: CGFloat = 0

  init(id: String, action: [String: Any]) {
    self.id = id
    self.action = action
    super.init(frame: .zero)
    messages.card = self
    clipsToBounds = true
    layer.cornerRadius = 16
    layer.cornerCurve = .continuous
    title.font = .systemFont(ofSize: 13, weight: .medium)
    title.lineBreakMode = .byTruncatingTail
    configure(comment, symbol: "text.bubble", label: "Comment", #selector(pressedComment))
    configure(expand, symbol: "arrow.up.left.and.arrow.down.right", label: "Open full view",
      #selector(pressedExpand))
    configure(close, symbol: "xmark", label: "Close draft", #selector(pressedClose))
    for view in [title, comment, expand, close] as [UIView] { header.addSubview(view) }
    body.clipsToBounds = true
    cover.contentMode = .scaleAspectFill
    cover.clipsToBounds = true
    cover.isHidden = true
    cover.isUserInteractionEnabled = false
    spinner.hidesWhenStopped = true
    body.addSubview(cover)
    body.addSubview(spinner)
    status.font = .systemFont(ofSize: 12)
    status.lineBreakMode = .byTruncatingTail
    dot.layer.cornerRadius = 3
    configurePill(send, #selector(pressedSend))
    configurePill(handBack, #selector(pressedHandBack))
    handBack.setTitle("Hand back", for: .normal)
    configurePill(open, #selector(pressedOpen))
    for view in [dot, status, send, handBack, open] as [UIView] { footer.addSubview(view) }
    for view in [header, body, footer] as [UIView] { addSubview(view) }
    isAccessibilityElement = false
  }
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
  deinit {
    web?.configuration.userContentController.removeScriptMessageHandler(forName: "mcPage")
  }

  private func configure(_ button: UIButton, symbol: String, label: String, _ action: Selector) {
    let image = UIImage(
      systemName: symbol, withConfiguration: UIImage.SymbolConfiguration(pointSize: 14))
    button.setImage(image, for: .normal)
    button.accessibilityLabel = label
    button.layer.cornerRadius = 8
    button.addTarget(self, action: action, for: .touchUpInside)
  }
  private func configurePill(_ button: UIButton, _ action: Selector) {
    button.titleLabel?.font = .systemFont(ofSize: 12, weight: .semibold)
    button.layer.cornerRadius = 16
    button.contentEdgeInsets = UIEdgeInsets(top: 0, left: 12, bottom: 0, right: 12)
    button.addTarget(self, action: action, for: .touchUpInside)
  }

  // MARK: What the transcript tells it

  func apply(style next: TranscriptStyle) {
    guard style != next else { return }
    style = next
    let content = UIColor.transcriptHex(next.content)
    let muted = UIColor.transcriptHex(next.muted)
    backgroundColor = content.withAlphaComponent(0.04)
    layer.borderColor = UIColor.transcriptHex(next.stroke).cgColor
    layer.borderWidth = 1 / max(1, next.pixelRatio)
    title.textColor = content
    status.textColor = muted
    dot.backgroundColor = UIColor.transcriptHex(next.accent)
    for button in [comment, expand, close] { button.tintColor = muted }
    for button in [handBack, open] {
      button.backgroundColor = content.withAlphaComponent(0.08)
      button.setTitleColor(content, for: .normal)
    }
    send.backgroundColor = UIColor.transcriptHex(next.accent)
    send.setTitleColor(.white, for: .normal)
    spinner.color = muted
    update()
  }

  func setChrome(_ next: [String: Any]) {
    chrome = next
    update()
  }

  private var mode: String { chrome["mode"] as? String ?? "loading" }
  private var hasFooter: Bool {
    switch mode {
    case "page":
      return chrome["status"] is String || (chrome["notes"] as? Int ?? 0) > 0
        || chrome["handBack"] as? Bool == true
    case "frame": return true
    default: return false
    }
  }

  /// How tall the page's place is now.
  private var bodyHeight: CGFloat {
    let width = max(1, bounds.width)
    switch mode {
    case "paused": return Self.pausedHeight
    case "closed": return 0
    case "frame":
      return pictureRatio.map { min(Self.tallest, width * $0) } ?? Self.estimate
    default:
      return pageHeight ?? (chrome["height"] as? CGFloat) ?? Self.estimate
    }
  }
  private var cardHeight: CGFloat {
    if mode == "closed" { return Self.closedHeight }
    return Self.headerHeight + bodyHeight + (hasFooter ? Self.footerHeight : 0)
  }

  private func update() {
    title.text = chrome["title"] as? String ?? action["title"] as? String
    title.accessibilityLabel = title.text
    let page = mode == "page"
    let closed = mode == "closed"
    header.isHidden = closed
    comment.isHidden = !page
    expand.isHidden = !page
    close.isHidden = !page && mode != "frame" && mode != "paused"
    let commenting = chrome["commenting"] as? Bool == true
    comment.tintColor =
      commenting ? UIColor.transcriptHex(style?.accent ?? "#4c9ef5") : style.map { .transcriptHex($0.muted) }
    comment.accessibilityTraits = commenting ? [.button, .selected] : .button
    comment.backgroundColor =
      commenting ? UIColor.transcriptHex(style?.accent ?? "#4c9ef5").withAlphaComponent(0.16) : .clear
    let notes = chrome["notes"] as? Int ?? 0
    send.isHidden = notes == 0
    send.setTitle("Send \(notes) \(notes == 1 ? "note" : "notes")", for: .normal)
    handBack.isHidden = chrome["handBack"] as? Bool != true
    dot.isHidden = chrome["waiting"] as? Bool != true
    status.text = chrome["status"] as? String
    let opening = chrome["open"] as? String
    open.isHidden = opening == nil
    open.setTitle(opening, for: .normal)
    footer.isHidden = !hasFooter && !closed && mode != "paused"
    if mode == "loading" {
      spinner.startAnimating()
    } else {
      spinner.stopAnimating()
    }
    web?.isHidden = !page
    if mode == "frame" {
      cover.isHidden = cover.image == nil
    } else if !page {
      cover.isHidden = true
    }
    fullView?.update(title: title.text ?? "", commenting: commenting)
    // The page went away under the full view, so there is nothing left in it.
    if !page, mode != "loading" { fullView?.leave() }
    setNeedsLayout()
    onHeight?(cardHeight)
  }

  override func layoutSubviews() {
    super.layoutSubviews()
    let width = bounds.width
    header.frame = CGRect(x: 0, y: 0, width: width, height: Self.headerHeight)
    var right = width - 6
    for button in [close, expand, comment] where !button.isHidden {
      button.frame = CGRect(x: right - 32, y: 6, width: 32, height: 32)
      right -= 34
    }
    title.frame = CGRect(x: 14, y: 0, width: max(1, right - 18), height: Self.headerHeight)
    let closed = mode == "closed"
    let height = bodyHeight
    body.frame = CGRect(x: 0, y: Self.headerHeight, width: width, height: height)
    if let web, fullView == nil {
      web.frame = CGRect(x: 0, y: 0, width: width, height: max(1, height))
    }
    cover.frame = body.bounds
    spinner.center = CGPoint(x: body.bounds.midX, y: body.bounds.midY)
    if mode == "paused" {
      footer.frame = CGRect(x: 0, y: Self.headerHeight, width: width, height: height)
      layoutPill(open, centeredIn: footer.bounds)
      return
    }
    footer.frame =
      closed
      ? bounds
      : CGRect(
        x: 0, y: Self.headerHeight + height, width: width, height: Self.footerHeight)
    let rowHeight = footer.bounds.height
    var edge = width - 8
    for button in [handBack, send, open] where !button.isHidden {
      let size = button.sizeThatFits(CGSize(width: width, height: 32))
      button.frame = CGRect(
        x: edge - size.width, y: (rowHeight - 32) / 2, width: size.width, height: 32)
      edge -= size.width + 6
    }
    var left: CGFloat = 14
    if !dot.isHidden {
      dot.frame = CGRect(x: left, y: (rowHeight - 6) / 2, width: 6, height: 6)
      left += 12
    }
    status.frame = CGRect(x: left, y: 0, width: max(1, edge - left - 4), height: rowHeight)
  }
  private func layoutPill(_ button: UIButton, centeredIn area: CGRect) {
    let size = button.sizeThatFits(CGSize(width: area.width, height: 36))
    button.frame = CGRect(
      x: area.midX - size.width / 2, y: area.midY - 18, width: size.width, height: 36)
    status.frame = .zero
  }

  // MARK: The page

  func load(html: String, generation: Int) {
    let replacing = self.html != nil && web?.window != nil
    self.html = html
    self.generation = generation
    let web = web ?? makeWeb()
    revealing = true
    web.isHidden = mode != "page"
    guard replacing else {
      web.loadHTMLString(html, baseURL: nil)
      return
    }
    // The old version stays on screen until the new one knows its height.
    web.takeSnapshot(with: nil) { [weak self] image, _ in
      guard let self, self.generation == generation, let page = self.web else { return }
      if let image {
        self.cover.image = image
        self.cover.alpha = 1
        self.cover.isHidden = false
        self.body.bringSubviewToFront(self.cover)
      }
      page.loadHTMLString(html, baseURL: nil)
    }
  }

  private func makeWeb() -> WKWebView {
    let configuration = WKWebViewConfiguration()
    configuration.websiteDataStore = .nonPersistent()
    configuration.allowsInlineMediaPlayback = true
    let content = configuration.userContentController
    // The page's wrapper speaks to the phone by this name, on every platform.
    content.addUserScript(
      WKUserScript(
        source:
          "window.ReactNativeWebView={postMessage:function(m){webkit.messageHandlers.mcPage.postMessage(String(m))}};",
        injectionTime: .atDocumentStart, forMainFrameOnly: true))
    content.add(messages, name: "mcPage")
    let web = WKWebView(frame: body.bounds, configuration: configuration)
    web.isOpaque = false
    web.backgroundColor = .clear
    web.scrollView.backgroundColor = .clear
    // The card is as tall as the page: the thread does the scrolling.
    web.scrollView.isScrollEnabled = false
    web.scrollView.contentInsetAdjustmentBehavior = .never
    web.allowsLinkPreview = false
    web.navigationDelegate = self
    let watcher = DraftTouchWatcher(target: nil, action: nil)
    watcher.onTouch = { [weak self] began, y in self?.touched(began: began, y: y) }
    web.addGestureRecognizer(watcher)
    body.insertSubview(web, at: 0)
    self.web = web
    return web
  }

  private func touched(began: Bool, y: CGFloat) {
    let now = Date().timeIntervalSince1970 * 1000
    if began { touchBefore = touchedAt }
    lastTouchY = y
    if !isThreadDragging() { touchedAt = now }
  }

  /// The thread took this touch: it was a scroll, not the person using the
  /// page, whatever the page does in the moment it takes to tell them apart.
  func threadTookTouch() {
    guard touchedAt != touchBefore else { return }
    touchedAt = touchBefore
    onEvent?(["kind": "touchTaken"])
  }

  func script(_ source: String) {
    web?.evaluateJavaScript(source, completionHandler: nil)
  }

  func pageSaid(_ text: String) {
    if let data = text.data(using: .utf8),
      let message = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
      message["k"] as? String == "h"
    {
      if let height = message["v"] as? Double { pageHeightChanged(CGFloat(height)) }
      return
    }
    onEvent?(["kind": "message", "data": text, "touchedAt": touchedAt])
  }

  private func pageHeightChanged(_ height: CGFloat) {
    // In the full view the page fills the screen and says nothing of the card.
    guard fullView == nil, height > 0 else { return }
    let next = min(Self.tallest, ceil(height))
    let changed = pageHeight != next
    pageHeight = next
    if changed {
      setNeedsLayout()
      onHeight?(cardHeight)
    }
    revealIfReady()
  }

  private func revealIfReady() {
    guard revealing, pageHeight != nil, web?.isLoading == false else { return }
    revealing = false
    guard !cover.isHidden, mode == "page" else { return }
    UIView.animate(
      withDuration: 0.2,
      animations: { self.cover.alpha = 0 },
      completion: { _ in
        guard self.mode == "page", !self.revealing else { return }
        self.cover.isHidden = true
        self.cover.image = nil
      })
  }

  func setPicture(_ uri: String?, ratio: CGFloat?) {
    guard let uri, let comma = uri.firstIndex(of: ","),
      let data = Data(base64Encoded: String(uri[uri.index(after: comma)...])),
      let image = UIImage(data: data)
    else {
      cover.image = nil
      cover.isHidden = true
      pictureRatio = nil
      update()
      return
    }
    cover.image = image
    cover.alpha = 1
    cover.isHidden = false
    body.bringSubviewToFront(cover)
    pictureRatio = ratio ?? (image.size.height / max(1, image.size.width))
    update()
  }

  /// The card left the screen: the page stops running. It keeps its place in
  /// memory until the transcript needs the room.
  func suspend() {
    guard fullView == nil, let web, web.superview != nil else { return }
    web.pauseAllMediaPlayback(completionHandler: nil)
    web.removeFromSuperview()
  }

  /// The card is back on screen.
  func resume() {
    guard let web, web.superview == nil else { return }
    body.insertSubview(web, at: 0)
    web.isHidden = mode != "page"
    setNeedsLayout()
  }

  var holdsLoadedPage: Bool { web != nil }
  var isFull: Bool { fullView != nil }

  /// Gives up the page, for good; `lost` tells the controller to fetch it again.
  func dropPage() {
    guard let web, fullView == nil else { return }
    web.configuration.userContentController.removeScriptMessageHandler(forName: "mcPage")
    web.stopLoading()
    web.removeFromSuperview()
    self.web = nil
    html = nil
    revealing = false
    onEvent?(["kind": "lost"])
  }

  /// Whether a drag this gesture started is on the live page, not the card's
  /// own header or footer.
  func holdsPage(at gesture: UIGestureRecognizer) -> Bool {
    guard mode == "page", web != nil else { return false }
    return body.bounds.contains(gesture.location(in: body))
  }

  /// Where the person last touched the page, in `view`'s coordinates.
  func touchPoint(in view: UIView) -> CGPoint? {
    web.map { $0.convert(CGPoint(x: 0, y: lastTouchY), to: view) }
  }

  var isEditing: Bool {
    guard let web else { return false }
    func editing(_ view: UIView) -> Bool {
      view.isFirstResponder || view.subviews.contains(where: editing)
    }
    return editing(web)
  }

  // MARK: Navigation

  func webView(
    _ webView: WKWebView, decidePolicyFor navigation: WKNavigationAction,
    decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
  ) {
    // The page and its sandboxed frame load from memory; nothing navigates.
    guard let url = navigation.request.url, url.scheme != "about" else {
      return decisionHandler(.allow)
    }
    decisionHandler(.cancel)
    if navigation.navigationType == .linkActivated, ["http", "https"].contains(url.scheme ?? "") {
      UIApplication.shared.open(url)
    }
  }
  func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
    onEvent?(["kind": "loaded", "generation": generation])
    revealIfReady()
    // A page that never reports its height still gets seen.
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
      guard let self, self.revealing else { return }
      self.pageHeight = self.pageHeight ?? self.bodyHeight
      self.revealIfReady()
    }
  }
  func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
    guard let html else { return }
    onEvent?(["kind": "loadStart"])
    revealing = true
    webView.loadHTMLString(html, baseURL: nil)
  }

  // MARK: The person's presses

  private func press(_ button: String) { onEvent?(["kind": "press", "button": button]) }
  @objc private func pressedComment() { press("comment") }
  @objc private func pressedClose() { press("close") }
  @objc private func pressedSend() {
    UIImpactFeedbackGenerator(style: .light).impactOccurred()
    press("send")
  }
  @objc private func pressedHandBack() { press("handBack") }
  @objc private func pressedOpen() { press("open") }
  @objc private func pressedExpand() { openFullView() }
  fileprivate func pressedFullComment() { press("comment") }

  /// The same page moves into a full view, so nothing it keeps to itself is
  /// lost; `closeFullView` brings it back.
  private func openFullView() {
    guard let web, fullView == nil, var top = window?.rootViewController else { return }
    while let next = top.presentedViewController { top = next }
    let view = DraftFullViewController(card: self, page: web)
    view.update(title: title.text ?? "", commenting: chrome["commenting"] as? Bool == true)
    fullView = view
    top.present(view, animated: true)
  }
  fileprivate func fullViewClosed(page: WKWebView) {
    fullView = nil
    body.insertSubview(page, at: 0)
    setNeedsLayout()
  }
}

/// A draft over the whole screen, with the card's page moved into it. A page
/// taller than the screen scrolls here, as it does in the desktop's full view.
final class DraftFullViewController: UIViewController {
  private weak var card: TranscriptDraftCard?
  private let page: WKWebView
  private let bar = UIView()
  private let name = UILabel()
  private let closeButton = UIButton(type: .system)
  private let commentButton = UIButton(type: .system)

  fileprivate init(card: TranscriptDraftCard, page: WKWebView) {
    self.card = card
    self.page = page
    super.init(nibName: nil, bundle: nil)
    modalPresentationStyle = .fullScreen
  }
  required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }

  override func viewDidLoad() {
    super.viewDidLoad()
    view.backgroundColor = .systemBackground
    name.font = .systemFont(ofSize: 15, weight: .semibold)
    name.textAlignment = .center
    closeButton.setImage(UIImage(systemName: "xmark"), for: .normal)
    closeButton.accessibilityLabel = "Back to the thread"
    closeButton.addTarget(self, action: #selector(leave), for: .touchUpInside)
    commentButton.setImage(UIImage(systemName: "text.bubble"), for: .normal)
    commentButton.accessibilityLabel = "Comment"
    commentButton.addTarget(self, action: #selector(commented), for: .touchUpInside)
    page.removeFromSuperview()
    page.isHidden = false
    page.scrollView.isScrollEnabled = true
    for item in [bar, name, closeButton, commentButton, page] as [UIView] {
      item.translatesAutoresizingMaskIntoConstraints = false
    }
    view.addSubview(bar)
    view.addSubview(page)
    bar.addSubview(closeButton)
    bar.addSubview(name)
    bar.addSubview(commentButton)
    NSLayoutConstraint.activate([
      bar.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
      bar.leadingAnchor.constraint(equalTo: view.leadingAnchor),
      bar.trailingAnchor.constraint(equalTo: view.trailingAnchor),
      bar.heightAnchor.constraint(equalToConstant: 44),
      closeButton.leadingAnchor.constraint(equalTo: bar.leadingAnchor, constant: 8),
      closeButton.centerYAnchor.constraint(equalTo: bar.centerYAnchor),
      closeButton.widthAnchor.constraint(equalToConstant: 44),
      closeButton.heightAnchor.constraint(equalToConstant: 44),
      commentButton.trailingAnchor.constraint(equalTo: bar.trailingAnchor, constant: -8),
      commentButton.centerYAnchor.constraint(equalTo: bar.centerYAnchor),
      commentButton.widthAnchor.constraint(equalToConstant: 44),
      commentButton.heightAnchor.constraint(equalToConstant: 44),
      name.centerXAnchor.constraint(equalTo: bar.centerXAnchor),
      name.centerYAnchor.constraint(equalTo: bar.centerYAnchor),
      name.leadingAnchor.constraint(greaterThanOrEqualTo: closeButton.trailingAnchor),
      name.trailingAnchor.constraint(lessThanOrEqualTo: commentButton.leadingAnchor),
      page.topAnchor.constraint(equalTo: bar.bottomAnchor),
      page.leadingAnchor.constraint(equalTo: view.leadingAnchor),
      page.trailingAnchor.constraint(equalTo: view.trailingAnchor),
      page.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor),
    ])
  }

  func update(title: String, commenting: Bool) {
    name.text = title
    commentButton.tintColor = commenting ? view.tintColor : .secondaryLabel
    commentButton.accessibilityTraits = commenting ? [.button, .selected] : .button
  }

  @objc private func commented() { card?.pressedFullComment() }
  @objc func leave() {
    guard presentingViewController != nil, !isBeingDismissed else { return }
    dismiss(animated: true) { [card, page] in
      page.removeFromSuperview()
      page.translatesAutoresizingMaskIntoConstraints = true
      page.scrollView.isScrollEnabled = false
      card?.fullViewClosed(page: page)
    }
  }
}
