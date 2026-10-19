import CoreText
import UIKit

struct TranscriptPixel: Equatable, Decodable {
  let x: CGFloat
  let y: CGFloat
  let w: CGFloat
  let h: CGFloat
  let fill: String
}

/// The same provider, Hugeicons and Material file artwork as the React UI.
/// Decoded once on the layout worker; visible views only draw cached images.
final class TranscriptArtwork {
  static let shared = TranscriptArtwork()
  private let images: [String: String]
  private let names: [String: String]
  private let extensions: [String: String]
  private let monochrome: Set<String>
  private let cache = NSCache<NSString, UIImage>()
  private class BundleAnchor: NSObject {}

  private init() {
    let host = Bundle(for: BundleAnchor.self)
    let bundle =
      host.url(forResource: "MonocodeTranscriptAssets", withExtension: "bundle")
      .flatMap(Bundle.init(url:)) ?? Bundle.main
    let url = bundle.url(forResource: "transcript-artwork", withExtension: "json")
    let data = url.flatMap { try? Data(contentsOf: $0) }
    let json = data.flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [String: Any] ?? [:]
    images = json["images"] as? [String: String] ?? [:]
    names = json["names"] as? [String: String] ?? [:]
    extensions = json["extensions"] as? [String: String] ?? [:]
    monochrome = Set(json["monochromeProviders"] as? [String] ?? [])
    cache.totalCostLimit = 3 * 1024 * 1024
  }
  func image(_ name: String) -> UIImage? {
    let key = name as NSString
    if let image = cache.object(forKey: key) { return image }
    guard let encoded = images[name], let data = Data(base64Encoded: encoded),
      let image = UIImage(data: data, scale: 3)
    else { return nil }
    let template =
      name.hasPrefix("icon:")
      || (name.hasPrefix("provider:") && monochrome.contains(String(name.dropFirst(9))))
    let result = image.withRenderingMode(template ? .alwaysTemplate : .alwaysOriginal)
    cache.setObject(result, forKey: key, cost: 48 * 48 * 4)
    return result
  }
  func file(_ name: String) -> String {
    let lower = name.lowercased()
    if let match = names[lower] { return "file:" + match }
    let parts = lower.split(separator: ".", omittingEmptySubsequences: false)
    if parts.count > 1 {
      for index in 1..<parts.count {
        if let match = extensions[parts[index...].joined(separator: ".")] { return "file:" + match }
      }
    }
    return "file:file"
  }
  static func inlineFileName(_ value: String) -> String? {
    let text = value.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !text.isEmpty, text.count <= 240,
      text.rangeOfCharacter(from: .whitespacesAndNewlines) == nil
    else { return nil }
    let path = text.replacingOccurrences(
      of: #"(?::\d+(?::\d+)?|#L\d+(?:-L\d+)?)$"#, with: "", options: .regularExpression)
    guard let name = path.split(whereSeparator: { $0 == "/" || $0 == "\\" }).last.map(String.init),
      name.range(of: #"^[\w%@+().-]+$"#, options: .regularExpression) != nil
    else { return nil }
    if ["dockerfile", "makefile", "gemfile", "license"].contains(name.lowercased()) { return name }
    guard name.contains("."), let ext = name.split(separator: ".").last,
      String(ext).range(
        of: #"^[a-z][a-z0-9+-]{0,11}$"#, options: [.regularExpression, .caseInsensitive]) != nil
    else { return nil }
    return name
  }
}

/// CoreText owns this small attachment through its run delegate.
final class TranscriptInlineIcon {
  let image: UIImage?
  let size: CGFloat
  init(_ image: UIImage?, size: CGFloat) {
    self.image = image
    self.size = size
  }
  func delegate() -> CTRunDelegate {
    var callbacks = CTRunDelegateCallbacks(
      version: kCTRunDelegateVersion1,
      dealloc: { Unmanaged<TranscriptInlineIcon>.fromOpaque($0).release() },
      getAscent: { Unmanaged<TranscriptInlineIcon>.fromOpaque($0).takeUnretainedValue().size - 2 },
      getDescent: { _ in 2 },
      getWidth: { Unmanaged<TranscriptInlineIcon>.fromOpaque($0).takeUnretainedValue().size + 4 })
    return CTRunDelegateCreate(&callbacks, Unmanaged.passRetained(self).toOpaque())!
  }
}

extension NSAttributedString.Key {
  static let transcriptIcon = NSAttributedString.Key("MonocodeTranscriptIcon")
}
