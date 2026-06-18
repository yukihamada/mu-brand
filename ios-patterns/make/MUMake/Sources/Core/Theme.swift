import SwiftUI
import UIKit

// MU ブランドトーン: 黒地に金 #e6c449。Make アプリは創作の高揚感を少し足す。
enum MUTheme {
    static let gold = Color(red: 0.90, green: 0.77, blue: 0.29) // #e6c449
    static let goldDim = Color(red: 0.90, green: 0.77, blue: 0.29).opacity(0.55)
    static let bg = Color.black
    static let card = Color(white: 0.085)
    static let cardBorder = Color(white: 0.22)

    static let goldGradient = LinearGradient(
        colors: [Color(red: 1.0, green: 0.92, blue: 0.62), gold, Color(red: 0.72, green: 0.58, blue: 0.18)],
        startPoint: .topLeading, endPoint: .bottomTrailing
    )
}

// 触覚 — 押した/できた/失敗した、を指に伝える。
enum Haptics {
    static func tap() { UIImpactFeedbackGenerator(style: .light).impactOccurred() }
    static func rigid() { UIImpactFeedbackGenerator(style: .rigid).impactOccurred() }
    static func success() { UINotificationFeedbackGenerator().notificationOccurred(.success) }
    static func failure() { UINotificationFeedbackGenerator().notificationOccurred(.error) }
}

// 読み込み/空/エラーの3状態を全画面で統一表示するための部品。
struct StateBanner: View {
    enum Kind { case loading, empty(String), error(String, retry: () -> Void) }
    let kind: Kind

    var body: some View {
        VStack(spacing: 14) {
            switch kind {
            case .loading:
                ProgressView().controlSize(.large).tint(MUTheme.gold)
            case .empty(let message):
                Image(systemName: "sparkles")
                    .font(.system(size: 40))
                    .foregroundStyle(MUTheme.goldDim)
                Text(message)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            case .error(let message, let retry):
                Image(systemName: "wifi.exclamationmark")
                    .font(.system(size: 40))
                    .foregroundStyle(.secondary)
                Text(message)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                Button {
                    Haptics.tap()
                    retry()
                } label: {
                    Text(String(localized: "common.retry"))
                        .font(.subheadline.weight(.semibold))
                        .padding(.horizontal, 18)
                        .padding(.vertical, 8)
                        .background(MUTheme.gold, in: Capsule())
                        .foregroundStyle(.black)
                }
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 60)
    }
}

// AsyncImage 共通プレースホルダ (URLCache を大きめに使う設定は MUAPI 側)。
struct MUAsyncImage: View {
    let url: URL?
    var contentMode: ContentMode = .fill

    var body: some View {
        AsyncImage(url: url, transaction: Transaction(animation: .easeOut(duration: 0.25))) { phase in
            switch phase {
            case .success(let img):
                img.resizable().aspectRatio(contentMode: contentMode)
            case .failure:
                Rectangle().fill(MUTheme.card)
                    .overlay(Image(systemName: "tshirt").font(.title).foregroundStyle(.tertiary))
            default:
                Rectangle().fill(MUTheme.card)
                    .overlay(ShimmerOverlay())
            }
        }
    }
}

// 生成中・ロード中のシマー (金の帯が流れる)。
struct ShimmerOverlay: View {
    @State private var phase: CGFloat = -1

    var body: some View {
        GeometryReader { geo in
            LinearGradient(
                colors: [.clear, MUTheme.gold.opacity(0.18), .clear],
                startPoint: .leading, endPoint: .trailing
            )
            .frame(width: geo.size.width * 0.7)
            .offset(x: phase * geo.size.width * 1.6)
            .onAppear {
                withAnimation(.linear(duration: 1.4).repeatForever(autoreverses: false)) {
                    phase = 1
                }
            }
        }
        .clipped()
    }
}
