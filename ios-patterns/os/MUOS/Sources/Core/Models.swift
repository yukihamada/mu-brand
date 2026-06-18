import Foundation

// /api/shop/feed.json の1商品 (既存 MU アプリと同一契約)
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
    var priceLabel: String { Fmt.yen(priceJpy) }

    // created_at は SQLite UTC "YYYY-MM-DD HH:MM:SS"
    var createdDate: Date? { Fmt.feedUTC.date(from: createdAt) }
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

// /api/brands
struct BrandSummary: Codable, Identifiable, Hashable {
    let slug: String
    let name: String
    let emoji: String?
    let tagline: String?
    let productCount: Int?

    var id: String { slug }

    enum CodingKeys: String, CodingKey {
        case slug, name, emoji, tagline
        case productCount = "product_count"
    }
}

struct BrandsResponse: Codable {
    let brands: [BrandSummary]
}

// /api/updates — MU の公開鼓動ログ
struct UpdateItem: Codable, Identifiable {
    let id: Int
    let at: String
    let author: String?
    let kind: String?
    let text: String
    let url: String?

    var date: Date? { Fmt.updateJST.date(from: at) }
}

struct UpdatesResponse: Codable {
    let updates: [UpdateItem]
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

// /api/transparency — 毎リクエスト再計算される公開実数。
// 全フィールド optional (サーバ側の進化に耐える防御的デコード)。
struct Transparency: Codable {
    struct Real: Codable {
        let purchases: Int?
        let revenueJpy: Int?
        let note: String?

        enum CodingKeys: String, CodingKey {
            case purchases, note
            case revenueJpy = "revenue_jpy"
        }
    }

    struct External: Codable {
        let purchases: Int?
        let purchases7d: Int?
        let revenueJpy: Int?
        let distinctCustomers: Int?
        let note: String?

        enum CodingKeys: String, CodingKey {
            case purchases, note
            case purchases7d = "purchases_7d"
            case revenueJpy = "revenue_jpy"
            case distinctCustomers = "distinct_customers"
        }
    }

    struct BrandRow: Codable {
        let brand: String
        let orders: Int
        let revenueJpy: Int

        enum CodingKeys: String, CodingKey {
            case brand, orders
            case revenueJpy = "revenue_jpy"
        }
    }

    struct Refunded: Codable {
        let orders: Int?
        let amountJpy: Int?

        enum CodingKeys: String, CodingKey {
            case orders
            case amountJpy = "amount_jpy"
        }
    }

    struct Catalog: Codable {
        let orders: Int?
        let revenueJpy: Int?
        let byBrand: [BrandRow]?
        let refundedExcluded: Refunded?
        let note: String?

        enum CodingKeys: String, CodingKey {
            case orders, note
            case revenueJpy = "revenue_jpy"
            case byBrand = "by_brand"
            case refundedExcluded = "refunded_excluded"
        }
    }

    struct MA: Codable {
        let sold: Int?
        let auctionRevenueBookedJpy: Int?

        enum CodingKeys: String, CodingKey {
            case sold
            case auctionRevenueBookedJpy = "auction_revenue_booked_jpy"
        }
    }

    struct Breakdown: Codable {
        let auctionsJpy: Int?
        let shirtsJpy: Int?
        let youTeeJpy: Int?

        enum CodingKeys: String, CodingKey {
            case auctionsJpy = "auctions_jpy"
            case shirtsJpy = "shirts_jpy"
            case youTeeJpy = "you_tee_jpy"
        }
    }

    struct Purchase: Codable {
        let atJst: String?
        let buyer: String?
        let name: String?
        let priceJpy: Int?
        let brand: String?

        enum CodingKeys: String, CodingKey {
            case buyer, name, brand
            case atJst = "at_jst"
            case priceJpy = "price_jpy"
        }

        var date: Date? { atJst.flatMap { Fmt.purchaseJST.date(from: $0) } }
    }

    struct MissingDrops: Codable {
        let mugenMissingDrops: [Int]?
        let muonMissingDates: [String]?
        let note: String?

        enum CodingKeys: String, CodingKey {
            case note
            case mugenMissingDrops = "mugen_missing_drops"
            case muonMissingDates = "muon_missing_dates"
        }
    }

    struct Pledge: Codable {
        let estimatedPledgeJpy: Int?
        let floorJpy: Int?
        let fiscalYearRevenueJpy: Int?
        let constitution: String?

        enum CodingKeys: String, CodingKey {
            case constitution
            case estimatedPledgeJpy = "estimated_pledge_jpy"
            case floorJpy = "floor_jpy"
            case fiscalYearRevenueJpy = "fiscal_year_revenue_jpy"
        }
    }

    struct Segment: Codable {
        let key: String
        let jpy: Int?
        let ratio: Double?
        let recipient: String?
    }

    struct SplitBreakdown: Codable {
        let tier: String?
        let donationRatio: Double?
        let netAfterTaxJpy: Int?
        let segments: [Segment]?

        enum CodingKeys: String, CodingKey {
            case tier, segments
            case donationRatio = "donation_ratio"
            case netAfterTaxJpy = "net_after_tax_jpy"
        }
    }

    struct ProfitSplit: Codable {
        let constitution: String?
        let breakdown: SplitBreakdown?
    }

    let asOf: String?
    let revenueTotalJpy: Int?
    let revenueJpy: Int?
    let purchasesRecorded: Int?
    let shirtsSold: Int?
    let real: Real?
    let external: External?
    let catalog: Catalog?
    let ma: MA?
    let revenueBreakdown: Breakdown?
    let recentPurchases: [Purchase]?
    let missingDrops: MissingDrops?
    let teshikagaPledge: Pledge?
    let profitSplit: ProfitSplit?

    enum CodingKeys: String, CodingKey {
        case real, external, catalog, ma
        case asOf = "as_of"
        case revenueTotalJpy = "revenue_total_jpy"
        case revenueJpy = "revenue_jpy"
        case purchasesRecorded = "purchases_recorded"
        case shirtsSold = "shirts_sold"
        case revenueBreakdown = "revenue_breakdown"
        case recentPurchases = "recent_purchases"
        case missingDrops = "missing_drops"
        case teshikagaPledge = "teshikaga_pledge"
        case profitSplit = "profit_split"
    }

    var asOfDate: Date? {
        asOf.flatMap(Double.init).map { Date(timeIntervalSince1970: $0) }
    }
}
