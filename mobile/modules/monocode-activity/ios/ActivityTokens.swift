import ActivityKit
import ExpoModulesCore
import Foundation

/// Starts watching the Live Activity tokens as the app launches, including a
/// launch in the background when the desktop's push starts an activity.
public class ActivityTokensAppDelegate: ExpoAppDelegateSubscriber {
  public func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
  ) -> Bool {
    ActivityTokens.start()
    return true
  }
}

/// Tells the desktop where Apple delivers this phone's Live Activity pushes:
/// the token that starts one (iOS 17.2+) and the running one's. The desktop
/// sends them only once it has an APNs key; without one they go unused.
enum ActivityTokens {
  @MainActor private static var started = false
  @MainActor private static var watched = Set<String>()

  @MainActor static func start() {
    guard !started else { return }
    started = true
    if #available(iOS 17.2, *) {
      Task {
        for await token in Activity<MonocodeActivityAttributes>.pushToStartTokenUpdates {
          await TokenReporter.shared.report(start: hex(token))
        }
      }
    }
    Task { @MainActor in
      for await activity in Activity<MonocodeActivityAttributes>.activityUpdates {
        watch(activity)
      }
    }
    for activity in Activity<MonocodeActivityAttributes>.activities { watch(activity) }
  }

  @MainActor private static func watch(_ activity: Activity<MonocodeActivityAttributes>) {
    guard watched.insert(activity.id).inserted else { return }
    Task {
      for await token in activity.pushTokenUpdates {
        await TokenReporter.shared.report(activity: hex(token))
      }
    }
    Task {
      for await state in activity.activityStateUpdates where state == .ended || state == .dismissed {
        // Its token takes no more pushes; the desktop starts a new one.
        if let token = activity.pushToken {
          await TokenReporter.shared.ended(activity: hex(token))
        }
      }
    }
  }

  /// Sends what the desktop hasn't heard yet, as when the phone pairs again.
  static func flush() {
    Task { await TokenReporter.shared.flush() }
  }

  private static func hex(_ data: Data) -> String {
    data.map { String(format: "%02x", $0) }.joined()
  }
}

private actor TokenReporter {
  static let shared = TokenReporter()

  private var start: String?
  /// The running activity's token, or NSNull once it has ended.
  private var activity: Any?
  private var sent = true

  func report(start token: String) async {
    start = token
    sent = false
    await flush()
  }

  func report(activity token: String) async {
    activity = token
    sent = false
    await flush()
  }

  func ended(activity token: String) async {
    guard activity as? String == token else { return }
    activity = NSNull()
    sent = false
    await flush()
  }

  func flush() async {
    guard !sent, let bundleId = Bundle.main.bundleIdentifier else { return }
    var params: [String: Any] = ["bundleId": bundleId, "environment": apsEnvironment]
    if let start { params["startToken"] = start }
    if let activity { params["activityToken"] = activity }
    // Unpaired, locked or out of reach: the next change or pairing retries.
    if (try? await SessionActions.call("device.activity", params)) != nil {
      sent = true
    }
  }
}

/// Which of Apple's push servers this build's tokens belong to: the sandbox
/// for development-signed builds and the Simulator, production otherwise.
private let apsEnvironment: String = {
  #if targetEnvironment(simulator)
    return "sandbox"
  #else
    guard
      let url = Bundle.main.url(forResource: "embedded", withExtension: "mobileprovision"),
      let data = try? Data(contentsOf: url),
      let text = String(data: data, encoding: .isoLatin1),
      let start = text.range(of: "<?xml"),
      let end = text.range(of: "</plist>"),
      let plist = try? PropertyListSerialization.propertyList(
        from: Data(text[start.lowerBound..<end.upperBound].utf8), format: nil)
        as? [String: Any],
      let entitlements = plist["Entitlements"] as? [String: Any]
    else { return "production" }
    return entitlements["aps-environment"] as? String == "development" ? "sandbox" : "production"
  #endif
}()
