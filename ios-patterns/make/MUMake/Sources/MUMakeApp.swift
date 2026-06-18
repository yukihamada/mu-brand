import SwiftUI

@main
struct MUMakeApp: App {
    @StateObject private var session = Session()
    @StateObject private var history = MakeHistory()

    init() {
        _ = MUAPI.bootstrapCache
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(session)
                .environmentObject(history)
                .preferredColorScheme(.dark) // MU = 黒地に金。ブランドトーン固定
                .tint(MUTheme.gold)
        }
    }
}

enum AppTab: String {
    case make, gallery, mine
}

struct RootView: View {
    // 起動引数 `-tab gallery|mine` で初期タブを切替 (スクショ自動化・deep link 代替)
    @State private var tab: AppTab = AppTab(rawValue: UserDefaults.standard.string(forKey: "tab") ?? "") ?? .make

    var body: some View {
        TabView(selection: $tab) {
            MakeView(switchTab: { tab = $0 })
                .tabItem { Label(String(localized: "tab.make"), systemImage: "wand.and.stars") }
                .tag(AppTab.make)
            GalleryView()
                .tabItem { Label(String(localized: "tab.gallery"), systemImage: "square.grid.2x2") }
                .tag(AppTab.gallery)
            MineView(switchTab: { tab = $0 })
                .tabItem { Label(String(localized: "tab.mine"), systemImage: "person.crop.square") }
                .tag(AppTab.mine)
        }
    }
}
