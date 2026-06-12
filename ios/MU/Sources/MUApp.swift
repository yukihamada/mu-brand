import SwiftUI

@main
struct MUApp: App {
    @StateObject private var session = Session()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(session)
                .preferredColorScheme(.dark) // MU = 黒地に金。ブランドトーン固定
                .tint(Color(red: 0.90, green: 0.77, blue: 0.29)) // MU gold #e6c449
        }
    }
}

struct RootView: View {
    var body: some View {
        TabView {
            LiveView()
                .tabItem { Label(String(localized: "tab.live"), systemImage: "flame.fill") }
            ShopView()
                .tabItem { Label(String(localized: "tab.shop"), systemImage: "bag.fill") }
            ClosetView()
                .tabItem { Label(String(localized: "tab.closet"), systemImage: "person.crop.square") }
        }
    }
}
