import AppIntents
import Foundation

#if canImport(MonocodeActivity)
  internal import MonocodeActivity
#endif

/// Allow or deny an agent's request from its Live Activity. Linked into the
/// app and the widget: the widget's button names it, and iOS runs it in the
/// app, the only one that can reach the desktop.
@available(iOS 17.0, *)
struct SessionDecisionIntent: LiveActivityIntent {
  static var title: LocalizedStringResource = "Answer an agent's request"
  static var isDiscoverable = false
  // Running a command on the desktop needs the phone unlocked.
  static var authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication

  @Parameter(title: "Session") var sessionId: String
  @Parameter(title: "Request") var requestId: String
  @Parameter(title: "Allow") var allow: Bool

  init() {}

  init(sessionId: String, requestId: String, allow: Bool) {
    self.sessionId = sessionId
    self.requestId = requestId
    self.allow = allow
  }

  func perform() async throws -> some IntentResult {
    #if canImport(MonocodeActivity)
      try await SessionActions.decide(sessionId: sessionId, requestId: requestId, allow: allow)
    #endif
    return .result()
  }
}
