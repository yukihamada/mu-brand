import Foundation
import SwiftUI

// ブランドの心拍を1つに束ねるモデル。
// データ源は全て公開 API: /api/transparency, /api/brands, /api/shop/feed.json, /api/updates。
// 派生値 (今日生まれた数・最終生成からの経過) は feed.json の created_at から実計算する。
@MainActor
final class PulseModel: ObservableObject {
    enum Phase: Equatable {
        case loading
        case ready
        case failed(String)
    }

    @Published private(set) var phase: Phase = .loading
    @Published private(set) var transparency: Transparency?
    @Published private(set) var brands: [BrandSummary] = []
    @Published private(set) var feed: [FeedProduct] = []
    @Published private(set) var updates: [UpdateItem] = []
    // feed を 4 ページ (240件) 読んでも今日生まれた分の底に届かなかった場合 true → "240+" 表示
    @Published private(set) var bornTodayCapped = false

    private var loadedOnce = false

    func loadIfNeeded() async {
        guard !loadedOnce else { return }
        await load()
    }

    func load() async {
        if feed.isEmpty && transparency == nil { phase = .loading }
        do {
            async let t = MUAPI.transparency()
            async let b = MUAPI.brands()
            async let u = MUAPI.updates()

            // feed は created_at 降順。今日(JST)の底が見えるまで最大4ページ取得。
            let todayStart = Self.jstStartOfToday()
            var all: [FeedProduct] = []
            var capped = true
            for page in 1...4 {
                let items = try await MUAPI.feed(page: page)
                all += items
                let oldest = items.compactMap(\.createdDate).min()
                if items.isEmpty || (oldest.map { $0 < todayStart } ?? false) {
                    capped = false
                    break
                }
            }

            self.feed = all
            self.bornTodayCapped = capped
            self.transparency = try await t
            self.brands = (try? await b) ?? []
            self.updates = (try? await u) ?? []
            self.loadedOnce = true
            phase = .ready
        } catch {
            if feed.isEmpty && transparency == nil {
                phase = .failed(error.localizedDescription)
            }
        }
    }

    // MARK: - 派生値 (全て実データからの計算)

    static func jstStartOfToday() -> Date {
        Fmt.jstCalendar.startOfDay(for: Date())
    }

    var totalSKU: Int {
        brands.compactMap(\.productCount).reduce(0, +)
    }

    var brandCount: Int { brands.count }

    var latestDropDate: Date? {
        feed.compactMap(\.createdDate).max()
    }

    var bornToday: Int {
        let start = Self.jstStartOfToday()
        return feed.compactMap(\.createdDate).filter { $0 >= start }.count
    }

    var bornTodayLabel: String {
        bornTodayCapped ? "\(bornToday)+" : "\(bornToday)"
    }

    var born24h: Int {
        let dayAgo = Date().addingTimeInterval(-24 * 3600)
        return feed.compactMap(\.createdDate).filter { $0 > dayAgo }.count
    }

    // ティッカー: 実 feed の新着 (JST 時刻 + SKU + 価格)
    var tickerItems: [String] {
        feed.prefix(14).map { p in
            let time = p.createdDate.map { Fmt.hhmmJST.string(from: $0) } ?? "--:--"
            return "\(time) \(p.sku) \(p.priceLabel)"
        }
    }

    // システムログ: GEN(feed 実生成) + SALE(/transparency 実購入) + LOG(/api/updates 実投稿) を時系列マージ
    struct LogEntry {
        enum Kind { case gen, sale, note }
        let kind: Kind
        let date: Date
        let text: String

        var tag: String {
            switch kind {
            case .gen: return "GEN"
            case .sale: return "SALE"
            case .note: return "LOG"
            }
        }

        var color: Color {
            switch kind {
            case .gen: return .muGold
            case .sale: return Color(red: 0.45, green: 0.85, blue: 0.55)
            case .note: return .muMute
            }
        }
    }

    var logEntries: [LogEntry] {
        var entries: [LogEntry] = []
        for p in feed.prefix(30) {
            guard let d = p.createdDate else { continue }
            entries.append(LogEntry(kind: .gen, date: d, text: "\(p.sku) \(p.priceLabel)"))
        }
        for s in transparency?.recentPurchases ?? [] {
            guard let d = s.date else { continue }
            let buyer = s.buyer ?? "?"
            let name = s.name ?? "?"
            let price = s.priceJpy.map(Fmt.yen) ?? ""
            entries.append(LogEntry(kind: .sale, date: d, text: "\(buyer) → \(name) \(price)"))
        }
        for u in updates.prefix(5) {
            guard let d = u.date else { continue }
            entries.append(LogEntry(kind: .note, date: d, text: u.text))
        }
        return entries.sorted { $0.date > $1.date }.prefix(36).map { $0 }
    }
}
