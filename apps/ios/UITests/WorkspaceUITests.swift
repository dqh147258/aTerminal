import XCTest

// Tapping an existing URL/JSON does not put the caret at its end. Select the
// complete value before replacing it, and keep failure assertions value-free.
private enum UITestInput {
    static func replace(_ field: XCUIElement, with value: String, app: XCUIApplication = XCUIApplication(), file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertTrue(field.waitForExistence(timeout: 20), "Expected input is unavailable", file: file, line: line)
        field.tap()
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5), "Input keyboard is unavailable", file: file, line: line)
        let existing = field.value as? String ?? ""
        if existing == value { return }
        let hasExistingText = !existing.isEmpty && existing != field.placeholderValue
        if hasExistingText {
            let clear = app.buttons[field.identifier + ".clear"]
            if clear.exists {
                if !clear.isHittable { app.scrollViews.firstMatch.swipeUp() }
                clear.tap()
                field.tap()
            } else {
                guard field.elementType == .textField || field.elementType == .secureTextField else {
                    XCTFail("The code editor clear action is unavailable", file: file, line: line); return
                }
                field.coordinate(withNormalizedOffset: CGVector(dx: 0.98, dy: 0.5)).tap()
                field.typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: existing.utf16.count))
            }
            let cleared = field.value as? String ?? ""
            if !cleared.isEmpty && cleared != field.placeholderValue {
                let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
                attachment.name = "fixture-input-clear-failure"; attachment.lifetime = .keepAlways
                XCTContext.runActivity(named: "Input must be completely cleared") { $0.add(attachment) }
                XCTFail("Native keyboard selection did not clear the complete input", file: file, line: line); return
            }
        }
        if !value.isEmpty { field.typeText(value) }
        else if hasExistingText { field.typeText(XCUIKeyboardKey.delete.rawValue) }
        let actual = field.value as? String ?? ""
        XCTAssertTrue(value.isEmpty ? actual.isEmpty || actual == field.placeholderValue : actual == value,
                      "Input replacement did not preserve the requested value", file: file, line: line)
    }
}

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
        XCTAssertFalse(app.textFields["login.server"].exists)
        app.buttons["login.server.edit"].tap()
        let server = app.textFields["login.server"]
        UITestInput.replace(server, with: "invalid", app: app)
        app.buttons["login.server.edit"].tap()
        XCTAssertTrue(app.staticTexts["请输入有效的服务地址"].waitForExistence(timeout: 3))
        UITestInput.replace(server, with: "https://example.invalid", app: app)
        app.buttons["login.server.edit"].tap()
        XCTAssertFalse(app.textFields["login.server"].exists)
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
        XCTAssertTrue(app.buttons["settings.reset"].waitForExistence(timeout: 3))
        XCTAssertTrue(app.buttons["settings.close"].isHittable)
        app.buttons["settings.reset"].tap()
        XCTAssertTrue(app.staticTexts["16 pt"].exists)
        XCTAssertTrue(app.staticTexts["88%"].exists)
        app.sliders["文字大小"].adjust(toNormalizedSliderPosition: 1)
        app.sliders["浮窗不透明度"].adjust(toNormalizedSliderPosition: 0)
        XCTAssertTrue(app.staticTexts["24 pt"].exists)
        XCTAssertTrue(app.staticTexts["0%"].exists)
        app.terminate()
        app = launch(["--workspace-fixture", "--show-settings"])
        XCTAssertTrue(app.staticTexts["0%"].waitForExistence(timeout: 3))
        XCTAssertTrue(app.staticTexts["24 pt"].exists)
        app.buttons["settings.reset"].tap()
        XCTAssertTrue(app.staticTexts["16 pt"].exists)
        XCTAssertTrue(app.staticTexts["88%"].exists)
        capture("fixture-02-settings-restored")
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
    private func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: id).firstMatch
    }
    private func replaceText(_ field: XCUIElement, with value: String) {
        UITestInput.replace(field, with: value)
    }
    private func settingsFixture(_ scenario: String? = nil) -> XCUIApplication {
        var arguments = ["--workspace-fixture", "--settings-fixture", "--show-settings"]
        if let scenario { arguments.append("--settings-fixture-scenario=" + scenario) }
        return launch(arguments)
    }
    private func addProvider(_ app: XCUIApplication) {
        if !app.buttons["settings.llm"].isHittable { app.scrollViews.firstMatch.swipeDown() }
        XCTAssertTrue(app.buttons["settings.llm"].waitForExistence(timeout: 5)); app.buttons["settings.llm"].tap()
        XCTAssertTrue(app.buttons["provider.add"].waitForExistence(timeout: 5)); app.buttons["provider.add"].tap()
        replaceText(app.textFields["provider.id"], with: "ui-provider")
        replaceText(app.textFields["provider.endpoint"], with: "https://fixture.invalid/v1")
    }
    func testUnifiedDrawerAndIndependentGlobalEntrypoint() {
        let app = launch(["--workspace-fixture", "--settings-fixture"])
        XCTAssertTrue(app.buttons["workspace.drawer"].waitForExistence(timeout: 5))
        capture("fixture-01-terminal")
        app.buttons["workspace.drawer"].tap()
        XCTAssertTrue(app.buttons["workspace.close"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.textFields["workspace.search"].exists)
        XCTAssertFalse(app.segmentedControls.buttons["AI 历史"].exists)
        XCTAssertFalse(app.segmentedControls.buttons["终端"].exists)
        XCTAssertFalse(app.buttons["旧归档"].exists)
        capture("fixture-17-unified-workspace")
        app.buttons["workspace.close"].tap()
        app.buttons["workspace.global"].tap()
        XCTAssertTrue(element(app, "global.list").waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["global.create"].exists)
        XCTAssertFalse(app.segmentedControls.buttons["当前终端"].exists)
        capture("fixture-19-independent-global-list")
        app.buttons["global.back"].tap()
        XCTAssertTrue(app.buttons["workspace.chat"].exists)
    }
    func testSettingsSectionsReturnAndCancel() {
        let app = settingsFixture()
        XCTAssertTrue(element(app, "settings.home").waitForExistence(timeout: 5))
        capture("fixture-02-settings-home")
        for id in ["llm", "reading", "mcp", "skills", "account"] {
            let entry = app.buttons["settings." + id]
            if !entry.isHittable { app.scrollViews.firstMatch.swipeUp() }
            XCTAssertTrue(entry.waitForExistence(timeout: 5)); entry.tap()
            let back = app.buttons[id == "account" ? "panel.close" : "settings.back"]
            XCTAssertTrue(back.waitForExistence(timeout: 5))
            if id == "account" { capture("fixture-16-account-devices") }
            back.tap()
            XCTAssertTrue(element(app, "settings.home").waitForExistence(timeout: 5))
        }
        addProvider(app)
        // A fixture-only string: no real credential is read or asserted.
        app.secureTextFields["provider.key"].tap(); app.secureTextFields["provider.key"].typeText("fixture-only")
        app.buttons["settings.cancel"].tap()
        XCTAssertTrue(app.buttons["provider.add"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["provider.select.ui-provider"].exists)
        app.buttons["provider.add"].tap()
        let key = app.secureTextFields["provider.key"]
        XCTAssertTrue(key.waitForExistence(timeout: 5))
        XCTAssertTrue((key.value as? String ?? "").isEmpty || key.value as? String == "留空保留原凭据",
                      "Cancelled provider credential must be cleared")
    }
    func testClosedAndOfflineHistoryRemainReachableAndReadOnly() {
        let app = launch(["--workspace-fixture", "--settings-fixture"])
        app.buttons["workspace.drawer"].tap()
        for session in ["fixture-session", "fixture-closed", "fixture-offline"] {
            XCTAssertTrue(app.buttons["session.select." + session].waitForExistence(timeout: 5))
            XCTAssertTrue(app.buttons["session.history." + session].exists)
        }
        replaceText(app.textFields["workspace.search"], with: "fixture-closed")
        XCTAssertTrue(app.buttons["session.history.fixture-closed"].exists)
        XCTAssertFalse(app.buttons["session.history.fixture-offline"].exists)
        replaceText(app.textFields["workspace.search"], with: "cedar-body-only-731")
        XCTAssertTrue(app.buttons["session.history.fixture-closed"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["session.history.fixture-offline"].exists)
        replaceText(app.textFields["workspace.search"], with: "")
        for session in ["fixture-closed", "fixture-offline"] {
            app.buttons["session.history." + session].tap()
            XCTAssertTrue(element(app, "chat.session").waitForExistence(timeout: 5))
            XCTAssertTrue(app.staticTexts[session == "fixture-closed" ? "会话已关闭 · 只读" : "设备离线 · 只读缓存"].exists)
            replaceText(app.textFields["chat.draft"], with: session + "-draft")
            XCTAssertFalse(app.buttons["chat.send"].isEnabled)
            XCTAssertFalse(app.buttons["chat.stop"].isEnabled)
            XCTAssertFalse(app.switches["允许操作终端与扩展"].isEnabled)
            capture("fixture-17-" + session + "-read-only")
            app.buttons["panel.close"].tap(); app.buttons["workspace.drawer"].tap()
        }
        app.buttons["workspace.close"].tap(); app.buttons["workspace.chat"].tap()
        XCTAssertTrue(app.textFields["chat.draft"].waitForExistence(timeout: 5))
        XCTAssertFalse(["fixture-closed-draft", "fixture-offline-draft"].contains(app.textFields["chat.draft"].value as? String ?? ""))
    }
    func testAzureConditionAndValidationPreserveDraft() {
        let app = settingsFixture(); addProvider(app)
        XCTAssertFalse(app.textFields["provider.apiVersion"].exists)
        capture("fixture-04-provider-new")
        app.buttons["provider.protocol"].tap(); app.buttons["azure_openai"].tap()
        XCTAssertTrue(app.textFields["provider.apiVersion"].waitForExistence(timeout: 3))
        capture("fixture-05-provider-azure")
        app.buttons["settings.save"].tap()
        XCTAssertTrue(element(app, "settings.error").waitForExistence(timeout: 3))
        XCTAssertTrue(app.textFields["provider.id"].value as? String == "ui-provider")
        XCTAssertTrue(app.textFields["provider.endpoint"].value as? String == "https://fixture.invalid/v1")
        app.buttons["provider.protocol"].tap(); app.buttons["openai_responses"].tap()
        XCTAssertFalse(app.textFields["provider.apiVersion"].exists)
        app.buttons["settings.cancel"].tap()
        XCTAssertTrue(app.buttons["provider.select.openai"].waitForExistence(timeout: 5)); app.buttons["provider.select.openai"].tap()
        XCTAssertTrue(app.textFields["provider.id"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.textFields["provider.id"].isEnabled)
        capture("fixture-04-provider-edit")
        app.buttons["settings.cancel"].tap()
    }
    func testAgentBodySearchFindsClosedHistory() {
        let app = launch(["--workspace-fixture", "--settings-fixture", "--show-drawer"])
        replaceText(app.textFields["workspace.search"], with: "cedar-body-only-731")
        let match = app.buttons["session.history.fixture-closed"]
        let filtered = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in
            match.exists && !app.buttons["session.select.fixture-session"].exists && !app.buttons["session.select.fixture-offline"].exists
        }, object: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [filtered], timeout: 5), .completed)
        capture("fixture-17-agent-body-search")
        match.tap()
        XCTAssertTrue(element(app, "chat.session").waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "cedar-body-only-731")).firstMatch.waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["chat.send"].isEnabled)
    }
    func testClosedHistoryCanBeFoundByFullWorkingDirectory() {
        let app = launch(["--workspace-fixture", "--settings-fixture", "--show-drawer"])
        replaceText(app.textFields["workspace.search"], with: "/fixture/closed/workspace")
        let closed = app.buttons["session.history.fixture-closed"]
        XCTAssertTrue(closed.waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["session.history.fixture-offline"].exists)
        closed.tap()
        XCTAssertTrue(element(app, "chat.session").waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["/fixture/closed/workspace"].exists)
        XCTAssertFalse(app.buttons["chat.send"].isEnabled)
        capture("history-closed-full-directory")
    }
    func testFailedSaveKeepsNonSensitiveDraftAndReleasesBusy() {
        let app = settingsFixture("save-failure"); addProvider(app)
        app.buttons["settings.save"].tap()
        XCTAssertTrue(element(app, "settings.error").waitForExistence(timeout: 5))
        XCTAssertTrue(app.textFields["provider.id"].value as? String == "ui-provider")
        XCTAssertTrue(app.textFields["provider.endpoint"].value as? String == "https://fixture.invalid/v1")
        XCTAssertTrue(app.buttons["settings.save"].isEnabled)
        XCTAssertTrue(app.buttons["settings.cancel"].isEnabled)
        capture("fixture-04-provider-failed-draft")
    }
    func testSaveBusyPreventsDuplicateSubmissionAndActionsStayVisible() {
        let app = settingsFixture("save-delayed"); addProvider(app)
        let save = app.buttons["settings.save"], cancel = app.buttons["settings.cancel"]
        XCTAssertTrue(save.isHittable && cancel.isHittable, "Form actions must remain reachable with the keyboard")
        XCTAssertTrue(save.frame.maxY <= app.frame.maxY && cancel.frame.maxY <= app.frame.maxY)
        save.tap()
        XCTAssertTrue(element(app, "settings.busy").waitForExistence(timeout: 3))
        XCTAssertFalse(save.isEnabled)
        XCTAssertFalse(app.textFields["provider.endpoint"].isEnabled)
        XCTAssertTrue(app.buttons["provider.select.ui-provider"].waitForExistence(timeout: 10))
        capture("fixture-04-provider-saved")
    }
    func testProviderActionsWithLargeTextKeyboardAndLandscape() {
        let app = launch(["--workspace-fixture", "--settings-fixture", "--show-settings",
                          "-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXL"])
        addProvider(app)
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 3))
        XCTAssertTrue(app.buttons["settings.cancel"].isHittable && app.buttons["settings.save"].isHittable)
        capture("fixture-04-provider-large-text-keyboard")
        XCUIDevice.shared.orientation = .landscapeLeft
        let landscape = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in app.frame.width > app.frame.height }, object: nil)
        XCTAssertTrue(XCTWaiter.wait(for: [landscape], timeout: 5) == .completed)
        XCTAssertTrue(app.buttons["settings.cancel"].isHittable && app.buttons["settings.save"].isHittable)
        capture("fixture-04-provider-landscape-keyboard")
        app.buttons["settings.cancel"].tap()
    }
    func testReadingValidationAndExtensionCancelRoutes() {
        let app = settingsFixture()
        app.buttons["settings.reading"].tap()
        XCTAssertTrue(app.textFields["reading.head"].waitForExistence(timeout: 5))
        capture("fixture-11-terminal-reading")
        replaceText(app.textFields["reading.head"], with: "0")
        app.buttons["settings.save"].tap()
        XCTAssertTrue(element(app, "settings.error").waitForExistence(timeout: 3))
        XCTAssertTrue(app.textFields["reading.head"].value as? String == "0")
        app.buttons["settings.cancel"].tap()
        app.buttons["settings.mcp"].tap()
        XCTAssertTrue(app.buttons["mcp.import"].waitForExistence(timeout: 5)); app.buttons["mcp.import"].tap()
        app.textViews["mcp.json"].tap(); app.textViews["mcp.json"].typeText("invalid-json")
        app.buttons["settings.cancel"].tap()
        XCTAssertTrue(app.buttons["mcp.import"].waitForExistence(timeout: 3)); app.buttons["settings.back"].tap()
        app.buttons["settings.skills"].tap()
        XCTAssertTrue(app.buttons["skill.install"].waitForExistence(timeout: 5)); app.buttons["skill.install"].tap()
        replaceText(app.textFields["skill.id"], with: "cancelled-skill")
        replaceText(app.textFields["skill.path"], with: "/fixture/cancelled")
        capture("fixture-15-skill-install")
        app.buttons["settings.cancel"].tap()
        XCTAssertTrue(app.buttons["skill.install"].waitForExistence(timeout: 3))
        XCTAssertFalse(app.buttons["skill.select.cancelled-skill"].exists)
    }
    func testCurrentBindingCanReturnToInheritanceAndReadingSaveReturnsHome() {
        let app = settingsFixture(); app.buttons["settings.llm"].tap()
        XCTAssertTrue(app.buttons["bindings.open"].waitForExistence(timeout: 5))
        capture("fixture-03-llm")
        app.buttons["bindings.open"].tap()
        XCTAssertTrue(app.buttons["bindings.current"].waitForExistence(timeout: 5))
        capture("fixture-10-default-bindings")
        app.buttons["bindings.current"].tap(); app.buttons["fixture-two"].tap()
        app.buttons["settings.save"].tap()
        XCTAssertTrue(app.buttons["bindings.open"].waitForExistence(timeout: 5)); app.buttons["bindings.open"].tap()
        let current = app.buttons["bindings.current"]
        XCTAssertTrue(current.label.contains("fixture-two") || (current.value as? String ?? "").contains("fixture-two"))
        current.tap(); app.buttons["继承 Session 默认"].tap()
        app.buttons["settings.save"].tap()
        XCTAssertTrue(app.buttons["bindings.open"].waitForExistence(timeout: 5)); app.buttons["bindings.open"].tap()
        XCTAssertTrue(current.label.contains("继承") || (current.value as? String ?? "").contains("继承"))
        let global = app.buttons["bindings.global"]
        XCTAssertTrue(global.label.contains("fixture-model") || (global.value as? String ?? "").contains("fixture-model"))
        app.buttons["settings.cancel"].tap(); app.buttons["settings.back"].tap()
        app.buttons["settings.reading"].tap()
        replaceText(app.textFields["reading.head"], with: "100")
        replaceText(app.textFields["reading.tail"], with: "1")
        app.buttons["settings.save"].tap()
        XCTAssertTrue(element(app, "settings.home").waitForExistence(timeout: 5))
    }
    func testCatalogPaginationQueryResetAndSelectionReturnsToModel() {
        let app = settingsFixture("catalog")
        app.buttons["settings.llm"].tap()
        XCTAssertTrue(app.buttons["model.select.fixture-model"].waitForExistence(timeout: 5)); app.buttons["model.select.fixture-model"].tap()
        XCTAssertTrue(app.textFields["model.name"].waitForExistence(timeout: 5))
        capture("fixture-06-model")
        element(app, "model.advanced").tap()
        XCTAssertTrue(app.textFields["model.context"].waitForExistence(timeout: 3))
        capture("fixture-07-model-advanced")
        element(app, "model.advanced").tap()
        app.buttons["catalog.open"].tap()
        XCTAssertTrue(app.buttons["catalog.select.fixture-one"].waitForExistence(timeout: 5))
        capture("fixture-08-model-catalog")
        app.buttons["catalog.next"].tap()
        XCTAssertTrue(app.buttons["catalog.select.fixture-page-two"].waitForExistence(timeout: 5))
        capture("fixture-09-model-catalog-page-two")
        XCTAssertFalse(app.buttons["catalog.next"].exists)
        replaceText(app.textFields["catalog.search"], with: "filter")
        app.buttons["catalog.submit"].tap()
        XCTAssertTrue(app.buttons["catalog.select.fixture-one"].waitForExistence(timeout: 5))
        // Editing a query invalidates its cursor without changing the model draft.
        replaceText(app.textFields["catalog.search"], with: "changed")
        XCTAssertFalse(app.buttons["catalog.next"].exists)
        app.buttons["catalog.submit"].tap()
        XCTAssertTrue(app.buttons["catalog.select.fixture-one"].waitForExistence(timeout: 5)); app.buttons["catalog.select.fixture-one"].tap()
        XCTAssertTrue(app.textFields["model.name"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.textFields["model.name"].value as? String == "fixture-one")
        XCTAssertFalse(app.textFields["catalog.search"].exists)
        app.buttons["settings.cancel"].tap()
        XCTAssertTrue(app.buttons["model.select.fixture-model"].waitForExistence(timeout: 5))
    }
    func testSessionAndGlobalDraftsStaySeparateAndReturnToList() {
        let app = launch(["--workspace-fixture", "--settings-fixture"])
        app.buttons["workspace.global"].tap()
        XCTAssertTrue(app.buttons["global.select.fixture-global"].waitForExistence(timeout: 5)); app.buttons["global.select.fixture-global"].tap()
        XCTAssertTrue(element(app, "chat.global").waitForExistence(timeout: 5))
        replaceText(app.textFields["chat.draft"], with: "global-only-draft")
        app.buttons["global.back"].tap()
        XCTAssertTrue(element(app, "global.list").waitForExistence(timeout: 5)); app.buttons["global.back"].tap()
        app.buttons["workspace.chat"].tap()
        XCTAssertTrue(element(app, "chat.session").waitForExistence(timeout: 5))
        XCTAssertFalse(app.textFields["chat.draft"].value as? String == "global-only-draft")
        replaceText(app.textFields["chat.draft"], with: "session-only-draft")
        XCTAssertFalse(app.segmentedControls.buttons["全局"].exists)
        capture("fixture-18-session-chat")
        app.buttons["panel.close"].tap(); app.buttons["workspace.global"].tap()
        XCTAssertTrue(app.buttons["global.select.fixture-global"].waitForExistence(timeout: 5)); app.buttons["global.select.fixture-global"].tap()
        XCTAssertTrue(app.textFields["chat.draft"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.textFields["chat.draft"].value as? String == "global-only-draft")
        capture("fixture-20-global-draft-isolation")
        app.buttons["global.back"].tap(); app.buttons["global.create"].tap()
        XCTAssertTrue(element(app, "chat.global").waitForExistence(timeout: 5))
        XCTAssertFalse(app.textFields["chat.draft"].value as? String == "global-only-draft")
        replaceText(app.textFields["chat.draft"], with: "second-global-draft")
        app.buttons["global.back"].tap()
        XCTAssertTrue(app.buttons["global.select.fixture-global"].waitForExistence(timeout: 5)); app.buttons["global.select.fixture-global"].tap()
        XCTAssertTrue(app.textFields["chat.draft"].value as? String == "global-only-draft")
    }
    func testFixtureMcpImportDeleteConfirmationAndSkillEdit() {
        let app = settingsFixture(); app.buttons["settings.mcp"].tap()
        XCTAssertTrue(app.buttons["mcp.import"].waitForExistence(timeout: 5))
        capture("fixture-12-mcp-list")
        app.buttons["mcp.import"].tap()
        replaceText(app.textViews["mcp.json"], with: #"{"mcpServers":{"ui-mcp":{"command":"fixture","args":[],"envSecretRefs":{"TOKEN":"vault/fixture"},"enabled":true}}}"#)
        capture("fixture-13-mcp-import")
        app.buttons["settings.save"].tap()
        XCTAssertTrue(app.buttons["mcp.select.ui-mcp"].waitForExistence(timeout: 5)); app.buttons["mcp.select.ui-mcp"].tap()
        XCTAssertTrue(app.buttons["extension.delete"].waitForExistence(timeout: 5))
        capture("fixture-12-mcp-detail")
        app.buttons["extension.delete"].tap()
        XCTAssertTrue(app.alerts.buttons["取消"].waitForExistence(timeout: 3)); app.alerts.buttons["取消"].tap()
        app.buttons["settings.back"].tap()
        XCTAssertTrue(app.buttons["mcp.select.ui-mcp"].exists)
        app.buttons["mcp.select.ui-mcp"].tap(); app.buttons["extension.delete"].tap()
        app.alerts.buttons["extension.delete.confirm"].tap()
        XCTAssertTrue(app.buttons["mcp.import"].waitForExistence(timeout: 5))
        XCTAssertFalse(app.buttons["mcp.select.ui-mcp"].exists)
        app.buttons["settings.back"].tap(); app.buttons["settings.skills"].tap()
        XCTAssertTrue(app.buttons["skill.select.user/fixture"].waitForExistence(timeout: 5))
        capture("fixture-14-skills-list")
        app.buttons["skill.select.user/fixture"].tap()
        XCTAssertTrue(app.buttons["extension.edit"].waitForExistence(timeout: 5))
        capture("fixture-14-skill-detail")
        app.buttons["extension.edit"].tap()
        XCTAssertTrue(app.textViews["skill.body"].waitForExistence(timeout: 5))
        replaceText(app.textViews["skill.body"], with: "# Fixture edited\n\nKeep package resources.")
        app.buttons["settings.save"].tap()
        XCTAssertTrue(app.buttons["extension.edit"].waitForExistence(timeout: 5))
        app.buttons["extension.edit"].tap()
        XCTAssertTrue(app.textViews["skill.body"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.textViews["skill.body"].value as? String == "# Fixture edited\n\nKeep package resources.")
        // Fixture UI verifies routes and command completion; real resource SHA256 is separate.
        capture("fixture-15-skill-editor")
    }
    func testDevicePanelHidesOfflineDevices() {
        let app = launch(["--workspace-fixture", "--devices-fixture"])
        app.buttons["选择设备"].tap()
        XCTAssertTrue(app.staticTexts["Online Desktop"].waitForExistence(timeout: 3))
        XCTAssertFalse(app.staticTexts["Old iPhone"].exists)
        XCTAssertFalse(app.staticTexts["离线"].exists)
        capture("online-devices-only")
    }
}

// Real Desktop RPC/PTY tests. These never authenticate, log out, clear an identity,
// create/close a session. Extension mutations additionally require the two
// disposable UUID IDs; the coordinator owns host verification and failure cleanup.
final class LiveServiceUITests: XCTestCase {
    private struct Fixture: Decodable {
        let session: String
        let typingMarker: String?
        let caPem: String?
        let caPemPath: String?
        let mcpId: String?
        let skillId: String?
        let skillPath: String?
    }
    override func setUp() { continueAfterFailure = false }
    private func wait(_ timeout: TimeInterval = 20, _ predicate: @escaping () -> Bool, file: StaticString = #filePath, line: UInt = #line) {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in predicate() }, object: nil)
        XCTAssertTrue(XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed,
                      "Dedicated fixture did not reach the expected state", file: file, line: line)
    }
    private func terminal(_ app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: "terminal.screen").firstMatch
    }
    private func text(_ app: XCUIApplication) -> String { terminal(app).value as? String ?? "" }
    private func hasLine(_ app: XCUIApplication, _ line: String) -> Bool {
        text(app).components(separatedBy: .newlines).contains { $0.trimmingCharacters(in: .whitespaces) == line }
    }
    private func disposableID(_ id: String) -> Bool {
        id.range(of: "^[A-Za-z0-9_-]{36,128}$", options: .regularExpression) != nil
            && !id.hasPrefix("builtin") && UUID(uuidString: String(id.suffix(36))) != nil
    }
    private func fixtureApp(requiredExtensions: Bool = false) throws -> (XCUIApplication, Fixture) {
        guard let path = ProcessInfo.processInfo.environment["AI_TERMINAL_IOS_FIXTURE"],
              path.hasPrefix("/"), let data = try? Data(contentsOf: URL(fileURLWithPath: path)),
              let fixture = try? JSONDecoder().decode(Fixture.self, from: data),
              !fixture.session.isEmpty, let marker = fixture.typingMarker, marker.utf8.count >= 16 else {
            throw XCTSkip("Pre-attached disposable integration fixture with a unique marker not supplied")
        }
        if requiredExtensions {
            guard let mcp = fixture.mcpId, let skill = fixture.skillId, let path = fixture.skillPath,
                  disposableID(mcp), disposableID(skill), mcp != skill, path.hasPrefix("/"), !path.contains("\0") else {
                throw XCTSkip("Disposable UUID MCP/Skill IDs and absolute Desktop Skill path not supplied")
            }
        }
        let app = XCUIApplication(); app.launchArguments = ["--service-test"]
        if let path = fixture.caPemPath {
            guard path.hasPrefix("/"), let ca = try? String(contentsOf: URL(fileURLWithPath: path), encoding: .utf8) else {
                throw XCTSkip("Dedicated fixture CA file is unavailable")
            }
            app.launchEnvironment["AI_TERMINAL_TEST_CA_PEM"] = ca
        } else if let caPem = fixture.caPem { app.launchEnvironment["AI_TERMINAL_TEST_CA_PEM"] = caPem }
        app.launch()
        XCTAssertTrue(terminal(app).waitForExistence(timeout: 30), "Attach the isolated service-test PTY before running live tests")
        wait(30) { self.hasLine(app, marker) }
        // Verify the selected session as well as the marker before emitting input.
        app.buttons["workspace.drawer"].tap()
        XCTAssertTrue(app.buttons["session.select." + fixture.session].waitForExistence(timeout: 10),
                      "Dedicated fixture session is absent")
        app.buttons["session.select." + fixture.session].tap()
        wait { self.hasLine(app, marker) }
        return (app, fixture)
    }
    private func specialKey(_ key: String, app: XCUIApplication) {
        app.buttons["workspace.keys"].tap()
        app.buttons["terminal.key." + key].tap()
    }
    private func tap(_ id: String, app: XCUIApplication) {
        let button = app.buttons[id]
        XCTAssertTrue(button.waitForExistence(timeout: 20), "Expected fixture action is unavailable")
        if !button.isHittable { app.scrollViews.firstMatch.swipeUp() }
        XCTAssertTrue(button.isHittable, "Expected fixture action is not reachable")
        button.tap()
    }
    private func replaceText(_ field: XCUIElement, with value: String) {
        UITestInput.replace(field, with: value)
    }
    private func json(_ value: [String: Any]) throws -> String {
        String(decoding: try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]), as: UTF8.self)
    }
    private func toggleTwice(_ app: XCUIApplication, first: String, second: String) {
        let toggle = app.buttons["extension.toggle"]
        wait { toggle.isEnabled && toggle.label == first }
        toggle.tap()
        wait { toggle.isEnabled && toggle.label == second }
        toggle.tap()
        wait { toggle.isEnabled && toggle.label == first }
    }
    private func deleteExtension(_ app: XCUIApplication, row: XCUIElement, listAction: String) {
        tap("extension.delete", app: app)
        let confirm = app.alerts.buttons["extension.delete.confirm"]
        XCTAssertTrue(confirm.waitForExistence(timeout: 5)); confirm.tap()
        wait { app.buttons[listAction].exists && !row.exists }
    }
    func testKeyboardInAttachedFixture() throws {
        let (app, _) = try fixtureApp()
        terminal(app).tap()
        app.typeText("printf 'IOS_KEYBOARD_FIXED\\n'")
        XCTAssertTrue(app.keyboards.buttons["Return"].waitForExistence(timeout: 3))
        app.keyboards.buttons["Return"].tap()
        wait { self.hasLine(app, "IOS_KEYBOARD_FIXED") }
        // Atomic typing, Unicode, Tab and Ctrl-C must remain on the same fixture PTY.
        app.typeText("git stat")
        wait { self.text(app).contains("git stat") }
        app.typeText(XCUIKeyboardKey.delete.rawValue + "\t")
        wait { self.text(app).contains("status") }
        specialKey("ctrl_c", app: app)
    }
    func testAttachedFixtureUnifiedWorkspaceAndSettingsRoutes() throws {
        let (app, _) = try fixtureApp()
        app.buttons["workspace.drawer"].tap()
        XCTAssertFalse(app.segmentedControls.buttons["AI 历史"].exists)
        XCTAssertFalse(app.buttons["旧归档"].exists)
        app.buttons["workspace.close"].tap()
        app.buttons["workspace.settings"].tap()
        XCTAssertTrue(app.buttons["settings.llm"].waitForExistence(timeout: 5))
        app.buttons["settings.llm"].tap()
        XCTAssertTrue(app.buttons["bindings.open"].waitForExistence(timeout: 20))
        app.buttons["settings.back"].tap()
        XCTAssertTrue(app.buttons["settings.reading"].exists)
        app.buttons["settings.close"].tap()
        app.buttons["workspace.chat"].tap()
        XCTAssertFalse(app.staticTexts["AI 助手即将开放"].exists)
        XCTAssertFalse(app.segmentedControls.buttons["全局"].exists)
        XCTAssertFalse(app.buttons["旧归档"].exists)
        // No Agent send/cancel: live configuration and history remain read-only here.
    }
    func testDisposableMcpAndSkillProductionFormsRoundTrip() throws {
        let (app, fixture) = try fixtureApp(requiredExtensions: true)
        let mcpID = fixture.mcpId!, skillID = fixture.skillId!, skillPath = fixture.skillPath!
        tap("workspace.settings", app: app); tap("settings.mcp", app: app)
        XCTAssertTrue(app.buttons["mcp.import"].waitForExistence(timeout: 20))
        let mcpRow = app.buttons["mcp.select." + mcpID]
        XCTAssertFalse(mcpRow.exists, "Disposable MCP already exists; host cleanup is required")
        var mcp: [String: Any] = ["transport": "streamable_http", "url": "http://localhost:9/mcp", "enabled": false,
                                 "startup_timeout_ms": 10000, "call_timeout_ms": 30000]
        tap("mcp.import", app: app)
        replaceText(app.textViews["mcp.json"], with: try json(["mcpServers": [mcpID: mcp]]))
        tap("settings.save", app: app)
        XCTAssertTrue(mcpRow.waitForExistence(timeout: 20)); mcpRow.tap()
        tap("extension.edit", app: app)
        mcp["call_timeout_ms"] = 12345
        replaceText(app.textViews["mcp.json"], with: try json(mcp))
        tap("settings.save", app: app); tap("extension.edit", app: app)
        let editor = app.textViews["mcp.json"]
        XCTAssertTrue(editor.waitForExistence(timeout: 20))
        let saved = (editor.value as? String).flatMap { try? JSONSerialization.jsonObject(with: Data($0.utf8)) as? [String: Any] }
        XCTAssertTrue(saved?["call_timeout_ms"] as? Int == 12345, "Disposable MCP timeout did not round-trip")
        tap("settings.cancel", app: app)
        toggleTwice(app, first: "启用", second: "停用")
        deleteExtension(app, row: mcpRow, listAction: "mcp.import")

        tap("settings.back", app: app); tap("settings.skills", app: app)
        XCTAssertTrue(app.buttons["skill.install"].waitForExistence(timeout: 20))
        let skillRow = app.buttons["skill.select." + skillID]
        XCTAssertFalse(skillRow.exists, "Disposable Skill already exists; host cleanup is required")
        tap("skill.install", app: app)
        replaceText(app.textFields["skill.id"], with: skillID)
        replaceText(app.textFields["skill.path"], with: skillPath)
        tap("settings.save", app: app)
        XCTAssertTrue(skillRow.waitForExistence(timeout: 30)); skillRow.tap()
        tap("extension.edit", app: app)
        let body = "---\nname: ios-ui-extension-fixture\ndescription: Dedicated disposable iOS UI regression package.\n---\n# UI round-trip edited\n\nOther package resources must remain intact.\n"
        replaceText(app.textViews["skill.body"], with: body)
        tap("settings.save", app: app); tap("extension.edit", app: app)
        XCTAssertTrue(app.textViews["skill.body"].waitForExistence(timeout: 20))
        XCTAssertTrue(app.textViews["skill.body"].value as? String == body, "Disposable SKILL.md did not round-trip")
        tap("settings.cancel", app: app)
        toggleTwice(app, first: "停用", second: "启用")
        deleteExtension(app, row: skillRow, listAction: "skill.install")
        tap("settings.back", app: app); tap("settings.close", app: app)
        // UI completion proves these UUID operations, not package/config integrity.
        // The coordinator observes actual revisions/hashes and cleans up on failure.
        // No Agent send, terminal command, authentication, or credential/config dump.
    }
}

final class AuthorizationUITests: XCTestCase {
    override func setUp() { continueAfterFailure = false }
    private func open(_ scenario: String = "", large: Bool = false) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = ["--workspace-fixture", "--settings-fixture", "--authorization-fixture", "--authorization-scenario=" + scenario, "--authorization-fixture-id=" + UUID().uuidString]
        app.launchArguments += ["-UIPreferredContentSizeCategoryName", large ? "UICTContentSizeCategoryAccessibilityXXXL" : "UICTContentSizeCategoryL"]
        app.launch()
        app.buttons["workspace.global"].tap()
        let row = app.buttons["global.select.fixture-global"]
        XCTAssertTrue(row.waitForExistence(timeout: 10)); row.tap()
        XCTAssertTrue(app.buttons["authorization.open"].waitForExistence(timeout: 10))
        return app
    }
    private func tap(_ id: String, _ app: XCUIApplication) {
        let button = app.buttons[id]
        XCTAssertTrue(button.waitForExistence(timeout: 10), id)
        var previousAbove = false
        for _ in 0..<20 {
            if button.exists && button.isHittable && button.isEnabled { break }
            let timeline = app.scrollViews["chat.timeline"]
            let scroll = timeline.exists ? timeline : app.scrollViews.firstMatch
            let exists = button.exists
            let above = exists ? button.frame.midY < scroll.frame.midY : !previousAbove
            let distance = exists ? abs(button.frame.midY - scroll.frame.midY) : 0
            previousAbove = above
            if distance > scroll.frame.height * 2 {
                if above { scroll.swipeDown() } else { scroll.swipeUp() }
            } else {
                let start = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: above ? 0.2 : 0.8))
                let end = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: above ? 0.8 : 0.2))
                start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0.3)
            }
        }
        XCTAssertTrue(button.exists && button.isHittable && button.isEnabled, id); button.tap()
    }
    private func wait(_ condition: @escaping () -> Bool) {
        let result = XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)], timeout: 10)
        XCTAssertEqual(result, .completed)
    }
    func testPermanentRuleAndConversationFullAuthorization() {
        let app = open()
        XCTAssertTrue(app.staticTexts["目标 Session：fixture-delegated-session"].firstMatch.waitForExistence(timeout: 10))
        tap("authorization.resolve.always", app)
        tap("authorization.open", app)
        let full = app.switches["authorization.full"]
        XCTAssertTrue(full.waitForExistence(timeout: 10)); wait { full.isEnabled }
        XCTAssertEqual(full.value as? String, "0"); full.coordinate(withNormalizedOffset: CGVector(dx: 0.93, dy: 0.5)).tap(); wait { full.value as? String == "1" }
        tap("authorization.close", app)
        tap("authorization.open", app)
        XCTAssertEqual(full.value as? String, "1")
        full.coordinate(withNormalizedOffset: CGVector(dx: 0.93, dy: 0.5)).tap(); wait { full.value as? String == "0" }
        tap("authorization.revoke.fixture-rule", app)
        wait { !app.buttons["authorization.revoke.fixture-rule"].exists }
    }
    func testOnceQuestionOptionAndFreeTextWithKeyboard() {
        let app = open()
        tap("authorization.resolve.once", app)
        tap("authorization.option.0", app)
        let answer = app.textFields["authorization.answer"]
        XCTAssertEqual(answer.value as? String, "Markdown")
        UITestInput.replace(answer, with: "Custom output", app: app)
        if app.buttons["authorization.answer.keyboard"].exists { tap("authorization.answer.keyboard", app) }
        else { tap("authorization.answer.send", app) }
        wait { app.staticTexts["Fixture result: Custom output"].exists }
        XCTAssertFalse(app.textFields["authorization.answer"].exists)
    }
    func testNonPermanentDenyAndStopAtLargeText() {
        let app = open("nonpermanent", large: true)
        let always = app.buttons["authorization.resolve.always"]
        for _ in 0..<6 { if always.exists { break }; app.scrollViews["chat.timeline"].swipeDown() }
        XCTAssertTrue(always.waitForExistence(timeout: 10))
        XCTAssertFalse(always.isEnabled)
        tap("authorization.resolve.deny", app)
        tap("authorization.stop", app)
        wait { app.staticTexts["Fixture result: cancelled"].exists }
        let screenshot = XCTAttachment(screenshot: XCUIScreen.main.screenshot()); screenshot.lifetime = .keepAlways; add(screenshot)
    }
    func testLongDetailsAreRequiredBeforeApproval() {
        let app = open("long")
        XCTAssertTrue(app.buttons["authorization.resolve.once"].waitForExistence(timeout: 10))
        wait { app.buttons["authorization.resolve.once"].isEnabled }
        XCTAssertTrue(app.staticTexts["Complete command: /usr/bin/printf literal detail end"].exists)
        tap("authorization.resolve.once", app)
        wait { app.staticTexts["Fixture result: once"].exists }
    }
    func testFailedDetailsKeepDenyAvailable() {
        let app = open("long-failure")
        XCTAssertTrue(app.staticTexts["authorization.details.status"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["authorization.resolve.once"].isEnabled)
        XCTAssertFalse(app.buttons["authorization.resolve.always"].isEnabled)
        tap("authorization.resolve.deny", app)
        wait { app.staticTexts["Fixture result: deny"].exists }
    }
    func testReadOnlyDeviceCanBrowseButCannotRespond() {
        let app = open("readonly")
        XCTAssertTrue(app.buttons["authorization.resolve.once"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["authorization.resolve.once"].isEnabled)
        XCTAssertFalse(app.buttons["authorization.resolve.deny"].isEnabled)
        XCTAssertFalse(app.buttons["authorization.answer.send"].isEnabled)
        tap("authorization.open", app)
        XCTAssertTrue(app.staticTexts["authorization.readonly"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.switches["authorization.full"].isEnabled)
    }
    func testUnsupportedDesktopDoesNotFallBack() {
        let app = open("unsupported")
        XCTAssertTrue(app.staticTexts["authorization.error"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.buttons["chat.send"].isEnabled)
        tap("authorization.open", app)
        XCTAssertTrue(app.switches["authorization.full"].waitForExistence(timeout: 10))
        XCTAssertFalse(app.switches["authorization.full"].isEnabled)
    }
}

// Uses the production encrypted Agent channel. The runner supplies a disposable
// account via the existing service-test login path and verifies PTY files itself.
final class AuthorizationRpcUITests: XCTestCase {
    private struct Context: Decodable { let session: String; let prefix: String; let stateDirectory: String }
    override func setUp() { continueAfterFailure = false }
    private func wait(_ timeout: TimeInterval = 40, file: StaticString = #filePath, line: UInt = #line, _ condition: @escaping () -> Bool) {
        let result = XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in condition() }, object: nil)], timeout: timeout)
        XCTAssertEqual(result, .completed, file: file, line: line)
    }
    private func tap(_ id: String, _ app: XCUIApplication) {
        let button = app.buttons[id]
        XCTAssertTrue(button.waitForExistence(timeout: 30), id)
        if !button.isHittable { app.scrollViews["chat.timeline"].swipeUp(velocity: .slow) }
        button.tap()
    }
    private func open() throws -> (XCUIApplication, Context) {
        guard let text = ProcessInfo.processInfo.environment["AI_TERMINAL_IOS_AUTHORIZATION_CONTEXT"],
              let context = try? JSONDecoder().decode(Context.self, from: Data(text.utf8)),
              UUID(uuidString: context.session) != nil,
              context.prefix.range(of: "^ios-auth-[a-f0-9]{12}$", options: .regularExpression) != nil else {
            throw XCTSkip("Dedicated authorization RPC fixture not supplied")
        }
        let app = XCUIApplication(); app.launchArguments = ["--service-test", "--local-login-fixture", "-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryL"]
        app.launch()
        XCTAssertTrue(app.buttons["workspace.drawer"].waitForExistence(timeout: 40))
        tap("workspace.drawer", app)
        XCTAssertTrue(app.buttons["session.select." + context.session].waitForExistence(timeout: 30))
        tap("session.select." + context.session, app)
        XCTAssertTrue(app.buttons["workspace.chat"].waitForExistence(timeout: 30))
        tap("workspace.chat", app)
        XCTAssertTrue(app.buttons["authorization.open"].waitForExistence(timeout: 30))
        return (app, context)
    }
    private func task(_ suffix: String, tool: String = "run_command", arguments: [String: Any], context: Context, app: XCUIApplication) throws -> String {
        let id = context.prefix + "-" + suffix
        let value: [String: Any] = ["id": id, "steps": [["tool": tool, "arguments": arguments]]]
        let text = "AUTH_REVIEW:" + String(decoding: try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]), as: UTF8.self)
        UITestInput.replace(app.textFields["chat.draft"], with: text, app: app)
        wait { app.buttons["chat.send"].isEnabled }; tap("chat.send", app)
        return id
    }
    private func done(_ id: String, _ app: XCUIApplication) {
        wait(50) { app.staticTexts.matching(NSPredicate(format: "label CONTAINS %@", "AUTH_REVIEW_DONE:" + id)).firstMatch.exists }
    }
    private func command(_ marker: String, file: String, context: Context) -> [String: Any] {
        ["command": "/usr/bin/printf '\(marker)\\n' >> \(context.prefix)-\(file).log"]
    }
    private func notExecuted(_ suffix: String, context: Context) {
        XCTAssertTrue(context.stateDirectory.hasPrefix("/"))
        let root = URL(fileURLWithPath: context.stateDirectory, isDirectory: true)
        XCTAssertTrue(FileManager.default.fileExists(atPath: root.appendingPathComponent("account-fixture.json").path), "The test runner cannot observe the disposable Desktop")
        XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent(context.prefix + "-" + suffix + ".log").path), "Action executed before its approval")
    }
    private func setFull(_ full: Bool, app: XCUIApplication) {
        tap("authorization.open", app)
        let control = app.switches["authorization.full"]
        XCTAssertTrue(control.waitForExistence(timeout: 20)); wait { control.isEnabled }
        let expected = full ? "1" : "0"
        if control.value as? String != expected { control.coordinate(withNormalizedOffset: CGVector(dx: 0.93, dy: 0.5)).tap() }
        wait { control.value as? String == expected }
        tap("authorization.close", app)
    }
    func testEncryptedApprovalQuestionRulesFullAndRecovery() throws {
        let (app, context) = try open()
        setFull(false, app: app)
        let once = try task("once", arguments: command("once", file: "once", context: context), context: context, app: app)
        XCTAssertTrue(app.buttons["authorization.resolve.once"].waitForExistence(timeout: 30))
        // Reopen the conversation while the Host is suspended; it must recover the request.
        tap("panel.close", app); tap("workspace.chat", app)
        XCTAssertTrue(app.buttons["authorization.resolve.once"].waitForExistence(timeout: 30))
        notExecuted("once", context: context)
        tap("authorization.resolve.once", app); done(once, app)
        let denied = try task("deny", arguments: command("denied", file: "deny", context: context), context: context, app: app)
        notExecuted("deny", context: context)
        tap("authorization.resolve.deny", app); done(denied, app)
        let question = try task("question", tool: "ask_user", arguments: ["question": "Choose output", "options": ["Markdown", "Plain text"]], context: context, app: app)
        tap("authorization.option.0", app)
        UITestInput.replace(app.textFields["authorization.answer"], with: "iOS custom answer", app: app)
        let answerButton = app.buttons["authorization.answer.keyboard"].exists ? "authorization.answer.keyboard" : "authorization.answer.send"
        tap(answerButton, app); done(question, app)
        let exact: [String: Any] = ["program": "/usr/bin/tee", "args": ["-a", context.prefix + "-always.log"], "stdin": "always\n"]
        let first = try task("always-first", tool: "run_program", arguments: exact, context: context, app: app)
        XCTAssertTrue(app.buttons["authorization.resolve.always"].waitForExistence(timeout: 30))
        XCTAssertTrue(app.buttons["authorization.resolve.always"].isEnabled, "Fixture command has no stable execution identity")
        notExecuted("always", context: context)
        tap("authorization.resolve.always", app); done(first, app)
        let repeated = try task("always-repeat", tool: "run_program", arguments: exact, context: context, app: app)
        done(repeated, app)
        XCTAssertFalse(app.buttons["authorization.resolve.once"].exists, "Exact permanent rule failed")
        tap("authorization.open", app)
        let revoke = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "authorization.revoke.")).firstMatch
        XCTAssertTrue(revoke.waitForExistence(timeout: 20)); revoke.tap()
        wait { !revoke.exists }; tap("authorization.close", app)
        let revoked = try task("revoked", tool: "run_program", arguments: exact, context: context, app: app)
        tap("authorization.resolve.deny", app); done(revoked, app)
        let regranted = try task("always-regrant", tool: "run_program", arguments: exact, context: context, app: app)
        tap("authorization.resolve.always", app); done(regranted, app)
        tap("authorization.open", app)
        let secondRevoke = app.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "authorization.revoke.")).firstMatch
        XCTAssertTrue(secondRevoke.waitForExistence(timeout: 20)); secondRevoke.tap()
        wait { !secondRevoke.exists }; tap("authorization.close", app)
        setFull(true, app: app)
        let full = try task("full", arguments: command("full", file: "full", context: context), context: context, app: app)
        done(full, app); XCTAssertFalse(app.buttons["authorization.resolve.once"].exists)
        // A second independent Run inherits the same server-backed conversation toggle.
        let secondFull = try task("full-second", arguments: command("full", file: "full", context: context), context: context, app: app)
        done(secondFull, app); setFull(false, app: app)
        let afterFull = try task("full-off", arguments: command("blocked", file: "full-off", context: context), context: context, app: app)
        tap("authorization.resolve.deny", app); done(afterFull, app)
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot()); attachment.name = "encrypted-authorization-final"; attachment.lifetime = .keepAlways; add(attachment)
    }
}
