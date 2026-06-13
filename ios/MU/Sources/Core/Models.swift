import Foundation

// /api/shop/feed.json の1商品。Live フィードと Shop グリッドの共通単位。
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

// /api/shop/related の {products:[...]} 用。
struct ProductList: Codable {
    let products: [FeedProduct]
}

// /api/brands の1ブランド (チップ表示に使う最小限)
struct BrandSummary: Codable, Identifiable, Hashable {
    let slug: String
    let name: String
    let emoji: String?
    let productCount: Int?

    var id: String { slug }

    enum CodingKeys: String, CodingKey {
        case slug, name, emoji
        case productCount = "product_count"
    }
}

struct BrandsResponse: Codable {
    let brands: [BrandSummary]
}

// /api/agent/sales (Bearer)
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

// POST /api/make の成功レスポンス (catalog.rs public_make)
struct MakeResult: Codable {
    let ok: Bool
    let sku: String
    let kind: String
    let display: String
    let hook: String
    var retailJpy: Int        // 作った後に価格変更できるよう var
    let designUrl: String
    let pdpUrl: String
    let status: String
    let autoApproved: Bool
    let buyUrl: String?
    let checkoutUrl: String?
    let note: String
    let editToken: String?
    let makerPct: Int?
    let makerEarnJpy: Int?

    enum CodingKeys: String, CodingKey {
        case ok, sku, kind, display, hook, status, note
        case retailJpy = "retail_jpy"
        case designUrl = "design_url"
        case pdpUrl = "pdp_url"
        case autoApproved = "auto_approved"
        case buyUrl = "buy_url"
        case checkoutUrl = "checkout_url"
        case editToken = "edit_token"
        case makerPct = "maker_pct"
        case makerEarnJpy = "maker_earn_jpy"
    }

    var designURL: URL? { URL(string: designUrl) }
    var priceLabel: String { "¥\(retailJpy.formatted())" }
}

// /api/make/peek?sku= — 着画(モデル着用 or 平置きmockup)が出来たかをポーリング。
struct PeekResult: Codable {
    let ok: Bool
    let status: String?
    let mockup: String?
    let isModel: Bool?     // true = 人が着ている写真
    enum CodingKeys: String, CodingKey { case ok, status, mockup, isModel = "is_model" }
    var mockupURL: URL? { mockup.flatMap(URL.init(string:)) }
}

// 5軸スコア (MUスコア)。/api/make/polish の before/after。
struct DesignScore: Codable {
    let total: Int
    let axes: [String: Int]
    let verdict: String

    // 表示順を固定 (visual→universality→craft→concept→desire)
    static let axisOrder = ["visual", "universality", "craft", "concept", "desire"]
    static func axisLabel(_ k: String) -> String {
        switch k {
        case "visual": return String(localized: "score.visual")
        case "universality": return String(localized: "score.universality")
        case "craft": return String(localized: "score.craft")
        case "concept": return String(localized: "score.concept")
        case "desire": return String(localized: "score.desire")
        default: return k
        }
    }
    var orderedAxes: [(String, Int)] {
        Self.axisOrder.compactMap { k in axes[k].map { (k, $0) } }
    }
}

// POST /api/make/polish/:sku の結果
struct PolishResult: Codable {
    let ok: Bool
    let improved: Bool
    let before: DesignScore?
    let after: DesignScore?
    let designUrl: String?
    let note: String

    enum CodingKeys: String, CodingKey {
        case ok, improved, before, after, note
        case designUrl = "design_url"
    }
    var designURL: URL? { designUrl.flatMap(URL.init(string:)) }
}

// /make で作れる種類。"" = AI におまかせ (kind 省略 → サーバが文面から判定)。
// raw value はサーバ側 allowed リスト (catalog.rs) と一致させる。
enum MakeKind: String, CaseIterable, Identifiable {
    case auto = ""
    case tee, hoodie, sticker
    case rashguard = "rashguard_ls"
    case tote, mug

    var id: String { rawValue }

    var label: String {
        switch self {
        case .auto: return String(localized: "make.kind.auto")
        case .tee: return "TEE"
        case .hoodie: return String(localized: "kind.hoodie")
        case .sticker: return String(localized: "kind.sticker")
        case .rashguard: return String(localized: "kind.rashguard")
        case .tote: return String(localized: "make.kind.tote")
        case .mug: return String(localized: "make.kind.mug")
        }
    }
}

// kind チップ (サーバ側ホワイトリストと一致させる)。
// App Store 3.1.1: デジタル(song/house)はアプリで売らない → チップから除外し、
// feed は常に physical=1 で叩く (デジタルSKUをフィードからも除外)。
enum ProductKind: String, CaseIterable, Identifiable {
    case all = ""
    case tee, rashguard, hoodie, sticker

    var id: String { rawValue }

    var label: String {
        switch self {
        case .all: return String(localized: "kind.all")
        case .tee: return "TEE"
        case .rashguard: return String(localized: "kind.rashguard")
        case .hoodie: return String(localized: "kind.hoodie")
        case .sticker: return String(localized: "kind.sticker")
        }
    }
}
