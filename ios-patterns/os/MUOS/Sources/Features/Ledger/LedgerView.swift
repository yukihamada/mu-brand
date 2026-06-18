import SwiftUI

// 📒 Ledger — /api/transparency (毎リクエスト再計算の公開実数 JSON) をネイティブで。
// 返金・欠番までそのまま見せる radical honesty の台帳。
struct LedgerView: View {
    @State private var t: Transparency?
    @State private var loading = false
    @State private var error: String?
    @State private var showWeb = false

    var body: some View {
        NavigationStack {
            ZStack {
                Color.muBg.ignoresSafeArea()
                if t == nil && loading {
                    ProgressView().tint(Color.muGold)
                } else if t == nil, let error {
                    errorView(error)
                } else if let t {
                    content(t)
                }
            }
            .navigationTitle(String(localized: "tab.ledger"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(Color.muBg, for: .navigationBar)
        }
        .task { if t == nil { await load() } }
        .sheet(isPresented: $showWeb) {
            SafariView(url: URL(string: "https://wearmu.com/transparency")!).ignoresSafeArea()
        }
    }

    private func load() async {
        loading = true
        defer { loading = false }
        do {
            t = try await MUAPI.transparency()
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func errorView(_ msg: String) -> some View {
        VStack(spacing: 12) {
            Text(msg)
                .font(Mono.font(11))
                .foregroundStyle(Color.muMute)
            Button(String(localized: "common.retry")) {
                Task { await load() }
            }
            .font(Mono.font(12, .semibold))
            .foregroundStyle(Color.muGold)
        }
    }

    // MARK: - content

    private func content(_ t: Transparency) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                lede(t)
                hero(t)
                realExternal(t)
                if let rows = t.catalog?.byBrand, !rows.isEmpty {
                    byBrand(rows)
                }
                honesty(t)
                if let recent = t.recentPurchases, !recent.isEmpty {
                    recentPurchases(recent)
                }
                pledgeAndSplit(t)
                openWebButton
            }
            .padding(.horizontal, 14)
            .padding(.top, 6)
            .padding(.bottom, 28)
        }
        .refreshable { await load() }
    }

    private func lede(_ t: Transparency) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(String(localized: "ledger.lede"))
                .font(Mono.font(9))
                .foregroundStyle(Color.muMute)
            if let asOf = t.asOfDate {
                Text("\(String(localized: "pulse.asOf")) \(Fmt.stampJST.string(from: asOf)) JST")
                    .font(Mono.font(9))
                    .foregroundStyle(Color.muMute)
            }
        }
    }

    private func hero(_ t: Transparency) -> some View {
        VStack(spacing: 6) {
            Text(String(localized: "pulse.totalRevenue"))
                .font(Mono.font(10, .semibold))
                .tracking(3)
                .foregroundStyle(Color.muMute)
                .textCase(.uppercase)
            if let total = t.revenueTotalJpy {
                CountUpText(value: total, prefix: "¥", font: Mono.font(44, .light))
            }
            if let b = t.revenueBreakdown {
                HStack(spacing: 14) {
                    miniStat(String(localized: "ledger.breakdown.auctions"), b.auctionsJpy)
                    miniStat(String(localized: "ledger.breakdown.shirts"), b.shirtsJpy)
                    miniStat(String(localized: "ledger.breakdown.youtee"), b.youTeeJpy)
                }
                .padding(.top, 6)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 20)
        .panel()
    }

    private func miniStat(_ label: String, _ value: Int?) -> some View {
        VStack(spacing: 2) {
            Text(label)
                .font(Mono.font(8))
                .tracking(1)
                .foregroundStyle(Color.muMute)
                .textCase(.uppercase)
            Text(value.map(Fmt.yen) ?? "—")
                .font(Mono.font(11, .semibold))
                .foregroundStyle(Color.muFg)
                .monospacedDigit()
        }
    }

    private func realExternal(_ t: Transparency) -> some View {
        let cols = [GridItem(.flexible(), spacing: 10), GridItem(.flexible(), spacing: 10)]
        return LazyVGrid(columns: cols, spacing: 10) {
            StatCell(label: String(localized: "ledger.real")) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(t.real?.revenueJpy.map(Fmt.yen) ?? "—")
                        .font(Mono.font(20, .medium))
                        .foregroundStyle(Color.muFg)
                        .monospacedDigit()
                    Text(String(format: String(localized: "ledger.purchases %lld"), t.real?.purchases ?? 0))
                        .font(Mono.font(9))
                        .foregroundStyle(Color.muMute)
                }
            }
            StatCell(label: String(localized: "ledger.external")) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(t.external?.revenueJpy.map(Fmt.yen) ?? "—")
                        .font(Mono.font(20, .medium))
                        .foregroundStyle(Color.muGold)
                        .monospacedDigit()
                    Text(String(format: String(localized: "ledger.externalSub %lld %lld"),
                                t.external?.purchases ?? 0,
                                t.external?.distinctCustomers ?? 0))
                        .font(Mono.font(9))
                        .foregroundStyle(Color.muMute)
                }
            }
        }
    }

    private func byBrand(_ rows: [Transparency.BrandRow]) -> some View {
        let maxRev = max(rows.map(\.revenueJpy).max() ?? 1, 1)
        return VStack(alignment: .leading, spacing: 10) {
            SectionHeader(text: String(localized: "ledger.byBrand"))
            VStack(spacing: 9) {
                ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                    VStack(alignment: .leading, spacing: 3) {
                        HStack {
                            Text(row.brand.uppercased())
                                .font(Mono.font(9, .semibold))
                                .foregroundStyle(Color.muFg)
                            Text(String(format: String(localized: "ledger.orders %lld"), row.orders))
                                .font(Mono.font(8))
                                .foregroundStyle(Color.muMute)
                            Spacer()
                            Text(Fmt.yen(row.revenueJpy))
                                .font(Mono.font(10, .semibold))
                                .foregroundStyle(Color.muGold)
                                .monospacedDigit()
                        }
                        GeometryReader { g in
                            ZStack(alignment: .leading) {
                                Rectangle().fill(Color.muLine)
                                Rectangle()
                                    .fill(Color.muGold.opacity(0.85))
                                    .frame(width: g.size.width * CGFloat(row.revenueJpy) / CGFloat(maxRev))
                            }
                        }
                        .frame(height: 3)
                    }
                }
            }
            .padding(12)
            .panel()
        }
    }

    // 返金・欠番 — 隠さないコーナー
    private func honesty(_ t: Transparency) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHeader(text: String(localized: "ledger.honesty"))
            VStack(spacing: 0) {
                if let r = t.catalog?.refundedExcluded {
                    honestyRow(
                        String(localized: "ledger.refunded"),
                        String(format: String(localized: "ledger.refunded.detail %lld %@"),
                               r.orders ?? 0, Fmt.yen(r.amountJpy ?? 0))
                    )
                }
                if let m = t.missingDrops?.mugenMissingDrops {
                    honestyRow(String(localized: "ledger.missing.mugen"), "\(m.count)")
                }
                if let m = t.missingDrops?.muonMissingDates {
                    honestyRow(String(localized: "ledger.missing.muon"), "\(m.count)", last: true)
                }
            }
            .panel()
        }
    }

    private func honestyRow(_ label: String, _ value: String, last: Bool = false) -> some View {
        HStack {
            Text(label)
                .font(Mono.font(10))
                .foregroundStyle(Color.muFg.opacity(0.85))
            Spacer()
            Text(value)
                .font(Mono.font(11, .semibold))
                .foregroundStyle(Color(red: 0.95, green: 0.45, blue: 0.40))
                .monospacedDigit()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .overlay(alignment: .bottom) {
            if !last { Rectangle().fill(Color.muLine).frame(height: 1) }
        }
    }

    private func recentPurchases(_ recent: [Transparency.Purchase]) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHeader(text: String(localized: "ledger.recent"))
            VStack(spacing: 0) {
                ForEach(Array(recent.enumerated()), id: \.offset) { i, p in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(p.atJst ?? "—")
                            .font(Mono.font(8))
                            .foregroundStyle(Color.muMute)
                            .frame(width: 104, alignment: .leading)
                        VStack(alignment: .leading, spacing: 1) {
                            Text(p.name ?? "—")
                                .font(Mono.font(10))
                                .foregroundStyle(Color.muFg)
                                .lineLimit(1)
                            Text(p.buyer ?? "—")
                                .font(Mono.font(8))
                                .foregroundStyle(Color.muMute)
                        }
                        Spacer()
                        Text(p.priceJpy.map(Fmt.yen) ?? "—")
                            .font(Mono.font(10, .semibold))
                            .foregroundStyle(Color.muGold)
                            .monospacedDigit()
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .overlay(alignment: .bottom) {
                        if i < recent.count - 1 {
                            Rectangle().fill(Color.muLine).frame(height: 1)
                        }
                    }
                }
            }
            .panel()
        }
    }

    private func pledgeAndSplit(_ t: Transparency) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHeader(text: String(localized: "ledger.split"))
            VStack(spacing: 0) {
                if let pledge = t.teshikagaPledge {
                    HStack {
                        Text(String(localized: "ledger.pledge"))
                            .font(Mono.font(10))
                            .foregroundStyle(Color.muFg.opacity(0.85))
                        Spacer()
                        Text(pledge.estimatedPledgeJpy.map(Fmt.yen) ?? "—")
                            .font(Mono.font(11, .semibold))
                            .foregroundStyle(Color.muGold)
                            .monospacedDigit()
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 10)
                    .overlay(alignment: .bottom) {
                        Rectangle().fill(Color.muLine).frame(height: 1)
                    }
                }
                if let segments = t.profitSplit?.breakdown?.segments {
                    ForEach(Array(segments.enumerated()), id: \.offset) { i, s in
                        HStack(alignment: .firstTextBaseline) {
                            Text(s.key.uppercased())
                                .font(Mono.font(9, .semibold))
                                .foregroundStyle(Color.muMute)
                                .frame(width: 80, alignment: .leading)
                            Text(s.ratio.map { "\(Int($0 * 100))%" } ?? "—")
                                .font(Mono.font(9))
                                .foregroundStyle(Color.muMute)
                            Spacer()
                            Text(s.jpy.map(Fmt.yen) ?? "—")
                                .font(Mono.font(10))
                                .foregroundStyle(Color.muFg)
                                .monospacedDigit()
                        }
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .overlay(alignment: .bottom) {
                            if i < segments.count - 1 {
                                Rectangle().fill(Color.muLine).frame(height: 1)
                            }
                        }
                    }
                }
            }
            .panel()
            Text(String(localized: "ledger.note"))
                .font(Mono.font(9))
                .foregroundStyle(Color.muMute)
        }
    }

    private var openWebButton: some View {
        Button {
            showWeb = true
        } label: {
            Label(String(localized: "ledger.openWeb"), systemImage: "safari")
                .font(Mono.font(11))
                .foregroundStyle(Color.muGold)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 12)
                .overlay(RoundedRectangle(cornerRadius: 4).stroke(Color.muGold.opacity(0.5), lineWidth: 1))
        }
    }
}
