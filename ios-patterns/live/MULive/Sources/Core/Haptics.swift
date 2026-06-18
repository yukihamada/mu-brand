import UIKit

// 触覚は MU Live の「気持ちよさ」の半分。スワイプ吸い付き=selection、
// ダブルタップ「欲しい」=heavy、シート/購入=success。
enum Haptics {
    static func selection() {
        UISelectionFeedbackGenerator().selectionChanged()
    }

    static func medium() {
        UIImpactFeedbackGenerator(style: .medium).impactOccurred()
    }

    static func heavy() {
        UIImpactFeedbackGenerator(style: .heavy).impactOccurred()
    }

    static func success() {
        UINotificationFeedbackGenerator().notificationOccurred(.success)
    }
}
