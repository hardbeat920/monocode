Pod::Spec.new do |s|
  s.name = 'MonocodeParakeet'
  s.version = '0.1.0'
  s.summary = 'MonoCode on-device dictation with NVIDIA Parakeet TDT v3 (Core ML, FluidAudio)'
  s.description = s.summary
  s.license = { :type => 'MIT' }
  s.author = 'MonoCode'
  s.homepage = 'https://usemono.dev'
  # FluidAudio's own minimum.
  s.platforms = { :ios => '17.0' }
  s.source = { :git => 'https://github.com/hardbeat920/monocode.git' }
  s.static_framework = true
  s.dependency 'ExpoModulesCore'
  # Resolved from git by plugins/with-fluidaudio.js: CocoaPods trunk stops at 0.7.
  s.dependency 'FluidAudio'
  s.source_files = '**/*.swift'
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES' }
end
