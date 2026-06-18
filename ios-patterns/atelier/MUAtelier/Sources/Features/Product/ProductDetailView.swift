import SwiftUI

// PDP — 大画像、丁寧な余白、購入導線 (Stripe Checkout / SFSafariViewController)。
// 2つの文脈で使う:
//   1. Collection からの matchedGeometryEffect オーバーレイ (ns + onClose を渡す)
//   2. Home / Wishlist からの NavigationStack push (デフォルト)
// 注: 公開 JSON API に追加画像 (extras) は無い (/api/products/item/:id は内部 i64 ID 専用
// と実打確認済) ため、画像は mockup 1枚。imageURLs は将来の複数化に備えた配列。
struct ProductDetailView: View {
    let product: FeedProduct
    var ns: Namespace.ID? = nil
    var onClose: (() -> Void)? = nil

    @EnvironmentObject private var wishlist: Wishlist
    @State private var showCheckout = false
    @State private var showWeb = false
    @State private var imageIndex = 0

    private var imageURLs: [URL] {
        [product.mockupURL].compactMap { $0 }
    }

    var body: some View {
        ZStack(alignment: .topLeading) {
            ScrollView(showsIndicators: false) {
                VStack(alignment: .leading, spacing: 0) {
                    gallery
                    content
                }
            }
            .ignoresSafeArea(edges: .top)
            .safeAreaInset(edge: .bottom) { buyBar }

            if onClose != nil {
                closeButton
            }
        }
        .background(Atelier.paper)
        .toolbarBackground(.hidden, for: .navigationBar)
        .sheet(isPresented: $showCheckout) {
            if let url = product.checkoutURL {
                SafariView(url: url).ignoresSafeArea()
            }
        }
        .sheet(isPresented: $showWeb) {
            if let url = product.pdpWebURL {
                SafariView(url: url).ignoresSafeArea()
            }
        }
    }

    // MARK: - Gallery (extras 複数化に備えた page TabView。現状1枚ならドットは出ない)

    @ViewBuilder
    private var gallery: some View {
        let height: CGFloat = 520
        Group {
            if imageURLs.count > 1 {
                TabView(selection: $imageIndex) {
                    ForEach(Array(imageURLs.enumerated()), id: \.offset) { i, url in
                        ProductImage(url: url)
                            .frame(height: height)
                            .clipped()
                            .tag(i)
                    }
                }
                .tabViewStyle(.page)
                .frame(height: height)
            } else {
                ProductImage(url: imageURLs.first)
                    .frame(maxWidth: .infinity)
                    .frame(height: height)
                    .clipped()
            }
        }
        .matchedIfAvailable(id: product.sku, in: ns)
    }

    // MARK: - Content

    private var content: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .firstTextBaseline) {
                    Text(product.brand.uppercased())
                        .eyebrow()
                    Spacer()
                    if product.sold > 0 {
                        HStack(spacing: 5) {
                            Circle().fill(Atelier.gold).frame(width: 4, height: 4)
                            Text(String(format: String(localized: "pdp.sold %lld"), product.sold))
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                Text(product.title)
                    .serif(.title2)
                    .lineSpacing(4)
                Text(product.priceLabel)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            .padding(.top, 28)

            Hairline().padding(.vertical, 24)

            Text(product.description)
                .font(.subheadline)
                .lineSpacing(7)
                .foregroundStyle(.primary.opacity(0.85))

            Hairline().padding(.vertical, 24)

            VStack(alignment: .leading, spacing: 14) {
                detailRow(key: "pdp.sizeLabel", value: "pdp.sizeValue")
                detailRow(key: "pdp.madeLabel", value: "pdp.madeValue")
            }

            Button {
                showWeb = true
            } label: {
                Text("pdp.viewWeb")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .underline()
            }
            .padding(.top, 28)
            .padding(.bottom, 36)
        }
        .padding(.horizontal, 24)
    }

    private func detailRow(key: LocalizedStringKey, value: LocalizedStringKey) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 16) {
            Text(key)
                .font(.caption2.weight(.medium))
                .tracking(1.6)
                .foregroundStyle(.secondary)
                .frame(width: 90, alignment: .leading)
            Text(value)
                .font(.caption)
                .foregroundStyle(.primary.opacity(0.8))
        }
    }

    // MARK: - Buy bar (購入転換の芯。常に視界に)

    private var buyBar: some View {
        VStack(spacing: 0) {
            Hairline()
            HStack(spacing: 14) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(product.priceLabel)
                        .serif(.headline)
                    Text("pdp.taxNote")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
                Spacer()
                wishlistButton
                Button {
                    showCheckout = true
                } label: {
                    Text("pdp.buy")
                }
                .buttonStyle(PrimaryButtonStyle())
                .frame(width: 168)
            }
            .padding(.horizontal, 24)
            .padding(.vertical, 14)
        }
        .background(Atelier.paper.opacity(0.97))
    }

    private var wishlistButton: some View {
        let saved = wishlist.contains(product)
        return Button {
            withAnimation(.easeOut(duration: 0.18)) { wishlist.toggle(product) }
        } label: {
            Image(systemName: saved ? "heart.fill" : "heart")
                .font(.body)
                .foregroundStyle(saved ? Atelier.gold : .primary)
                .frame(width: 52, height: 52)
                .overlay(Rectangle().strokeBorder(Atelier.hairline, lineWidth: 1))
        }
        .buttonStyle(.plain)
        .accessibilityLabel(
            saved
                ? String(localized: "pdp.removeWishlist")
                : String(localized: "pdp.addWishlist")
        )
    }

    private var closeButton: some View {
        Button {
            onClose?()
        } label: {
            Image(systemName: "xmark")
                .font(.footnote.weight(.medium))
                .foregroundStyle(.primary)
                .frame(width: 38, height: 38)
                .background(.ultraThinMaterial, in: Circle())
        }
        .padding(.leading, 20)
        .padding(.top, 8)
        .accessibilityLabel(String(localized: "pdp.close"))
    }
}

// matchedGeometryEffect を Namespace がある時だけ適用する小さな橋。
private extension View {
    @ViewBuilder
    func matchedIfAvailable(id: String, in ns: Namespace.ID?) -> some View {
        if let ns {
            matchedGeometryEffect(id: id, in: ns)
        } else {
            self
        }
    }
}
