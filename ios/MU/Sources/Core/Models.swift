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
        // Server timestamps are fixed-format, independent of device calendar/12-hour settings.
        f.locale = Locale(identifier: "en_US_POSIX")
        f.calendar = Calendar(identifier: .gregorian)
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

// /api/app/agent/chat — アプリ内AIエージェントの意図判定結果。
struct AgentChatResponse: Codable {
    let ok: Bool
    let reply: String
    let action: String          // make | sales | list_mine | none
    let args: AgentArgs?
    struct AgentArgs: Codable {
        let prompt: String?
        let kind: String?
        let royalty: Int?
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

// POST /api/make の成功レスポンス (catalog.rs public_make)
struct MakeResult: Codable {
    let ok: Bool
    let sku: String
    let kind: String
    let display: String
    let hook: String
    var retailJpy: Int        // 作った後に価格変更できるよう var
    var designUrl: String
    let pdpUrl: String
    var status: String
    var autoApproved: Bool
    let buyUrl: String?
    var checkoutUrl: String?
    var note: String
    let editToken: String?
    let makerPct: Int?
    var makerEarnJpy: Int?
    var canRemix: Bool?
    let fulfillmentRoute: String?
    // 2026-08-28: 登録必須化に伴い作者は常に判明 → 紹介リンクも常に返る。
    // 広めて売れた分の10%が作者に入る(サーバ側 apply_maker_commission と同率)。
    let affiliateLink: String?

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
        case affiliateLink = "affiliate_link"
        case canRemix = "can_remix"
        case fulfillmentRoute = "fulfillment_route"
    }

    var designURL: URL? { URL(string: designUrl) }
    var priceLabel: String { "¥\(retailJpy.formatted())" }

    var isLive: Bool { status == "live" }
    func supportsRemix(kinds: [MakeKindOption]) -> Bool {
        guard isLive, editToken != nil else { return false }
        if let canRemix { return canRemix }
        // An explicit incompatible route always wins over a kind-level capability.
        if let fulfillmentRoute, fulfillmentRoute != "printful_dtg" { return false }
        return kinds.first { $0.kind == kind }?.canRemix == true
    }

    mutating func applyPrice(_ saved: SavedMakePrice) {
        retailJpy = saved.priceJpy
        makerEarnJpy = saved.makerEarnJpy ?? makerPct.map { saved.priceJpy * $0 / 100 }
    }

    mutating func applyStatus(_ status: String?) {
        guard let status else { return }
        if self.status != status {
            switch status {
            case "live": note = String(localized: "make.done")
            case "review": note = String(localized: "make.reviewPending")
            default: note = String(localized: "make.notLive")
            }
        }
        self.status = status
        autoApproved = status == "live"
        if isLive {
            if checkoutUrl == nil {
                var url = URLComponents(string: "https://wearmu.com/api/shop/checkout")!
                url.queryItems = [URLQueryItem(name: "sku", value: sku)]
                checkoutUrl = url.url?.absoluteString
            }
        } else {
            checkoutUrl = nil
        }
    }
}

enum MakePreviewKind: String, Codable {
    case printful, card, mockup, lifestyle, design, unknown
    init(from decoder: Decoder) throws {
        let value = try decoder.singleValueContainer().decode(String.self)
        self = Self(rawValue: value) ?? .unknown
    }
}

// Optional additions keep older servers compatible. is_model is not proof of on-body accuracy.
struct PeekResult: Codable {
    let ok: Bool
    let sku: String?
    let status: String?
    let mockup: String?
    let isModel: Bool?
    let designUrl: String?
    let previewKind: MakePreviewKind?
    let previewRevision: PreviewRevision?
    let ready: Bool?
    enum CodingKeys: String, CodingKey {
        case ok, sku, status, mockup, ready, isModel = "is_model"
        case designUrl = "design_url", previewKind = "preview_kind", previewRevision = "preview_revision"
    }
    var mockupURL: URL? { mockup.flatMap(URL.init(string:)) }
    var isProductPreview: Bool { previewKind == .printful || previewKind == .mockup || previewKind == .lifestyle }
}

// The revision is an opaque server token (string or integer), never a guessed timestamp.
struct PreviewRevision: Codable, Equatable {
    let value: String
    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if let string = try? c.decode(String.self) { value = string }
        else { value = String(try c.decode(Int64.self)) }
    }
    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        try c.encode(value)
    }
}

struct MakePriceResponse: Decodable {
    let ok: Bool
    let priceJpy: Int?
    let makerEarnJpy: Int?
    enum CodingKeys: String, CodingKey {
        case ok, priceJpy = "price_jpy", makerEarnJpy = "maker_earn_jpy"
    }
    var saved: SavedMakePrice? {
        guard ok, let priceJpy, priceJpy > 0 else { return nil }
        return SavedMakePrice(priceJpy: priceJpy, makerEarnJpy: makerEarnJpy)
    }
}

struct SavedMakePrice {
    let priceJpy: Int
    let makerEarnJpy: Int?
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
    var retainedScore: DesignScore? { improved ? after : before }
}

struct MakeKindsResponse: Decodable {
    let ok: Bool
    let items: [MakeKindOption]
}

// The server's canonical physical catalog is deliberately not an enum or a local whitelist.
struct MakeKindOption: Codable, Identifiable, Hashable {
    let kind: String
    let labelJa: String
    let labelEn: String
    let retailJpy: Int?
    let category: String
    let canRemix: Bool
    var canMake: Bool? = nil
    var unavailableReason: String? = nil
    var unavailableReasonJa: String? = nil
    var unavailableReasonEn: String? = nil
    var temporary: Bool? = nil
    var requiresVendorPreflight: Bool? = nil
    var country: String? = nil
    var allowedCountries: [String]? = nil
    var excludedCountries: [String]? = nil
    var stockCheckedAt: String? = nil
    var id: String { kind }
    enum CodingKeys: String, CodingKey {
        case kind, category, labelJa = "label_ja", labelEn = "label_en"
        case retailJpy = "retail_jpy", canRemix = "can_remix"
        case canMake = "can_make"
        case unavailableReason = "unavailable_reason"
        case unavailableReasonJa = "unavailable_reason_ja", unavailableReasonEn = "unavailable_reason_en"
        case temporary, country, requiresVendorPreflight = "requires_vendor_preflight"
        case allowedCountries = "allowed_countries", excludedCountries = "excluded_countries"
        case stockCheckedAt = "stock_checked_at"
    }
    var label: String {
        label(locale: .current)
    }
    func label(locale: Locale) -> String {
        locale.language.languageCode?.identifier == "ja" ? labelJa : labelEn
    }
    func unavailableMessage(locale: Locale = .current) -> String? {
        guard !isAvailable else { return nil }
        let japanese = locale.language.languageCode?.identifier == "ja"
        let reasons = japanese ? [unavailableReasonJa, unavailableReasonEn] : [unavailableReasonEn, unavailableReasonJa]
        for reason in reasons {
            if let text = reason?.trimmingCharacters(in: .whitespacesAndNewlines), !text.isEmpty { return text }
        }
        return String(localized: "make.kind.unavailableHint", locale: locale)
    }
    // Auto has no price by definition. Physical kinds require a usable spec/price;
    // an explicit server denial wins even when a historical price is present.
    var isAvailable: Bool {
        canMake != false && (kind.isEmpty || (retailJpy ?? 0) > 0)
    }
    static func isAvailable(_ kind: String, in kinds: [MakeKindOption]) -> Bool {
        kinds.first { $0.kind == kind }?.isAvailable == true
    }
    func matches(_ query: String) -> Bool {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        return q.isEmpty || [kind, labelJa, labelEn, category].contains { $0.localizedStandardContains(q) }
    }
    func estimatedPrice(royalty: Int) -> Int? {
        guard isAvailable, let base = retailJpy, base > 0 else { return nil }
        let pct = min(50, max(10, royalty))
        let raw = Int((Double(base) * 0.9 / (1 - Double(pct) / 100)).rounded())
        return min(max(((raw + 50) / 100) * 100, base), 99_000)
    }
    static let auto = MakeKindOption(kind: "", labelJa: "おまかせ", labelEn: "Auto",
                                     retailJpy: nil, category: "common", canRemix: false)
    // Network failure only: the previously shipped seven choices, with verified base prices.
    // Remix stays unavailable until server capabilities have loaded.
    static let fallback: [MakeKindOption] = [
        .auto,
        .init(kind: "tee", labelJa: "Tシャツ（黒）", labelEn: "T-shirt (black)", retailJpy: 4900, category: "wear", canRemix: false),
        .init(kind: "hoodie", labelJa: "パーカー", labelEn: "Hoodie", retailJpy: 8800, category: "wear", canRemix: false),
        .init(kind: "sticker", labelJa: "ステッカー", labelEn: "Sticker", retailJpy: 800, category: "carry", canRemix: false),
        .init(kind: "rashguard_ls", labelJa: "ラッシュガード", labelEn: "Rashguard", retailJpy: 9800, category: "wear", canRemix: false),
        .init(kind: "tote", labelJa: "トートバッグ", labelEn: "Tote bag", retailJpy: 3800, category: "carry", canRemix: false),
        .init(kind: "mug", labelJa: "マグカップ", labelEn: "Mug", retailJpy: 2200, category: "carry", canRemix: false)
    ]
}

// Retain a successful catalog through refresh failures. Never resurrect a denied
// kind (or old price/label) using the initial offline fallback after a live load.
struct MakeKindCatalog {
    private(set) var items: [MakeKindOption] = [.auto]
    private(set) var loaded = false
    private(set) var refreshFailed = false
    mutating func received(_ items: [MakeKindOption]) {
        self.items = items
        loaded = true
        refreshFailed = false
    }
    mutating func failed() {
        if !loaded { items = MakeKindOption.fallback }
        loaded = true
        refreshFailed = true
    }
}

// Each top-level request owns a generation; each per-SKU operation also owns a token.
// Cancellation is an optimization. Token checks are what reject late network responses.
struct MakeRequestScope {
    private(set) var generation = UUID()
    private var operations: [String: UUID] = [:]
    mutating func reset() { generation = UUID(); operations.removeAll() }
    mutating func begin(_ key: String) -> UUID {
        let token = UUID()
        operations[key] = token
        return token
    }
    func accepts(_ generation: UUID, key: String, token: UUID) -> Bool {
        self.generation == generation && operations[key] == token
    }
}

struct MakeInput {
    let prompt: String
    let kind: String
    let royalty: Int
}

enum MakeIntent {
    case create(MakeInput)
    case variation(MakeInput)
    case remix(sku: String, words: String)
}

struct DesignVariant: Identifiable {
    var result: MakeResult
    var mockupURL: URL?
    var previewKind: MakePreviewKind?
    var previewRevision: PreviewRevision?
    var polishedURL: URL?
    var score: DesignScore?
    var polishNote: String?
    var previewPending = true
    var previewTimedOut = false
    // After polish, legacy peek responses cannot prove which design they represent.
    var requiresDesignMatch = false
    var invalidatedRevision: PreviewRevision?
    var id: String { result.sku }
    var shownURL: URL? { mockupURL ?? polishedURL ?? result.designURL }
    var previewLabelKey: String {
        mockupURL != nil ? "make.preview.reference" : "make.preview.design"
    }

    mutating func applyPolish(_ polish: PolishResult) {
        if let retained = polish.retainedScore { score = retained }
        polishNote = polish.note
        if polish.improved, let url = polish.designURL {
            invalidatedRevision = previewRevision
            polishedURL = url
            result.designUrl = url.absoluteString
            mockupURL = nil
            previewKind = nil
            requiresDesignMatch = true
            previewPending = true
            previewTimedOut = false
        }
    }

    // Returns true only when the typed preview is complete for the current design.
    mutating func applyPeek(_ peek: PeekResult) -> Bool {
        guard peek.ok, peek.sku == nil || peek.sku == id else { return false }
        if requiresDesignMatch {
            guard peek.designUrl == result.designUrl else { return false }
            // A new design with the old revision must not resurrect its old preview.
            if let old = invalidatedRevision {
                guard let revision = peek.previewRevision, revision != old else { return false }
            }
        } else if let design = peek.designUrl, design != result.designUrl {
            return false
        }
        result.applyStatus(peek.status)
        if let url = peek.mockupURL, peek.previewKind == nil || peek.isProductPreview,
           url != result.designURL, peek.ready != false {
            mockupURL = url
            previewKind = peek.previewKind
            previewRevision = peek.previewRevision
            previewTimedOut = false
        }
        let complete = peek.ready == true && peek.isProductPreview && mockupURL != nil
        previewPending = !complete
        return complete
    }
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
