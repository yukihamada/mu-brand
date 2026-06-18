import Foundation

// /api/shop/feed.json の1商品 (2026-06-13 実打確認済のスキーマ)。
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
    var checkoutURL: URL? { URL(string: checkoutUrl) }
    var pdpWebURL: URL? { URL(string: pdpUrl) }

    var priceLabel: String { "¥\(priceJpy.formatted())" }

    /// 編集的な短いタイトル。feed に name フィールドは無いので、説明文の最初の一文を使う。
    var title: String {
        let separators = CharacterSet(charactersIn: "。.!?！？\n")
        let first = description.components(separatedBy: separators).first ?? description
        let trimmed = first.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? description : trimmed
    }

    // created_at は SQLite UTC "YYYY-MM-DD HH:MM:SS"
    var createdDate: Date? {
        Self.dateFormatter.date(from: createdAt)
    }

    private static let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm:ss"
        f.timeZone = TimeZone(identifier: "UTC")
        f.locale = Locale(identifier: "en_US_POSIX")
        return f
    }()
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

// /api/agent/sales (Bearer)。公開契約は totals まで (既存 MU アプリと同じ)。
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

// kind タブ。サーバ側ホワイトリスト (tee/rashguard/hoodie/sticker/song/house) と一致
// — 2026-06-13 実打確認: kind=tee/hoodie/rashguard/sticker は絞り込み有効、未知 kind は全件。
enum AtelierKind: String, CaseIterable, Identifiable {
    case all = ""
    case tee, hoodie, rashguard, sticker, song, house

    var id: String { rawValue }

    var label: String {
        switch self {
        case .all: return String(localized: "kind.all")
        case .tee: return String(localized: "kind.tee")
        case .hoodie: return String(localized: "kind.hoodie")
        case .rashguard: return String(localized: "kind.rashguard")
        case .sticker: return String(localized: "kind.sticker")
        case .song: return String(localized: "kind.song")
        case .house: return String(localized: "kind.house")
        }
    }
}
