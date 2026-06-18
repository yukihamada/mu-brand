import Foundation

// ── GET/POST /api/make の成功レスポンス (store/src/catalog.rs public_make 実装確認済) ──
// flagged (要人間レビュー) の場合 buy_url / checkout_url は null になる。
struct MakeResult: Codable, Identifiable, Hashable {
    let ok: Bool
    let sku: String
    let kind: String
    let display: String
    let hook: String
    let retailJpy: Int
    let designUrl: String
    let pdpUrl: String
    let status: String          // "live" | "review"
    let autoApproved: Bool
    let buyUrl: String?
    let checkoutUrl: String?
    let note: String
    let editToken: String
    let editUrl: String

    var id: String { sku }

    enum CodingKeys: String, CodingKey {
        case ok, sku, kind, display, hook, status, note
        case retailJpy = "retail_jpy"
        case designUrl = "design_url"
        case pdpUrl = "pdp_url"
        case autoApproved = "auto_approved"
        case buyUrl = "buy_url"
        case checkoutUrl = "checkout_url"
        case editToken = "edit_token"
        case editUrl = "edit_url"
    }

    var designURL: URL? { URL(string: designUrl) }
    var priceLabel: String { "¥\(retailJpy.formatted())" }
    var isLive: Bool { status == "live" }
}

// ── GET /api/make/peek?sku= — 着用イメージ(on-body mockup)の完成ポーリング ──
struct MakePeek: Codable {
    let ok: Bool
    let status: String?
    let mockup: String?
}

// ── GET /api/make/recent — みんなが /make で作ったばかりの 8 件 ──
struct RecentMake: Codable, Identifiable, Hashable {
    let sku: String
    let label: String
    let img: String
    let price: Int

    var id: String { sku }
    var imgURL: URL? { URL(string: img) }
    var pdpURL: URL? { URL(string: "https://wearmu.com/shop/\(sku)") }
    var title: String { label.components(separatedBy: " — ").first ?? label }
}

struct RecentMakesResponse: Codable {
    let items: [RecentMake]
}

// ── GET /api/shop/feed.json — Gallery (既存 MU アプリと同じ契約) ──
struct FeedProduct: Codable, Identifiable, Hashable {
    let sku: String
    let brand: String
    let description: String
    let priceJpy: Int
    let mockupUrl: String?
    let sold: Int
    let createdAt: String
    let pdpUrl: String
    let checkoutUrl: String

    var id: String { sku }

    enum CodingKeys: String, CodingKey {
        case sku, brand, description, sold
        case priceJpy = "price_jpy"
        case mockupUrl = "mockup_url"
        case createdAt = "created_at"
        case pdpUrl = "pdp_url"
        case checkoutUrl = "checkout_url"
    }

    var mockupURL: URL? { mockupUrl.flatMap(URL.init(string:)) }
    var priceLabel: String { "¥\(priceJpy.formatted())" }

    // created_at は SQLite UTC "YYYY-MM-DD HH:MM:SS"
    var createdDate: Date? {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm:ss"
        f.timeZone = TimeZone(identifier: "UTC")
        return f.date(from: createdAt)
    }
}

struct FeedPage: Codable {
    let page: Int
    let pageSize: Int
    let products: [FeedProduct]

    enum CodingKeys: String, CodingKey {
        case page, products
        case pageSize = "page_size"
    }
}

// ── GET /api/agent/sales (Bearer) ──
struct SalesResponse: Codable {
    let ok: Bool?
    let total: SalesTotal?

    struct SalesTotal: Codable {
        let orderCount: Int?
        let revenueJpy: Int?

        enum CodingKeys: String, CodingKey {
            case orderCount = "order_count"
            case revenueJpy = "revenue_jpy"
        }
    }
}

// ── GET /api/agent/products (Bearer) — 自ストアの商品 ──
struct AgentProduct: Codable, Identifiable, Hashable {
    let sku: String
    let store: String?
    let label: String?
    let kind: String?
    let retailPriceJpy: Int?
    let status: String?
    let designFile: String?
    let pdpUrl: String?

    var id: String { sku }

    enum CodingKeys: String, CodingKey {
        case sku, store, label, kind, status
        case retailPriceJpy = "retail_price_jpy"
        case designFile = "design_file"
        case pdpUrl = "pdp_url"
    }

    var imageURL: URL? { designFile.flatMap(URL.init(string:)) }
}

struct AgentProductsResponse: Codable {
    let ok: Bool?
    let products: [AgentProduct]?
}

// ── 端末ローカルの「自分が作ったもの」履歴 (匿名 /make は端末が唯一の台帳) ──
struct LocalCreation: Codable, Identifiable, Hashable {
    let sku: String
    let prompt: String
    let kind: String
    let designUrl: String
    var mockupUrl: String?
    let pdpUrl: String
    let checkoutUrl: String?
    let editUrl: String
    let priceJpy: Int
    let status: String
    let createdAt: Date

    var id: String { sku }
    var imageURL: URL? { (mockupUrl ?? designUrl).isEmpty ? nil : URL(string: mockupUrl ?? designUrl) }
    var priceLabel: String { "¥\(priceJpy.formatted())" }

    init(result: MakeResult, prompt: String) {
        self.sku = result.sku
        self.prompt = prompt
        self.kind = result.kind
        self.designUrl = result.designUrl
        self.mockupUrl = nil
        self.pdpUrl = result.pdpUrl
        self.checkoutUrl = result.checkoutUrl
        self.editUrl = result.editUrl
        self.priceJpy = result.retailJpy
        self.status = result.status
        self.createdAt = Date()
    }
}

// /make に渡せる kind (サーバ側 allowed リストの代表サブセット。おまかせ = nil)
enum MakeKind: String, CaseIterable, Identifiable {
    case auto = ""
    case tee, hoodie, sticker, mug, tote, poster

    var id: String { rawValue }

    var label: String {
        switch self {
        case .auto: return String(localized: "make.kind.auto")
        case .tee: return "TEE"
        case .hoodie: return String(localized: "make.kind.hoodie")
        case .sticker: return String(localized: "make.kind.sticker")
        case .mug: return String(localized: "make.kind.mug")
        case .tote: return String(localized: "make.kind.tote")
        case .poster: return String(localized: "make.kind.poster")
        }
    }

    var icon: String {
        switch self {
        case .auto: return "wand.and.stars"
        case .tee: return "tshirt"
        case .hoodie: return "figure.walk"
        case .sticker: return "seal"
        case .mug: return "cup.and.saucer"
        case .tote: return "bag"
        case .poster: return "photo.artframe"
        }
    }
}
