import SwiftUI

// 🖼 Gallery — 「他の人が作ったもの」の棚。データ源は実 API /api/shop/feed.json。
// 主役は Make なので、ここは眺めて刺激をもらう脇役。マソナリー2カラム。
struct GalleryView: View {
    @State private var products: [FeedProduct] = []
    @State private var page = 1
    @State private var loading = false
    @State private var failed = false
    @State private var reachedEnd = false
    @State private var detail: FeedProduct?

    var body: some View {
        NavigationStack {
            ZStack {
                MUTheme.bg.ignoresSafeArea()
                content
            }
            .navigationTitle(String(localized: "tab.gallery"))
            .sheet(item: $detail) { p in
                GalleryDetailSheet(product: p)
                    .presentationDetents([.large])
            }
        }
    }

    @ViewBuilder
    private var content: some View {
        if loading && products.isEmpty {
            StateBanner(kind: .loading)
        } else if failed && products.isEmpty {
            StateBanner(kind: .error(String(localized: "common.error"), retry: {
                Task { await reload() }
            }))
        } else if products.isEmpty {
            StateBanner(kind: .empty(String(localized: "gallery.empty")))
                .task { await reload() }
        } else {
            ScrollView {
                MasonryGrid(products: products, onTap: { p in
                    Haptics.tap()
                    detail = p
                }, onReachEnd: {
                    Task { await loadMore() }
                })
                .padding(.horizontal, 16)
                if loading { ProgressView().tint(MUTheme.gold).padding() }
            }
            .refreshable { await reload() }
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
            let new = try await MUAPI.feed(page: page)
            if new.isEmpty { reachedEnd = true }
            products = replace ? new : products + new
            failed = false
        } catch {
            failed = true
        }
    }
}

// 2カラムのマソナリー: 高さは SKU ハッシュから決めて毎回同じ「ゆらぎ」を出す。
struct MasonryGrid: View {
    let products: [FeedProduct]
    let onTap: (FeedProduct) -> Void
    let onReachEnd: () -> Void

    private var columns: ([FeedProduct], [FeedProduct]) {
        var left: [FeedProduct] = []
        var right: [FeedProduct] = []
        var lh: CGFloat = 0
        var rh: CGFloat = 0
        for p in products {
            let h = Self.height(for: p)
            if lh <= rh { left.append(p); lh += h } else { right.append(p); rh += h }
        }
        return (left, right)
    }

    static func height(for product: FeedProduct) -> CGFloat {
        let hash = product.sku.unicodeScalars.reduce(0) { ($0 &* 31 &+ Int($1.value)) & 0xFFFF }
        return 170 + CGFloat(hash % 90) // 170–259pt
    }

    var body: some View {
        let (left, right) = columns
        HStack(alignment: .top, spacing: 12) {
            column(left)
            column(right)
        }
    }

    private func column(_ items: [FeedProduct]) -> some View {
        LazyVStack(spacing: 12) {
            ForEach(items) { p in
                MasonryCard(product: p, height: Self.height(for: p))
                    .onTapGesture { onTap(p) }
                    .onAppear { if p == products.last { onReachEnd() } }
            }
        }
    }
}

struct MasonryCard: View {
    let product: FeedProduct
    let height: CGFloat

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            MUAsyncImage(url: product.mockupURL)
                .frame(height: height)
                .frame(maxWidth: .infinity)
                .clipShape(RoundedRectangle(cornerRadius: 14))
            Text(product.description)
                .font(.caption)
                .lineLimit(2)
                .foregroundStyle(.primary)
            HStack {
                Text(product.priceLabel)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(MUTheme.gold)
                Spacer()
                if product.sold > 0 {
                    Text(String(format: String(localized: "feed.sold %lld"), product.sold))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }
}

// PDP 簡易シート: 大きい画像 + 購入 (Stripe Checkout) + Web で見る。
struct GalleryDetailSheet: View {
    let product: FeedProduct
    @State private var safariURL: URL?

    var body: some View {
        NavigationStack {
            ZStack {
                MUTheme.bg.ignoresSafeArea()
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        MUAsyncImage(url: product.mockupURL, contentMode: .fit)
                            .frame(maxWidth: .infinity)
                            .frame(minHeight: 300)
                            .background(MUTheme.card)
                            .clipShape(RoundedRectangle(cornerRadius: 18))
                        Text(product.brand.uppercased())
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)
                        Text(product.description)
                            .font(.body)
                        HStack {
                            Text(product.priceLabel).font(.title2.bold())
                            Spacer()
                            if product.sold > 0 {
                                Text(String(format: String(localized: "feed.sold %lld"), product.sold))
                                    .font(.footnote)
                                    .foregroundStyle(MUTheme.gold)
                            }
                        }
                        Button {
                            Haptics.rigid()
                            safariURL = URL(string: product.checkoutUrl)
                        } label: {
                            Text(String(localized: "result.buy"))
                                .font(.headline)
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 15)
                                .background(MUTheme.goldGradient, in: Capsule())
                                .foregroundStyle(.black)
                        }
                        Button {
                            Haptics.tap()
                            safariURL = URL(string: product.pdpUrl)
                        } label: {
                            Label(String(localized: "result.openPDP"), systemImage: "safari")
                                .font(.subheadline)
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 12)
                        }
                        .foregroundStyle(MUTheme.gold)
                    }
                    .padding(20)
                }
            }
            .navigationTitle(product.sku)
            .navigationBarTitleDisplayMode(.inline)
        }
        .sheet(item: $safariURL) { url in
            SafariView(url: url).ignoresSafeArea()
        }
    }
}
