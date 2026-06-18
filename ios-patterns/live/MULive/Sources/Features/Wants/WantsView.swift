import SwiftUI

// ❤️ Wants — ダブルタップした商品のコレクション。完全ローカル (WantsStore)。
// グリッド表示 + 左スワイプ削除 (長押しメニューでも削除可)。
struct WantsView: View {
    @EnvironmentObject private var wants: WantsStore
    @State private var detailProduct: FeedProduct?

    private let columns = [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)]

    var body: some View {
        NavigationStack {
            Group {
                if wants.items.isEmpty {
                    emptyView
                } else {
                    grid
                }
            }
            .navigationTitle(String(localized: "tab.wants"))
            .background(Color.black)
            .sheet(item: $detailProduct) { DetailSheetView(product: $0) }
        }
    }

    private var grid: some View {
        ScrollView {
            LazyVGrid(columns: columns, spacing: 16) {
                ForEach(wants.items) { item in
                    WantCell(item: item) {
                        Haptics.medium()
                        detailProduct = item.product
                    } onDelete: {
                        Haptics.success()
                        withAnimation(.spring(response: 0.35, dampingFraction: 0.8)) {
                            wants.remove(sku: item.product.sku)
                        }
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
        }
    }

    private var emptyView: some View {
        VStack(spacing: 14) {
            Image(systemName: "heart")
                .font(.system(size: 52))
                .foregroundStyle(Color.muGold.opacity(0.6))
            Text(String(localized: "wants.empty.title"))
                .font(.headline)
            Text(String(localized: "wants.empty.body"))
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .padding(.horizontal, 40)
    }
}

// グリッドセル: 左スワイプで削除ボタンがせり出す。タップで詳細シート。
private struct WantCell: View {
    let item: WantsStore.Item
    var onTap: () -> Void
    var onDelete: () -> Void

    @State private var offsetX: CGFloat = 0
    @GestureState private var dragging = false

    private let revealWidth: CGFloat = 72

    var body: some View {
        ZStack(alignment: .trailing) {
            // 背面: 削除ボタン
            Button(action: onDelete) {
                VStack(spacing: 4) {
                    Image(systemName: "trash.fill")
                    Text(String(localized: "wants.delete"))
                        .font(.caption2)
                }
                .foregroundStyle(.white)
                .frame(width: revealWidth)
                .frame(maxHeight: .infinity)
                .background(.red, in: RoundedRectangle(cornerRadius: 14))
            }
            .opacity(offsetX < -8 ? 1 : 0)

            // 前面: カード本体
            card
                .offset(x: offsetX)
                .gesture(swipe)
                .onTapGesture {
                    if offsetX < 0 {
                        withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) { offsetX = 0 }
                    } else {
                        onTap()
                    }
                }
                .contextMenu {
                    Button(role: .destructive, action: onDelete) {
                        Label(String(localized: "wants.delete"), systemImage: "trash")
                    }
                }
        }
    }

    private var card: some View {
        VStack(alignment: .leading, spacing: 8) {
            AsyncImage(url: item.product.mockupURL) { phase in
                if case .success(let image) = phase {
                    image.resizable().scaledToFill()
                } else {
                    Rectangle().fill(.quaternary)
                        .overlay(Image(systemName: "tshirt").foregroundStyle(.tertiary))
                }
            }
            .frame(height: 190)
            .frame(maxWidth: .infinity)
            .clipped()

            VStack(alignment: .leading, spacing: 4) {
                Text(item.product.brand.uppercased())
                    .font(.caption2.weight(.bold))
                    .tracking(1.5)
                    .foregroundStyle(Color.muGold)
                Text(item.product.displayTitle)
                    .font(.caption)
                    .lineLimit(2)
                    .foregroundStyle(.primary)
                Text(item.product.priceLabel)
                    .font(.footnote.weight(.bold))
                    .foregroundStyle(Color.muGold)
            }
            .padding(.horizontal, 10)
            .padding(.bottom, 10)
        }
        .background(Color(white: 0.09), in: RoundedRectangle(cornerRadius: 14))
    }

    private var swipe: some Gesture {
        DragGesture(minimumDistance: 18)
            .updating($dragging) { _, state, _ in state = true }
            .onChanged { value in
                // 横方向の引きだけ拾う (縦はスクロールに譲る)
                guard abs(value.translation.width) > abs(value.translation.height) else { return }
                offsetX = min(0, max(-revealWidth - 16, value.translation.width))
            }
            .onEnded { value in
                withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                    offsetX = value.translation.width < -revealWidth * 0.6 ? -revealWidth - 8 : 0
                }
            }
    }
}
