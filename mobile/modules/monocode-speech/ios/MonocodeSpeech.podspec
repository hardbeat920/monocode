Pod::Spec.new do |s|
  s.name = 'MonocodeSpeech'
  s.version = '0.1.0'
  s.summary = 'MonoCode on-device dictation helpers'
  s.description = s.summary
  s.license = { :type => 'MIT' }
  s.author = 'MonoCode'
  s.homepage = 'https://usemono.dev'
  s.platforms = { :ios => '16.4' }
  s.source = { :git => 'https://github.com/hardbeat920/monocode.git' }
  s.static_framework = true
  s.dependency 'ExpoModulesCore'
  s.source_files = '**/*.swift'
  s.frameworks = 'Speech', 'AVFoundation'
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES' }
end
