import SwiftUI

// MU OS — AIが運営するブランドの心臓部を覗くダッシュボード。
// データ源は wearmu.com の公開 API のみ (admin token は一切使わない)。
@main
struct MUOSApp: App {
    @StateObject private var session = Session()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(session)
                .preferredColorScheme(.dark) // MU = 黒地に金。固定
                .tint(Color.muGold)
        }
    }
}

enum AppTab: String {
    case pulse, drops, ledger, closet
}

struct RootView: View {
    @State private var tab: AppTab

    // スクショ/テスト用: `simctl launch ... -tab drops` で初期タブを切替可能
    init() {
        var initial = AppTab.pulse
        let args = ProcessInfo.processInfo.arguments
        if let i = args.firstIndex(of: "-tab"), i + 1 < args.count,
           let parsed = AppTab(rawValue: args[i + 1]) {
            initial = parsed
        }
        _tab = State(initialValue: initial)
    }

    var body: some View {
        TabView(selection: $tab) {
            PulseView()
                .tabItem { Label(String(localized: "tab.pulse"), systemImage: "waveform.path.ecg") }
                .tag(AppTab.pulse)
            DropsView()
                .tabItem { Label(String(localized: "tab.drops"), systemImage: "clock.arrow.2.circlepath") }
                .tag(AppTab.drops)
            LedgerView()
                .tabItem { Label(String(localized: "tab.ledger"), systemImage: "list.bullet.rectangle.portrait") }
                .tag(AppTab.ledger)
            ClosetView()
                .tabItem { Label(String(localized: "tab.closet"), systemImage: "person.crop.square") }
                .tag(AppTab.closet)
        }
    }
}
