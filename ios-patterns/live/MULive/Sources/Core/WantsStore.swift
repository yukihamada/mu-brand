import Foundation

// 「欲しい」コレクション。サーバに wants API は存在しないため**ローカル保存のみ**
// (UserDefaults JSON)。PII なし・端末内完結。CONCEPT.md に明記済み。
@MainActor
final class WantsStore: ObservableObject {
    struct Item: Codable, Identifiable, Hashable {
        let product: FeedProduct
        let addedAt: Date
        var id: String { product.sku }
    }

    @Published private(set) var items: [Item] = [] {
        didSet { persist() }
    }

    private static let key = "mu.live.wants.v1"

    init() {
        if let data = UserDefaults.standard.data(forKey: Self.key),
           let saved = try? JSONDecoder().decode([Item].self, from: data) {
            items = saved
        }
    }

    func contains(_ product: FeedProduct) -> Bool {
        items.contains { $0.product.sku == product.sku }
    }

    func add(_ product: FeedProduct) {
        guard !contains(product) else { return }
        items.insert(Item(product: product, addedAt: Date()), at: 0)
    }

    func remove(sku: String) {
        items.removeAll { $0.product.sku == sku }
    }

    func toggle(_ product: FeedProduct) {
        if contains(product) {
            remove(sku: product.sku)
        } else {
            add(product)
        }
    }

    private func persist() {
        if let data = try? JSONEncoder().encode(items) {
            UserDefaults.standard.set(data, forKey: Self.key)
        }
    }
}
