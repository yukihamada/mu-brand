import WidgetKit
import SwiftUI
import UIKit

// 最新ドロップを見せるホーム画面ウィジェット。1時間ごとに feed.json を取り直す。
// アプリ本体と同じ /api/shop/feed.json?physical=1 を直接叩く (App Group 不要)。

struct DropEntry: TimelineEntry {
    let date: Date
    let title: String
    let price: String
    let imageData: Data?
}

struct Provider: TimelineProvider {
    func placeholder(in context: Context) -> DropEntry {
        DropEntry(date: Date(), title: "MU", price: "", imageData: nil)
    }

    func getSnapshot(in context: Context, completion: @escaping (DropEntry) -> Void) {
        Task { completion(await Self.fetchLatest()) }
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<DropEntry>) -> Void) {
        Task {
            let entry = await Self.fetchLatest()
            let next = Date().addingTimeInterval(60 * 60) // 1時間後に更新
            completion(Timeline(entries: [entry], policy: .after(next)))
        }
    }

    // 最新の1着を取得。失敗時はタイトルのみのエントリ。
    static func fetchLatest() async -> DropEntry {
        guard let url = URL(string: "https://wearmu.com/api/shop/feed.json?physical=1&page=1") else {
            return DropEntry(date: Date(), title: "MU", price: "", imageData: nil)
        }
        do {
            let (data, _) = try await URLSession.shared.data(from: url)
            let page = try JSONDecoder().decode(FeedPageLite.self, from: data)
            guard let first = page.products.first else {
                return DropEntry(date: Date(), title: "MU", price: "", imageData: nil)
            }
            var img: Data?
            if let m = first.mockup_url, let iu = URL(string: m) {
                img = try? await URLSession.shared.data(from: iu).0
            }
            return DropEntry(date: Date(), title: first.description,
                             price: "¥\(first.price_jpy.formatted())", imageData: img)
        } catch {
            return DropEntry(date: Date(), title: "MU", price: "", imageData: nil)
        }
    }

    // ウィジェット側の最小デコード型 (本体 Models とは独立)。
    struct FeedPageLite: Decodable { let products: [P] }
    struct P: Decodable {
        let description: String
        let price_jpy: Int
        let mockup_url: String?
    }
}

struct MUWidgetEntryView: View {
    var entry: Provider.Entry

    var body: some View {
        ZStack {
            if let d = entry.imageData, let ui = UIImage(data: d) {
                Image(uiImage: ui).resizable().scaledToFill()
            } else {
                Color.black
            }
            LinearGradient(colors: [.clear, .black.opacity(0.75)],
                           startPoint: .center, endPoint: .bottom)
            VStack(alignment: .leading, spacing: 2) {
                Spacer()
                Text("MU · NEW DROP")
                    .font(.caption2.weight(.bold))
                    .foregroundStyle(Color(red: 0.90, green: 0.77, blue: 0.29))
                Text(entry.title)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.white)
                    .lineLimit(2)
                if !entry.price.isEmpty {
                    Text(entry.price)
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.white.opacity(0.9))
                }
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .containerBackground(.black, for: .widget)
    }
}

@main
struct MUWidget: Widget {
    let kind = "MUWidget"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: kind, provider: Provider()) { entry in
            MUWidgetEntryView(entry: entry)
        }
        .configurationDisplayName("MU — 最新ドロップ")
        .description("毎時生まれる新作を、ホーム画面で。")
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}
