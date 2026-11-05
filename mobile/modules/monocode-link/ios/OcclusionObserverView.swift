import ExpoModulesCore
import UIKit

/// Measures the actual floating control, not the full-height safe-area strip.
final class OcclusionObserverView: ExpoView {
  let onOcclusionChange = EventDispatcher()
  private var lastValue = ""
  private var keyboardMoving = false
  private var sampledBounds = CGRect.null
  private var boundsUpdateScheduled = false

  required init(appContext: AppContext? = nil) {
    super.init(appContext: appContext)
    isUserInteractionEnabled = false
    NotificationCenter.default.addObserver(self, selector: #selector(keyboardWillMove), name: UIResponder.keyboardWillChangeFrameNotification, object: nil)
    NotificationCenter.default.addObserver(self, selector: #selector(keyboardDidMove), name: UIResponder.keyboardDidChangeFrameNotification, object: nil)
  }

  deinit { NotificationCenter.default.removeObserver(self) }

  override func didMoveToWindow() {
    super.didMoveToWindow()
    setNeedsUpdateConstraints()
  }

  override func layoutSubviews() {
    super.layoutSubviews()
    guard bounds != sampledBounds else { return }
    sampledBounds = bounds
    guard !boundsUpdateScheduled else { return }
    boundsUpdateScheduled = true
    // Never dirty constraints from the layout pass itself. A new control
    // size needs one deferred sample; unchanged layout needs no extra pass.
    DispatchQueue.main.async { [weak self] in
      guard let self else { return }
      self.boundsUpdateScheduled = false
      self.setNeedsUpdateConstraints()
      self.updateConstraintsIfNeeded()
    }
  }

  override func safeAreaInsetsDidChange() {
    super.safeAreaInsetsDidChange()
    setNeedsUpdateConstraints()
  }

  @objc private func keyboardWillMove() {
    // Reanimated moves our ancestor without laying it out. Keep conservative
    // clearance during that motion, then query in the settled coordinates.
    keyboardMoving = true
    setNeedsUpdateConstraints()
  }

  @objc private func keyboardDidMove() {
    DispatchQueue.main.async { [weak self] in
      self?.keyboardMoving = false
      self?.setNeedsUpdateConstraints()
    }
  }

  override func updateConstraints() {
    defer { super.updateConstraints() }
    guard window != nil, bounds.width > 0, bounds.height > 0 else { return }
    var frames: [[String: Double]] = []
    var supported = false
    if #available(iOS 27.1, *), !keyboardMoving {
      supported = true
      // Reading here lets UIKit invalidate constraints when an occlusion
      // activates, grows, moves, or disappears, including Live Activities.
      frames = reservedRegions(kind: .occlusion).map { region in
        let r = region.frame
        // UIKit's frame already includes the margins for interactive content.
        return ["x": Double(r.minX), "y": Double(r.minY),
                "width": Double(r.width), "height": Double(r.height)]
      }
    }
    let payload: [String: Any] = ["supported": supported, "width": Double(bounds.width),
                                  "height": Double(bounds.height), "regions": frames]
    // Avoid re-rendering the JS control when its occupied space hasn't changed.
    let value = String(data: (try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])) ?? Data(), encoding: .utf8) ?? ""
    guard value != lastValue else { return }
    lastValue = value
    onOcclusionChange(payload)
  }
}
