import SwiftUI

// ダブルタップ位置から金のハートが炸裂する 0.9 秒のパーティクル。
struct HeartBurst: Identifiable {
    let id = UUID()
    let point: CGPoint
}

struct HeartBurstView: View {
    let burst: HeartBurst
    var onFinished: () -> Void

    @State private var animate = false

    private struct Particle: Identifiable {
        let id = UUID()
        let angle: Double
        let distance: Double
        let size: Double
        let spin: Double
    }

    private let particles: [Particle] = (0..<14).map { i in
        Particle(
            angle: Double(i) / 14 * 2 * .pi + .random(in: -0.25...0.25),
            distance: .random(in: 60...140),
            size: .random(in: 12...24),
            spin: .random(in: -60...60)
        )
    }

    var body: some View {
        ZStack {
            // 中央のビッグハート: ポンと膨らんで消える
            Image(systemName: "heart.fill")
                .font(.system(size: 88))
                .foregroundStyle(Color.muGold)
                .scaleEffect(animate ? 1.35 : 0.3)
                .opacity(animate ? 0 : 0.95)

            // 放射状パーティクル
            ForEach(particles) { p in
                Image(systemName: "heart.fill")
                    .font(.system(size: p.size))
                    .foregroundStyle(Color.muGold)
                    .rotationEffect(.degrees(animate ? p.spin : 0))
                    .offset(
                        x: animate ? cos(p.angle) * p.distance : 0,
                        y: animate ? sin(p.angle) * p.distance : 0
                    )
                    .scaleEffect(animate ? 0.25 : 1.0)
                    .opacity(animate ? 0 : 1)
            }
        }
        .position(burst.point)
        .allowsHitTesting(false)
        .onAppear {
            withAnimation(.easeOut(duration: 0.8)) {
                animate = true
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.9) {
                onFinished()
            }
        }
    }
}
