import Foundation
import SwiftUI

// 生成フローの状態機械。入力 → 生成中 → 結果/失敗。
// 結果は MakeHistory (端末台帳) にも書き込む。
@MainActor
final class MakeFlow: ObservableObject {
    enum Phase: Equatable {
        case idle
        case generating(prompt: String)
        case failed(String)
    }

    @Published var phase: Phase = .idle
    @Published var result: MakeResult?      // fullScreenCover(item:) 用
    @Published var lastPrompt: String = ""

    private var task: Task<Void, Never>?

    var isGenerating: Bool {
        if case .generating = phase { return true }
        return false
    }

    func start(prompt: String, kind: MakeKind, apiKey: String?, history: MakeHistory) {
        let trimmed = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, !isGenerating else { return }
        lastPrompt = trimmed
        phase = .generating(prompt: trimmed)
        Haptics.rigid()
        task = Task { [weak self] in
            do {
                let result = try await MUAPI.make(prompt: trimmed, kind: kind, apiKey: apiKey)
                guard !Task.isCancelled else { return }
                history.add(LocalCreation(result: result, prompt: trimmed))
                self?.phase = .idle
                self?.result = result
                Haptics.success()
            } catch is CancellationError {
                self?.phase = .idle
            } catch {
                guard !Task.isCancelled else { return }
                self?.phase = .failed(error.localizedDescription)
                Haptics.failure()
            }
        }
    }

    func cancel() {
        task?.cancel()
        task = nil
        phase = .idle
    }
}
