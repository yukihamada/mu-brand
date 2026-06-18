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

// wearmu.com REST クライアント。全 API は既存本番ルート (docs/IOS_APP_DESIGN.md §2)。
struct MUAPI {
    static let base = URL(string: "https://wearmu.com")!

    static func feed(page: Int = 1, kind: ProductKind = .all, query: String = "") async throws -> [FeedProduct] {
        var comps = URLComponents(url: base.appendingPathComponent("api/shop/feed.json"), resolvingAgainstBaseURL: false)!
        // physical=1: デジタル(song等)を除外 (App Store 3.1.1)。アプリは物理グッズのみ扱う。
        var items = [URLQueryItem(name: "page", value: String(page)),
                     URLQueryItem(name: "physical", value: "1")]
        if !kind.rawValue.isEmpty { items.append(URLQueryItem(name: "kind", value: kind.rawValue)) }
        if !query.isEmpty { items.append(URLQueryItem(name: "q", value: query)) }
        comps.queryItems = items
        return try await get(comps.url!, as: FeedPage.self).products
    }

    // 商品ページの「関連商品」(同タイプ・売れ筋順)。GET /api/shop/related?sku=
    static func related(sku: String, limit: Int = 8) async throws -> [FeedProduct] {
        var comps = URLComponents(url: base.appendingPathComponent("api/shop/related"), resolvingAgainstBaseURL: false)!
        comps.queryItems = [URLQueryItem(name: "sku", value: sku), URLQueryItem(name: "limit", value: String(limit))]
        return try await get(comps.url!, as: ProductList.self).products
    }

    // 売れ筋・人気(feed の既定ソート=MUスコア順の先頭)。Make の「人気から作る」用。
    static func popular() async throws -> [FeedProduct] {
        try await feed(page: 1)
    }

    static func brands() async throws -> [BrandSummary] {
        try await get(base.appendingPathComponent("api/brands"), as: BrandsResponse.self).brands
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

    // 「言えば、作れる」— POST /api/make?prompt=&kind=。AI がデザイン生成 → 即棚に並ぶ。
    // apiKey があればログインユーザーに帰属 (売れるたび売上10%が作者へ・apply_maker_commission)。
    // 画像生成に時間がかかるため timeout を延ばす。
    static func make(prompt: String, kind: MakeKind, royalty: Int, apiKey: String?) async throws -> MakeResult {
        var comps = URLComponents(url: base.appendingPathComponent("api/make"), resolvingAgainstBaseURL: false)!
        var items = [URLQueryItem(name: "prompt", value: prompt),
                     URLQueryItem(name: "royalty", value: String(royalty))]
        if !kind.rawValue.isEmpty { items.append(URLQueryItem(name: "kind", value: kind.rawValue)) }
        comps.queryItems = items

        var req = URLRequest(url: comps.url!)
        req.httpMethod = "POST"
        req.timeoutInterval = 90
        if let key = apiKey, !key.isEmpty {
            req.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization")
        }
        let (data, resp) = try await URLSession.shared.data(for: req)
        guard let http = resp as? HTTPURLResponse else { throw APIError.badStatus(-1) }
        guard (200...299).contains(http.statusCode) else {
            let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
            throw APIError.message((json?["error"] as? String) ?? "HTTP \(http.statusCode)")
        }
        return try JSONDecoder().decode(MakeResult.self, from: data)
    }

    // 「磨く」— POST /api/make/polish/:sku?t=。現デザインを5軸採点し、弱点を改善した候補を
    // best-of-N 再生成。元より高得点なら差し替え (improved=true)。採点+生成で時間がかかる。
    static func polish(sku: String, editToken: String) async throws -> PolishResult {
        var comps = URLComponents(
            url: base.appendingPathComponent("api/make/polish/\(sku)"),
            resolvingAgainstBaseURL: false)!
        comps.queryItems = [URLQueryItem(name: "t", value: editToken)]
        var req = URLRequest(url: comps.url!)
        req.httpMethod = "POST"
        req.timeoutInterval = 150
        let (data, resp) = try await URLSession.shared.data(for: req)
        guard let http = resp as? HTTPURLResponse else { throw APIError.badStatus(-1) }
        guard (200...299).contains(http.statusCode) else {
            let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
            throw APIError.message((json?["error"] as? String) ?? "HTTP \(http.statusCode)")
        }
        return try JSONDecoder().decode(PolishResult.self, from: data)
    }

    // リミックス — 既存デザインに一言足して別バージョンを織る。POST /make/remix
    // (form: sku, words)。元の作者にはリミックス印税5%が流れる。応答は make 互換。
    static func remix(sku: String, words: String, apiKey: String?) async throws -> MakeResult {
        var req = URLRequest(url: base.appendingPathComponent("api/design-remix"))
        req.httpMethod = "POST"
        req.timeoutInterval = 120
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        if let key = apiKey, !key.isEmpty {
            req.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization")
        }
        var comps = URLComponents()
        comps.queryItems = [URLQueryItem(name: "sku", value: sku), URLQueryItem(name: "words", value: words)]
        req.httpBody = comps.percentEncodedQuery?.data(using: .utf8)
        let (data, resp) = try await URLSession.shared.data(for: req)
        guard let http = resp as? HTTPURLResponse else { throw APIError.badStatus(-1) }
        guard (200...299).contains(http.statusCode) else {
            let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
            throw APIError.message((json?["error"] as? String) ?? "HTTP \(http.statusCode)")
        }
        return try JSONDecoder().decode(MakeResult.self, from: data)
    }

    // アプリ内 AI エージェント。意図判定(make/sales/list_mine/none)を返す。POST /api/app/agent/chat
    static func agentChat(message: String, history: [[String: String]], apiKey: String?) async throws -> AgentChatResponse {
        var req = URLRequest(url: base.appendingPathComponent("api/app/agent/chat"))
        req.httpMethod = "POST"
        req.timeoutInterval = 60
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let key = apiKey, !key.isEmpty { req.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization") }
        req.httpBody = try JSONSerialization.data(withJSONObject: ["message": message, "history": history])
        let (data, resp) = try await URLSession.shared.data(for: req)
        guard let http = resp as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
            throw APIError.message((json?["error"] as? String) ?? "HTTP \((resp as? HTTPURLResponse)?.statusCode ?? -1)")
        }
        return try JSONDecoder().decode(AgentChatResponse.self, from: data)
    }

    // 作った後に価格を変更。POST /api/make/edit/:sku?t= {price_jpy}。送った価格を返す
    // (サーバは原価フロア〜¥99,000にクランプ)。
    @discardableResult
    static func editPrice(sku: String, editToken: String, priceJpy: Int) async throws -> Int {
        var comps = URLComponents(url: base.appendingPathComponent("api/make/edit/\(sku)"), resolvingAgainstBaseURL: false)!
        comps.queryItems = [URLQueryItem(name: "t", value: editToken)]
        var req = URLRequest(url: comps.url!)
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try JSONSerialization.data(withJSONObject: ["price_jpy": priceJpy])
        let (data, resp) = try await URLSession.shared.data(for: req)
        guard let http = resp as? HTTPURLResponse, (200...299).contains(http.statusCode) else {
            let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
            throw APIError.message((json?["error"] as? String) ?? "HTTP \((resp as? HTTPURLResponse)?.statusCode ?? -1)")
        }
        return priceJpy
    }

    // 着画(オンボディmockup)が出来たか確認。GET /api/make/peek?sku=
    static func peek(sku: String) async throws -> PeekResult {
        var comps = URLComponents(url: base.appendingPathComponent("api/make/peek"), resolvingAgainstBaseURL: false)!
        comps.queryItems = [URLQueryItem(name: "sku", value: sku)]
        let (data, resp) = try await URLSession.shared.data(from: comps.url!)
        guard let http = resp as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.badStatus((resp as? HTTPURLResponse)?.statusCode ?? -1)
        }
        return try JSONDecoder().decode(PeekResult.self, from: data)
    }

    // APNs デバイストークンを登録 (ドロップ/売れた通知の宛先)。ログイン時は Bearer も付ける。
    static func registerPush(token: String, apiKey: String?) async {
        var req = URLRequest(url: base.appendingPathComponent("api/app/push/register"))
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let key = apiKey, !key.isEmpty {
            req.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization")
        }
        req.httpBody = try? JSONSerialization.data(withJSONObject: ["token": token, "platform": "ios"])
        _ = try? await URLSession.shared.data(for: req)
    }

    // 軽量 funnel 計測。失敗は握り潰す (計測でユーザー体験を止めない)。
    static func track(_ event: String, props: [String: Any] = [:], apiKey: String?) async {
        var req = URLRequest(url: base.appendingPathComponent("api/app/event"))
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let key = apiKey, !key.isEmpty {
            req.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization")
        }
        var body: [String: Any] = ["event": event]
        if !props.isEmpty { body["props"] = props }
        req.httpBody = try? JSONSerialization.data(withJSONObject: body)
        _ = try? await URLSession.shared.data(for: req)
    }

    // App Store Guideline 5.1.1(v): アカウント削除 (サーバ側は冪等・compliance log のみ保持)
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

    // 自分が作った商品一覧。GET /api/agent/products (Bearer)
    static func listMine(apiKey: String) async throws -> ProductsResponse {
        try await authedGet(base.appendingPathComponent("api/agent/products"), apiKey: apiKey, as: ProductsResponse.self)
    }

    // アカウント状態 (クレジット残高・ストア)。GET /api/agent/me (Bearer)
    static func status(apiKey: String) async throws -> MeResponse {
        try await authedGet(base.appendingPathComponent("api/agent/me"), apiKey: apiKey, as: MeResponse.self)
    }

    // 紹介リンクと実績。GET /api/agent/affiliate (Bearer)
    static func affiliate(apiKey: String) async throws -> AffiliateResponse {
        try await authedGet(base.appendingPathComponent("api/agent/affiliate"), apiKey: apiKey, as: AffiliateResponse.self)
    }

    // 配送状況 (PIIマスク済み)。GET /api/agent/ship/orders (Bearer)
    static func shipOrders(apiKey: String) async throws -> ShipOrdersResponse {
        try await authedGet(base.appendingPathComponent("api/agent/ship/orders"), apiKey: apiKey, as: ShipOrdersResponse.self)
    }

    // MARK: - plumbing

    private static func get<T: Decodable>(_ url: URL, as type: T.Type) async throws -> T {
        let (data, resp) = try await URLSession.shared.data(from: url)
        guard let http = resp as? HTTPURLResponse, http.statusCode == 200 else {
            throw APIError.badStatus((resp as? HTTPURLResponse)?.statusCode ?? -1)
        }
        return try JSONDecoder().decode(T.self, from: data)
    }

    // Bearer 認証つき GET (sales と同じエラー処理規約)。
    private static func authedGet<T: Decodable>(_ url: URL, apiKey: String, as type: T.Type) async throws -> T {
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
