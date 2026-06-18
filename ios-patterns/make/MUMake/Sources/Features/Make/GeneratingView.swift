import SwiftUI

// 生成中のフルスクリーン演出。金の粒子が立ちのぼり、工程テキストが移ろう。
// 実際の /api/make は同期で 20〜90 秒かかるので「待つ時間そのものを体験」にする。
struct GeneratingView: View {
    let prompt: String
    let onCancel: () -> Void

    @State private var stepIndex = 0
    @State private var pulse = false

    private var steps: [String] {
        [
            String(localized: "make.gen.1"),
            String(localized: "make.gen.2"),
            String(localized: "make.gen.3"),
            String(localized: "make.gen.4"),
        ]
    }

    var body: some View {
        ZStack {
            MUTheme.bg.ignoresSafeArea()

            ParticleField()
                .ignoresSafeArea()
                .allowsHitTesting(false)

            VStack(spacing: 28) {
                Spacer()

                Image(systemName: "moon.fill")
                    .font(.system(size: 44))
                    .foregroundStyle(MUTheme.goldGradient)
                    .scaleEffect(pulse ? 1.12 : 0.94)
                    .shadow(color: MUTheme.gold.opacity(0.6), radius: pulse ? 26 : 10)
                    .animation(.easeInOut(duration: 1.2).repeatForever(autoreverses: true), value: pulse)

                VStack(spacing: 10) {
                    Text("“\(prompt)”")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                        .lineLimit(3)
                        .padding(.horizontal, 36)

                    Text(steps[stepIndex])
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(MUTheme.gold)
                        .id(stepIndex)
                        .transition(.asymmetric(
                            insertion: .move(edge: .bottom).combined(with: .opacity),
                            removal: .move(edge: .top).combined(with: .opacity)
                        ))
                        .overlay(ShimmerOverlay().clipShape(Capsule()))
                }
                .animation(.spring(duration: 0.5), value: stepIndex)

                Spacer()

                Button {
                    Haptics.tap()
                    onCancel()
                } label: {
                    Text(String(localized: "common.cancel"))
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .padding(.vertical, 10)
                        .padding(.horizontal, 24)
                        .background(MUTheme.card, in: Capsule())
                }
                .padding(.bottom, 36)
            }
        }
        .onAppear { pulse = true }
        .task {
            // 工程テキストの移ろい (実工程と同期はしない正直演出: 最後の項目で待つ)
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(5))
                if stepIndex < steps.count - 1 {
                    stepIndex += 1
                }
            }
        }
    }
}

// 金の粒子が下から立ちのぼる Canvas。TimelineView 駆動・決定論的擬似乱数。
struct ParticleField: View {
    private let count = 46

    var body: some View {
        TimelineView(.animation(minimumInterval: 1.0 / 30.0)) { timeline in
            Canvas { context, size in
                let t = timeline.date.timeIntervalSinceReferenceDate
                for i in 0..<count {
                    let seed = Double(i)
                    let speed = 0.03 + frac(seed * 0.137) * 0.06
                    let progress = frac(t * speed + frac(seed * 0.911))
                    let x = (frac(seed * 0.371) + sin(t * 0.4 + seed) * 0.03) * size.width
                    let y = size.height * (1.05 - progress * 1.1)
                    let r = 1.2 + frac(seed * 0.531) * 2.6
                    // 出現直後と消える直前はフェード
                    let fade = min(progress * 4, (1 - progress) * 3, 1)
                    let alpha = fade * (0.25 + frac(seed * 0.713) * 0.5)
                    let rect = CGRect(x: x - r, y: y - r, width: r * 2, height: r * 2)
                    context.fill(
                        Circle().path(in: rect),
                        with: .color(MUTheme.gold.opacity(alpha))
                    )
                }
            }
        }
    }

    private func frac(_ v: Double) -> Double { v - v.rounded(.down) }
}
