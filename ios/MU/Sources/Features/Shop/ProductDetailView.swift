import SwiftUI

// PDP — 大きく見せて、Apple Pay 込みの Stripe Checkout (Safari sheet) で買う。
// checkout は既存 GET /api/shop/checkout?sku= (Stripe Checkout は Safari 内で
// Apple Pay を出す)。ネイティブ PaymentSheet 化は P1。
struct ProductDetailView: View {
    let product: FeedProduct
    @State private var showCheckout = false

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
                    showCheckout = true
                } label: {
                    Label(String(localized: "pdp.buy"), systemImage: "applelogo")
                        .font(.headline)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 6)
                }
                .buttonStyle(.borderedProminent)
                .foregroundStyle(.black)

                Link(destination: URL(string: product.pdpUrl)!) {
                    Label(String(localized: "pdp.openWeb"), systemImage: "safari")
                        .font(.subheadline)
                        .frame(maxWidth: .infinity)
                }
                .padding(.bottom, 24)
            }
            .padding(.horizontal)
        }
        .navigationTitle(product.sku)
        .navigationBarTitleDisplayMode(.inline)
        .sheet(isPresented: $showCheckout) {
            if let url = URL(string: product.checkoutUrl) {
                SafariView(url: url).ignoresSafeArea()
            }
        }
    }
}
