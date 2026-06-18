import SwiftUI

// ⚡️ Pulse — 「ブランドの鼓動」。feed.json の新着 (created_at 降順 = API 既定順) を
// タイムラインで見せて、AI が毎時生み続けている「生きてる感」を可視化する。
// 60秒ごとに静かに再取得。
@MainActor
final class PulseViewModel: ObservableObject {
    @Published private(set) var products: [FeedProduct] = []
    @Published private(set) var loading = false
    @Published private(set) var errorMessage: String?

    func load() async {
        if products.isEmpty { loading = true }
        defer { loading = false }
        do {
            products = try await MUAPI.feed(page: 1)
            errorMessage = nil
        } catch {
            if products.isEmpty { errorMessage = error.localizedDescription }
        }
    }

    var latestDate: Date? {
        products.compactMap(\.createdDate).max()
    }

    var born24h: Int {
        let dayAgo = Date().addingTimeInterval(-24 * 3600)
        return products.compactMap(\.createdDate).filter { $0 > dayAgo }.count
    }
}

struct PulseView: View {
    @StateObject private var vm = PulseViewModel()
    @State private var detailProduct: FeedProduct?

    var body: some View {
        NavigationStack {
            Group {
                if vm.loading && vm.products.isEmpty {
                    ProgressView().tint(.muGold)
                } else if let error = vm.errorMessage {
                    VStack(spacing: 10) {
                        Text(String(localized: "feed.error.title")).font(.headline)
                        Text(error).font(.footnote).foregroundStyle(.secondary)
                        Button(String(localized: "feed.retry")) { Task { await vm.load() } }
                            .buttonStyle(.borderedProminent)
                            .foregroundStyle(.black)
                    }
                } else {
                    timeline
                }
            }
            .navigationTitle(String(localized: "tab.pulse"))
            .background(Color.black)
            .sheet(item: $detailProduct) { DetailSheetView(product: $0) }
        }
        .task {
            await vm.load()
            // 60秒ごとの鼓動 (タブを離れると .task ごとキャンセルされる)
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(60))
                await vm.load()
            }
        }
    }

    private var timeline: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                header
                    .padding(.bottom, 20)
                ForEach(Array(vm.products.prefix(30).enumerated()), id: \.element.sku) { index, product in
                    PulseRow(product: product, isFirst: index == 0) {
                        Haptics.medium()
                        detailProduct = product
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
        }
        .refreshable { await vm.load() }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 12) {
                PulsingDot()
                Text(String(localized: "pulse.alive"))
                    .font(.title2.bold())
            }
            Text(String(localized: "pulse.subtitle"))
                .font(.subheadline)
                .foregroundStyle(.secondary)

            HStack(spacing: 12) {
                statCard(
                    value: String(format: String(localized: "pulse.born24h %lld"), vm.born24h),
                    label: String(localized: "pulse.born24h.label")
                )
                if let latest = vm.latestDate {
                    statCard(
                        value: latest.formatted(.relative(presentation: .named)),
                        label: String(localized: "pulse.latestDrop")
                    )
                }
            }
        }
        .padding(.top, 8)
    }

    private func statCard(value: String, label: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(value)
                .font(.headline)
                .foregroundStyle(Color.muGold)
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(white: 0.09), in: RoundedRectangle(cornerRadius: 14))
    }
}

// 金の鼓動ドット (波紋が広がり続ける)
private struct PulsingDot: View {
    @State private var pulse = false

    var body: some View {
        ZStack {
            Circle()
                .stroke(Color.muGold.opacity(0.7), lineWidth: 2)
                .frame(width: 14, height: 14)
                .scaleEffect(pulse ? 2.6 : 1)
                .opacity(pulse ? 0 : 0.8)
            Circle()
                .fill(Color.muGold)
                .frame(width: 14, height: 14)
        }
        .onAppear {
            withAnimation(.easeOut(duration: 1.4).repeatForever(autoreverses: false)) {
                pulse = true
            }
        }
    }
}

private struct PulseRow: View {
    let product: FeedProduct
    let isFirst: Bool
    var onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            HStack(alignment: .top, spacing: 14) {
                // タイムラインの軸
                VStack(spacing: 0) {
                    Circle()
                        .fill(isFirst ? Color.muGold : Color(white: 0.3))
                        .frame(width: 8, height: 8)
                        .padding(.top, 26)
                    Rectangle()
                        .fill(Color(white: 0.18))
                        .frame(width: 1.5)
                        .frame(maxHeight: .infinity)
                }
                .frame(width: 8)

                AsyncImage(url: product.mockupURL) { phase in
                    if case .success(let image) = phase {
                        image.resizable().scaledToFill()
                    } else {
                        Rectangle().fill(.quaternary)
                    }
                }
                .frame(width: 64, height: 64)
                .clipShape(RoundedRectangle(cornerRadius: 10))

                VStack(alignment: .leading, spacing: 4) {
                    if let age = product.relativeAgeLabel {
                        Text(String(format: String(localized: "feed.generated %@"), age))
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(isFirst ? Color.muGold : .secondary)
                    }
                    Text(product.displayTitle)
                        .font(.subheadline)
                        .foregroundStyle(.primary)
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)
                    HStack(spacing: 8) {
                        Text(product.brand.uppercased())
                            .font(.caption2.weight(.bold))
                            .tracking(1.5)
                            .foregroundStyle(.tertiary)
                        Text(product.priceLabel)
                            .font(.caption.weight(.bold))
                            .foregroundStyle(Color.muGold)
                    }
                }
                Spacer(minLength: 0)
            }
            .padding(.bottom, 18)
        }
        .buttonStyle(.plain)
    }
}
