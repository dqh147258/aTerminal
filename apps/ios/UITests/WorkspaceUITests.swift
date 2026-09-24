import XCTest

final class WorkspaceUITests: XCTestCase {
    override func setUp() { continueAfterFailure = false }
    override func tearDown() { XCUIDevice.shared.orientation = .portrait }
    private func launch(_ arguments: [String]) -> XCUIApplication {
        let app = XCUIApplication(); app.launchArguments = arguments; app.launch(); return app
    }
    private func capture(_ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
    func testLoginValidationAndPasswordVisibility() {
        let app = launch(["--login-fixture"])
        app.buttons["login.submit"].tap()
        XCTAssertTrue(app.staticTexts["请输入有效的服务地址"].waitForExistence(timeout: 3))
        let server = app.textFields["login.server"]; server.tap(); server.typeText("https://example.invalid")
        app.textFields["login.username"].tap(); app.textFields["login.username"].typeText("layout-check")
        app.secureTextFields["login.password"].tap(); app.secureTextFields["login.password"].typeText("fixture-only")
        app.buttons["显示密码"].tap()
        XCTAssertEqual(app.textFields["login.password"].value as? String, "fixture-only")
        app.buttons["隐藏密码"].tap()
        XCTAssertTrue(app.secureTextFields["login.password"].exists)
        capture("login-keyboard-validation")
    }
    func testSettingsPersistenceAndReset() {
        var app = launch(["--workspace-fixture", "--show-settings"])
        XCTAssertTrue(app.buttons["恢复默认"].waitForExistence(timeout: 3))
        XCTAssertGreaterThan(app.buttons["关闭终端设置"].frame.minY, app.frame.minY + 100)
        app.buttons["恢复默认"].tap()
        XCTAssertTrue(app.staticTexts["16 px"].exists)
        XCTAssertTrue(app.staticTexts["88%"].exists)
        app.sliders["文字大小"].adjust(toNormalizedSliderPosition: 1)
        app.sliders["浮窗不透明度"].adjust(toNormalizedSliderPosition: 0)
        XCTAssertTrue(app.staticTexts["24 px"].exists)
        XCTAssertTrue(app.staticTexts["60%"].exists)
        app.terminate()
        app = launch(["--workspace-fixture", "--show-settings"])
        XCTAssertTrue(app.staticTexts["60%"].waitForExistence(timeout: 3))
        XCTAssertTrue(app.staticTexts["24 px"].exists)
        app.buttons["恢复默认"].tap()
        XCTAssertTrue(app.staticTexts["16 px"].exists)
        XCTAssertTrue(app.staticTexts["88%"].exists)
        capture("settings-restored")
    }
    func testBackgroundClearsConnectionBusyAndIgnoresOldFailure() {
        let app = launch(["--workspace-fixture", "--pending-connection-fixture"])
        XCTAssertTrue(app.activityIndicators.firstMatch.exists)
        XCUIDevice.shared.press(.home)
        app.activate()
        XCTAssertFalse(app.activityIndicators.firstMatch.exists)
        XCTAssertTrue(app.staticTexts["已断开，选择设备恢复连接"].exists)
        XCTAssertFalse(app.staticTexts["stale-operation-error"].waitForExistence(timeout: 11))
        XCTAssertFalse(app.activityIndicators.firstMatch.exists)
    }
    func testDrawerAndAssistantPlaceholder() {
        let app = launch(["--workspace-fixture", "--long-chat"])
        let edge = app.coordinate(withNormalizedOffset: CGVector(dx: 0.01, dy: 0.45))
        edge.press(forDuration: 0.1, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.45)))
        XCTAssertTrue(app.buttons["关闭工作空间"].waitForExistence(timeout: 3))
        app.segmentedControls.buttons["AI 历史"].tap()
        XCTAssertTrue(app.staticTexts["暂无 AI 对话"].exists)
        app.buttons["关闭工作空间"].tap()
        app.buttons["workspace.chat"].tap()
        XCTAssertTrue(app.staticTexts["AI 助手即将开放"].exists)
        XCTAssertGreaterThan(app.buttons["关闭AI Agent"].frame.minY, app.frame.minY + 70)
        XCTAssertFalse(app.buttons["chat.send"].isEnabled)
        XCTAssertFalse(app.buttons["语音输入"].exists)
        capture("assistant-placeholder")
        XCUIDevice.shared.orientation = .landscapeLeft
        Thread.sleep(forTimeInterval: 1.5)
        XCTAssertTrue(app.buttons["关闭AI Agent"].isHittable)
        XCUIDevice.shared.orientation = .portrait
        Thread.sleep(forTimeInterval: 1.5)
        app.buttons["关闭AI Agent"].tap()
        app.buttons["workspace.settings"].tap()
        XCTAssertTrue(app.staticTexts["文字大小"].exists)
    }
}

final class LiveServiceUITests: XCTestCase {
    private struct Fixture: Decodable {
        let server: String
        let username: String
        let password: String
        let session: String
        let benchmarks: [Benchmark]
        let caPem: String?
        let createCommand: String?
        let closeCommandPrefix: String?
        struct Benchmark: Decodable { let id: String; let cols: Int }
    }
    override func setUp() { continueAfterFailure = false }
    private func wait(_ timeout: TimeInterval = 20, _ predicate: @escaping () -> Bool, file: StaticString = #filePath, line: UInt = #line) {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in predicate() }, object: nil)
        let result = XCTWaiter.wait(for: [expectation], timeout: timeout)
        if result != .completed { capture("live-failure") }
        XCTAssertEqual(result, .completed, file: file, line: line)
    }
    private func capture(_ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot()); attachment.name = name; attachment.lifetime = .keepAlways; add(attachment)
    }
    private func terminal(_ app: XCUIApplication) -> XCUIElement { app.descendants(matching: .any).matching(identifier: "terminal.screen").firstMatch }
    private func text(_ app: XCUIApplication) -> String { terminal(app).value as? String ?? "" }
    private func hasLine(_ app: XCUIApplication, _ line: String) -> Bool { text(app).components(separatedBy: .newlines).contains { $0.trimmingCharacters(in: .whitespaces) == line } }
    private func metrics(_ app: XCUIApplication) -> [String: Int] {
        guard let value = app.scrollViews["terminal.scroll"].value as? String,
              let object = try? JSONSerialization.jsonObject(with: Data(value.utf8)) as? [String: Int] else { return [:] }
        return object
    }
    private func select(_ id: String, app: XCUIApplication) {
        app.buttons["workspace.drawer"].tap()
        app.segmentedControls.buttons["终端"].tap()
        let row = app.buttons["session.select." + id]
        XCTAssertTrue(row.waitForExistence(timeout: 10)); row.tap()
        XCTAssertTrue(terminal(app).waitForExistence(timeout: 20))
    }
    private func command(_ command: String, app: XCUIApplication, hideInput: Bool = true) {
        let control = app.switches["接管输入"]
        wait { control.exists && control.isEnabled }
        if control.value as? String == "0" { control.coordinate(withNormalizedOffset: CGVector(dx: 0.9, dy: 0.5)).tap() }
        wait { control.value as? String == "1" }
        let screen = terminal(app); screen.tap()
        XCTAssertTrue(app.buttons["隐藏键盘"].waitForExistence(timeout: 3))
        XCTAssertTrue(app.textFields["terminal.input"].exists)
        XCTAssertFalse(app.textFields["terminal.draft"].exists)
        app.typeText(command)
        XCTAssertTrue(app.keyboards.buttons["Return"].waitForExistence(timeout: 3))
        app.keyboards.buttons["Return"].tap()
        if hideInput { app.buttons["隐藏键盘"].tap() }
    }
    func testRealTerminalWorkflowAndPlaceholder() throws {
        guard let path = ProcessInfo.processInfo.environment["AI_TERMINAL_IOS_FIXTURE"], path.hasPrefix("/"), FileManager.default.fileExists(atPath: path) else { throw XCTSkip("Dedicated integration fixture not supplied") }
        let fixture = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: URL(fileURLWithPath: path)))
        let app = XCUIApplication(); app.launchArguments = ["--service-test"]
        if let caPem = fixture.caPem { app.launchEnvironment["AI_TERMINAL_TEST_CA_PEM"] = caPem }
        app.launch()
        if !app.textFields["login.server"].waitForExistence(timeout: 5) {
            if app.buttons["关闭设备"].exists { app.buttons["关闭设备"].tap() }
            XCTAssertTrue(app.buttons["workspace.drawer"].waitForExistence(timeout: 15)); app.buttons["workspace.drawer"].tap()
            wait { app.buttons["退出登录"].isEnabled }; app.buttons["退出登录"].tap(); app.alerts.buttons["退出登录"].tap()
        }
        XCTAssertTrue(app.textFields["login.server"].waitForExistence(timeout: 10))
        app.textFields["login.server"].tap(); app.textFields["login.server"].typeText(fixture.server)
        app.textFields["login.username"].tap(); app.textFields["login.username"].typeText(fixture.username)
        app.secureTextFields["login.password"].tap(); app.secureTextFields["login.password"].typeText(fixture.password)
        app.buttons["login.submit"].tap()
        let desktop = app.buttons.matching(NSPredicate(format: "label CONTAINS %@", "Local Desktop")).firstMatch
        wait { self.terminal(app).exists || desktop.exists || app.staticTexts["login.error"].exists }
        XCTAssertFalse(app.staticTexts["login.error"].exists, app.staticTexts["login.error"].exists ? app.staticTexts["login.error"].label : "")
        if !terminal(app).exists { desktop.tap() }
        XCTAssertTrue(terminal(app).waitForExistence(timeout: 20))
        select(fixture.session, app: app)
        wait { self.metrics(app)["columns"] == 120 }
        command("printf '\\033[2J\\033[H'", app: app)
        wait { !self.hasLine(app, "AI_DEVICE_OK") && !self.hasLine(app, "AI_DEVICE_DONE") }
        command("printf '\\nIOS_PTY_OK\\n'", app: app)
        wait { self.hasLine(app, "IOS_PTY_OK") }
        let tabDirectory = "/private/tmp/i" + String(fixture.session.prefix(8))
        command("mkdir -p \(tabDirectory)", app: app)
        command("echo IOS_TAB_OK > \(tabDirectory)/complete_marker", app: app)
        command("echo IOS_TAB_READY", app: app)
        wait { self.hasLine(app, "IOS_TAB_READY") }
        terminal(app).tap()
        app.typeText("cat \(tabDirectory)/comple")
        app.buttons["terminal.key.tab"].tap(); app.buttons["terminal.key.enter"].tap()
        wait { self.hasLine(app, "IOS_TAB_OK") }
        command("rm -r \(tabDirectory)", app: app)
        if let createCommand = fixture.createCommand, let closeCommandPrefix = fixture.closeCommandPrefix {
            command(createCommand, app: app)
            var externalID = ""
            wait {
                let text = self.text(app)
                guard let range = text.range(of: "AIT_TERMINAL_TEST_SESSION=[0-9a-f]{16}", options: .regularExpression) else { return false }
                externalID = String(text[range].suffix(16))
                return true
            }
            app.buttons["workspace.drawer"].tap()
            app.segmentedControls.buttons["终端"].tap()
            wait { app.buttons["session.select." + externalID].exists }
            app.buttons["关闭工作空间"].tap()
            command(closeCommandPrefix + " " + externalID, app: app)
            app.buttons["workspace.drawer"].tap()
            app.segmentedControls.buttons["终端"].tap()
            wait { !app.buttons["session.select." + externalID].exists }
            app.buttons["关闭工作空间"].tap()
        }
        capture("live-pty-120-columns")
        command("printf '%120s\\n' IOS_RIGHT_EDGE", app: app)
        wait { self.text(app).contains("IOS_RIGHT_EDGE") }
        app.scrollViews["terminal.scroll"].swipeLeft()
        wait { (self.metrics(app)["offset"] ?? 0) > 100 }
        XCTAssertEqual(metrics(app)["columns"], 120)
        capture("live-horizontal-original-columns")
        app.scrollViews["terminal.scroll"].swipeRight()
        if app.buttons["关闭工作空间"].exists { app.buttons["关闭工作空间"].tap() }
        app.buttons["workspace.settings"].tap()
        XCTAssertGreaterThan(app.buttons["关闭终端设置"].frame.minY, app.frame.minY + 100)
        app.sliders["文字大小"].adjust(toNormalizedSliderPosition: 1)
        capture("live-font-24-floating-preview")
        app.buttons["关闭终端设置"].tap()
        wait { self.metrics(app)["font"] == 24 }
        XCTAssertEqual(metrics(app)["columns"], 120)
        app.buttons["workspace.settings"].tap(); app.buttons["恢复默认"].tap(); app.buttons["关闭终端设置"].tap()
        if let bench = fixture.benchmarks.first(where: { $0.cols == 80 }) {
            select(bench.id, app: app); wait { self.metrics(app)["columns"] == 80 }
            select(fixture.session, app: app); wait { self.metrics(app)["columns"] == 120 }
        }
        app.buttons["workspace.chat"].tap()
        XCTAssertTrue(app.staticTexts["AI 助手即将开放"].waitForExistence(timeout: 3))
        XCTAssertFalse(app.buttons["chat.send"].isEnabled)
        XCTAssertFalse(app.buttons["语音输入"].exists)
        capture("live-assistant-placeholder")
        app.buttons["关闭AI Agent"].tap()
        command("printf '\\nIOS_RESTORE_MARKER\\n'", app: app)
        wait { self.hasLine(app, "IOS_RESTORE_MARKER") }
        app.terminate(); app.launch()
        wait(30) { self.hasLine(app, "IOS_RESTORE_MARKER") }
        XCTAssertEqual(metrics(app)["columns"], 120)
        capture("live-last-terminal-restored")
        command("sleep 30", app: app, hideInput: false)
        app.buttons["terminal.key.ctrl_c"].tap()
        command("printf '\\nIOS_CTRL_C_OK\\n'", app: app)
        wait { self.hasLine(app, "IOS_CTRL_C_OK") }
        capture("live-ctrl-c-shell-continues")
        app.buttons["关闭会话"].tap(); app.alerts.buttons["关闭会话"].tap()
        wait { app.staticTexts["会话已关闭"].exists }
        app.buttons["workspace.drawer"].tap()
        app.segmentedControls.buttons["终端"].tap()
        XCTAssertFalse(app.buttons["session.select." + fixture.session].exists)
        if let remaining = fixture.benchmarks.first {
            app.buttons["session.select." + remaining.id].tap()
            XCTAssertTrue(terminal(app).waitForExistence(timeout: 10))
            app.buttons["workspace.drawer"].tap()
        }
        capture("live-terminal-closed-channel-retained")
        app.buttons["退出登录"].tap(); app.alerts.buttons["退出登录"].tap()
        XCTAssertTrue(app.textFields["login.server"].waitForExistence(timeout: 15))
        XCTAssertFalse(app.staticTexts["AI_DEVICE_DONE"].exists)
    }
}
