import SwiftUI

// 美学の単一ソース。墨と紙の二極、金は針の先ほど。
enum Atelier {
    /// MU gold #e6c449 — 売れた点・保存済みハートなど、極小の合図にだけ使う。
    static let gold = Color(red: 230 / 255, green: 196 / 255, blue: 73 / 255)

    /// 紙 — ライト: オフホワイト / ダーク: 墨に近い黒。
    static let paper = Color(uiColor: UIColor { trait in
        trait.userInterfaceStyle == .dark
            ? UIColor(red: 0.043, green: 0.043, blue: 0.039, alpha: 1) // #0B0B0A
            : UIColor(red: 0.980, green: 0.976, blue: 0.965, alpha: 1) // #FAF9F6
    })

    /// 罫線は使わない。使うときは髪の毛一本。
    static let hairline = Color.primary.opacity(0.12)

    static let spring = Animation.spring(response: 0.42, dampingFraction: 0.86)
}

// MARK: - Typography

extension View {
    /// 小さな見出し語 (EYEBROW)。広いトラッキング、控えめな存在感。
    func eyebrow() -> some View {
        font(.caption2.weight(.medium))
            .tracking(2.4)
            .foregroundStyle(.secondary)
    }

    /// セリフ見出し (New York)。ブランドの声。
    func serif(_ style: Font.TextStyle, weight: Font.Weight = .medium) -> some View {
        font(.system(style, design: .serif).weight(weight))
    }
}

// MARK: - Atoms

struct Hairline: View {
    var body: some View {
        Rectangle().fill(Atelier.hairline).frame(height: 0.5)
    }
}

/// スケルトンの呼吸。派手にしない。
struct Pulse: ViewModifier {
    @State private var dim = false

    func body(content: Content) -> some View {
        content
            .opacity(dim ? 0.35 : 0.7)
            .animation(.easeInOut(duration: 0.9).repeatForever(autoreverses: true), value: dim)
            .onAppear { dim = true }
    }
}

/// 商品画像。読み込み中は静かな面、失敗しても世界観を壊さない。
struct ProductImage: View {
    let url: URL?

    var body: some View {
        AsyncImage(url: url) { phase in
            switch phase {
            case .success(let image):
                image.resizable().scaledToFill()
            case .failure:
                Rectangle()
                    .fill(Color.primary.opacity(0.04))
                    .overlay {
                        Image(systemName: "circle.dotted")
                            .font(.title3)
                            .foregroundStyle(.tertiary)
                    }
            default:
                Rectangle()
                    .fill(Color.primary.opacity(0.05))
                    .modifier(Pulse())
            }
        }
    }
}

/// 主ボタン。墨ベタ・大文字・広いトラッキング。
struct PrimaryButtonStyle: ButtonStyle {
    @Environment(\.colorScheme) private var scheme

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.footnote.weight(.semibold))
            .tracking(2.0)
            .frame(maxWidth: .infinity)
            .frame(height: 52)
            .background(Color.primary)
            .foregroundStyle(scheme == .dark ? Color.black : Color.white)
            .opacity(configuration.isPressed ? 0.75 : 1)
    }
}

/// 副ボタン。髪の毛一本の枠。
struct HairlineButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.footnote.weight(.medium))
            .tracking(2.0)
            .frame(maxWidth: .infinity)
            .frame(height: 52)
            .overlay(Rectangle().strokeBorder(Atelier.hairline, lineWidth: 1))
            .foregroundStyle(.primary)
            .opacity(configuration.isPressed ? 0.6 : 1)
    }
}
