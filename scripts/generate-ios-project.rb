#!/usr/bin/env ruby
# Run once (or regenerate after source changes) with the xcodeproj gem available.
require 'xcodeproj'
require 'pathname'
require 'rexml/document'

root = Pathname.new(__dir__).parent
path = root.join('apps/ios/aTerminal.xcodeproj')
abort 'Project already exists; preserve local signing changes instead of overwriting it' if path.exist?
project = Xcodeproj::Project.new(path.to_s)
target = project.new_target(:application, 'aTerminal', :ios, '15.0')
group = project.main_group.new_group('aTerminal', 'aTerminal')
root.join('apps/ios/aTerminal').glob('*.swift').sort.each do |file|
  target.source_build_phase.add_file_reference(group.new_file(file.basename.to_s))
end
generated = project.main_group.new_group('Rust Bindings')
target.source_build_phase.add_file_reference(generated.new_file('../../build/bindings/ai_terminal_mobile.swift'))
core = project.frameworks_group.new_file('../../build/aTerminalCore.xcframework')
target.frameworks_build_phase.add_file_reference(core)
fixture = project.main_group.new_file('../../build/fixtures/screen.pb')
target.resources_build_phase.add_file_reference(fixture)
target.build_configurations.each do |config|
  config.build_settings.merge!({
    'PRODUCT_BUNDLE_IDENTIFIER' => 'com.yxf.aterminal',
    'SWIFT_VERSION' => '5.0',
    'GENERATE_INFOPLIST_FILE' => 'YES',
    'INFOPLIST_KEY_CFBundleDisplayName' => 'aTerminal',
    'INFOPLIST_KEY_UILaunchScreen_Generation' => 'YES',
    'INFOPLIST_KEY_UIApplicationSceneManifest_Generation' => 'YES',
    'TARGETED_DEVICE_FAMILY' => '1,2',
    'CODE_SIGN_STYLE' => 'Automatic',
    'CURRENT_PROJECT_VERSION' => '1',
    'MARKETING_VERSION' => '0.1.0',
    'OTHER_LDFLAGS' => ['$(inherited)', '-lc++', '-framework', 'Security'],
    'SWIFT_ACTIVE_COMPILATION_CONDITIONS' => config.name == 'Debug' ? ['$(inherited)', 'DEBUG'] : ['$(inherited)'],
    'HEADER_SEARCH_PATHS' => ['$(inherited)', '$(SRCROOT)/../../build/bindings'],
    'SWIFT_INCLUDE_PATHS' => ['$(inherited)', '$(SRCROOT)/../../build/bindings'],
    'OTHER_SWIFT_FLAGS' => ['$(inherited)', '-Xcc', '-fmodule-map-file=$(SRCROOT)/../../build/bindings/ai_terminal_mobileFFI.modulemap']
  })
end
ui_tests = project.new_target(:ui_test_bundle, 'aTerminalUITests', :ios, '15.0')
ui_tests.add_dependency(target)
ui_tests.source_build_phase.add_file_reference(project.main_group.new_file('UITests/WorkspaceUITests.swift'))
ui_tests.build_configurations.each do |config|
  config.build_settings.merge!({
    'PRODUCT_BUNDLE_IDENTIFIER' => 'com.yxf.aterminal.uitests',
    'GENERATE_INFOPLIST_FILE' => 'YES',
    'SWIFT_VERSION' => '5.0',
    'TEST_TARGET_NAME' => 'aTerminal',
    'TARGETED_DEVICE_FAMILY' => '1,2'
  })
end
project.save
scheme = Xcodeproj::XCScheme.new
scheme.add_build_target(target)
scheme.add_test_target(ui_tests)
scheme.set_launch_target(target)
scheme.save_as(path.to_s, 'aTerminal', true)
scheme_path = path.join('xcshareddata/xcschemes/aTerminal.xcscheme')
document = REXML::Document.new(scheme_path.read)
test_action = document.elements['Scheme/TestAction']
test_action.attributes['shouldUseLaunchSchemeArgsEnv'] = 'NO'
variables = test_action.add_element('EnvironmentVariables')
variables.add_element('EnvironmentVariable', {
  'key' => 'AI_TERMINAL_IOS_FIXTURE',
  'value' => '$(AI_TERMINAL_IOS_FIXTURE)',
  'isEnabled' => 'YES'
})
File.open(scheme_path, 'w') { |file| document.write(file, 2) }
puts path
