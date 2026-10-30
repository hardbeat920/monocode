import ActivityKit
import ExpoModulesCore
import MonocodeLink
import Security

struct ActivitySession: Record {
  @Field var id: String = ""
  @Field var projectId: String = ""
  @Field var project: String = ""
  @Field var title: String = ""
  @Field var model: String = ""
  @Field var color: String = ""
  @Field var grid: String = ""
  @Field var status: String = "working"
  @Field var request: String = ""
  @Field var requestId: String = ""
  @Field var requests: Int = 0
  /// Seconds since 1970.
  @Field var since: Double = 0
}

/// How long a finished activity stays on the Lock Screen.
private let finishedFor: TimeInterval = 15 * 60
/// Past this without news the activity says so, since the phone may have
/// missed a push.
private let staleAfter: TimeInterval = 10 * 60

public class MonocodeActivityModule: Module {
  public func definition() -> ModuleDefinition {
    Name("MonocodeActivity")

    OnCreate {
      Task { @MainActor in ActivityTokens.start() }
    }

    Function("isEnabled") { () -> Bool in
      ActivityAuthorizationInfo().areActivitiesEnabled
    }

    Function("isRunning") { () -> Bool in
      !Activity<MonocodeActivityAttributes>.activities.isEmpty
    }

    /// Shows the sessions: starts the activity when there is none (only
    /// possible in the foreground), redraws it, or ends it once nothing works
    /// or waits. Returns whether an activity is running afterwards.
    AsyncFunction("sync") { (desktop: String, sessions: [ActivitySession]) async -> Bool in
      await sync(desktop: desktop, sessions: sessions)
    }

    AsyncFunction("end") { () async in
      for activity in Activity<MonocodeActivityAttributes>.activities {
        await activity.end(nil, dismissalPolicy: .immediate)
      }
    }

    /// What the approval buttons need to reach the desktop without the app's
    /// JavaScript: the pairing identity, the desktop's pin and addresses.
    Function("rememberLink") { (link: String) in
      SessionActions.remember(link)
    }

    Function("forgetLink") {
      SessionActions.remember(nil)
    }
  }
}

private func sync(desktop: String, sessions: [ActivitySession]) async -> Bool {
  let running = Activity<MonocodeActivityAttributes>.activities
  let state = MonocodeActivityAttributes.ContentState(
    sessions: sessions.prefix(4).map {
      .init(
        id: $0.id, projectId: $0.projectId, project: $0.project, title: $0.title,
        model: $0.model, color: $0.color, grid: $0.grid, status: $0.status,
        request: $0.request, requestId: $0.requestId, requests: $0.requests,
        since: Date(timeIntervalSince1970: $0.since))
    })
  let now = Date()
  if state.sessions.isEmpty {
    for activity in running { await activity.end(nil, dismissalPolicy: .immediate) }
    return false
  }
  if state.sessions.allSatisfy({ $0.status == "done" }) {
    let content = ActivityContent(state: state, staleDate: nil)
    for activity in running {
      await activity.end(content, dismissalPolicy: .after(now.addingTimeInterval(finishedFor)))
    }
    return false
  }
  let content = ActivityContent(state: state, staleDate: now.addingTimeInterval(staleAfter))
  // One per desktop; extras are left from an earlier pairing.
  for activity in running.dropFirst() { await activity.end(nil, dismissalPolicy: .immediate) }
  if let activity = running.first {
    await activity.update(content)
    return true
  }
  // The widget that draws it needs iOS 17.
  guard #available(iOS 17.0, *), ActivityAuthorizationInfo().areActivitiesEnabled else {
    return false
  }
  let attributes = MonocodeActivityAttributes(desktop: desktop)
  do {
    // With a push token the desktop can redraw it while the app is closed.
    _ = try Activity.request(attributes: attributes, content: content, pushType: .token)
    return true
  } catch {
    // A build without push still shows one the app redraws itself.
    do {
      _ = try Activity.request(attributes: attributes, content: content, pushType: nil)
      return true
    } catch {
      // Starting needs the app in front; the next foreground sync starts it.
      return false
    }
  }
}

/// Answers an approval from the Live Activity: the intent in
/// targets/_shared calls this in the app's process, with or without the
/// JavaScript running.
public enum SessionActions {
  private static let service = "monocode.live-activity"
  private static let account = "link"

  struct DecisionError: LocalizedError {
    let errorDescription: String?
  }

  static func remember(_ link: String?) {
    let query: [String: Any] = [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: account,
    ]
    SecItemDelete(query as CFDictionary)
    guard let data = link?.data(using: .utf8) else { return }
    var item = query
    item[kSecValueData as String] = data
    // Readable only while unlocked; the intent asks to unlock first.
    item[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
    SecItemAdd(item as CFDictionary, nil)
    // Paired (again): tell this desktop where its activity pushes go.
    ActivityTokens.flush()
  }

  private static func link() -> [String: Any]? {
    let query: [String: Any] = [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: account,
      kSecReturnData as String: true,
    ]
    var result: AnyObject?
    guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
      let data = result as? Data
    else { return nil }
    return try? JSONSerialization.jsonObject(with: data) as? [String: Any]
  }

  /// One request to the paired desktop over the link, from native code.
  static func call(_ method: String, _ params: [String: Any]) async throws {
    guard let link = link() else {
      throw DecisionError(errorDescription: "Open MonoCode to reconnect to your desktop")
    }
    let input: [String: Any] = [
      "operation": "exchange",
      "identity": link["identity"] ?? [:],
      "pin": link["pin"] ?? "",
      "addresses": link["addresses"] ?? [],
      "request": ["version": 1, "id": UUID().uuidString, "method": method, "params": params],
    ]
    let json = String(data: try JSONSerialization.data(withJSONObject: input), encoding: .utf8) ?? "{}"
    let output = await Task.detached { MonocodeLinkCall.execute(json) }.value
    let response =
      (try? JSONSerialization.jsonObject(with: Data(output.utf8)) as? [String: Any]) ?? [:]
    guard response["ok"] as? Bool == true else {
      throw DecisionError(errorDescription: response["error"] as? String ?? "Desktop unavailable")
    }
  }

  /// Allows or denies `requestId` on the desktop, then shows the session
  /// working again rather than waiting for the next push.
  public static func decide(
    sessionId: String, requestId: String, allow: Bool
  ) async throws {
    try await call(
      "session.decide",
      ["conversationId": sessionId, "requestId": requestId, "decision": allow ? "allow" : "deny"])
    for activity in Activity<MonocodeActivityAttributes>.activities {
      var state = activity.content.state
      guard let index = state.sessions.firstIndex(where: { $0.id == sessionId }) else { continue }
      var session = state.sessions[index]
      // Either way the agent carries on, with or without the command.
      session.status = "working"
      session.request = ""
      session.requestId = ""
      session.requests = 0
      state.sessions[index] = session
      await activity.update(
        ActivityContent(state: state, staleDate: Date().addingTimeInterval(staleAfter)))
    }
  }
}
