import SwiftUI

// ドロップ詳細 — 計器盤のメタデータ表 + 購入 (Stripe Checkout を Safari sheet で)。
struct DropDetailView: View {
    let product: FeedProduct
    @State private var showCheckout = false

    var body: some View {
        ZStack {
            Color.muBg.ignoresSafeArea()
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    AsyncImage(url: product.mockupURL) { phase in
                        switch phase {
                        case .success(let img):
                            img.resizable().scaledToFit()
                        default:
                            Rectangle().fill(Color.muCard)
                                .frame(height: 360)
                                .overlay(ProgressView().tint(Color.muGold))
                        }
                    }
                    .frame(maxWidth: .infinity)
                    .clipShape(RoundedRectangle(cornerRadius: 6))
                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color.muLine, lineWidth: 1))

                    Text(product.description)
                        .font(.system(size: 16, weight: .medium))
                        .foregroundStyle(Color.muFg)

                    metaTable

                    Button {
                        showCheckout = true
                    } label: {
                        Text(String(localized: "pdp.buy"))
                            .font(Mono.font(14, .bold))
                            .tracking(2)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 14)
                            .background(Color.muGold)
                            .foregroundStyle(.black)
                            .clipShape(RoundedRectangle(cornerRadius: 4))
                    }

                    Link(destination: URL(string: product.pdpUrl)!) {
                        Label(String(localized: "pdp.openWeb"), systemImage: "safari")
                            .font(Mono.font(11))
                            .foregroundStyle(Color.muMute)
                            .frame(maxWidth: .infinity)
                    }
                    .padding(.bottom, 24)
                }
                .padding(.horizontal, 14)
            }
        }
        .navigationTitle(product.sku)
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(Color.muBg, for: .navigationBar)
        .sheet(isPresented: $showCheckout) {
            if let url = URL(string: product.checkoutUrl) {
                SafariView(url: url).ignoresSafeArea()
            }
        }
    }

    private var metaTable: some View {
        VStack(spacing: 0) {
            metaRow(String(localized: "detail.sku"), product.sku)
            metaRow(String(localized: "detail.brand"), product.brand.uppercased())
            metaRow(
                String(localized: "detail.created"),
                product.createdDate.map { Fmt.stampJST.string(from: $0) + " JST" } ?? product.createdAt
            )
            metaRow(String(localized: "detail.price"), product.priceLabel, gold: true)
            metaRow(String(localized: "detail.sold"), "\(product.sold)", last: true)
        }
        .panel()
    }

    private func metaRow(_ label: String, _ value: String, gold: Bool = false, last: Bool = false) -> some View {
        HStack(alignment: .firstTextBaseline) {
            Text(label)
                .font(Mono.font(9, .semibold))
                .tracking(1.6)
                .foregroundStyle(Color.muMute)
                .textCase(.uppercase)
                .frame(width: 86, alignment: .leading)
            Text(value)
                .font(Mono.font(11))
                .foregroundStyle(gold ? Color.muGold : Color.muFg)
                .frame(maxWidth: .infinity, alignment: .trailing)
                .lineLimit(1)
                .minimumScaleFactor(0.6)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .overlay(alignment: .bottom) {
            if !last {
                Rectangle().fill(Color.muLine).frame(height: 1)
            }
        }
    }
}
