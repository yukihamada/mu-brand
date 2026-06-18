import SwiftUI

// 生成結果。デザイン画像を即見せ、着用イメージ (on-body mockup) は
// /api/make/peek を 6 秒間隔でポーリングして完成したらクロスフェード。
// 購入 = 既存 Stripe Checkout (checkout_url) を SafariView で。
struct MakeResultView: View {
    let result: MakeResult
    let prompt: String
    let onClose: () -> Void

    @EnvironmentObject private var history: MakeHistory
    @State private var mockupURL: URL?
    @State private var mockupReady = false
    @State private var safariURL: URL?
    @State private var appeared = false

    var body: some View {
        NavigationStack {
            ZStack {
                MUTheme.bg.ignoresSafeArea()
                ScrollView {
                    VStack(alignment: .leading, spacing: 18) {
                        heroImage
                        titleBlock
                        noteBlock
                        actions
                    }
                    .padding(.horizontal, 20)
                    .padding(.bottom, 40)
                }
            }
            .navigationTitle(result.isLive
                ? String(localized: "result.title")
                : String(localized: "result.reviewTitle"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        Haptics.tap()
                        onClose()
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                }
                if let url = URL(string: result.pdpUrl) {
                    ToolbarItem(placement: .topBarLeading) {
                        ShareLink(item: url) {
                            Image(systemName: "square.and.arrow.up")
                        }
                    }
                }
            }
        }
        .sheet(item: $safariURL) { url in
            SafariView(url: url).ignoresSafeArea()
        }
        .task { await pollMockup() }
        .onAppear {
            withAnimation(.spring(duration: 0.6).delay(0.1)) { appeared = true }
        }
    }

    // MARK: - pieces

    private var heroImage: some View {
        ZStack {
            MUAsyncImage(url: mockupURL ?? result.designURL, contentMode: .fit)
                .id(mockupURL?.absoluteString ?? result.designUrl) // 差し替えでリロード
                .frame(maxWidth: .infinity)
                .frame(minHeight: 320)
                .background(MUTheme.card)
                .clipShape(RoundedRectangle(cornerRadius: 20))
                .overlay(
                    RoundedRectangle(cornerRadius: 20)
                        .strokeBorder(MUTheme.goldGradient, lineWidth: 1)
                )
                .shadow(color: MUTheme.gold.opacity(0.18), radius: 24)
        }
        .scaleEffect(appeared ? 1 : 0.92)
        .opacity(appeared ? 1 : 0)
        .overlay(alignment: .bottomTrailing) {
            mockupBadge.padding(10)
        }
        .animation(.easeInOut(duration: 0.45), value: mockupURL)
    }

    private var mockupBadge: some View {
        HStack(spacing: 5) {
            if mockupReady {
                Image(systemName: "checkmark.seal.fill")
                Text(String(localized: "result.mockupReady"))
            } else {
                ProgressView().controlSize(.mini).tint(.black)
                Text(String(localized: "result.mockupPending"))
            }
        }
        .font(.caption2.weight(.semibold))
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(mockupReady ? AnyShapeStyle(MUTheme.gold) : AnyShapeStyle(.ultraThinMaterial), in: Capsule())
        .foregroundStyle(mockupReady ? .black : .primary)
    }

    private var titleBlock: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(result.display)
                .font(.title.bold())
                .foregroundStyle(MUTheme.goldGradient)
            Text(result.hook)
                .font(.subheadline)
                .foregroundStyle(.secondary)
            HStack {
                Text(result.priceLabel)
                    .font(.title2.bold())
                Spacer()
                Text(result.kind.uppercased())
                    .font(.caption.weight(.semibold))
                    .padding(.horizontal, 10)
                    .padding(.vertical, 5)
                    .background(MUTheme.card, in: Capsule())
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var noteBlock: some View {
        Text(result.note)
            .font(.footnote)
            .foregroundStyle(.secondary)
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(MUTheme.card, in: RoundedRectangle(cornerRadius: 14))
    }

    private var actions: some View {
        VStack(spacing: 12) {
            if result.isLive, let checkout = result.checkoutUrl, let url = URL(string: checkout) {
                Button {
                    Haptics.rigid()
                    safariURL = url
                } label: {
                    Text(String(localized: "result.buy"))
                        .font(.headline)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 15)
                        .background(MUTheme.goldGradient, in: Capsule())
                        .foregroundStyle(.black)
                }
            } else if !result.isLive {
                Label(String(localized: "result.reviewBody"), systemImage: "hourglass")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }

            HStack(spacing: 12) {
                if let url = URL(string: result.pdpUrl), result.isLive {
                    secondaryButton(String(localized: "result.openPDP"), icon: "safari") {
                        safariURL = url
                    }
                }
                if let url = URL(string: result.editUrl) {
                    secondaryButton(String(localized: "result.edit"), icon: "slider.horizontal.3") {
                        safariURL = url
                    }
                }
            }

            Button {
                Haptics.tap()
                onClose()
            } label: {
                Text(String(localized: "result.again"))
                    .font(.subheadline.weight(.medium))
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 12)
            }
            .foregroundStyle(MUTheme.gold)
        }
        .padding(.top, 4)
    }

    private func secondaryButton(_ title: String, icon: String, action: @escaping () -> Void) -> some View {
        Button {
            Haptics.tap()
            action()
        } label: {
            Label(title, systemImage: icon)
                .font(.subheadline.weight(.medium))
                .frame(maxWidth: .infinity)
                .padding(.vertical, 13)
                .background(MUTheme.card, in: Capsule())
                .overlay(Capsule().strokeBorder(MUTheme.cardBorder, lineWidth: 1))
                .foregroundStyle(.primary)
        }
    }

    // MARK: - mockup polling (max-age=5 の軽量 API・約4分で打ち切り)

    private func pollMockup() async {
        for _ in 0..<40 {
            if Task.isCancelled || mockupReady { return }
            if let peek = try? await MUAPI.peek(sku: result.sku),
               let mockup = peek.mockup, !mockup.isEmpty, let url = URL(string: mockup) {
                mockupURL = url
                mockupReady = true
                history.updateMockup(sku: result.sku, mockupUrl: mockup)
                Haptics.success()
                return
            }
            try? await Task.sleep(for: .seconds(6))
        }
    }
}

// sheet(item:) で URL を直接使うための準拠
extension URL: Identifiable {
    public var id: String { absoluteString }
}
