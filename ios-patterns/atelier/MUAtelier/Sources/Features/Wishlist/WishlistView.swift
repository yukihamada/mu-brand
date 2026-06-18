import SwiftUI

// Wishlist — 保存した商品の静かなグリッド。完全ローカル (サーバ API は存在しない)。
struct WishlistView: View {
    @EnvironmentObject private var wishlist: Wishlist

    private let columns = [
        GridItem(.flexible(), spacing: 20),
        GridItem(.flexible(), spacing: 20),
    ]

    var body: some View {
        NavigationStack {
            ScrollView(showsIndicators: false) {
                VStack(alignment: .leading, spacing: 0) {
                    header
                    if wishlist.items.isEmpty {
                        empty
                    } else {
                        grid
                    }
                }
                .padding(.horizontal, 24)
            }
            .background(Atelier.paper)
            .toolbar(.hidden, for: .navigationBar)
            .navigationDestination(for: FeedProduct.self) { ProductDetailView(product: $0) }
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(verbatim: "MU ATELIER").eyebrow()
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text("wishlist.title").serif(.largeTitle)
                if !wishlist.items.isEmpty {
                    Text(verbatim: "\(wishlist.items.count)")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(.top, 12)
        .padding(.bottom, 26)
    }

    private var grid: some View {
        LazyVGrid(columns: columns, spacing: 32) {
            ForEach(wishlist.items) { p in
                NavigationLink(value: p) {
                    WishCard(product: p)
                }
                .buttonStyle(.plain)
                .contextMenu {
                    Button(role: .destructive) {
                        withAnimation(Atelier.spring) { wishlist.remove(p) }
                    } label: {
                        Label(String(localized: "wishlist.remove"), systemImage: "heart.slash")
                    }
                }
            }
        }
        .padding(.bottom, 36)
    }

    private var empty: some View {
        VStack(spacing: 16) {
            Rectangle().fill(Atelier.gold).frame(width: 24, height: 1)
            Text("wishlist.emptyTitle")
                .serif(.title3)
            Text("wishlist.emptyBody")
                .font(.footnote)
                .foregroundStyle(.secondary)
                .lineSpacing(5)
        }
        .multilineTextAlignment(.center)
        .frame(maxWidth: .infinity)
        .padding(.vertical, 120)
    }
}

private struct WishCard: View {
    let product: FeedProduct

    var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            Color.clear
                .aspectRatio(3 / 4, contentMode: .fit)
                .overlay { ProductImage(url: product.mockupURL) }
                .clipped()
            Text(product.title)
                .font(.caption)
                .lineLimit(1)
                .foregroundStyle(.primary)
            Text(product.priceLabel)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }
}
