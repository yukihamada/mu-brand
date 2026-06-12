import SwiftUI

// 🔥 Live — 毎時生成のドロップフィード。新着順 + 次の一着カウントダウン。
struct LiveView: View {
    @State private var products: [FeedProduct] = []
    @State private var page = 1
    @State private var kind: ProductKind = .all
    @State private var loading = false
    @State private var reachedEnd = false
    @State private var error: String?

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(spacing: 16) {
                    LatestDropBar(products: products)
                    KindChips(selected: $kind)
                    ForEach(products) { p in
                        NavigationLink(value: p) { DropCard(product: p) }
                            .buttonStyle(.plain)
                            .onAppear { if p == products.last { Task { await loadMore() } } }
                    }
                    if loading { ProgressView().padding() }
                    if let error { Text(error).font(.footnote).foregroundStyle(.secondary) }
                }
                .padding(.horizontal)
            }
            .navigationTitle(String(localized: "tab.live"))
            .navigationDestination(for: FeedProduct.self) { ProductDetailView(product: $0) }
            .refreshable { await reload() }
            .task { if products.isEmpty { await reload() } }
            .onChange(of: kind) { Task { await reload() } }
        }
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
            let new = try await MUAPI.feed(page: page, kind: kind)
            if new.isEmpty { reachedEnd = true }
            products = replace ? new : products + new
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }
}

// 実データの鼓動: 「最新ドロップ n分前 ・ 24時間で n着」。
// 生成はバッチで毎時00分に揃わないため、予告カウントダウンは出さない (正直さ優先)。
struct LatestDropBar: View {
    let products: [FeedProduct]

    var body: some View {
        let latest = products.compactMap(\.createdDate).max()
        let dayAgo = Date().addingTimeInterval(-24 * 3600)
        let count24h = products.compactMap(\.createdDate).filter { $0 > dayAgo }.count
        HStack(spacing: 8) {
            Image(systemName: "flame.fill")
            if let latest {
                Text(String(localized: "live.latestDrop")) + Text(" ") + Text(latest, style: .relative)
                if count24h > 0 {
                    Text("·").foregroundStyle(.tertiary)
                    Text(String(format: String(localized: "live.born24h %lld"), count24h))
                }
            } else {
                Text(String(localized: "live.latestDrop"))
            }
        }
        .font(.subheadline)
        .padding(.vertical, 10)
        .frame(maxWidth: .infinity)
        .background(.quaternary, in: Capsule())
    }
}

struct KindChips: View {
    @Binding var selected: ProductKind

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(ProductKind.allCases) { k in
                    Button {
                        selected = k
                    } label: {
                        Text(k.label)
                            .font(.footnote.weight(.medium))
                            .padding(.horizontal, 14)
                            .padding(.vertical, 7)
                            .background(selected == k ? AnyShapeStyle(.tint) : AnyShapeStyle(.quaternary), in: Capsule())
                            .foregroundStyle(selected == k ? .black : .primary)
                    }
                }
            }
        }
    }
}

struct DropCard: View {
    let product: FeedProduct

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            AsyncImage(url: product.mockupURL) { phase in
                switch phase {
                case .success(let img): img.resizable().scaledToFill()
                default: Rectangle().fill(.quaternary).overlay(Image(systemName: "tshirt").font(.largeTitle).foregroundStyle(.tertiary))
                }
            }
            .frame(maxWidth: .infinity)
            .frame(height: 320)
            .clipShape(RoundedRectangle(cornerRadius: 14))
            HStack(alignment: .firstTextBaseline) {
                Text(product.description)
                    .font(.subheadline)
                    .lineLimit(2)
                Spacer()
                Text(product.priceLabel)
                    .font(.headline)
            }
            HStack(spacing: 6) {
                Text(product.brand.uppercased())
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)
                if product.sold > 0 {
                    Text(String(format: String(localized: "feed.sold %lld"), product.sold))
                        .font(.caption2)
                        .foregroundStyle(.tint)
                }
                Spacer()
                if let d = product.createdDate {
                    Text(d, style: .relative)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }
        }
        .padding(.bottom, 4)
    }
}
