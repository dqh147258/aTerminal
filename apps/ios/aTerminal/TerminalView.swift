import UIKit
import SwiftUI
import os.signpost


struct TerminalSurface: UIViewRepresentable {
    let frame: RenderFrame
    var zoom: Double = 1
    var generation: Int = 0
    var core: RemoteTerminal? = nil
    var canInput = false
    var keyboardRequested = false
    var onText: ((String) -> Bool)? = nil
    var onPaste: ((String) -> Bool)? = nil
    var onKey: ((String) -> Void)? = nil
    var onKeyboardChange: ((Bool) -> Void)? = nil
    var onReadOnly: (() -> Void)? = nil
    var onStatus: ((RenderFrame?, String, Bool, String?) -> Void)? = nil
    var onOpenWorkspace: (() -> Void)? = nil
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeUIView(context: Context) -> UIScrollView {
        let scroll = UIScrollView()
        scroll.alwaysBounceHorizontal = true
        scroll.showsHorizontalScrollIndicator = true
        scroll.isDirectionalLockEnabled = true
        scroll.accessibilityIdentifier = "terminal.scroll"
        scroll.backgroundColor = UIColor(red: 16/255, green: 16/255, blue: 20/255, alpha: 1)
        let terminal = TerminalView(frame: .zero); terminal.tag = 10
        scroll.addSubview(terminal)
        context.coordinator.view = terminal
        context.coordinator.scroll = scroll
        scroll.delegate = context.coordinator
        scroll.panGestureRecognizer.addTarget(context.coordinator, action: #selector(Coordinator.panned(_:)))
        context.coordinator.start()
        return scroll
    }
    func updateUIView(_ scroll: UIScrollView, context: Context) {
        guard let view = scroll.viewWithTag(10) as? TerminalView else { return }
        let coordinator = context.coordinator
        context.coordinator.core = core; context.coordinator.onStatus = onStatus
        context.coordinator.onOpenWorkspace = onOpenWorkspace
        view.canInput = canInput
        view.onText = { text in coordinator.followCursor = true; coordinator.revealCursor(); return onText?(text) ?? false }
        view.onPaste = { text in coordinator.followCursor = true; coordinator.revealCursor(); return onPaste?(text) ?? false }
        view.onKey = { key in coordinator.followCursor = true; coordinator.revealCursor(); onKey?(key) }
        view.onKeyboardChange = { open in
            onKeyboardChange?(open)
            if open { coordinator.followCursor = true; coordinator.revealCursor() }
        }
        view.onReadOnly = onReadOnly
        if keyboardRequested && canInput { view.focusKeyboard() }
        else if canInput { view.hideKeyboard(); view.focusHardwareKeyboard() }
        else { view.hideKeyboard(); view.resignFirstResponder() }
        if view.generation != generation { view.generation = generation; view.screen = frame }
        if view.zoom != zoom { view.zoom = zoom }
        let size = view.intrinsicContentSize
        if view.frame.size != size { view.frame = CGRect(origin: .zero, size: size); scroll.contentSize = size }
        coordinator.revealCursor()
        context.coordinator.updateTestMetrics()
    }
    static func dismantleUIView(_ view: UIScrollView, coordinator: Coordinator) { coordinator.link?.invalidate(); coordinator.link = nil }
    final class Coordinator: NSObject, UIScrollViewDelegate {
        weak var view: TerminalView?
        weak var scroll: UIScrollView?
        var core: RemoteTerminal?
        var onStatus: ((RenderFrame?, String, Bool, String?) -> Void)?
        var onOpenWorkspace: (() -> Void)?
        private var edgeStart: CGFloat?
        var followCursor = true
        func scrollViewDidScroll(_ scrollView: UIScrollView) { updateTestMetrics() }
        func revealCursor() {
            guard followCursor, let scroll, scroll.bounds.width > 0, let cursor = view?.cursorRect else { return }
            let visible = CGRect(origin: scroll.contentOffset, size: scroll.bounds.size)
            let target = cursor.insetBy(dx: -max(12, cursor.width * 6), dy: -max(8, cursor.height))
            if !visible.contains(target) { scroll.scrollRectToVisible(target, animated: false) }
        }
        func updateTestMetrics() {
            #if DEBUG
            guard WorkspacePreferences.serviceTest, let scroll, let view, let screen = view.screen else { return }
            let metrics: [String: Any] = ["columns": screen.cols, "font": Int(15 * view.zoom), "offset": Int(scroll.contentOffset.x)]
            if let data = try? JSONSerialization.data(withJSONObject: metrics) { scroll.accessibilityValue = String(decoding: data, as: UTF8.self) }
            #endif
        }
        @objc func panned(_ gesture: UIPanGestureRecognizer) {
            guard let window = scroll?.window else { return }
            if gesture.state == .began {
                edgeStart = gesture.location(in: window).x - gesture.translation(in: window).x
                if (edgeStart ?? 0) >= 24 { followCursor = false }
            }
            if gesture.state == .ended {
                if let start = edgeStart, start < 24, gesture.translation(in: window).x > 60 { onOpenWorkspace?() }
                edgeStart = nil
            }
        }
        var link: CADisplayLink?
        private var lastPath = ""
        private var lastControl = false
        private var lastDesktopAttached = false
        private var lastExited = false
        func start() { link = CADisplayLink(target: self, selector: #selector(tick)); link?.preferredFramesPerSecond = 60; link?.add(to: .main, forMode: .common) }
        @objc func tick() {
            guard let core, let view else { return }
            do {
                let update = try core.drainUpdate()
                if let update { view.apply(update) }
                let size = view.intrinsicContentSize
                if view.frame.size != size { view.frame.size = size; scroll?.contentSize = size }
                if update != nil { revealCursor() }
                updateTestMetrics()
                let path = core.connectionPath(); let control = core.hasControl()
                let desktopAttached = core.desktopAttached(); let exited = core.sessionExited()
                if update != nil || path != lastPath || control != lastControl || desktopAttached != lastDesktopAttached || exited != lastExited {
                    lastPath = path; lastControl = control; lastDesktopAttached = desktopAttached; lastExited = exited
                    onStatus?(view.screen, path, control, nil)
                }
            } catch { onStatus?(nil, "offline", false, terminalError(error)); link?.invalidate() }
        }
    }
}

private final class TerminalInputField: UITextField {
    var onSpecialKey: ((String) -> Void)?
    var onPaste: (() -> Void)?
    override func paste(_ sender: Any?) { onPaste?() }
    override func deleteBackward() {
        if markedTextRange == nil && (text ?? "").isEmpty { onSpecialKey?("backspace") }
        else { super.deleteBackward() }
    }
    override var keyCommands: [UIKeyCommand]? {
        [UIKeyCommand(input: "\t", modifierFlags: [], action: #selector(tabKey)),
         UIKeyCommand(input: UIKeyCommand.inputEscape, modifierFlags: [], action: #selector(escapeKey)),
         UIKeyCommand(input: UIKeyCommand.inputUpArrow, modifierFlags: [], action: #selector(upKey)),
         UIKeyCommand(input: UIKeyCommand.inputDownArrow, modifierFlags: [], action: #selector(downKey)),
         UIKeyCommand(input: UIKeyCommand.inputLeftArrow, modifierFlags: [], action: #selector(leftKey)),
         UIKeyCommand(input: UIKeyCommand.inputRightArrow, modifierFlags: [], action: #selector(rightKey)),
         UIKeyCommand(input: "c", modifierFlags: .control, action: #selector(ctrlCKey))]
    }
    @objc private func tabKey() { onSpecialKey?("tab") }
    @objc private func escapeKey() { onSpecialKey?("escape") }
    @objc private func upKey() { onSpecialKey?("up") }
    @objc private func downKey() { onSpecialKey?("down") }
    @objc private func leftKey() { onSpecialKey?("left") }
    @objc private func rightKey() { onSpecialKey?("right") }
    @objc private func ctrlCKey() { onSpecialKey?("ctrl_c") }
}

final class TerminalView: UIView, UIContextMenuInteractionDelegate, UITextFieldDelegate {
    private let performanceLog = OSLog(subsystem: "dev.aiterminal", category: .pointsOfInterest)
    var canInput = false { didSet { screenElement.accessibilityLabel = canInput ? "终端画面，点击输入" : "终端画面，只读" } }
    var onText: ((String) -> Bool)?
    var onPaste: ((String) -> Bool)?
    var onKey: ((String) -> Void)?
    var onKeyboardChange: ((Bool) -> Void)?
    var onReadOnly: (() -> Void)?
    private let inputField = TerminalInputField(frame: .zero)
    override var canBecomeFirstResponder: Bool { canInput }
    override var keyCommands: [UIKeyCommand]? {
        guard canInput && !inputField.isFirstResponder else { return nil }
        return [UIKeyCommand(input: "\r", modifierFlags: [], action: #selector(returnKey)),
                UIKeyCommand(input: "\t", modifierFlags: [], action: #selector(tabKey)),
                UIKeyCommand(input: UIKeyCommand.inputEscape, modifierFlags: [], action: #selector(escapeKey)),
                UIKeyCommand(input: UIKeyCommand.inputUpArrow, modifierFlags: [], action: #selector(upKey)),
                UIKeyCommand(input: UIKeyCommand.inputDownArrow, modifierFlags: [], action: #selector(downKey)),
                UIKeyCommand(input: UIKeyCommand.inputLeftArrow, modifierFlags: [], action: #selector(leftKey)),
                UIKeyCommand(input: UIKeyCommand.inputRightArrow, modifierFlags: [], action: #selector(rightKey)),
                UIKeyCommand(input: "c", modifierFlags: .control, action: #selector(ctrlCKey))]
    }
    @objc private func returnKey() { onKey?("enter") }
    @objc private func tabKey() { onKey?("tab") }
    @objc private func escapeKey() { onKey?("escape") }
    @objc private func upKey() { onKey?("up") }
    @objc private func downKey() { onKey?("down") }
    @objc private func leftKey() { onKey?("left") }
    @objc private func rightKey() { onKey?("right") }
    @objc private func ctrlCKey() { onKey?("ctrl_c") }
    override func pressesBegan(_ presses: Set<UIPress>, with event: UIPressesEvent?) {
        var unhandled = Set<UIPress>()
        for press in presses {
            guard canInput, let key = press.key,
                  !key.modifierFlags.contains(.command), !key.modifierFlags.contains(.control), !key.modifierFlags.contains(.alternate),
                  !key.characters.isEmpty,
                  key.characters.unicodeScalars.allSatisfy({ $0.value >= 32 && $0.value != 127 && !(0xF700...0xF8FF).contains($0.value) })
            else { unhandled.insert(press); continue }
            _ = onText?(key.characters)
        }
        if !unhandled.isEmpty { super.pressesBegan(unhandled, with: event) }
    }
    private lazy var screenElement: UIAccessibilityElement = {
        let element = UIAccessibilityElement(accessibilityContainer: self)
        element.accessibilityIdentifier = "terminal.screen"
        element.accessibilityLabel = "终端画面，点击输入"
        element.accessibilityTraits = .staticText
        return element
    }()
    override var accessibilityElements: [Any]? {
        get {
            screenElement.accessibilityFrameInContainerSpace = bounds
            return WorkspacePreferences.serviceTest ? [screenElement, inputField] : [screenElement]
        }
        set {}
    }
    var generation = -1
    private var dirtyRows: Set<Int>?
    var screen: RenderFrame? { didSet {
        if UIAccessibility.isVoiceOverRunning || WorkspacePreferences.serviceTest, let screen {
            screenElement.accessibilityValue = (0..<Int(screen.rows)).map { row in
                screen.cells[(row * Int(screen.cols))..<((row + 1) * Int(screen.cols))].filter { $0.width > 0 }.map(\.text).joined().trimmingCharacters(in: .whitespaces)
            }.joined(separator: "\n")
        }
        if oldValue?.rows != screen?.rows || oldValue?.cols != screen?.cols { invalidateIntrinsicContentSize() }
        if let dirtyRows { for row in dirtyRows { setNeedsDisplay(CGRect(x: 0, y: CGFloat(row) * font.lineHeight, width: intrinsicContentSize.width, height: font.lineHeight)) } }
        else { setNeedsDisplay() }
    } }
    func apply(_ update: RenderUpdate) {
        guard update.full || (screen?.rows == update.rows && screen?.cols == update.cols) else { return }
        var cells: [RenderCell]
        var rows = Set<Int>()
        if update.full { cells = update.patches.map(\.cell) }
        else {
            guard let old = screen else { return }
            cells = old.cells
            rows.insert(Int(old.cursorRow)); rows.insert(Int(update.cursorRow))
            for patch in update.patches { let index = Int(patch.index); guard index < cells.count else { return }; cells[index] = patch.cell; rows.insert(index / Int(update.cols)) }
        }
        dirtyRows = update.full ? nil : rows
        screen = RenderFrame(rows: update.rows, cols: update.cols, revision: update.revision, cells: cells, cursorRow: update.cursorRow, cursorCol: update.cursorCol, cursorVisible: update.cursorVisible, cursorShape: update.cursorShape)
        dirtyRows = nil
    }
    var zoom: Double = 1 { didSet { if oldValue != zoom { rebuildFont(); invalidateIntrinsicContentSize(); setNeedsDisplay() } } }
    private var font = UIFont.monospacedSystemFont(ofSize: 15, weight: .regular)
    private var fonts: [UIFont] = []
    private var cellWidth: CGFloat = 0
    private var attributes: [UInt64: [NSAttributedString.Key: Any]] = [:]
    private var colors: [UInt32: UIColor] = [:]
    override init(frame: CGRect) { super.init(frame: frame); contentMode = .redraw; rebuildFont(); setupInput(); addInteraction(UIContextMenuInteraction(delegate: self)); isAccessibilityElement = false }
    required init?(coder: NSCoder) { super.init(coder: coder); rebuildFont(); setupInput() }
    private func setupInput() {
        inputField.frame = CGRect(x: 0, y: 0, width: 1, height: 1)
        inputField.alpha = 0.01
        inputField.textColor = .clear; inputField.tintColor = .clear
        inputField.autocorrectionType = .no; inputField.spellCheckingType = .no
        inputField.autocapitalizationType = .none; inputField.smartQuotesType = .no
        inputField.smartDashesType = .no
        inputField.isAccessibilityElement = WorkspacePreferences.serviceTest
        inputField.accessibilityIdentifier = "terminal.input"
        inputField.delegate = self
        inputField.addTarget(self, action: #selector(inputChanged), for: .editingChanged)
        inputField.onSpecialKey = { [weak self] key in self?.onKey?(key) }
        inputField.onPaste = { [weak self] in self?.paste(nil) }
        addSubview(inputField)
        addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(tapped)))
    }
    @objc private func tapped() {
        if canInput { focusKeyboard() } else { onReadOnly?() }
    }
    func focusKeyboard() {
        guard canInput else { return }
        if !inputField.isFirstResponder && inputField.becomeFirstResponder() { onKeyboardChange?(true) }
    }
    func focusHardwareKeyboard() {
        if canInput && !inputField.isFirstResponder && !isFirstResponder { becomeFirstResponder() }
    }
    func hideKeyboard() {
        if inputField.isFirstResponder { inputField.resignFirstResponder() }
        inputField.text = ""
    }
    func textFieldDidEndEditing(_ textField: UITextField) { onKeyboardChange?(false) }
    func textFieldDidChangeSelection(_ textField: UITextField) { inputChanged() }
    func textFieldShouldReturn(_ textField: UITextField) -> Bool { onKey?("enter"); return false }
    @objc private func inputChanged() {
        guard inputField.markedTextRange == nil, let text = inputField.text, !text.isEmpty else { return }
        inputField.text = ""
        committedText(text)
    }
    func committedText(_ text: String) {
        guard !text.isEmpty else { return }
        // Text replacements/IME commits may contain pasted content without a
        // paste action. Keep batches intact; only standalone controls are keys.
        switch text {
        case "\t": onKey?("tab")
        case "\n", "\r", "\r\n": onKey?("enter")
        default:
            if text.contains(where: { $0 == "\t" || $0 == "\n" || $0 == "\r" || $0 == "\r\n" }) { _ = onPaste?(text) }
            else { _ = onText?(text) }
        }
    }
    override func paste(_ sender: Any?) {
        guard canInput else { onReadOnly?(); return }
        guard let text = UIPasteboard.general.string, !text.isEmpty else { return }
        _ = onPaste?(text)
    }
    func contextMenuInteraction(_ interaction: UIContextMenuInteraction, configurationForMenuAtLocation location: CGPoint) -> UIContextMenuConfiguration? {
        UIContextMenuConfiguration(identifier: nil, previewProvider: nil) { [weak self] _ in
            UIMenu(children: [UIAction(title: "复制屏幕") { _ in
                guard let screen = self?.screen else { return }
                UIPasteboard.general.string = (0..<Int(screen.rows)).map { row in screen.cells[(row * Int(screen.cols))..<((row + 1) * Int(screen.cols))].filter { $0.width > 0 }.map(\.text).joined() }.joined(separator: "\n")
            }, UIAction(title: "粘贴", attributes: self?.canInput == true ? [] : .disabled) { _ in self?.paste(nil) }])
        }
    }
    private func rebuildFont() {
        font = UIFont.monospacedSystemFont(ofSize: 15 * zoom, weight: .regular)
        cellWidth = ("M" as NSString).size(withAttributes: [.font: font]).width
        fonts = (0..<4).map { style in
            var traits: UIFontDescriptor.SymbolicTraits = []
            if style & 1 != 0 { traits.insert(.traitBold) }
            if style & 2 != 0 { traits.insert(.traitItalic) }
            return font.fontDescriptor.withSymbolicTraits(traits).map { UIFont(descriptor: $0, size: font.pointSize) } ?? font
        }
        attributes.removeAll()
    }
    private func textAttributes(_ cell: RenderCell) -> [NSAttributedString.Key: Any] {
        let key = UInt64(cell.foreground) << 32 | UInt64(cell.style)
        if let value = attributes[key] { return value }
        var value: [NSAttributedString.Key: Any] = [.font: fonts[Int(cell.style & 3)], .ligature: 0, .kern: 0, .foregroundColor: color(cell.foreground).withAlphaComponent(cell.style & 16 != 0 ? 0.67 : 1)]
        if cell.style & 4 != 0 { value[.underlineStyle] = NSUnderlineStyle.single.rawValue }
        if cell.style & 8 != 0 { value[.strikethroughStyle] = NSUnderlineStyle.single.rawValue }
        if attributes.count >= 4096 { attributes.removeAll(keepingCapacity: true) }
        attributes[key] = value
        return value
    }
    override var intrinsicContentSize: CGSize {
        guard let screen else { return .zero }
        return CGSize(width: CGFloat(screen.cols) * cellWidth, height: CGFloat(screen.rows) * font.lineHeight)
    }
    var cursorRect: CGRect? {
        guard let screen, screen.cursorVisible else { return nil }
        return CGRect(x: CGFloat(screen.cursorCol) * cellWidth, y: CGFloat(screen.cursorRow) * font.lineHeight,
                      width: cellWidth, height: font.lineHeight)
    }
    override func draw(_ rect: CGRect) {
        os_signpost(.begin, log: performanceLog, name: "TerminalDraw")
        defer { os_signpost(.end, log: performanceLog, name: "TerminalDraw") }
        guard let screen, let context = UIGraphicsGetCurrentContext() else { return }
        let cols = Int(screen.cols)
        let firstRow = max(0, Int(floor(rect.minY / font.lineHeight)))
        let lastRow = min(Int(screen.rows), Int(ceil(rect.maxY / font.lineHeight)))
        guard firstRow < lastRow else { return }
        for row in firstRow..<lastRow {
            var col = 0
            context.setShouldAntialias(false)
            while col < cols {
                let start = col; let background = screen.cells[row * cols + col].background
                while col < cols && screen.cells[row * cols + col].background == background { col += 1 }
                color(background).setFill()
                context.fill(CGRect(x: CGFloat(start) * cellWidth, y: CGFloat(row) * font.lineHeight, width: CGFloat(col - start) * cellWidth, height: font.lineHeight))
            }
            context.setShouldAntialias(true); col = 0
            while col < cols {
                let cell = screen.cells[row * cols + col]
                if cell.width == 0 { col += 1; continue }
                let start = col; var text = cell.text; col += Int(cell.width)
                // Combine fixed-width ASCII runs; preserve per-cell clipping for wide/complex glyphs.
                if cell.width == 1 && cell.text.utf8.count == 1 {
                    while col < cols {
                        let next = screen.cells[row * cols + col]
                        if next.width != 1 || next.text.utf8.count != 1 || next.foreground != cell.foreground || next.style != cell.style { break }
                        text += next.text; col += 1
                    }
                }
                if cell.style & 12 == 0 && text.allSatisfy({ $0 == " " }) { continue }
                let box = CGRect(x: CGFloat(start) * cellWidth, y: CGFloat(row) * font.lineHeight, width: cellWidth * CGFloat(col - start), height: font.lineHeight)
                context.saveGState(); context.clip(to: box)
                (text as NSString).draw(at: box.origin, withAttributes: textAttributes(cell))
                context.restoreGState()
            }
        }
        if let box = cursorRect {
            let x = box.minX; let y = box.minY
            let stroke: CGFloat = 2
            UIColor.white.withAlphaComponent(0.92).setFill()
            switch screen.cursorShape {
            case 0, 1: context.fill(CGRect(x: x, y: y, width: stroke, height: font.lineHeight))
            case 2: context.fill(CGRect(x: x, y: box.maxY - stroke, width: cellWidth, height: stroke))
            case 3: UIColor.white.setStroke(); context.setLineWidth(stroke); context.stroke(box.insetBy(dx: stroke / 2, dy: stroke / 2))
            default:
                context.fill(box)
                let cell = screen.cells[Int(screen.cursorRow * screen.cols + screen.cursorCol)]
                if cell.width > 0 && !cell.text.trimmingCharacters(in: .whitespaces).isEmpty {
                    context.saveGState(); context.clip(to: box)
                    var style = textAttributes(cell)
                    style[.foregroundColor] = UIColor(red: 16/255, green: 16/255, blue: 20/255, alpha: 1)
                    (cell.text as NSString).draw(at: box.origin, withAttributes: style)
                    context.restoreGState()
                }
            }
        }
    }
    private func color(_ value: UInt32) -> UIColor {
        if let cached = colors[value] { return cached }
        let result = UIColor(red: CGFloat((value >> 16) & 255) / 255, green: CGFloat((value >> 8) & 255) / 255, blue: CGFloat(value & 255) / 255, alpha: 1)
        if colors.count >= 1024 { colors.removeAll(keepingCapacity: true) }
        colors[value] = result
        return result
    }
}

#if DEBUG
enum TerminalBenchmark {
    static func run(_ source: RenderFrame) {
        guard let path = ProcessInfo.processInfo.environment["AI_TERMINAL_RENDER_BENCHMARK"] else { return }
        DispatchQueue.main.async {
            var results: [[String: Any]] = []
            for (rows, cols) in [(24, 80), (40, 120)] {
                let screen = RenderFrame(rows: UInt32(rows), cols: UInt32(cols), revision: source.revision,
                    cells: (0..<(rows * cols)).map { source.cells[$0 % source.cells.count] },
                    cursorRow: 0, cursorCol: 0, cursorVisible: true, cursorShape: 0)
                let view = TerminalView(frame: .zero)
                view.screen = screen
                view.frame.size = view.intrinsicContentSize
                let format = UIGraphicsImageRendererFormat(); format.scale = 1; format.opaque = true
                let renderer = UIGraphicsImageRenderer(size: view.bounds.size, format: format)
                var samples: [Double] = []
                let count = min(2000, max(100, Int(ProcessInfo.processInfo.environment["AI_TERMINAL_RENDER_SAMPLES"] ?? "100") ?? 100))
                for index in 0..<(count + 10) {
                    let start = CACurrentMediaTime()
                    _ = renderer.image { _ in view.draw(view.bounds) }
                    if index >= 10 { samples.append((CACurrentMediaTime() - start) * 1000) }
                }
                samples.sort()
                results.append(["rows": rows, "cols": cols, "samples": samples.count,
                    "p50_ms": samples[count / 2], "p95_ms": samples[(count * 95 + 99) / 100 - 1], "p99_ms": samples[(count * 99 + 99) / 100 - 1]])
            }
            if let data = try? JSONSerialization.data(withJSONObject: results, options: [.prettyPrinted, .sortedKeys]) {
                try? data.write(to: URL(fileURLWithPath: path), options: .atomic)
            }
        }
    }
}
#endif
