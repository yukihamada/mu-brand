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

// wearmu.com REST クライアント。全エンドポイントは store/src/{main,catalog,agent_api}.rs で
// 実在確認済み (2026-06-13 curl 実打)。想像上の API は呼ばない。
struct MUAPI {
    static let base = URL(string: "https://wearmu.com")!

    // 画像が多いので URLCache を広めに (AsyncImage は共有 URLCache を使う)。
    static let bootstrapCache: Void = {
        URLCache.shared = URLCache(
            memoryCapacity: 64 * 1024 * 1024,
            diskCapacity: 256 * 1024 * 1024
        )
    }()

    // ── 創作の本丸: GET /api/make?prompt=&kind= (匿名可・40件/時の全体キャップ) ──
    // ログイン済みなら Bearer を添えると maker_email が刻まれ、売上の10%が作者に入る。
    static func make(prompt: String, kind: MakeKind, apiKey: String?) async throws -> MakeResult {
        var comps = URLComponents(url: base.appendingPathComponent("api/make"), resolvingAgainstBaseURL: false)!
        var items = [URLQueryItem(name: "prompt", value: prompt)]
        if !kind.rawValue.isEmpty { items.append(URLQueryItem(name: "kind", value: kind.rawValue)) }
        comps.queryItems = items
        var req = URLRequest(url: comps.url!)
        req.timeoutInterval = 180 // Gemini parse + 画像生成 + 棚入れまで同期で返る
        if let apiKey { req.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization") }
        let (data, resp) = try await URLSession.shared.data(for: req)
        guard let http = resp as? HTTPURLResponse else { throw APIError.badStatus(-1) }
        if http.statusCode == 200, let result = try? JSONDecoder().decode(MakeResult.self, from: data) {
            return result
        }
        let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        throw APIError.message((json?["error"] as? String) ?? "HTTP \(http.statusCode)")
    }

    // 着用イメージ(on-body mockup)の完成をポーリング (max-age=5 の軽量 API)
    static func peek(sku: String) async throws -> MakePeek {
        var comps = URLComponents(url: base.appendingPathComponent("api/make/peek"), resolvingAgainstBaseURL: false)!
        comps.queryItems = [URLQueryItem(name: "sku", value: sku)]
        return try await get(comps.url!, as: MakePeek.self)
    }

    // みんなが作ったばかりの 8 件
    static func recentMakes() async throws -> [RecentMake] {
        try await get(base.appendingPathComponent("api/make/recent"), as: RecentMakesResponse.self).items
    }

    // Gallery: 公開フィード
    static func feed(page: Int = 1, query: String = "") async throws -> [FeedProduct] {
        var comps = URLComponents(url: base.appendingPathComponent("api/shop/feed.json"), resolvingAgainstBaseURL: false)!
        var items = [URLQueryItem(name: "page", value: String(page))]
        if !query.isEmpty { items.append(URLQueryItem(name: "q", value: query)) }
        comps.queryItems = items
        return try await get(comps.url!, as: FeedPage.self).products
    }

    // ── 認証: メール → 6桁コード → api_key (既存 MU アプリと同じ契約) ──
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
        try await getAuthed(base.appendingPathComponent("api/agent/sales"), apiKey: apiKey, as: SalesResponse.self)
    }

    static func myProducts(apiKey: String) async throws -> [AgentProduct] {
        try await getAuthed(base.appendingPathComponent("api/agent/products"), apiKey: apiKey, as: AgentProductsResponse.self).products ?? []
    }

    // MARK: - plumbing

    private static func get<T: Decodable>(_ url: URL, as type: T.Type) async throws -> T {
        let (data, resp) = try await URLSession.shared.data(from: url)
        guard let http = resp as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.badStatus((resp as? HTTPURLResponse)?.statusCode ?? -1)
        }
        return try JSONDecoder().decode(T.self, from: data)
    }

    private static func getAuthed<T: Decodable>(_ url: URL, apiKey: String, as type: T.Type) async throws -> T {
        var req = URLRequest(url: url)
        req.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        let (data, resp) = try await URLSession.shared.data(for: req)
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
