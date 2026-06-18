import SwiftUI

@main
struct MULiveApp: App {
    @StateObject private var wants = WantsStore()

    init() {
        // フルブリード画像フィードの命: 大きめの URLCache (AsyncImage + プリフェッチが共有)
        URLCache.shared = URLCache(memoryCapacity: 128 << 20, diskCapacity: 512 << 20)
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(wants)
                .preferredColorScheme(.dark) // MU = 黒地に金
                .tint(.muGold)
        }
    }
}

extension Color {
    // MU gold #e6c449
    static let muGold = Color(red: 0.902, green: 0.769, blue: 0.286)
}

enum AppTab: String {
    case feed, wants, pulse
}

// スクショ自動化用の起動引数 (リリース挙動には影響しない):
//   -initialTab feed|wants|pulse / -seedWants (フィード先頭6点を欲しいに投入) /
//   -autoDetail (フィード読込後に詳細シートを自動表示)
enum LaunchArgs {
    private static let args = ProcessInfo.processInfo.arguments

    static var initialTab: AppTab {
        guard let i = args.firstIndex(of: "-initialTab"), args.indices.contains(i + 1),
              let tab = AppTab(rawValue: args[i + 1]) else { return .feed }
        return tab
    }

    static var seedWants: Bool { args.contains("-seedWants") }
    static var autoDetail: Bool { args.contains("-autoDetail") }
}

struct RootView: View {
    @EnvironmentObject private var wants: WantsStore
    @State private var tab: AppTab = LaunchArgs.initialTab

    var body: some View {
        TabView(selection: $tab) {
            FeedView()
                .tabItem { Label(String(localized: "tab.feed"), systemImage: "flame.fill") }
                .tag(AppTab.feed)
            WantsView()
                .tabItem { Label(String(localized: "tab.wants"), systemImage: "heart.fill") }
                .tag(AppTab.wants)
            PulseView()
                .tabItem { Label(String(localized: "tab.pulse"), systemImage: "waveform.path.ecg") }
                .tag(AppTab.pulse)
        }
        .task {
            // スクショ用シード: 実フィードから6点を「欲しい」へ
            if LaunchArgs.seedWants, wants.items.isEmpty,
               let products = try? await MUAPI.feed(page: 1) {
                products.prefix(6).forEach { wants.add($0) }
            }
        }
    }
}
