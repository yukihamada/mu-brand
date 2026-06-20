import SwiftUI

// 🛍 Shop — 検索 + kind 絞り込みのグリッド。データ源は Live と同じ feed API。
struct ShopView: View {
    @EnvironmentObject private var app: AppState
    @State private var products: [FeedProduct] = []
    @State private var page = 1
    @State private var kind: ProductKind = .all
    @State private var query = ""
    @State private var loading = false
    @State private var reachedEnd = false
    @State private var error: String?
    @State private var loadedOnce = false

    private let columns = [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)]

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(spacing: 12) {
                    KindChips(selected: $kind)
                    LazyVGrid(columns: columns, spacing: 16) {
                        ForEach(products) { p in
                            NavigationLink(value: p) { GridCard(product: p) }
                                .buttonStyle(.plain)
                                .onAppear { if p == products.last { Task { await loadMore() } } }
                        }
                    }
                    if loading { ProgressView().padding() }
                    // 空状態 / エラー (初回ロード後・0件のときだけ出す)
                    if !loading && loadedOnce && products.isEmpty {
                        VStack(spacing: 18) {
                            ContentUnavailableView(
                                error == nil ? String(localized: "shop.empty") : String(localized: "shop.error"),
                                systemImage: error == nil ? "magnifyingglass" : "wifi.exclamationmark"
                            )
                            // MISSION核: 検索0件 → MUでは「作りますか?」。検索語があるときだけ、
                            // その語から即デザイン生成へ（startMake が Make タブで自動生成）。
                            if error == nil {
                                let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
                                if !q.isEmpty {
                                    let isJa = Locale.current.language.languageCode?.identifier == "ja"
                                    Button {
                                        Analytics.track("shop_empty_make")
                                        app.startMake(q)
                                    } label: {
                                        Label(isJa ? "「\(q)」を作る" : "Make “\(q)”",
                                              systemImage: "sparkles")
                                            .font(.headline)
                                            .frame(maxWidth: .infinity)
                                            .padding(.vertical, 14)
                                    }
                                    .buttonStyle(.borderedProminent)
                                    .tint(.yellow)
                                    .foregroundStyle(.black)
                                    .padding(.horizontal, 24)
                                }
                            }
                        }
                        .padding(.top, 60)
                    }
                }
                .padding(.horizontal)
            }
            .navigationTitle(String(localized: "tab.shop"))
            .navigationDestination(for: FeedProduct.self) { ProductDetailView(product: $0) }
            .searchable(text: $query, prompt: String(localized: "shop.search"))
            .onSubmit(of: .search) { Task { await reload() } }
            .refreshable { await reload() }
            .task {
                if products.isEmpty { await reload() }
                Analytics.track("view_shop")
            }
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
        defer { loading = false; loadedOnce = true }
        do {
            let new = try await MUAPI.feed(page: page, kind: kind, query: query)
            if new.isEmpty { reachedEnd = true }
            products = replace ? new : products + new
            error = nil
        } catch {
            self.error = error.localizedDescription
            if replace { products = [] }
        }
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
