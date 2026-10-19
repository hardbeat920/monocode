package expo.modules.monocodelink

import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class MonocodeLinkModule : Module() {
  companion object { init { System.loadLibrary("monocode_mobile_link") } }
  private external fun nativeCall(input: String): String
  override fun definition() = ModuleDefinition {
    Name("MonocodeLink")
    AsyncFunction("execute") { input: String -> nativeCall(input) }
  }
}
