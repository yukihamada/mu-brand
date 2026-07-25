import SwiftUI

// Apple App Review Guideline 5.1.1(i)/(ii) 対応(2026-07-24 却下分)。
// 「言えば、作れる」(Make)とエージェントチャットで打ったテキストは、デザイン生成/意図判定のため
// サーバ経由で Google Gemini API に送信される(store/src/gemini.rs)。送信前に必ず一度だけ同意を取る。
enum AIConsent {
    private static let storageKey = "aiConsentGiven"

    static var given: Bool {
        get { UserDefaults.standard.bool(forKey: storageKey) }
        set { UserDefaults.standard.set(newValue, forKey: storageKey) }
    }
}

private struct AIConsentAlert: ViewModifier {
    @Binding var isPresented: Bool
    let onAgree: () -> Void

    func body(content: Content) -> some View {
        content.alert(String(localized: "aiConsent.title"), isPresented: $isPresented) {
            Button(String(localized: "aiConsent.agree")) {
                AIConsent.given = true
                onAgree()
            }
            Button(String(localized: "make.cancel"), role: .cancel) {}
        } message: {
            Text(String(localized: "aiConsent.body"))
        }
    }
}

extension View {
    /// 初回のAI送信前に同意を取るゲート。同意済みなら以後は出さない(呼び出し側で AIConsent.given を先にチェックする設計)。
    func aiConsentAlert(isPresented: Binding<Bool>, onAgree: @escaping () -> Void) -> some View {
        modifier(AIConsentAlert(isPresented: isPresented, onAgree: onAgree))
    }
}
