import SwiftUI

// タブ選択とオンボーディングからの橋渡し。
// 作る = 中央タグ(2)。オンボーディングが「最初の一着」を作るとき
// pendingPrompt をセット → MakeView が拾って自動生成する。
@MainActor
final class AppState: ObservableObject {
    @Published var selectedTab = 2          // 起動時は中央の「作る」を見せる
    @Published var pendingPrompt: String?   // オンボーディング → Make への受け渡し

    // タブのタグ(順序: ライブ0 / ショップ1 / 作る2[中央] / AI3 / アカウント4)。
    // スキャンはタブ上限(5)を保つためツールバーボタンへ移設。
    enum Tab { static let live = 0, shop = 1, make = 2, agent = 3, account = 4 }

    private var pushObserver: NSObjectProtocol?

    init() {
        // 通知タップ(AppDelegate)→ タブ遷移。queue:.main だが @MainActor 隔離を守るため Task で渡す。
        pushObserver = NotificationCenter.default.addObserver(
            forName: .muPushOpen, object: nil, queue: .main
        ) { [weak self] note in
            let info = note.userInfo ?? [:]
            Task { @MainActor in self?.handlePush(info) }
        }
    }

    deinit {
        if let o = pushObserver { NotificationCenter.default.removeObserver(o) }
    }

    func startMake(_ prompt: String) {
        pendingPrompt = prompt
        selectedTab = Tab.make
    }

    // 通知ペイロードの "tab" に従って遷移。指定なし(drop/sold 等)は
    // 最新が見える Live へ。SKU 単体PDPへの深掘りは将来 pendingPrompt と同様に拡張可。
    func handlePush(_ info: [AnyHashable: Any]) {
        switch info["tab"] as? String {
        case "shop": selectedTab = Tab.shop
        case "make": selectedTab = Tab.make
        case "agent": selectedTab = Tab.agent
        case "account": selectedTab = Tab.account
        default: selectedTab = Tab.live
        }
    }
}
