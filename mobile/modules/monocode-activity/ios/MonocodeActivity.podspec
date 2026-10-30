Pod::Spec.new do |s|
  s.name = 'MonocodeActivity'
  s.version = '0.1.0'
  s.summary = 'MonoCode session Live Activity'
  s.description = s.summary
  s.license = { :type => 'MIT' }
  s.author = 'MonoCode'
  s.homepage = 'https://usemono.dev'
  s.platforms = { :ios => '16.4' }
  s.source = { :git => 'https://github.com/hardbeat920/monocode.git' }
  s.static_framework = true
  s.dependency 'ExpoModulesCore'
  # The approval buttons answer the desktop over the link.
  s.dependency 'MonocodeLink'
  s.source_files = '**/*.swift'
  s.frameworks = 'ActivityKit', 'Security'
  s.pod_target_xcconfig = { 'DEFINES_MODULE' => 'YES' }
end
