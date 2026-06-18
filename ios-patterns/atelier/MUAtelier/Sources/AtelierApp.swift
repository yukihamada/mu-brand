import SwiftUI

// MU Atelier — 世界品質のミニマル・ラグジュアリー EC。
// 編集的な静けさ (SSENSE / Aesop)。タイポグラフィ主導、余白のリズム、購入転換に振り切る。
@main
struct MUAtelierApp: App {
    @StateObject private var session = Session()
    @StateObject private var wishlist = Wishlist()

    init() {
        // AsyncImage は URLCache.shared を使う。画像はモック PNG 中心なので大きめに。
        URLCache.shared = URLCache(memoryCapacity: 64 << 20, diskCapacity: 512 << 20)
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(session)
                .environmentObject(wishlist)
                .tint(.primary) // 金 #e6c449 は極小アクセントのみ。基調は墨と紙
        }
    }
}

enum AppTab: String {
    case home, collection, wishlist, account
}

// スクリーンショット/検証用の起動引数 (UI には一切影響しない):
//   -atelier-tab <home|collection|wishlist|account>  初期タブ
//   -atelier-open-first                              Collection 読込後に先頭商品の PDP を開く
//   -atelier-seed-wishlist                           Wishlist が空ならフィード先頭4点を保存
enum LaunchOptions {
    static let args = ProcessInfo.processInfo.arguments

    static var initialTab: AppTab {
        guard let i = args.firstIndex(of: "-atelier-tab"), i + 1 < args.count,
              let tab = AppTab(rawValue: args[i + 1]) else { return .home }
        return tab
    }

    static var openFirstProduct: Bool { args.contains("-atelier-open-first") }
    static var seedWishlist: Bool { args.contains("-atelier-seed-wishlist") }
}

struct RootView: View {
    @State private var tab = LaunchOptions.initialTab
    @EnvironmentObject private var wishlist: Wishlist

    var body: some View {
        TabView(selection: $tab) {
            HomeView()
                .tabItem { Label(String(localized: "tab.home"), systemImage: "house") }
                .tag(AppTab.home)
            CollectionView()
                .tabItem { Label(String(localized: "tab.collection"), systemImage: "square.grid.2x2") }
                .tag(AppTab.collection)
            WishlistView()
                .tabItem { Label(String(localized: "tab.wishlist"), systemImage: "heart") }
                .tag(AppTab.wishlist)
            AccountView()
                .tabItem { Label(String(localized: "tab.account"), systemImage: "person") }
                .tag(AppTab.account)
        }
        .task {
            if LaunchOptions.seedWishlist, wishlist.items.isEmpty,
               let products = try? await MUAPI.feed() {
                wishlist.seed(Array(products.prefix(4)))
            }
        }
    }
}
