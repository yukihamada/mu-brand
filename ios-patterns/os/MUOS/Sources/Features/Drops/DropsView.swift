import SwiftUI

// 🕐 Drops — AI が服を作った瞬間のタイムライン。
// feed.json (created_at 降順) をそのまま「HH:mm AIがこれを作った」の形で流す。
struct DropsView: View {
    @State private var products: [FeedProduct] = []
    @State private var page = 1
    @State private var loading = false
    @State private var reachedEnd = false
    @State private var error: String?

    var body: some View {
        NavigationStack {
            ZStack {
                Color.muBg.ignoresSafeArea()
                if products.isEmpty && loading {
                    ProgressView().tint(Color.muGold)
                } else if products.isEmpty, let error {
                    errorView(error)
                } else if products.isEmpty && reachedEnd {
                    Text(String(localized: "drops.empty"))
                        .font(Mono.font(12))
                        .foregroundStyle(Color.muMute)
                } else {
                    timeline
                }
            }
            .navigationTitle(String(localized: "tab.drops"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(Color.muBg, for: .navigationBar)
            .navigationDestination(for: FeedProduct.self) { DropDetailView(product: $0) }
        }
        .task { if products.isEmpty { await reload() } }
    }

    private func errorView(_ msg: String) -> some View {
        VStack(spacing: 12) {
            Text(msg)
                .font(Mono.font(11))
                .foregroundStyle(Color.muMute)
            Button(String(localized: "common.retry")) {
                Task { await reload() }
            }
            .font(Mono.font(12, .semibold))
            .foregroundStyle(Color.muGold)
        }
    }

    private var timeline: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0, pinnedViews: [.sectionHeaders]) {
                ForEach(grouped, id: \.day) { group in
                    Section {
                        ForEach(group.items) { p in
                            NavigationLink(value: p) { DropRow(product: p) }
                                .buttonStyle(.plain)
                                .onAppear {
                                    if p == products.last { Task { await loadMore() } }
                                }
                        }
                    } header: {
                        DayHeader(day: group.day)
                    }
                }
                if loading {
                    ProgressView()
                        .tint(Color.muGold)
                        .frame(maxWidth: .infinity)
                        .padding()
                }
            }
            .padding(.horizontal, 14)
        }
        .refreshable { await reload() }
    }

    private struct DayGroup {
        let day: String
        let items: [FeedProduct]
    }

    private var grouped: [DayGroup] {
        var order: [String] = []
        var map: [String: [FeedProduct]] = [:]
        for p in products {
            let day = p.createdDate.map { Fmt.dayJST.string(from: $0) } ?? "—"
            if map[day] == nil { order.append(day) }
            map[day, default: []].append(p)
        }
        return order.map { DayGroup(day: $0, items: map[$0] ?? []) }
    }

    private func reload() async {
        page = 1
        reachedEnd = false
        await fetch(replace: true)
    }

    private func loadMore() async {
        guard !loading, !reachedEnd else { return }
        page += 1
        await fetch(replace: false)
    }

    private func fetch(replace: Bool) async {
        loading = true
        defer { loading = false }
        do {
            let new = try await MUAPI.feed(page: page)
            if new.isEmpty { reachedEnd = true }
            products = replace ? new : products + new
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }
}

private struct DayHeader: View {
    let day: String

    var body: some View {
        HStack(spacing: 8) {
            Text(day)
                .font(Mono.font(10, .semibold))
                .tracking(2)
                .foregroundStyle(Color.muGold)
            Text("JST")
                .font(Mono.font(8))
                .foregroundStyle(Color.muMute)
            Rectangle().fill(Color.muLine).frame(height: 1)
        }
        .padding(.vertical, 10)
        .background(Color.muBg)
    }
}

private struct DropRow: View {
    let product: FeedProduct

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .trailing, spacing: 2) {
                Text(product.createdDate.map { Fmt.hhmmJST.string(from: $0) } ?? "--:--")
                    .font(Mono.font(13, .semibold))
                    .foregroundStyle(Color.muGold)
                    .monospacedDigit()
                if let d = product.createdDate {
                    Text(d, style: .relative)
                        .font(Mono.font(8))
                        .foregroundStyle(Color.muMute)
                        .lineLimit(1)
                }
            }
            .frame(width: 64, alignment: .trailing)

            AsyncImage(url: product.mockupURL) { phase in
                switch phase {
                case .success(let img):
                    img.resizable().scaledToFill()
                default:
                    Rectangle().fill(Color.muCard)
                        .overlay(
                            Image(systemName: "tshirt")
                                .font(.system(size: 18))
                                .foregroundStyle(Color.muMute)
                        )
                }
            }
            .frame(width: 62, height: 62)
            .clipShape(RoundedRectangle(cornerRadius: 4))
            .overlay(RoundedRectangle(cornerRadius: 4).stroke(Color.muLine, lineWidth: 1))

            VStack(alignment: .leading, spacing: 4) {
                Text(String(localized: "drops.madeThis"))
                    .font(Mono.font(8))
                    .tracking(1.2)
                    .foregroundStyle(Color.muMute)
                    .textCase(.uppercase)
                Text(product.description)
                    .font(.system(size: 13))
                    .foregroundStyle(Color.muFg)
                    .lineLimit(2)
                HStack(spacing: 8) {
                    Text(product.brand.uppercased())
                        .font(Mono.font(8, .semibold))
                        .foregroundStyle(Color.muMute)
                    Text(product.priceLabel)
                        .font(Mono.font(11, .semibold))
                        .foregroundStyle(Color.muGold)
                    if product.sold > 0 {
                        Text(String(format: String(localized: "feed.sold %lld"), product.sold))
                            .font(Mono.font(8))
                            .foregroundStyle(Color(red: 0.45, green: 0.85, blue: 0.55))
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, 9)
        .overlay(alignment: .bottom) {
            Rectangle().fill(Color.muLine).frame(height: 1)
        }
    }
}
