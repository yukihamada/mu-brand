import SwiftUI

// MU OS の計器盤トーン: 黒地 #0A0A0A・金 #e6c449・等幅数字。
// wearmu.com の CSS 変数 (--bg/--fg/--y/--line/--card) と同値。
extension Color {
    static let muBg = Color(red: 0.039, green: 0.039, blue: 0.039)    // #0A0A0A
    static let muCard = Color(red: 0.067, green: 0.067, blue: 0.067)  // #111111
    static let muGold = Color(red: 0.902, green: 0.769, blue: 0.286)  // #e6c449
    static let muFg = Color(red: 0.961, green: 0.961, blue: 0.941)    // #F5F5F0
    static let muLine = Color.white.opacity(0.08)
    static let muMute = Color(red: 0.961, green: 0.961, blue: 0.941).opacity(0.55)
}

enum Mono {
    static func font(_ size: CGFloat, _ weight: Font.Weight = .regular) -> Font {
        .system(size: size, weight: weight, design: .monospaced)
    }
}

// 共通フォーマッタ (JST 基準)
enum Fmt {
    static let jst = TimeZone(identifier: "Asia/Tokyo")!

    static var jstCalendar: Calendar {
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = jst
        return cal
    }

    // feed.json: SQLite UTC "yyyy-MM-dd HH:mm:ss"
    static let feedUTC: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm:ss"
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "UTC")
        return f
    }()

    // /api/transparency recent_purchases: "yyyy-MM-dd HH:mm JST"
    static let purchaseJST: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm 'JST'"
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = jst
        return f
    }()

    // /api/updates: "yyyy-MM-dd HH:mm:ss JST"
    static let updateJST: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm:ss 'JST'"
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = jst
        return f
    }()

    static let hhmmJST: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "HH:mm"
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = jst
        return f
    }()

    static let dayJST: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = jst
        return f
    }()

    static let stampJST: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm:ss"
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = jst
        return f
    }()

    static func yen(_ v: Int) -> String { "¥" + v.formatted() }
}

// 計器盤パネル (角は固め・ヘアライン枠)
struct Panel: ViewModifier {
    func body(content: Content) -> some View {
        content
            .background(Color.muCard)
            .overlay(RoundedRectangle(cornerRadius: 4).stroke(Color.muLine, lineWidth: 1))
            .clipShape(RoundedRectangle(cornerRadius: 4))
    }
}

extension View {
    func panel() -> some View { modifier(Panel()) }
}

// 点滅する LIVE インジケータ
struct LiveDot: View {
    var color: Color = .muGold
    @State private var on = false

    var body: some View {
        Circle()
            .fill(color)
            .frame(width: 7, height: 7)
            .opacity(on ? 1 : 0.2)
            .shadow(color: color.opacity(on ? 0.8 : 0), radius: 4)
            .onAppear {
                withAnimation(.easeInOut(duration: 0.9).repeatForever(autoreverses: true)) {
                    on = true
                }
            }
    }
}

// 数字のカウントアップ (numericText ロール)
struct CountUpText: View {
    let value: Int
    var prefix: String = ""
    var font: Font = Mono.font(40, .light)
    var color: Color = .muGold
    @State private var shown: Int = 0

    var body: some View {
        Text("\(prefix)\(shown.formatted())")
            .font(font)
            .foregroundStyle(color)
            .monospacedDigit()
            .contentTransition(.numericText(value: Double(shown)))
            .onAppear {
                withAnimation(.easeOut(duration: 1.4)) { shown = value }
            }
            .onChange(of: value) { _, new in
                withAnimation(.easeOut(duration: 0.8)) { shown = new }
            }
    }
}

// 最終生成からの経過 (1秒ごとに刻む — 偽カウントダウンではなく実経過時間)
struct ElapsedText: View {
    let since: Date
    var font: Font = Mono.font(24, .medium)
    var color: Color = .muFg

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { tl in
            Text(Self.format(tl.date.timeIntervalSince(since)))
                .font(font)
                .foregroundStyle(color)
                .monospacedDigit()
        }
    }

    static func format(_ ti: TimeInterval) -> String {
        let s = max(0, Int(ti))
        let h = s / 3600
        let m = (s % 3600) / 60
        let sec = s % 60
        if h > 0 { return String(format: "%d:%02d:%02d", h, m, sec) }
        return String(format: "%02d:%02d", m, sec)
    }
}

// 横に流れるティッカー (実 feed の新着のみ)
struct Marquee: View {
    let items: [String]
    @State private var contentWidth: CGFloat = 0

    var body: some View {
        TimelineView(.animation(minimumInterval: 1.0 / 30.0)) { tl in
            let t = tl.date.timeIntervalSinceReferenceDate
            let offset: CGFloat = contentWidth > 0
                ? -CGFloat((t * 28.0).truncatingRemainder(dividingBy: Double(contentWidth)))
                : 0
            Color.muCard
                .frame(height: 36)
                .overlay(alignment: .leading) {
                    HStack(spacing: 0) {
                        row.background(
                            GeometryReader { g in
                                Color.clear.onAppear { contentWidth = g.size.width }
                            }
                        )
                        row
                    }
                    .offset(x: offset)
                }
                .clipped()
        }
        .overlay(RoundedRectangle(cornerRadius: 4).stroke(Color.muLine, lineWidth: 1))
        .clipShape(RoundedRectangle(cornerRadius: 4))
    }

    private var row: some View {
        HStack(spacing: 16) {
            ForEach(Array(items.enumerated()), id: \.offset) { _, s in
                Text(s)
                    .font(Mono.font(11))
                    .foregroundStyle(Color.muGold)
                    .lineLimit(1)
                Text("◆")
                    .font(.system(size: 6))
                    .foregroundStyle(Color.muGold.opacity(0.35))
            }
        }
        .padding(.horizontal, 8)
        .fixedSize()
    }
}

// 計器セル
struct StatCell<Content: View>: View {
    let label: String
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 7) {
            Text(label)
                .font(Mono.font(9, .semibold))
                .tracking(1.6)
                .foregroundStyle(Color.muMute)
                .textCase(.uppercase)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
            content
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .panel()
    }
}

// セクション見出し (端末ログ風)
struct SectionHeader: View {
    let text: String

    var body: some View {
        HStack(spacing: 8) {
            Text(text)
                .font(Mono.font(10, .semibold))
                .tracking(2.4)
                .foregroundStyle(Color.muMute)
                .textCase(.uppercase)
            Rectangle().fill(Color.muLine).frame(height: 1)
        }
    }
}
