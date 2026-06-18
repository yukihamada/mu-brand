import SwiftUI

// Home — 編集的なトップ。フルブリードのヒーロー (最新作・パララックス)、
// キュレーションレール、ブランドステートメントの静かな一文。
struct HomeView: View {
    @State private var newArrivals: [FeedProduct] = []
    @State private var essentials: [FeedProduct] = []
    @State private var bjj: [FeedProduct] = []
    @State private var loading = true
    @State private var failed = false

    var body: some View {
        NavigationStack {
            ScrollView(showsIndicators: false) {
                VStack(spacing: 0) {
                    hero
                    if failed {
                        ErrorBlock { await load() }
                            .padding(.vertical, 80)
                    } else {
                        rails
                    }
                    statement
                    footer
                }
            }
            .ignoresSafeArea(edges: .top)
            .background(Atelier.paper)
            .navigationDestination(for: FeedProduct.self) { ProductDetailView(product: $0) }
            .toolbar(.hidden, for: .navigationBar)
            .refreshable { await load() }
            .task { if newArrivals.isEmpty { await load() } }
        }
    }

    // MARK: - Hero (最新の一着・stretchy parallax)

    @ViewBuilder
    private var hero: some View {
        if let latest = newArrivals.first {
            NavigationLink(value: latest) {
                HeroImage(product: latest)
            }
            .buttonStyle(.plain)
        } else {
            Rectangle()
                .fill(Color.primary.opacity(0.05))
                .frame(height: 540)
                .modifier(Pulse())
                .overlay(alignment: .bottomLeading) {
                    VStack(alignment: .leading, spacing: 10) {
                        Text("home.latest").eyebrow()
                        Rectangle().fill(Color.primary.opacity(0.08)).frame(width: 220, height: 22)
                    }
                    .padding(28)
                }
        }
    }

    // MARK: - Rails

    @ViewBuilder
    private var rails: some View {
        let arrivalsRest = Array(newArrivals.dropFirst().prefix(12))
        Rail(title: String(localized: "home.newArrivals"), products: arrivalsRest, loading: loading)
        Rail(title: String(localized: "home.essentials"), products: Array(essentials.prefix(12)), loading: loading)
        Rail(title: String(localized: "home.bjj"), products: Array(bjj.prefix(12)), loading: loading)
    }

    private var statement: some View {
        VStack(spacing: 18) {
            Rectangle().fill(Atelier.gold).frame(width: 24, height: 1)
            Text("home.statement")
                .serif(.title3)
                .multilineTextAlignment(.center)
                .lineSpacing(8)
                .padding(.horizontal, 44)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 72)
    }

    private var footer: some View {
        VStack(spacing: 8) {
            Hairline().padding(.horizontal, 24)
            Text(verbatim: "MU — wearmu.com")
                .font(.caption2)
                .tracking(1.6)
                .foregroundStyle(.tertiary)
                .padding(.vertical, 28)
        }
    }

    // MARK: - Data

    private func load() async {
        loading = true
        failed = false
        defer { loading = false }
        async let arrivals = MUAPI.feed(page: 1)
        async let tees = MUAPI.feed(page: 1, kind: .tee)
        async let rash = MUAPI.feed(page: 1, kind: .rashguard)
        do {
            let (a, t, r) = try await (arrivals, tees, rash)
            newArrivals = a
            essentials = t
            bjj = r
            MUAPI.prefetch((a.prefix(8) + t.prefix(8) + r.prefix(8)).compactMap(\.mockupURL))
        } catch {
            failed = newArrivals.isEmpty
        }
    }
}

// MARK: - Hero image

private struct HeroImage: View {
    let product: FeedProduct

    var body: some View {
        GeometryReader { geo in
            let minY = geo.frame(in: .global).minY
            let stretch = max(0, minY)
            ProductImage(url: product.mockupURL)
                .frame(width: geo.size.width, height: geo.size.height + stretch)
                .clipped()
                .offset(y: -stretch)
                .overlay(alignment: .bottomLeading) { caption }
        }
        .frame(height: 540)
    }

    private var caption: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("home.latest")
                .font(.caption2.weight(.medium))
                .tracking(2.4)
                .foregroundStyle(.white.opacity(0.75))
            Text(product.title)
                .serif(.title2)
                .foregroundStyle(.white)
                .lineLimit(2)
                .multilineTextAlignment(.leading)
            HStack(spacing: 14) {
                Text(product.priceLabel)
                    .font(.subheadline)
                    .foregroundStyle(.white.opacity(0.9))
                Text("home.view")
                    .font(.caption2.weight(.semibold))
                    .tracking(2.0)
                    .foregroundStyle(.white)
                    .underline()
            }
        }
        .padding(28)
        .padding(.bottom, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background {
            LinearGradient(
                colors: [.clear, .black.opacity(0.55)],
                startPoint: .top, endPoint: .bottom
            )
        }
    }
}

// MARK: - Rail

private struct Rail: View {
    let title: String
    let products: [FeedProduct]
    let loading: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(title)
                .serif(.title3)
                .padding(.horizontal, 24)
            ScrollView(.horizontal, showsIndicators: false) {
                LazyHStack(alignment: .top, spacing: 14) {
                    if products.isEmpty && loading {
                        ForEach(0..<4, id: \.self) { _ in RailSkeleton() }
                    } else {
                        ForEach(products) { p in
                            NavigationLink(value: p) { RailCard(product: p) }
                                .buttonStyle(.plain)
                        }
                    }
                }
                .padding(.horizontal, 24)
            }
        }
        .padding(.top, 44)
    }
}

private struct RailCard: View {
    let product: FeedProduct

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ProductImage(url: product.mockupURL)
                .frame(width: 188, height: 244)
                .clipped()
            Text(product.title)
                .font(.caption)
                .lineLimit(1)
            Text(product.priceLabel)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .frame(width: 188, alignment: .leading)
    }
}

private struct RailSkeleton: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Rectangle().fill(Color.primary.opacity(0.05))
                .frame(width: 188, height: 244)
            Rectangle().fill(Color.primary.opacity(0.05))
                .frame(width: 120, height: 10)
            Rectangle().fill(Color.primary.opacity(0.05))
                .frame(width: 56, height: 9)
        }
        .modifier(Pulse())
    }
}

// MARK: - Error (世界観を壊さない)

struct ErrorBlock: View {
    let retry: () async -> Void

    var body: some View {
        VStack(spacing: 20) {
            Text("common.errorTitle")
                .serif(.title3)
            Text("common.errorBody")
                .font(.footnote)
                .foregroundStyle(.secondary)
            Button {
                Task { await retry() }
            } label: {
                Text("common.retry")
            }
            .buttonStyle(HairlineButtonStyle())
            .frame(width: 180)
        }
        .multilineTextAlignment(.center)
        .padding(.horizontal, 44)
    }
}
