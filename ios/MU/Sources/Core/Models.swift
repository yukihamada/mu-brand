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

// kind チップ (サーバ側ホワイトリストと一致させる)
enum ProductKind: String, CaseIterable, Identifiable {
    case all = ""
    case tee, rashguard, hoodie, sticker, song, house

    var id: String { rawValue }

    var label: String {
        switch self {
        case .all: return String(localized: "kind.all")
        case .tee: return "TEE"
        case .rashguard: return String(localized: "kind.rashguard")
        case .hoodie: return String(localized: "kind.hoodie")
        case .sticker: return String(localized: "kind.sticker")
        case .song: return String(localized: "kind.song")
        case .house: return String(localized: "kind.house")
        }
    }
}
