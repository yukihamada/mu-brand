import SwiftUI

// ⚡️ Pulse — ブランドの心拍。
// Bloomberg 端末 × 腕時計のコンプリケーション。全数字 = 公開 API の実数。
struct PulseView: View {
    @StateObject private var model = PulseModel()

    var body: some View {
        NavigationStack {
            ZStack {
                Color.muBg.ignoresSafeArea()
                switch model.phase {
                case .loading:
                    loadingView
                case .failed(let msg):
                    errorView(msg)
                case .ready:
                    content
                }
            }
            .toolbar {
                ToolbarItem(placement: .principal) {
                    HStack(spacing: 8) {
                        Text("MU OS")
                            .font(Mono.font(13, .bold))
                            .tracking(5)
                            .foregroundStyle(Color.muFg)
                        LiveDot()
                    }
                }
            }
            .toolbarBackground(Color.muBg, for: .navigationBar)
        }
        .task { await model.loadIfNeeded() }
    }

    // MARK: - states

    private var loadingView: some View {
        VStack(spacing: 14) {
            ProgressView().tint(Color.muGold)
            Text(String(localized: "pulse.loading"))
                .font(Mono.font(11))
                .foregroundStyle(Color.muMute)
        }
    }

    private func errorView(_ msg: String) -> some View {
        VStack(spacing: 14) {
            Text(String(localized: "common.error"))
                .font(Mono.font(12, .semibold))
                .foregroundStyle(Color.muMute)
            Text(msg)
                .font(Mono.font(11))
                .foregroundStyle(Color.muMute)
                .multilineTextAlignment(.center)
            Button(String(localized: "common.retry")) {
                Task { await model.load() }
            }
            .font(Mono.font(12, .semibold))
            .foregroundStyle(Color.muGold)
            .padding(.horizontal, 22)
            .padding(.vertical, 9)
            .overlay(RoundedRectangle(cornerRadius: 4).stroke(Color.muGold.opacity(0.6), lineWidth: 1))
        }
        .padding(.horizontal, 32)
    }

    // MARK: - content

    private var content: some View {
        ScrollView {
            VStack(spacing: 14) {
                statusStrip
                heroRevenue
                Marquee(items: model.tickerItems)
                statGrid
                systemLog
                honestyNote
            }
            .padding(.horizontal, 14)
            .padding(.top, 6)
            .padding(.bottom, 28)
        }
        .refreshable { await model.load() }
    }

    private var statusStrip: some View {
        HStack(spacing: 8) {
            LiveDot(color: Color(red: 0.45, green: 0.85, blue: 0.55))
            Text(String(localized: "pulse.live"))
                .font(Mono.font(9, .bold))
                .tracking(2.4)
                .foregroundStyle(Color(red: 0.45, green: 0.85, blue: 0.55))
            Text("WEARMU.COM")
                .font(Mono.font(9))
                .tracking(1.8)
                .foregroundStyle(Color.muMute)
            Spacer()
            if let asOf = model.transparency?.asOfDate {
                Text("\(String(localized: "pulse.asOf")) \(Fmt.stampJST.string(from: asOf)) JST")
                    .font(Mono.font(9))
                    .foregroundStyle(Color.muMute)
            }
        }
        .padding(.horizontal, 2)
    }

    private var heroRevenue: some View {
        VStack(spacing: 6) {
            Text(String(localized: "pulse.totalRevenue"))
                .font(Mono.font(10, .semibold))
                .tracking(3)
                .foregroundStyle(Color.muMute)
                .textCase(.uppercase)
            if let total = model.transparency?.revenueTotalJpy {
                CountUpText(value: total, prefix: "¥", font: Mono.font(52, .light))
            } else {
                Text("—").font(Mono.font(52, .light)).foregroundStyle(Color.muMute)
            }
            Text(String(localized: "pulse.revenueNote"))
                .font(Mono.font(9))
                .foregroundStyle(Color.muMute)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 22)
        .panel()
    }

    private var statGrid: some View {
        let cols = [GridItem(.flexible(), spacing: 10), GridItem(.flexible(), spacing: 10)]
        return LazyVGrid(columns: cols, spacing: 10) {
            StatCell(label: String(localized: "pulse.stat.sku")) {
                CountUpText(value: model.totalSKU, font: Mono.font(28, .medium), color: .muFg)
            }
            StatCell(label: String(localized: "pulse.stat.bornToday")) {
                Text(model.bornTodayLabel)
                    .font(Mono.font(28, .medium))
                    .foregroundStyle(Color.muGold)
                    .monospacedDigit()
            }
            StatCell(label: String(localized: "pulse.stat.sinceLastGen")) {
                if let last = model.latestDropDate {
                    ElapsedText(since: last, font: Mono.font(28, .medium), color: .muFg)
                } else {
                    Text("—").font(Mono.font(28, .medium)).foregroundStyle(Color.muMute)
                }
            }
            StatCell(label: String(localized: "pulse.stat.buyers")) {
                CountUpText(
                    value: model.transparency?.external?.distinctCustomers ?? 0,
                    font: Mono.font(28, .medium),
                    color: .muFg
                )
            }
            StatCell(label: String(localized: "pulse.stat.purchases7d")) {
                CountUpText(
                    value: model.transparency?.external?.purchases7d ?? 0,
                    font: Mono.font(28, .medium),
                    color: .muGold
                )
            }
            StatCell(label: String(localized: "pulse.stat.brands")) {
                CountUpText(value: model.brandCount, font: Mono.font(28, .medium), color: .muFg)
            }
        }
    }

    private var systemLog: some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHeader(text: String(localized: "pulse.systemLog"))
            VStack(alignment: .leading, spacing: 7) {
                ForEach(Array(model.logEntries.enumerated()), id: \.offset) { _, e in
                    HStack(alignment: .top, spacing: 8) {
                        Text(e.tag)
                            .font(Mono.font(9, .bold))
                            .foregroundStyle(e.color)
                            .frame(width: 32, alignment: .leading)
                        Text(Fmt.hhmmJST.string(from: e.date))
                            .font(Mono.font(9))
                            .foregroundStyle(Color.muMute)
                        Text(e.text)
                            .font(Mono.font(9))
                            .foregroundStyle(Color.muFg.opacity(0.85))
                            .lineLimit(2)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .panel()
        }
    }

    private var honestyNote: some View {
        Text(String(localized: "pulse.honesty"))
            .font(Mono.font(9))
            .foregroundStyle(Color.muMute)
            .multilineTextAlignment(.leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 2)
    }
}
