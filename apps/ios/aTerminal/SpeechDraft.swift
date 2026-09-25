import SwiftUI
import Speech
import AVFoundation

@MainActor
final class SpeechDraft: ObservableObject {
    @Published private(set) var recording = false
    @Published private(set) var authorizing = false
    @Published var feedback = ""
    private let engine = AVAudioEngine()
    private var recognition: SFSpeechRecognitionTask?
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var tapInstalled = false
    private var token = UUID()
    private var original = ""
    private var deliver: ((String) -> Void)?

    func start(draft: String, update: @escaping (String) -> Void) {
        guard !recording, !authorizing else { return }
        original = draft; deliver = update; feedback = "正在请求语音权限"; authorizing = true
        let id = UUID(); token = id
        SFSpeechRecognizer.requestAuthorization { [weak self] permission in
            Task { @MainActor in
                guard let self, self.token == id else { return }
                guard permission == .authorized else { self.authorizing = false; self.feedback = "语音识别未获授权，可在系统设置中开启"; return }
                AVAudioSession.sharedInstance().requestRecordPermission { [weak self] allowed in
                    Task { @MainActor in
                        guard let self, self.token == id else { return }
                        self.authorizing = false
                        guard allowed else { self.feedback = "麦克风未获授权，可在系统设置中开启"; return }
                        self.begin(id)
                    }
                }
            }
        }
    }
    private func begin(_ id: UUID) {
        guard let recognizer = SFSpeechRecognizer(locale: Locale.current), recognizer.isAvailable else { feedback = "当前设备的语音识别不可用"; return }
        do {
            let audio = AVAudioSession.sharedInstance()
            try audio.setCategory(.record, mode: .measurement, options: .duckOthers)
            try audio.setActive(true, options: .notifyOthersOnDeactivation)
            let request = SFSpeechAudioBufferRecognitionRequest(); request.shouldReportPartialResults = true
            self.request = request
            let input = engine.inputNode; let format = input.outputFormat(forBus: 0)
            guard format.sampleRate > 0, format.channelCount > 0 else { throw ChatFailure.message("未找到可用麦克风") }
            input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in request.append(buffer) }
            tapInstalled = true; engine.prepare(); try engine.start(); recording = true; feedback = "正在聆听"
            recognition = recognizer.recognitionTask(with: request) { [weak self] result, error in
                Task { @MainActor in
                    guard let self, self.token == id else { return }
                    if let result {
                        let prefix = self.original.isEmpty ? "" : self.original + " "
                        self.deliver?(prefix + result.bestTranscription.formattedString)
                    }
                    if error != nil || result?.isFinal == true {
                        self.finish()
                        self.feedback = error == nil ? "语音已填入草稿" : "语音识别中断，已保留草稿"
                    }
                }
            }
        } catch { finish(); feedback = "语音不可用：\(error.localizedDescription)" }
    }
    func finish() {
        token = UUID(); recording = false; authorizing = false
        engine.stop()
        if tapInstalled { engine.inputNode.removeTap(onBus: 0); tapInstalled = false }
        request?.endAudio(); recognition?.cancel(); recognition = nil; request = nil
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        feedback = "语音已填入草稿"
    }
    func cancel(restoringDraft: Bool = true) { let wasActive = recording || authorizing; finish(); if wasActive && restoringDraft { deliver?(original) }; deliver = nil; feedback = "已取消录音" }
}
