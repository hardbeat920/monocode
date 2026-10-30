import ActivityKit
import SwiftUI
import WidgetKit

typealias Session = MonocodeActivityAttributes.Session
typealias SessionState = MonocodeActivityAttributes.ContentState

/// How each session state looks, as in the design mockup.
struct Look {
  let color: Color
  /// What the compact island says when it is not counting time.
  let short: String
  /// The status line under the title.
  let long: String
  let symbol: String

  init(_ status: String) {
    switch status {
    case "approval":
      (color, short, long, symbol) = (
        Color(red: 1, green: 0.70, blue: 0.25), "Approve?", "Needs your approval",
        "exclamationmark.circle.fill"
      )
    case "question":
      (color, short, long, symbol) = (
        Color(red: 0.77, green: 0.55, blue: 1), "Asks", "Has a question for you",
        "questionmark.circle.fill"
      )
    case "done":
      (color, short, long, symbol) = (
        Color(red: 0.30, green: 0.85, blue: 0.48), "Done", "Finished", "checkmark.circle.fill"
      )
    default:
      (color, short, long, symbol) = (
        Color(red: 0.48, green: 0.64, blue: 1), "Working", "Working", "circle.dotted"
      )
    }
  }
}

private let tile = Color(red: 0.106, green: 0.106, blue: 0.122)
private let secondaryButton = Color(red: 0.12, green: 0.12, blue: 0.14)

extension Color {
  /// #rrggbb, as the desktop's project colors.
  init(hex: String, fallback: Color = .white) {
    let digits = hex.trimmingCharacters(in: CharacterSet(charactersIn: "#"))
    guard digits.count == 6, let value = UInt32(digits, radix: 16) else {
      self = fallback
      return
    }
    self.init(
      red: Double((value >> 16) & 0xFF) / 255, green: Double((value >> 8) & 0xFF) / 255,
      blue: Double(value & 0xFF) / 255)
  }
}

extension SessionState {
  /// The session the activity is about; the rest are listed or counted.
  var headline: Session? { sessions.first }
  var waiting: Int { sessions.filter { $0.status == "approval" || $0.status == "question" }.count }
  var working: Int { sessions.filter { $0.status == "working" }.count }
  var done: Int { sessions.filter { $0.status == "done" }.count }
}

func link(_ session: Session?) -> URL {
  let root = URL(string: "monocode://")!
  guard let session,
    let project = session.projectId.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed),
    let id = session.id.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed)
  else { return root }
  return URL(string: "monocode://project/\(project)/session/\(id)") ?? root
}

struct SessionActivityWidget: Widget {
  var body: some WidgetConfiguration {
    ActivityConfiguration(for: MonocodeActivityAttributes.self) { context in
      LockScreenView(state: context.state, stale: context.isStale)
        .activityBackgroundTint(Color.black.opacity(0.82))
        .activitySystemActionForegroundColor(.white)
        .widgetURL(link(context.state.headline))
    } dynamicIsland: { context in
      island(context.state, stale: context.isStale)
    }
  }
}

func island(_ state: SessionState, stale: Bool) -> DynamicIsland {
  let session = state.headline
  let look = Look(session?.status ?? "working")
  return DynamicIsland {
    DynamicIslandExpandedRegion(.leading) {
      if let session {
        Link(destination: link(session)) { MascotTile(session: session, size: 44) }.padding(.leading, 4)
      }
    }
    DynamicIslandExpandedRegion(.trailing) {
      if let session {
        Trailing(session: session, look: look)
          .font(.system(size: 16, weight: .semibold))
          .padding(.trailing, 4)
          .frame(maxHeight: .infinity, alignment: .center)
      }
    }
    DynamicIslandExpandedRegion(.center) {
      if let session {
        Link(destination: link(session)) { Heading(session: session) }.padding(.top, 2)
      }
    }
    DynamicIslandExpandedRegion(.bottom) {
      Details(state: state, stale: stale).padding(.horizontal, 4).padding(.top, 4)
    }
  } compactLeading: {
    if let session { Mascot(grid: session.grid, color: Color(hex: session.color)).frame(width: 20, height: 20) }
  } compactTrailing: {
    CompactTrailing(state: state)
  } minimal: {
    if let session {
      Mascot(grid: session.grid, color: look.color).frame(width: 18, height: 18)
    }
  }
  .keylineTint(look.color)
  .widgetURL(link(session))
}

/// The project's 8×8 mascot, as the phone's project rail draws it.
struct Mascot: View {
  let grid: String
  let color: Color

  var body: some View {
    Canvas { context, size in
      let cells = Array(grid)
      guard cells.count >= 64 else { return }
      let cell = min(size.width, size.height) / 8
      for row in 0..<8 {
        for column in 0..<8 where cells[row * 8 + column] == "#" {
          context.fill(
            Path(CGRect(x: CGFloat(column) * cell, y: CGFloat(row) * cell, width: cell, height: cell)),
            with: .color(color))
        }
      }
    }
  }
}

/// The mascot on its dark tile, with the session's state at the corner.
struct MascotTile: View {
  let session: Session
  let size: CGFloat

  var body: some View {
    let look = Look(session.status)
    ZStack(alignment: .bottomTrailing) {
      RoundedRectangle(cornerRadius: size * 0.3, style: .continuous)
        .fill(tile)
        .overlay(
          Mascot(grid: session.grid, color: Color(hex: session.color))
            .padding(size * 0.2))
      Circle()
        .fill(look.color)
        .frame(width: size * 0.3, height: size * 0.3)
        .overlay(Circle().stroke(Color.black, lineWidth: 2.5))
        .offset(x: size * 0.08, y: size * 0.08)
    }
    .frame(width: size, height: size)
  }
}

struct Heading: View {
  let session: Session

  var body: some View {
    VStack(alignment: .leading, spacing: 2) {
      Text(session.title.isEmpty ? "Session" : session.title)
        .font(.system(size: 15, weight: .semibold))
        .lineLimit(1)
      Text([session.project, session.model].filter { !$0.isEmpty }.joined(separator: " · "))
        .font(.system(size: 12))
        .foregroundStyle(Color(white: 0.6))
        .lineLimit(1)
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
}

/// Ticks on its own, so the activity needs no update to count up.
struct Elapsed: View {
  let since: Date

  var body: some View {
    Text(timerInterval: since...Date.distantFuture, countsDown: false)
      .monospacedDigit()
      .lineLimit(1)
      .multilineTextAlignment(.trailing)
  }
}

struct Trailing: View {
  let session: Session
  let look: Look

  var body: some View {
    Group {
      if session.status == "working" {
        Elapsed(since: session.since).frame(width: 58, alignment: .trailing)
      } else {
        Image(systemName: look.symbol).font(.system(size: 22))
      }
    }
    .foregroundStyle(look.color)
  }
}

struct CompactTrailing: View {
  let state: SessionState

  var body: some View {
    Group {
      if state.sessions.count > 1, state.waiting == 0 {
        // Done · working; a waiting session says what it needs instead.
        HStack(spacing: 3) {
          Text("\(state.done)").foregroundStyle(Look("done").color)
          Text("·").foregroundStyle(Color(white: 0.4))
          Text("\(state.working)").foregroundStyle(Look("working").color)
        }
      } else if let session = state.headline {
        let look = Look(session.status)
        if session.status == "working" {
          Elapsed(since: session.since).foregroundStyle(look.color).frame(width: 40, alignment: .trailing)
        } else {
          Text(look.short).foregroundStyle(look.color).lineLimit(1)
        }
      }
    }
    .font(.system(size: 14, weight: .semibold))
    .monospacedDigit()
  }
}

/// What the session waits on, the other sessions and the buttons, shared by
/// the expanded island and the Lock Screen. Both stop at about 160 points, so
/// a request leaves room only for the other sessions' mascots; otherwise
/// they get a row each.
struct Details: View {
  let state: SessionState
  let stale: Bool

  var body: some View {
    VStack(alignment: .leading, spacing: 7) {
      if let session = state.headline {
        let look = Look(session.status)
        let others = Array(state.sessions.dropFirst())
        if stale {
          StatusLine(text: "Open MonoCode to refresh", color: Color(white: 0.6))
        } else if !session.request.isEmpty {
          RequestBox(session: session, others: others)
        } else if others.isEmpty {
          StatusLine(text: look.long, color: look.color)
        } else {
          // A button row costs one session row.
          VStack(spacing: 6) {
            ForEach(others.prefix(session.status == "working" ? 3 : 2), id: \.id) {
              SessionRow(session: $0)
            }
          }
        }
        if !stale && session.status != "working" { Buttons(session: session, state: state) }
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
}

struct StatusLine: View {
  let text: String
  let color: Color

  var body: some View {
    Text(text)
      .font(.system(size: 13, weight: .medium))
      .foregroundStyle(color)
      .lineLimit(1)
  }
}

/// Another session, under the one the activity leads with.
struct SessionRow: View {
  let session: Session

  var body: some View {
    let look = Look(session.status)
    Link(destination: link(session)) {
      HStack(spacing: 8) {
        Mascot(grid: session.grid, color: Color(hex: session.color)).frame(width: 14, height: 14)
        Text(session.title.isEmpty ? "Session" : session.title)
          .font(.system(size: 13, weight: .medium))
          .foregroundStyle(Color(white: 0.92))
          .lineLimit(1)
        Spacer(minLength: 8)
        Group {
          if session.status == "working" {
            Elapsed(since: session.since).frame(width: 44, alignment: .trailing)
          } else {
            Text(look.short).lineLimit(1)
          }
        }
        .font(.system(size: 12.5, weight: .semibold))
        .monospacedDigit()
        .foregroundStyle(look.color)
        Image(systemName: "chevron.right")
          .font(.system(size: 10, weight: .semibold))
          .foregroundStyle(Color(white: 0.4))
      }
      .frame(height: 20)
      .contentShape(Rectangle())
    }
  }
}

/// The approval's command or the question, as the agent put it.
struct RequestBox: View {
  let session: Session
  let others: [Session]

  var body: some View {
    HStack(alignment: .top, spacing: 8) {
      Text(session.request)
        .font(
          session.status == "approval"
            ? .system(size: 12.5, design: .monospaced) : .system(size: 13))
        .foregroundStyle(Color(white: 0.92))
        .lineLimit(2)
        .frame(maxWidth: .infinity, alignment: .leading)
      OtherMascots(others: others).padding(.top, 2)
    }
    .padding(.horizontal, 11)
    .padding(.vertical, 6)
    .background(
      RoundedRectangle(cornerRadius: 12, style: .continuous)
        .fill(Color(red: 0.09, green: 0.09, blue: 0.10))
        .overlay(
          RoundedRectangle(cornerRadius: 12, style: .continuous)
            .stroke(Color(white: 0.16), lineWidth: 1)))
  }
}

struct OtherMascots: View {
  let others: [Session]

  var body: some View {
    HStack(spacing: 5) {
      ForEach(others.prefix(3), id: \.id) { other in
        Mascot(grid: other.grid, color: Color(hex: other.color)).frame(width: 13, height: 13)
      }
    }
  }
}

struct Buttons: View {
  let session: Session
  let state: SessionState

  var body: some View {
    let look = Look(session.status)
    let url = link(session)
    // One approval can be answered here; several need the app to tell apart.
    let approvals = state.sessions.filter { $0.status == "approval" }
      .reduce(0) { $0 + max($1.requests, 1) }
    HStack(spacing: 8) {
      switch session.status {
      case "approval" where approvals == 1 && !session.requestId.isEmpty:
        Pill(title: "Review", url: url)
        DecisionPill(session: session, allow: false)
        DecisionPill(session: session, allow: true)
      case "approval":
        Pill(title: "Review \(approvals) approvals", url: url, color: look.color)
      case "question":
        Pill(title: "Reply", url: url, color: look.color)
      case "done":
        Pill(title: "Review", url: url, color: look.color)
      default:
        EmptyView()
      }
    }
  }
}

/// Answers the request on the desktop without opening the app.
struct DecisionPill: View {
  let session: Session
  let allow: Bool

  var body: some View {
    Button(intent: SessionDecisionIntent(sessionId: session.id, requestId: session.requestId, allow: allow)) {
      Text(allow ? "Approve" : "Deny")
        .font(.system(size: 14, weight: .semibold))
        .foregroundStyle(allow ? Color.black : denyColor)
        .frame(maxWidth: .infinity)
        .frame(height: pillHeight)
        .background(Capsule().fill(allow ? Look("approval").color : secondaryButton))
    }
    .buttonStyle(.plain)
  }
}

private let pillHeight: CGFloat = 30
private let denyColor = Color(red: 1, green: 0.42, blue: 0.40)

struct Pill: View {
  let title: String
  let url: URL
  var color: Color? = nil

  var body: some View {
    Link(destination: url) {
      Text(title)
        .font(.system(size: 14, weight: .semibold))
        .foregroundStyle(color == nil ? Color.white : Color.black)
        .frame(maxWidth: .infinity)
        .frame(height: pillHeight)
        .background(Capsule().fill(color ?? secondaryButton))
    }
  }
}

struct LockScreenView: View {
  let state: SessionState
  let stale: Bool

  var body: some View {
    VStack(alignment: .leading, spacing: 10) {
      if let session = state.headline {
        Link(destination: link(session)) {
          HStack(spacing: 12) {
            MascotTile(session: session, size: 40)
            Heading(session: session)
            Trailing(session: session, look: Look(session.status))
              .font(.system(size: 16, weight: .semibold))
          }
        }
      }
      Details(state: state, stale: stale)
    }
    .padding(14)
    .foregroundStyle(.white)
  }
}

// Every state the design shows; open this file in Xcode to see them.
#if DEBUG
  private let invader = "..#..#...######.##.##.##########.######..#.##.#.#.#..#.#........"
  private let octopus = "..####...######.##.##.###################################.##.##."

  private func preview(_ status: String, _ title: String, grid: String = invader, color: String = "#4fd1c5")
    -> Session
  {
    Session(
      id: title, projectId: "p", project: "monocode", title: title, model: "Opus 5.5",
      color: color, grid: grid, status: status,
      request: status == "approval"
        ? "Run cargo test -p monocode-core mobile::"
        : status == "question" ? "Deploy the relay to Workers or Supabase?" : "",
      requestId: status == "approval" ? "r1" : "", requests: status == "approval" ? 1 : 0,
      since: Date().addingTimeInterval(-252))
  }

  private let working = SessionState(sessions: [preview("working", "Fix mobile notifications")])
  private let approval = SessionState(sessions: [preview("approval", "Fix mobile notifications")])
  private let question = SessionState(sessions: [
    preview("question", "Relay service", grid: octopus, color: "#c48cff")
  ])
  private let done = SessionState(sessions: [preview("done", "Fix mobile notifications")])
  private let several = SessionState(sessions: [
    preview("approval", "Fix mobile notifications"),
    preview("working", "Relay service", grid: octopus, color: "#c48cff"),
    preview("working", "Terminal keys polish", color: "#ffb340"),
  ])
  private let attributes = MonocodeActivityAttributes(desktop: "MacBook Pro")

  #Preview("Compact", as: .dynamicIsland(.compact), using: attributes) {
    SessionActivityWidget()
  } contentStates: {
    working
    approval
    done
    several
  }

  #Preview("Expanded", as: .dynamicIsland(.expanded), using: attributes) {
    SessionActivityWidget()
  } contentStates: {
    working
    approval
    question
    done
    several
  }

  #Preview("Minimal", as: .dynamicIsland(.minimal), using: attributes) {
    SessionActivityWidget()
  } contentStates: {
    working
    approval
  }

  #Preview("Lock Screen", as: .content, using: attributes) {
    SessionActivityWidget()
  } contentStates: {
    working
    approval
    several
  }
#endif
