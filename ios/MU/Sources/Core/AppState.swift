import SwiftUI

// タブ選択とオンボーディングからの橋渡し。
// 作る = 中央タグ(2)。オンボーディングが「最初の一着」を作るとき
// pendingPrompt をセット → MakeView が拾って自動生成する。
@MainActor
final class AppState: ObservableObject {
    @Published var selectedTab = 2          // 起動時は中央の「作る」を見せる
    @Published var pendingPrompt: String?   // オンボーディング → Make への受け渡し

    // タブのタグ(順序: ライブ0 / ショップ1 / 作る2[中央] / スキャン3 / アカウント4)
    enum Tab { static let live = 0, shop = 1, make = 2, scan = 3, account = 4 }

    func startMake(_ prompt: String) {
        pendingPrompt = prompt
        selectedTab = Tab.make
    }
}
