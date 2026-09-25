import UIKit

// Standalone simulator probe links the production TerminalView and Rust bindings,
// without launching the account/session model or connecting to a user's shell.
enum WorkspacePreferences { static let serviceTest = false }
func terminalError(_ error: Error) -> String { error.localizedDescription }

@main
final class TerminalInputChecks: UIResponder, UIApplicationDelegate {
    var window: UIWindow?
    static func main() {
        UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(TerminalInputChecks.self))
    }
    func application(_ application: UIApplication, didFinishLaunchingWithOptions options: [UIApplication.LaunchOptionsKey: Any]?) -> Bool {
        window = UIWindow(frame: UIScreen.main.bounds)
        window?.rootViewController = UIViewController()
        window?.makeKeyAndVisible()
        DispatchQueue.main.async { self.checkInput(); exit(0) }
        return true
    }
    private func checkInput() {
        let view = TerminalView(frame: .zero)
        view.canInput = true
        let field = view.subviews.compactMap { $0 as? UITextField }.first!
        var events: [String] = []
        view.onText = { events.append("text:\($0)"); return true }
        view.onKey = { events.append("key:\($0)") }
        view.onPaste = { events.append("paste:\($0)"); return true }
        let commits = [
            ("git stat", "text:git stat"), ("中文🙂", "text:中文🙂"),
            ("\t", "key:tab"), ("\n", "key:enter"), ("\r\n", "key:enter"),
            ("git\tstatus", "paste:git\tstatus"),
            ("printf one\r\nprintf two\n", "paste:printf one\r\nprintf two\n")
        ]
        for (text, expected) in commits {
            events = []
            view.committedText(text)
            precondition(events == [expected], "Unexpected committed-text routing: \(events)")
        }
        events = []
        field.text = "中文🙂"
        field.sendActions(for: .editingChanged)
        precondition(events == ["text:中文🙂"], "UITextField edits must reach typing")
        events = []
        view.onPaste = { events.append("rejected:\($0)"); return false }
        view.committedText("failed\tcommand\n")
        precondition(events == ["rejected:failed\tcommand\n"], "Rejected paste must not emit Enter or Tab")
        view.onPaste = { events.append("paste:\($0)"); return true }
        let saved = UIPasteboard.general.items
        defer { UIPasteboard.general.items = saved }
        for text in ["\t", "\n", "printf one\r\nprintf two\n", "中文🙂"] {
            UIPasteboard.general.string = text
            events = []
            field.paste(nil)
            precondition(events == ["paste:\(text)"], "UITextField paste must bypass typing")
            events = []
            view.paste(nil)
            precondition(events == ["paste:\(text)"], "Terminal paste must preserve the clipboard")
        }
        events = []
        view.canInput = false
        field.paste(nil)
        view.paste(nil)
        precondition(events.isEmpty, "Read-only paste must not send input")
        print("PASS: UIKit typing, standalone keys, atomic batch paste, clipboard paste, rejection and read-only input")
    }
}
