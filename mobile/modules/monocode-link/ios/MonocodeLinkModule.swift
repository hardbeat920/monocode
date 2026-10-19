import ExpoModulesCore
import MonocodeLinkCore

public class MonocodeLinkModule: Module {
  public func definition() -> ModuleDefinition {
    Name("MonocodeLink")
    Constant("nativeTranscriptVersion") { 1 }
    View(NativeTranscriptView.self) {
      Events("onStatus", "onAction", "onScrollState")
      Prop("configuration") { (view: NativeTranscriptView, value: String) in
        view.setConfiguration(value)
      }
      Prop("pending") { (view: NativeTranscriptView, value: String) in view.setPending(value) }
      Prop("appearance") { (view: NativeTranscriptView, value: String) in view.setAppearance(value)
      }
      Prop("active") { (view: NativeTranscriptView, value: Bool) in view.setActive(value) }
      Prop("topInset") { (view: NativeTranscriptView, value: Double) in view.setInsets(top: value) }
      Prop("bottomInset") { (view: NativeTranscriptView, value: Double) in
        view.setInsets(bottom: value)
      }
      Prop("composerExtra") { (view: NativeTranscriptView, value: Double) in
        view.setInsets(extra: value)
      }
      Prop("keyboardOffset") { (view: NativeTranscriptView, value: Double) in
        view.setInsets(keyboardOffset: value)
      }
      OnViewDidUpdateProps { (view: NativeTranscriptView) in view.updateProps() }
      AsyncFunction("scrollToEnd") { (view: NativeTranscriptView) in view.jumpToEnd() }
    }
    AsyncFunction("execute") { (input: String) -> String in
      return input.withCString { pointer in
        guard let output = monocode_link_call(pointer) else {
          return "{\"ok\":false,\"error\":\"Native connection failed\"}"
        }
        defer { monocode_link_free(output) }
        return String(cString: output)
      }
    }.runOnQueue(.global(qos: .userInitiated))
  }
}
