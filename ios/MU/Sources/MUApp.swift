import SwiftUI

@main
struct MUApp: App {
    @StateObject private var session = Session()
    @StateObject private var app = AppState()
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @AppStorage("hasOnboarded") private var hasOnboarded = false

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(session)
                .environmentObject(app)
                .preferredColorScheme(.dark) // MU = 黒地に金。ブランドトーン固定
                .tint(Color(red: 0.90, green: 0.77, blue: 0.29)) // MU gold #e6c449
                .task { Analytics.track("app_open") }
                .fullScreenCover(isPresented: .constant(!hasOnboarded)) {
                    OnboardingView()
                        .environmentObject(app)
                }
        }
    }
}

struct RootView: View {
    @EnvironmentObject private var app: AppState

    var body: some View {
        // 順序: ライブ / ショップ / 作る(中央) / スキャン / アカウント。
        // 作る = アプリの背骨なので、5タブのど真ん中に置く。
        TabView(selection: $app.selectedTab) {
            LiveView()
                .tabItem { Label(String(localized: "tab.live"), systemImage: "flame.fill") }
                .tag(AppState.Tab.live)
            ShopView()
                .tabItem { Label(String(localized: "tab.shop"), systemImage: "bag.fill") }
                .tag(AppState.Tab.shop)
            MakeView()
                .tabItem { Label(String(localized: "tab.make"), systemImage: "wand.and.stars") }
                .tag(AppState.Tab.make)
            AgentView()
                .tabItem { Label(String(localized: "tab.agent"), systemImage: "bubbles.and.sparkles.fill") }
                .tag(AppState.Tab.agent)
            ClosetView()
                .tabItem { Label(String(localized: "tab.closet"), systemImage: "person.crop.square") }
                .tag(AppState.Tab.account)
        }
    }
}
