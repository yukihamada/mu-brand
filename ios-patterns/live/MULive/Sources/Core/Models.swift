import Foundation

// /api/shop/feed.json の1商品。フィールドは 2026-06-13 実打で確認済み:
// sku / brand / description / price_jpy / mockup_url / sold / created_at / pdp_url / checkout_url
// (商品名フィールドは存在しない → description の第一文を表示タイトルとして使う)
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
    var pdpURL: URL? { URL(string: pdpUrl) }
    var checkoutURL: URL? { URL(string: checkoutUrl) }
    var priceLabel: String { "¥\(priceJpy.formatted())" }

    // description の第一文 = 表示タイトル (feed.json に name が無いため)
    var displayTitle: String {
        let first = description.split(separator: "。").first.map(String.init) ?? description
        return first.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    // created_at は SQLite UTC "YYYY-MM-DD HH:MM:SS" (既存 MU アプリと同じ解釈)
    private static let sqliteUTC: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm:ss"
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "UTC")
        return f
    }()

    private static let relative: RelativeDateTimeFormatter = {
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        return f
    }()

    var createdDate: Date? { Self.sqliteUTC.date(from: createdAt) }

    // "3時間前" / "3 hr. ago" (ロケール追従)
    var relativeAgeLabel: String? {
        createdDate.map { Self.relative.localizedString(for: $0, relativeTo: Date()) }
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

// kind チャンネル。値はサーバ側ホワイトリストと一致 (tee/hoodie/rashguard/sticker は
// 2026-06-13 に ?kind= 実打で結果が正しく絞れることを確認済み。song は TEE が返る・
// house は1件のみなのでチャンネルから除外)。
enum ProductKind: String, CaseIterable, Identifiable {
    case all = ""
    case tee, hoodie, rashguard, sticker

    var id: String { rawValue }

    var label: String {
        switch self {
        case .all: return String(localized: "kind.all")
        case .tee: return "TEE"
        case .hoodie: return String(localized: "kind.hoodie")
        case .rashguard: return String(localized: "kind.rashguard")
        case .sticker: return String(localized: "kind.sticker")
        }
    }
}
