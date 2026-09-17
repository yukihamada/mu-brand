import SwiftUI

// 🛍 Shop — 検索 + kind 絞り込みのグリッド。データ源は Live と同じ feed API。
struct ShopView: View {
    @StateObject private var feed = ProductFeed()
    @State private var kind: ProductKind = .all
    @State private var query = ""

    private let columns = [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)]

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(spacing: 12) {
                    KindChips(selected: $kind)
                    LazyVGrid(columns: columns, spacing: 16) {
                        ForEach(feed.products) { p in
                            NavigationLink(value: p) { GridCard(product: p) }
                                .buttonStyle(.plain)
                                .accessibilityIdentifier("shop.product.\(p.sku)")
                                .onAppear {
                                    if p == feed.products.last, feed.error == nil {
                                        Task { await feed.loadMore() }
                                    }
                                }
                        }
                    }
                    if feed.loading { ProgressView().padding() }
                    if feed.error != nil {
                        Button(String(localized: "feed.retry")) { Task { await feed.loadMore() } }
                            .accessibilityIdentifier("feed.retry")
                    }
                    // 空状態 / エラー (初回ロード後・0件のときだけ出す)
                    if !feed.loading && feed.loadedOnce && feed.products.isEmpty {
                        ContentUnavailableView(
                            feed.error == nil ? String(localized: "shop.empty") : String(localized: "shop.error"),
                            systemImage: feed.error == nil ? "magnifyingglass" : "wifi.exclamationmark"
                        )
                        .padding(.top, 60)
                    }
                }
                .padding(.horizontal)
            }
            .navigationTitle(String(localized: "tab.shop"))
            .navigationDestination(for: FeedProduct.self) { ProductDetailView(product: $0) }
            .searchable(text: $query, prompt: String(localized: "shop.search"))
            .refreshable { await reload() }
            .task(id: query) {
                if !query.isEmpty {
                    do { try await Task.sleep(for: .milliseconds(300)) } catch { return }
                }
                await reload()
            }
            .task { Analytics.track("view_shop") }
            .onChange(of: kind) { Task { await reload() } }
        }
    }

    private func reload() async {
        await feed.reload(kind: kind, query: query)
    }
}

struct GridCard: View {
    let product: FeedProduct

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            AsyncImage(url: product.mockupURL) { phase in
                switch phase {
                case .success(let img): img.resizable().scaledToFill()
                default: Rectangle().fill(.quaternary)
                }
            }
            .frame(height: 180)
            .frame(maxWidth: .infinity)
            .clipShape(RoundedRectangle(cornerRadius: 10))
            Text(product.description)
                .font(.caption)
                .lineLimit(1)
            Text(product.priceLabel)
                .font(.footnote.weight(.semibold))
        }
    }
}
