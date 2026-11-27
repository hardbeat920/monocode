import UIKit

/// Turns off iOS 26's scroll edge effects on the scroll view inside the view
/// tagged `testID`, for a screen that draws its own gradual blur under its
/// header and footer. The stack only does this for a scroll view it finds
/// down its screen's first subviews, and misses some.
enum ScrollEdgeEffects {
  @MainActor static func hide(testID: String) -> Bool {
    guard #available(iOS 26.0, *) else { return true }
    let windows = UIApplication.shared.connectedScenes
      .compactMap { $0 as? UIWindowScene }
      .flatMap { $0.windows }
    func tagged(_ view: UIView) -> UIView? {
      if view.accessibilityIdentifier == testID { return view }
      for sub in view.subviews { if let found = tagged(sub) { return found } }
      return nil
    }
    func scroll(in view: UIView) -> UIScrollView? {
      if let scroll = view as? UIScrollView { return scroll }
      for sub in view.subviews { if let found = scroll(in: sub) { return found } }
      return nil
    }
    for window in windows {
      guard let host = tagged(window), let list = scroll(in: host) else { continue }
      list.topEdgeEffect.isHidden = true
      list.bottomEdgeEffect.isHidden = true
      return true
    }
    return false
  }
}
