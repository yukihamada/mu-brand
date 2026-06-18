import SwiftUI

// 1ページ=1商品のフルブリードカード。
// Ken Burns ズーム + 黒グラデ + 左下情報 + 右側アクションバー + ダブルタップで「欲しい」。
struct ProductPageView: View {
    let product: FeedProduct
    var onDetail: () -> Void

    @EnvironmentObject private var wants: WantsStore
    @State private var kenBurns = false
    @State private var bursts: [HeartBurst] = []
    @State private var heartPop = false

    var body: some View {
        GeometryReader { geo in
            ZStack {
                imageLayer(size: geo.size)
                    .contentShape(Rectangle())
                    .onTapGesture(count: 2, coordinateSpace: .local) { point in
                        wantByDoubleTap(at: point)
                    }

                infoOverlay

                ForEach(bursts) { burst in
                    HeartBurstView(burst: burst) {
                        bursts.removeAll { $0.id == burst.id }
                    }
                }
            }
        }
        .clipped()
        .background(Color.black)
    }

    // MARK: - image + gradients (Ken Burns)

    private func imageLayer(size: CGSize) -> some View {
        ZStack {
            AsyncImage(url: product.mockupURL) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .scaledToFill()
                        .frame(width: size.width, height: size.height)
                        .clipped()
                        .scaleEffect(kenBurns ? 1.16 : 1.03)
                        .offset(y: kenBurns ? -12 : 12)
                        .onAppear {
                            withAnimation(.easeInOut(duration: 14).repeatForever(autoreverses: true)) {
                                kenBurns = true
                            }
                        }
                case .failure:
                    Image(systemName: "tshirt")
                        .font(.system(size: 64))
                        .foregroundStyle(.tertiary)
                        .frame(width: size.width, height: size.height)
                default:
                    ZStack {
                        Color.black
                        ProgressView().tint(.muGold)
                    }
                    .frame(width: size.width, height: size.height)
                }
            }

            // 没入感を保つ薄い黒グラデ (上下)
            LinearGradient(
                stops: [
                    .init(color: .black.opacity(0.55), location: 0.0),
                    .init(color: .clear, location: 0.22),
                    .init(color: .clear, location: 0.55),
                    .init(color: .black.opacity(0.75), location: 1.0),
                ],
                startPoint: .top, endPoint: .bottom
            )
        }
    }

    // MARK: - info + actions

    private var infoOverlay: some View {
        VStack {
            Spacer()
            HStack(alignment: .bottom, spacing: 12) {
                productInfo
                Spacer(minLength: 16)
                actionBar
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 104) // タブバー + ホームインジケータの下に潜らない
        }
    }

    private var productInfo: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(product.brand.uppercased())
                .font(.caption.weight(.bold))
                .tracking(2)
                .foregroundStyle(Color.muGold)

            Text(product.displayTitle)
                .font(.title3.weight(.bold))
                .foregroundStyle(.white)
                .lineLimit(3)
                .multilineTextAlignment(.leading)

            HStack(spacing: 10) {
                Text(product.priceLabel)
                    .font(.title2.bold())
                    .foregroundStyle(Color.muGold)
                if product.sold > 0 {
                    Text(String(format: String(localized: "detail.sold %lld"), product.sold))
                        .font(.caption.weight(.semibold))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 3)
                        .background(.white.opacity(0.15), in: Capsule())
                        .foregroundStyle(.white)
                }
            }

            if let age = product.relativeAgeLabel {
                Text(String(format: String(localized: "feed.generated %@"), age))
                    .font(.caption)
                    .foregroundStyle(.white.opacity(0.85))
            }
        }
        .shadow(color: .black.opacity(0.6), radius: 4, y: 1)
    }

    private var actionBar: some View {
        VStack(spacing: 24) {
            // 欲しい (ローカル保存 — サーバ API は存在しない)
            Button {
                Haptics.heavy()
                wants.toggle(product)
                popHeart()
            } label: {
                actionLabel(
                    icon: wants.contains(product) ? "heart.fill" : "heart",
                    text: wants.contains(product)
                        ? String(localized: "action.wanted")
                        : String(localized: "action.want"),
                    tint: wants.contains(product) ? Color.muGold : .white
                )
                .scaleEffect(heartPop ? 1.25 : 1.0)
            }

            // 共有 = 実商品ページ URL
            if let url = product.pdpURL {
                ShareLink(item: url) {
                    actionLabel(icon: "arrowshape.turn.up.right.fill",
                                text: String(localized: "action.share"),
                                tint: .white)
                }
            }

            Button(action: onDetail) {
                actionLabel(icon: "info.circle.fill",
                            text: String(localized: "action.details"),
                            tint: .white)
            }
        }
        .shadow(color: .black.opacity(0.6), radius: 5, y: 1)
    }

    private func actionLabel(icon: String, text: String, tint: Color) -> some View {
        VStack(spacing: 4) {
            Image(systemName: icon)
                .font(.system(size: 30, weight: .semibold))
                .foregroundStyle(tint)
            Text(text)
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.white)
        }
    }

    // MARK: - double tap

    private func wantByDoubleTap(at point: CGPoint) {
        Haptics.heavy()
        wants.add(product) // ダブルタップは常に追加 (取り消しはハートボタン/Wants から)
        bursts.append(HeartBurst(point: point))
        popHeart()
    }

    private func popHeart() {
        withAnimation(.spring(response: 0.25, dampingFraction: 0.5)) {
            heartPop = true
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
            withAnimation(.spring(response: 0.3, dampingFraction: 0.6)) {
                heartPop = false
            }
        }
    }
}
