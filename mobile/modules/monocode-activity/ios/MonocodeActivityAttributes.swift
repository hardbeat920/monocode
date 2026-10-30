import ActivityKit
import Foundation

// The same file lives in targets/live-activity, which draws the activity.
// ActivityKit pairs the app and the widget by this type's name and shape, so
// the two copies must stay identical; tests/liveActivity.test.ts checks it.
struct MonocodeActivityAttributes: ActivityAttributes {
  struct Session: Codable, Hashable {
    var id: String
    var projectId: String
    var project: String
    var title: String
    var model: String
    /// The project's color, as #rrggbb.
    var color: String
    /// The project's 8×8 mascot, row by row: "#" filled, anything else empty.
    var grid: String
    /// working, approval, question or done.
    var status: String
    /// What it waits on, e.g. "Run cargo test"; empty while it works.
    var request: String
    var requestId: String
    /// How many approvals (or questions) it waits on.
    var requests: Int
    /// When it started working, for the elapsed timer.
    var since: Date
  }

  struct ContentState: Codable, Hashable {
    /// Most urgent first, at most four.
    var sessions: [Session]
  }

  /// The paired desktop's name.
  var desktop: String
}
