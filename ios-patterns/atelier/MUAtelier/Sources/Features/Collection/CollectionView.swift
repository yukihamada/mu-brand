import SwiftUI

// Collection — 2カラムの端正なグリッド。kind はテキストタブ、検索 + 最近の検索語。
// グリッド → PDP は matchedGeometryEffect で同一階層オーバーレイ遷移。
struct CollectionView: View {
    @Namespace private var ns
    @StateObject private var recents = RecentSearches()

    @State private var products: [FeedProduct] = []
    @State private var page = 1
    @State private var kind: AtelierKind = .all
    @State private var query = ""
    @State private var submittedQuery = ""
    @State private var loading = false
    @State private var initialLoading = true
    @State private var reachedEnd = false
    @State private var failed = false
    @State private var selected: FeedProduct?
    @FocusState private var searchFocused: Bool

    private let columns = [
        GridItem(.flexible(), spacing: 20),
        GridItem(.flexible(), spacing: 20),
    ]

    var body: some View {
        ZStack {
            browse
            if let p = selected {
                ProductDetailView(product: p, ns: ns) {
                    withAnimation(Atelier.spring) { selected = nil }
                }
                .zIndex(2)
                .transition(.opacity)
            }
        }
        .toolbar(selected == nil ? .visible : .hidden, for: .tabBar)
        .background(Atelier.paper)
    }

    // MARK: - Browse

    private var browse: some View {
        VStack(spacing: 0) {
            header
            ScrollView(showsIndicators: false) {
                LazyVStack(spacing: 0) {
                    if searchFocused && query.isEmpty && !recents.terms.isEmpty {
                        recentSearches
                    }
                    kindTabs
                        .padding(.bottom, 20)
                    grid
                }
                .padding(.horizontal, 24)
            }
            .refreshable { await reload() }
        }
        .task { if products.isEmpty { await reload() } }
        .onChange(of: kind) { _, _ in Task { await reload() } }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(verbatim: "MU ATELIER").eyebrow()
            Text("collection.title").serif(.largeTitle)
            searchField
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 24)
        .padding(.top, 12)
        .padding(.bottom, 18)
    }

    private var searchField: some View {
        VStack(spacing: 8) {
            HStack(spacing: 10) {
                Image(systemName: "magnifyingglass")
                    .font(.footnote)
                    .foregroundStyle(.tertiary)
                TextField(String(localized: "collection.search"), text: $query)
                    .font(.subheadline)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.search)
                    .focused($searchFocused)
                    .onSubmit {
                        recents.add(query)
                        Task { await reload() }
                    }
                if !query.isEmpty {
                    Button {
                        query = ""
                        Task { await reload() }
                    } label: {
                        Image(systemName: "xmark")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                    .accessibilityLabel(String(localized: "collection.clear"))
                }
            }
            Hairline()
        }
    }

    private var recentSearches: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("collection.recent").eyebrow()
                Spacer()
                Button {
                    recents.clear()
                } label: {
                    Text("collection.clearRecent")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .underline()
                }
            }
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(recents.terms, id: \.self) { term in
                        Button {
                            query = term
                            searchFocused = false
                            Task { await reload() }
                        } label: {
                            Text(term)
                                .font(.caption)
                                .padding(.horizontal, 14)
                                .padding(.vertical, 8)
                                .overlay(Rectangle().strokeBorder(Atelier.hairline, lineWidth: 1))
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
        .padding(.bottom, 20)
    }

    private var kindTabs: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 24) {
                ForEach(AtelierKind.allCases) { k in
                    Button {
                        guard kind != k else { return }
                        kind = k
                    } label: {
                        VStack(spacing: 6) {
                            Text(k.label)
                                .font(.caption.weight(kind == k ? .semibold : .regular))
                                .tracking(1.8)
                                .foregroundStyle(kind == k ? .primary : .secondary)
                            Rectangle()
                                .fill(kind == k ? Atelier.gold : .clear)
                                .frame(height: 1.5)
                        }
                        .fixedSize()
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.vertical, 4)
        }
    }

    // MARK: - Grid

    @ViewBuilder
    private var grid: some View {
        if initialLoading {
            LazyVGrid(columns: columns, spacing: 32) {
                ForEach(0..<6, id: \.self) { _ in GridSkeleton() }
            }
        } else if failed && products.isEmpty {
            ErrorBlock { await reload() }
                .padding(.vertical, 60)
        } else if products.isEmpty {
            emptyState
        } else {
            LazyVGrid(columns: columns, spacing: 32) {
                ForEach(products) { p in
                    GridCard(
                        product: p,
                        ns: ns,
                        isSource: selected?.sku != p.sku
                    )
                    .onTapGesture {
                        searchFocused = false
                        withAnimation(Atelier.spring) { selected = p }
                    }
                    .onAppear {
                        if p == products.last { Task { await loadMore() } }
                    }
                }
            }
            if loading {
                ProgressView()
                    .controlSize(.small)
                    .padding(.vertical, 28)
            } else {
                Color.clear.frame(height: 28)
            }
        }
    }

    private var emptyState: some View {
        VStack(spacing: 14) {
            Text("collection.emptyTitle").serif(.title3)
            Text("collection.emptyBody")
                .font(.footnote)
                .foregroundStyle(.secondary)
            if !submittedQuery.isEmpty {
                Button {
                    query = ""
                    Task { await reload() }
                } label: {
                    Text("collection.clear")
                }
                .buttonStyle(HairlineButtonStyle())
                .frame(width: 200)
                .padding(.top, 10)
            }
        }
        .multilineTextAlignment(.center)
        .frame(maxWidth: .infinity)
        .padding(.vertical, 70)
    }

    // MARK: - Data

    private func reload() async {
        page = 1
        reachedEnd = false
        submittedQuery = query
        await fetch(replace: true)
    }

    private func loadMore() async {
        guard !loading, !reachedEnd else { return }
        page += 1
        await fetch(replace: false)
    }

    private func fetch(replace: Bool) async {
        loading = true
        defer {
            loading = false
            initialLoading = false
        }
        do {
            let new = try await MUAPI.feed(page: page, kind: kind, query: submittedQuery)
            if new.isEmpty { reachedEnd = true }
            products = replace ? new : products + new
            failed = false
            MUAPI.prefetch(new.compactMap(\.mockupURL))
            if LaunchOptions.openFirstProduct, selected == nil, let first = products.first {
                withAnimation(Atelier.spring) { selected = first }
            }
        } catch {
            failed = true
            if replace { products = [] }
        }
    }
}

// MARK: - Grid card (影なし・罫線なしの潔さ)

private struct GridCard: View {
    let product: FeedProduct
    let ns: Namespace.ID
    let isSource: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            Color.clear
                .aspectRatio(3 / 4, contentMode: .fit)
                .overlay {
                    ProductImage(url: product.mockupURL)
                }
                .clipped()
                .matchedGeometryEffect(id: product.sku, in: ns, isSource: isSource)
            Text(product.title)
                .font(.caption)
                .lineLimit(1)
                .foregroundStyle(.primary)
            Text(product.priceLabel)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .contentShape(Rectangle())
    }
}

private struct GridSkeleton: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            Rectangle()
                .fill(Color.primary.opacity(0.05))
                .aspectRatio(3 / 4, contentMode: .fit)
            Rectangle().fill(Color.primary.opacity(0.05))
                .frame(width: 110, height: 10)
            Rectangle().fill(Color.primary.opacity(0.05))
                .frame(width: 52, height: 9)
        }
        .modifier(Pulse())
    }
}
