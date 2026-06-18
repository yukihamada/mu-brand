import SwiftUI

// 🔥 Feed — TikTok 型・縦スワイプ全画面フィード。1スワイプ=1商品。
// データ源は実在の GET /api/shop/feed.json (page= で追い読み・kind= で絞り込み)。
@MainActor
final class FeedViewModel: ObservableObject {
    enum LoadState {
        case idle, loading, loaded, empty
        case error(String)
    }

    @Published private(set) var products: [FeedProduct] = []
    @Published private(set) var state: LoadState = .idle
    @Published var kind: ProductKind = .all

    private var page = 1
    private var reachedEnd = false
    private var loadingMore = false

    func reload() async {
        page = 1
        reachedEnd = false
        state = .loading
        do {
            let new = try await MUAPI.feed(page: 1, kind: kind)
            products = new
            state = new.isEmpty ? .empty : .loaded
            prefetch(from: 0)
        } catch {
            products = []
            state = .error(error.localizedDescription)
        }
    }

    // 現在表示中の商品から残り3枚を切ったら次ページを読む + 直近4枚をプリフェッチ
    func pageDidChange(to sku: String?) {
        guard let sku, let index = products.firstIndex(where: { $0.sku == sku }) else { return }
        prefetch(from: index + 1)
        if index >= products.count - 3 {
            Task { await loadMore() }
        }
    }

    private func loadMore() async {
        guard !loadingMore, !reachedEnd, case .loaded = state else { return }
        loadingMore = true
        defer { loadingMore = false }
        do {
            let new = try await MUAPI.feed(page: page + 1, kind: kind)
            if new.isEmpty {
                reachedEnd = true
            } else {
                page += 1
                let known = Set(products.map(\.sku))
                products += new.filter { !known.contains($0.sku) }
            }
        } catch {
            // 追い読み失敗は静かに無視 (次のスワイプで再試行される)
        }
    }

    private func prefetch(from index: Int) {
        guard index < products.count else { return }
        ImagePrefetcher.shared.prefetch(products[index...].prefix(4).compactMap(\.mockupURL))
    }
}

struct FeedView: View {
    @StateObject private var vm = FeedViewModel()
    @State private var currentSKU: String?
    @State private var detailProduct: FeedProduct?

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()

            switch vm.state {
            case .idle, .loading:
                loadingView
            case .error(let message):
                errorView(message)
            case .empty:
                emptyView
            case .loaded:
                pager
            }
        }
        .overlay(alignment: .top) { channelBar }
        .sheet(item: $detailProduct) { product in
            DetailSheetView(product: product)
        }
        .task {
            if vm.products.isEmpty {
                await vm.reload()
                currentSKU = vm.products.first?.sku
                if LaunchArgs.autoDetail, let first = vm.products.first {
                    try? await Task.sleep(for: .seconds(1.5)) // 画像が出てから
                    detailProduct = first
                }
            }
        }
    }

    // MARK: - pager

    private var pager: some View {
        ScrollView(.vertical, showsIndicators: false) {
            LazyVStack(spacing: 0) {
                ForEach(vm.products) { product in
                    ProductPageView(product: product) {
                        Haptics.medium()
                        detailProduct = product
                    }
                    .containerRelativeFrame([.horizontal, .vertical])
                    .id(product.sku)
                }
            }
            .scrollTargetLayout()
        }
        .scrollTargetBehavior(.paging)
        .scrollPosition(id: $currentSKU)
        .ignoresSafeArea()
        .onChange(of: currentSKU) { _, new in
            Haptics.selection()
            vm.pageDidChange(to: new)
        }
    }

    // MARK: - channel bar (kind チャンネル切替)

    private var channelBar: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(ProductKind.allCases) { k in
                    Button {
                        guard vm.kind != k else { return }
                        Haptics.selection()
                        vm.kind = k
                        Task {
                            await vm.reload()
                            currentSKU = vm.products.first?.sku
                        }
                    } label: {
                        Text(k.label)
                            .font(.footnote.weight(.semibold))
                            .padding(.horizontal, 14)
                            .padding(.vertical, 7)
                            .background(
                                vm.kind == k ? AnyShapeStyle(Color.muGold) : AnyShapeStyle(.ultraThinMaterial),
                                in: Capsule()
                            )
                            .foregroundStyle(vm.kind == k ? .black : .white)
                    }
                }
            }
            .padding(.horizontal, 16)
        }
        .padding(.top, 4)
    }

    // MARK: - 3 states

    private var loadingView: some View {
        VStack(spacing: 16) {
            ProgressView()
                .controlSize(.large)
                .tint(.muGold)
            Text(String(localized: "feed.loading"))
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
    }

    private func errorView(_ message: String) -> some View {
        VStack(spacing: 12) {
            Image(systemName: "wifi.exclamationmark")
                .font(.system(size: 44))
                .foregroundStyle(.tertiary)
            Text(String(localized: "feed.error.title"))
                .font(.headline)
            Text(message)
                .font(.footnote)
                .foregroundStyle(.secondary)
            Button {
                Task {
                    await vm.reload()
                    currentSKU = vm.products.first?.sku
                }
            } label: {
                Text(String(localized: "feed.retry"))
                    .font(.subheadline.weight(.semibold))
                    .padding(.horizontal, 24)
                    .padding(.vertical, 10)
            }
            .buttonStyle(.borderedProminent)
            .foregroundStyle(.black)
        }
        .padding(.horizontal, 32)
    }

    private var emptyView: some View {
        VStack(spacing: 12) {
            Image(systemName: "tshirt")
                .font(.system(size: 44))
                .foregroundStyle(.tertiary)
            Text(String(localized: "feed.empty"))
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
    }
}
