Pod::Spec.new do |s|
  s.name = 'MonocodeLink'
  s.version = '0.1.0'
  s.summary = 'MonoCode direct QUIC connection'
  s.description = s.summary
  s.license = { :type => 'MIT' }
  s.author = 'MonoCode'
  s.homepage = 'https://usemono.dev'
  s.platforms = { :ios => '16.4' }
  s.source = { :git => 'https://github.com/hardbeat920/monocode.git' }
  s.static_framework = true
  s.dependency 'ExpoModulesCore'
  s.source_files = '**/*.swift'
  s.resource_bundles = { 'MonocodeTranscriptAssets' => ['Resources/transcript-artwork.json'] }
  s.vendored_frameworks = 'MonocodeLinkCore.xcframework'
  s.frameworks = 'Security', 'SystemConfiguration', 'UIKit', 'CoreText'
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES' }
end
