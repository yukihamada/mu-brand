import Foundation
import Combine

/// Shared paging state for Shop and Live. Only the latest request can publish results.
@MainActor
final class ProductFeed: ObservableObject {
    typealias Loader = (Int, ProductKind, String) async throws -> [FeedProduct]

    @Published private(set) var products: [FeedProduct] = []
    @Published private(set) var loading = false
    @Published private(set) var loadedOnce = false
    @Published private(set) var reachedEnd = false
    @Published private(set) var error: String?
    private var page = 0
    private var kind: ProductKind = .all
    private var query = ""
    private var requestID = UUID()
    private let loader: Loader

    init(loader: @escaping Loader = { try await MUAPI.feed(page: $0, kind: $1, query: $2) }) {
        self.loader = loader
    }

    func reload(kind: ProductKind, query: String = "") async {
        self.kind = kind
        self.query = query.trimmingCharacters(in: .whitespacesAndNewlines)
        page = 0
        reachedEnd = false
        products = []
        await fetch(page: 1, replace: true)
    }

    func loadMore() async {
        guard !loading, !reachedEnd else { return }
        await fetch(page: page + 1, replace: page == 0)
    }

    private func fetch(page nextPage: Int, replace: Bool) async {
        let id = UUID()
        requestID = id
        loading = true
        error = nil
        defer {
            if requestID == id { loading = false; loadedOnce = true }
        }
        do {
            let incoming = try await loader(nextPage, kind, query)
            guard requestID == id, !Task.isCancelled else { return }
            var seen = Set(replace ? [] : products.map(\.sku))
            let unique = incoming.filter { seen.insert($0.sku).inserted }
            products = replace ? unique : products + unique
            page = nextPage // A failed page must be retried, never skipped.
            reachedEnd = incoming.isEmpty
        } catch {
            guard requestID == id, !Task.isCancelled else { return }
            self.error = error.localizedDescription
        }
    }
}
