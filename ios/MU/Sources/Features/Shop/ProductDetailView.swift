import SwiftUI

// PDP — 大きく見せて、Apple Pay 込みの Stripe Checkout (Safari sheet) で買う。
// checkout は既存 GET /api/shop/checkout?sku= (Stripe Checkout は Safari 内で
// Apple Pay を出す)。ネイティブ PaymentSheet 化は P1。
struct ProductDetailView: View {
    let product: FeedProduct
    @State private var showCheckout = false
    @State private var showGift = false
    @State private var related: [FeedProduct] = []

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                AsyncImage(url: product.mockupURL) { phase in
                    switch phase {
                    case .success(let img): img.resizable().scaledToFit()
                    default: Rectangle().fill(.quaternary).frame(height: 380)
                    }
                }
                .frame(maxWidth: .infinity)
                .clipShape(RoundedRectangle(cornerRadius: 16))

                VStack(alignment: .leading, spacing: 8) {
                    Text(product.brand.uppercased())
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    Text(product.description)
                        .font(.title3.weight(.medium))
                    HStack {
                        Text(product.priceLabel).font(.title2.bold())
                        Spacer()
                        if product.sold > 0 {
                            Text(String(format: String(localized: "feed.sold %lld"), product.sold))
                                .font(.footnote)
                                .foregroundStyle(.tint)
                        }
                    }
                }

                Button {
                    Analytics.track("pdp_buy", ["sku": product.sku])
                    showCheckout = true
                } label: {
                    // Apple Pay マーク/文言はカスタムボタンに使えない (PKPaymentButton 限定・
                    // Marketing Guidelines)。実態は Stripe Checkout なので「購入する」が正直。
                    Text(String(localized: "pdp.buy"))
                        .font(.headline)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 6)
                }
                .buttonStyle(.borderedProminent)
                .foregroundStyle(.black)

                // 🎁 プレゼントする
                Button {
                    Analytics.track("pdp_gift", ["sku": product.sku])
                    showGift = true
                } label: {
                    Label(String(localized: "buy.gift"), systemImage: "gift.fill")
                        .font(.subheadline.weight(.semibold))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 4)
                }
                .buttonStyle(.bordered)

                if let pdp = URL(string: product.pdpUrl) {
                    Link(destination: pdp) {
                        Label(String(localized: "pdp.openWeb"), systemImage: "safari")
                            .font(.subheadline)
                            .frame(maxWidth: .infinity)
                    }
                }

                // 関連商品(同じ商品タイプ・売れ筋順)
                if !related.isEmpty {
                    Divider().padding(.vertical, 4)
                    Text(String(localized: "pdp.related"))
                        .font(.headline)
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(alignment: .top, spacing: 12) {
                            ForEach(related) { p in
                                NavigationLink(value: p) {
                                    VStack(alignment: .leading, spacing: 4) {
                                        AsyncImage(url: p.mockupURL) { phase in
                                            switch phase {
                                            case .success(let img): img.resizable().scaledToFill()
                                            default: Rectangle().fill(.quaternary)
                                            }
                                        }
                                        .frame(width: 130, height: 130)
                                        .clipShape(RoundedRectangle(cornerRadius: 10))
                                        Text(p.description).font(.caption2).lineLimit(1).frame(width: 130, alignment: .leading)
                                        Text(p.priceLabel).font(.caption.weight(.semibold))
                                    }
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                    .padding(.bottom, 24)
                }
            }
            .padding(.horizontal)
        }
        .navigationTitle(product.sku)
        .navigationBarTitleDisplayMode(.inline)
        .task {
            Analytics.track("pdp_view", ["sku": product.sku])
            related = (try? await MUAPI.related(sku: product.sku)) ?? []
        }
        .sheet(isPresented: $showCheckout) {
            if let url = URL(string: product.checkoutUrl) {
                SafariView(url: url).ignoresSafeArea()
            }
        }
        .sheet(isPresented: $showGift) {
            // checkout_url に `?` の有無を見てギフトフラグを安全に付与。
            let sep = product.checkoutUrl.contains("?") ? "&" : "?"
            if let url = URL(string: product.checkoutUrl + sep + "gift=1") {
                SafariView(url: url).ignoresSafeArea()
            }
        }
    }
}
