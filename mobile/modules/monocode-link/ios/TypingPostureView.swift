import ExpoModulesCore
import UIKit

/// Observes the hinge belonging to this window, without changing the layout.
final class TypingPostureView: ExpoView {
  let onEnterTypingPosture = EventDispatcher()
  var enabled = false
  private var halfFolded = false
  private var inTypingPosture = false

  required init(appContext: AppContext? = nil) {
    super.init(appContext: appContext)
    isUserInteractionEnabled = false
    if #available(iOS 27.1, *) {
      addInteraction(UIHingeInteraction { [weak self] _, update in
        guard let self, let hinge = update.hinge else { return }
        switch hinge.status {
        case .partiallyOpen:
          // A slight bend is also "partially open". Enter around a 120-degree
          // opening and leave above 150 degrees, so small adjustments cannot
          // rearm a dismissed keyboard near the entry threshold.
          if hinge.angle <= .pi * 2 / 3 {
            self.halfFolded = true
          } else if hinge.angle >= .pi * 5 / 6 {
            self.halfFolded = false
          }
        case .fullyOpen, .closed:
          self.halfFolded = false
          self.inTypingPosture = false
        default:
          // Unknown or temporarily unavailable geometry is not a new fold.
          return
        }
        self.updatePosture()
      })
    }
  }

  override func didMoveToWindow() {
    super.didMoveToWindow()
    updatePosture()
  }

  // The observer fills the composer, so window rotation lays it out again.
  override func layoutSubviews() {
    super.layoutSubviews()
    updatePosture()
  }

  func updatePosture() {
    // UIKit may detach/reparent this view while the keyboard or layout changes.
    // Keep the latch across that transition; only a real posture exit rearms it.
    guard let window else { return }
    let portrait = window.windowScene?.interfaceOrientation.isPortrait == true
    let typing = halfFolded && portrait
    guard typing != inTypingPosture else { return }
    inTypingPosture = typing
    // Emit only on entry: dismissing the keyboard stays respected until the
    // user leaves this pose and enters it again, rather than on every angle.
    // Entry while disabled is also consumed: reconnecting, finishing a send,
    // or returning from a sheet must not unexpectedly bring up the keyboard.
    if typing && enabled && canRequestFocus { onEnterTypingPosture([:]) }
  }

  private var canRequestFocus: Bool {
    guard window?.windowScene?.activationState == .foregroundActive else { return false }
    var responder: UIResponder? = self
    while let current = responder {
      if let view = current as? UIView, view.isHidden || view.alpha == 0 { return false }
      // Covers UIKit/SwiftUI sheets, menus, and React Native Modal without
      // threading a separate overlay flag through every composer action.
      if let controller = current as? UIViewController,
        controller.presentedViewController != nil { return false }
      responder = current.next
    }
    return true
  }
}
