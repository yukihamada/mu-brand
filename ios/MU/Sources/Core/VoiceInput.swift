import Foundation
import Speech
import AVFoundation

// 声で作る — オンデバイス音声認識(Apple Speech)。喋った言葉をそのまま prompt に流す。
// クラウド不要・無料。「言えば作れる」を文字通りに。
@MainActor
final class VoiceInput: ObservableObject {
    @Published var transcript = ""
    @Published var isRecording = false
    @Published var denied = false

    private let recognizer = SFSpeechRecognizer(locale: Locale(identifier: "ja-JP"))
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?
    private let engine = AVAudioEngine()

    func toggle() async {
        if isRecording { stop() } else { await start() }
    }

    func start() async {
        guard let recognizer, recognizer.isAvailable else { denied = true; return }
        let speechOK: Bool = await withCheckedContinuation { cont in
            SFSpeechRecognizer.requestAuthorization { cont.resume(returning: $0 == .authorized) }
        }
        let micOK = await AVAudioApplication.requestRecordPermission()
        guard speechOK, micOK else { denied = true; return }

        transcript = ""
        let req = SFSpeechAudioBufferRecognitionRequest()
        req.shouldReportPartialResults = true
        request = req

        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.record, mode: .measurement, options: .duckOthers)
            try session.setActive(true, options: .notifyOthersOnDeactivation)
            let node = engine.inputNode
            let format = node.outputFormat(forBus: 0)
            node.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buf, _ in
                self?.request?.append(buf)
            }
            engine.prepare()
            try engine.start()
            isRecording = true
            task = recognizer.recognitionTask(with: req) { [weak self] result, error in
                guard let self else { return }
                if let r = result {
                    Task { @MainActor in self.transcript = r.bestTranscription.formattedString }
                }
                if error != nil || (result?.isFinal ?? false) {
                    Task { @MainActor in self.stop() }
                }
            }
        } catch {
            denied = true
            stop()
        }
    }

    func stop() {
        if engine.isRunning {
            engine.stop()
            engine.inputNode.removeTap(onBus: 0)
        }
        request?.endAudio()
        task?.cancel()
        request = nil
        task = nil
        isRecording = false
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }
}
