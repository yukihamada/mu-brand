import Foundation

enum APIError: LocalizedError {
    case badStatus(Int)
    case message(String)

    var errorDescription: String? {
        switch self {
        case .badStatus(let code): return "HTTP \(code)"
        case .message(let m): return m
        }
    }
}

// wearmu.com REST クライアント。公開ルートのみ (admin 系は使用禁止 — トークン不要構成)。
struct MUAPI {
    static let base = URL(string: "https://wearmu.com")!

    static func feed(page: Int = 1) async throws -> [FeedProduct] {
        var comps = URLComponents(
            url: base.appendingPathComponent("api/shop/feed.json"),
            resolvingAgainstBaseURL: false
        )!
        comps.queryItems = [URLQueryItem(name: "page", value: String(page))]
        return try await get(comps.url!, as: FeedPage.self).products
    }

    static func brands() async throws -> [BrandSummary] {
        try await get(base.appendingPathComponent("api/brands"), as: BrandsResponse.self).brands
    }

    // 透明性レポート (毎リクエスト再計算の実数 JSON)
    static func transparency() async throws -> Transparency {
        try await get(base.appendingPathComponent("api/transparency"), as: Transparency.self)
    }

    // 公開鼓動ログ
    static func updates() async throws -> [UpdateItem] {
        try await get(base.appendingPathComponent("api/updates"), as: UpdatesResponse.self).updates
    }

    // メール → 6桁コード → api_key (POST /api/agent/register → /register/verify)
    static func register(email: String) async throws {
        _ = try await post(base.appendingPathComponent("api/agent/register"), body: ["email": email])
    }

    static func verify(email: String, code: String) async throws -> String {
        let json = try await post(
            base.appendingPathComponent("api/agent/register/verify"),
            body: ["email": email, "code": code]
        )
        guard let key = json["api_key"] as? String, !key.isEmpty else {
            throw APIError.message((json["error"] as? String) ?? "no api_key in response")
        }
        return key
    }

    // App Store Guideline 5.1.1(v): アカウント削除
    static func deleteAccount(apiKey: String) async throws {
        var req = URLRequest(url: base.appendingPathComponent("api/collab/account/delete"))
        req.httpMethod = "POST"
        req.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        let (data, resp) = try await URLSession.shared.data(for: req)
        guard let http = resp as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
            throw APIError.message((json?["error"] as? String) ?? "HTTP \((resp as? HTTPURLResponse)?.statusCode ?? -1)")
        }
    }

    static func sales(apiKey: String) async throws -> SalesResponse {
        var req = URLRequest(url: base.appendingPathComponent("api/agent/sales"))
        req.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        let (data, resp) = try await URLSession.shared.data(for: req)
        guard let http = resp as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.badStatus((resp as? HTTPURLResponse)?.statusCode ?? -1)
        }
        return try JSONDecoder().decode(SalesResponse.self, from: data)
    }

    // MARK: - plumbing

    private static func get<T: Decodable>(_ url: URL, as type: T.Type) async throws -> T {
        let (data, resp) = try await URLSession.shared.data(from: url)
        guard let http = resp as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.badStatus((resp as? HTTPURLResponse)?.statusCode ?? -1)
        }
        return try JSONDecoder().decode(T.self, from: data)
    }

    private static func post(_ url: URL, body: [String: String]) async throws -> [String: Any] {
        var req = URLRequest(url: url)
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (data, resp) = try await URLSession.shared.data(for: req)
        let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
        guard let http = resp as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            let msg = (json["error"] as? String) ?? "HTTP \((resp as? HTTPURLResponse)?.statusCode ?? -1)"
            throw APIError.message(msg)
        }
        return json
    }
}
