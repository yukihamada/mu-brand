import Foundation

enum APIError: LocalizedError {
    case badStatus(Int)

    var errorDescription: String? {
        switch self {
        case .badStatus(let code): return "HTTP \(code)"
        }
    }
}

// wearmu.com REST クライアント (既存 MU アプリの MUAPI と同じ作法・読み取り専用)。
// MU Live が使うのは feed のみ。「欲しい」はサーバに API が無いためローカル保存 (WantsStore)。
struct MUAPI {
    static let base = URL(string: "https://wearmu.com")!

    static func feed(page: Int = 1, kind: ProductKind = .all) async throws -> [FeedProduct] {
        var comps = URLComponents(url: base.appendingPathComponent("api/shop/feed.json"), resolvingAgainstBaseURL: false)!
        var items = [URLQueryItem(name: "page", value: String(page))]
        if !kind.rawValue.isEmpty { items.append(URLQueryItem(name: "kind", value: kind.rawValue)) }
        comps.queryItems = items
        return try await get(comps.url!, as: FeedPage.self).products
    }

    private static func get<T: Decodable>(_ url: URL, as type: T.Type) async throws -> T {
        let (data, resp) = try await URLSession.shared.data(from: url)
        guard let http = resp as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.badStatus((resp as? HTTPURLResponse)?.statusCode ?? -1)
        }
        return try JSONDecoder().decode(T.self, from: data)
    }
}

// 次ページ/次カードの画像を URLCache に温めておく (AsyncImage は URLSession.shared を
// 使うため、先読みしておけばスワイプ時に即表示される)。
final class ImagePrefetcher: @unchecked Sendable {
    static let shared = ImagePrefetcher()
    private var seen = Set<URL>()
    private let lock = NSLock()

    func prefetch(_ urls: [URL]) {
        for url in urls {
            lock.lock()
            let inserted = seen.insert(url).inserted
            lock.unlock()
            guard inserted else { continue }
            Task.detached(priority: .utility) {
                _ = try? await URLSession.shared.data(from: url)
            }
        }
    }
}
