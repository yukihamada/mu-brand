import SwiftUI

// 下からせり上がるハーフモーダル詳細。購入 = 実在の GET /api/shop/checkout?sku=
// (Stripe Checkout) を SFSafariViewController で開く。サイズ選択は checkout 側で行う
// (feed.json にサイズ情報フィールドは無い — 正直にその旨を表示)。
struct DetailSheetView: View {
    let product: FeedProduct
    @State private var checkoutURL: URL?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                header

                Text(product.description)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineSpacing(3)

                if let age = product.relativeAgeLabel {
                    Label(
                        String(format: String(localized: "feed.generated %@"), age),
                        systemImage: "bolt.fill"
                    )
                    .font(.footnote)
                    .foregroundStyle(Color.muGold)
                    .labelStyle(.titleAndIcon)
                }

                Text(String(localized: "detail.sizeNote"))
                    .font(.footnote)
                    .foregroundStyle(.tertiary)

                buyButton

                if let pdp = product.pdpURL {
                    Link(destination: pdp) {
                        Label(String(localized: "detail.openWeb"), systemImage: "safari")
                            .font(.subheadline)
                            .frame(maxWidth: .infinity)
                    }
                    .padding(.bottom, 16)
                }
            }
            .padding(20)
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .presentationBackground(Color(white: 0.07))
        .sheet(item: $checkoutURL) { url in
            SafariView(url: url).ignoresSafeArea()
        }
    }

    private var header: some View {
        HStack(alignment: .top, spacing: 14) {
            AsyncImage(url: product.mockupURL) { phase in
                if case .success(let image) = phase {
                    image.resizable().scaledToFill()
                } else {
                    Rectangle().fill(.quaternary)
                }
            }
            .frame(width: 92, height: 92)
            .clipShape(RoundedRectangle(cornerRadius: 12))

            VStack(alignment: .leading, spacing: 6) {
                Text(product.brand.uppercased())
                    .font(.caption.weight(.bold))
                    .tracking(2)
                    .foregroundStyle(Color.muGold)
                Text(product.displayTitle)
                    .font(.headline)
                    .lineLimit(3)
                HStack(spacing: 8) {
                    Text(product.priceLabel)
                        .font(.title3.bold())
                        .foregroundStyle(Color.muGold)
                    if product.sold > 0 {
                        Text(String(format: String(localized: "detail.sold %lld"), product.sold))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
    }

    private var buyButton: some View {
        Button {
            Haptics.success()
            checkoutURL = product.checkoutURL
        } label: {
            Text(String(localized: "detail.buy"))
                .font(.headline)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 8)
        }
        .buttonStyle(.borderedProminent)
        .foregroundStyle(.black)
        .disabled(product.checkoutURL == nil)
    }
}

// sheet(item:) に URL を直接渡すための準拠
extension URL: Identifiable {
    public var id: String { absoluteString }
}
