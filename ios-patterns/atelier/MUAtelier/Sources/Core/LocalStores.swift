import Foundation

// サーバに Wishlist API は存在しない (2026-06-13 確認) — 端末ローカル保存で実装。
// 商品 JSON 丸ごと保存 (オフラインでも一覧が崩れない)。
@MainActor
final class Wishlist: ObservableObject {
    @Published private(set) var items: [FeedProduct] = []

    private static let key = "atelier.wishlist.v1"

    init() {
        if let data = UserDefaults.standard.data(forKey: Self.key),
           let saved = try? JSONDecoder().decode([FeedProduct].self, from: data) {
            items = saved
        }
    }

    func contains(_ product: FeedProduct) -> Bool {
        items.contains { $0.sku == product.sku }
    }

    func toggle(_ product: FeedProduct) {
        if let i = items.firstIndex(where: { $0.sku == product.sku }) {
            items.remove(at: i)
        } else {
            items.insert(product, at: 0)
        }
        persist()
    }

    func remove(_ product: FeedProduct) {
        items.removeAll { $0.sku == product.sku }
        persist()
    }

    /// スクリーンショット用シード (-atelier-seed-wishlist)
    func seed(_ products: [FeedProduct]) {
        items = products
        persist()
    }

    private func persist() {
        if let data = try? JSONEncoder().encode(items) {
            UserDefaults.standard.set(data, forKey: Self.key)
        }
    }
}

// 最近の検索語 — ローカルのみ。最大8件、重複は先頭へ。
@MainActor
final class RecentSearches: ObservableObject {
    @Published private(set) var terms: [String] = []

    private static let key = "atelier.recentSearches.v1"
    private static let limit = 8

    init() {
        terms = UserDefaults.standard.stringArray(forKey: Self.key) ?? []
    }

    func add(_ term: String) {
        let t = term.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty else { return }
        terms.removeAll { $0.caseInsensitiveCompare(t) == .orderedSame }
        terms.insert(t, at: 0)
        if terms.count > Self.limit { terms = Array(terms.prefix(Self.limit)) }
        UserDefaults.standard.set(terms, forKey: Self.key)
    }

    func clear() {
        terms = []
        UserDefaults.standard.removeObject(forKey: Self.key)
    }
}
