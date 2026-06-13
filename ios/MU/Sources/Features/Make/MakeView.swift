import SwiftUI

// 「言えば、作れる」— MU の背骨。ひとこと打つと AI がデザインを起こし、即棚に並ぶ。
// 生成は POST /api/make。結果は design 画像 + 名前 + ひとこと + 購入導線。
struct MakeView: View {
    @EnvironmentObject private var session: Session

    @State private var prompt = ""
    @State private var kind: MakeKind = .auto
    @State private var isMaking = false
    @State private var result: MakeResult?
    @State private var errorMessage: String?
    @State private var showCheckout = false
    @FocusState private var promptFocused: Bool

    // 「磨く」(5軸自己改善) の状態
    @State private var isPolishing = false
    @State private var polishedURL: URL?     // 磨いた後の差し替え画像
    @State private var score: DesignScore?   // 最新スコア (after)
    @State private var polishNote: String?

    // 作った直後 = 高意欲の瞬間に1回だけ通知許可を促す
    @AppStorage("didPromptPushAfterMake") private var didPromptPush = false

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    header

                    // 入力
                    VStack(alignment: .leading, spacing: 12) {
                        TextField(String(localized: "make.placeholder"), text: $prompt, axis: .vertical)
                            .lineLimit(2...5)
                            .textFieldStyle(.plain)
                            .padding(12)
                            .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 12))
                            .focused($promptFocused)
                            .disabled(isMaking)

                        // 種類チップ (おまかせ既定)
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(spacing: 8) {
                                ForEach(MakeKind.allCases) { k in
                                    chip(k)
                                }
                            }
                        }

                        Button(action: make) {
                            HStack {
                                if isMaking {
                                    ProgressView().tint(.black)
                                    Text(String(localized: "make.making"))
                                } else {
                                    Image(systemName: "sparkles")
                                    Text(String(localized: "make.go"))
                                }
                            }
                            .font(.headline)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 6)
                        }
                        .buttonStyle(.borderedProminent)
                        .foregroundStyle(.black)
                        .disabled(isMaking || prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }

                    if let errorMessage {
                        Label(errorMessage, systemImage: "exclamationmark.triangle")
                            .font(.footnote)
                            .foregroundStyle(.red)
                    }

                    if let result {
                        resultCard(result)
                    } else if !isMaking {
                        hints
                    }
                }
                .padding()
            }
            .navigationTitle(String(localized: "tab.make"))
            .task { Analytics.track("view_make") }
            .sheet(isPresented: $showCheckout) {
                if let s = result?.checkoutUrl, let url = URL(string: s) {
                    SafariView(url: url).ignoresSafeArea()
                }
            }
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(String(localized: "make.title"))
                .font(.title2.bold())
            Text(String(localized: "make.subtitle"))
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
    }

    private func chip(_ k: MakeKind) -> some View {
        let selected = k == kind
        return Button {
            kind = k
        } label: {
            Text(k.label)
                .font(.subheadline.weight(selected ? .bold : .regular))
                .padding(.horizontal, 14).padding(.vertical, 8)
                .background(selected ? AnyShapeStyle(.tint) : AnyShapeStyle(.quaternary.opacity(0.4)),
                            in: Capsule())
                .foregroundStyle(selected ? .black : .primary)
        }
        .disabled(isMaking)
    }

    @ViewBuilder
    private func resultCard(_ r: MakeResult) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            ZStack {
                AsyncImage(url: polishedURL ?? r.designURL) { phase in
                    switch phase {
                    case .success(let img): img.resizable().scaledToFit()
                    default: Rectangle().fill(.quaternary).frame(height: 320)
                    }
                }
                .frame(maxWidth: .infinity)
                .clipShape(RoundedRectangle(cornerRadius: 16))
                .opacity(isPolishing ? 0.4 : 1)

                if isPolishing {
                    VStack(spacing: 8) {
                        ProgressView().tint(.white)
                        Text(String(localized: "make.polishing"))
                            .font(.footnote).foregroundStyle(.white)
                    }
                }
            }
            .id(polishedURL?.absoluteString ?? r.designUrl) // 差し替え時に再描画

            Text(r.display.uppercased())
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            Text(r.hook)
                .font(.title3.weight(.medium))

            // MUスコア (5軸)。磨くと更新される。
            if let s = score {
                scoreView(s)
            }

            Text(polishNote ?? r.note)
                .font(.footnote)
                .foregroundStyle(.secondary)

            HStack {
                Text(r.priceLabel).font(.title2.bold())
                Spacer()
            }

            // 磨く — 5軸で自己改善 (AI生成・自動承認の live 商品のみ)。
            if r.editToken != nil && r.autoApproved {
                Button {
                    polish(r)
                } label: {
                    HStack {
                        Image(systemName: "wand.and.stars")
                        Text(isPolishing ? String(localized: "make.polishing") : String(localized: "make.polish"))
                    }
                    .font(.headline)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 6)
                }
                .buttonStyle(.bordered)
                .tint(.yellow)
                .disabled(isPolishing)
            }

            // 自動承認なら即購入。要審査(flagged)は checkout_url が null。
            if r.checkoutUrl != nil {
                Button {
                    Analytics.track("make_buy", ["sku": r.sku])
                    showCheckout = true
                } label: {
                    Text(String(localized: "pdp.buy"))
                        .font(.headline)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 6)
                }
                .buttonStyle(.borderedProminent)
                .foregroundStyle(.black)
                .disabled(isPolishing)
            } else {
                Label(String(localized: "make.reviewPending"), systemImage: "clock")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            Link(destination: URL(string: r.pdpUrl)!) {
                Label(String(localized: "pdp.openWeb"), systemImage: "safari")
                    .font(.subheadline)
                    .frame(maxWidth: .infinity)
            }

            Button(String(localized: "make.again")) {
                withAnimation {
                    result = nil
                    polishedURL = nil
                    score = nil
                    polishNote = nil
                }
                prompt = ""
                promptFocused = true
            }
            .font(.subheadline)
            .frame(maxWidth: .infinity)
            .padding(.top, 4)
        }
        .padding(16)
        .background(.quaternary.opacity(0.25), in: RoundedRectangle(cornerRadius: 20))
    }

    private var hints: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(String(localized: "make.examplesTitle"))
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.secondary)
            ForEach(exampleKeys, id: \.self) { key in
                Button {
                    prompt = String(localized: String.LocalizationValue(key))
                    promptFocused = true
                } label: {
                    HStack {
                        Image(systemName: "text.bubble").font(.caption)
                        Text(String(localized: String.LocalizationValue(key)))
                            .multilineTextAlignment(.leading)
                        Spacer()
                    }
                    .font(.subheadline)
                    .padding(12)
                    .background(.quaternary.opacity(0.3), in: RoundedRectangle(cornerRadius: 12))
                }
                .buttonStyle(.plain)
            }
        }
    }

    private let exampleKeys = ["make.example1", "make.example2", "make.example3"]

    // MUスコア表示 (5軸 + 合計バッジ)
    @ViewBuilder
    private func scoreView(_ s: DesignScore) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Text("MU \(s.total)")
                    .font(.subheadline.bold())
                    .padding(.horizontal, 10).padding(.vertical, 4)
                    .background(.tint, in: Capsule())
                    .foregroundStyle(.black)
                Text(s.verdict)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            // 各軸 0-20 のバー
            ForEach(s.orderedAxes, id: \.0) { axis, v in
                HStack(spacing: 8) {
                    Text(DesignScore.axisLabel(axis))
                        .font(.caption2)
                        .frame(width: 56, alignment: .leading)
                        .foregroundStyle(.secondary)
                    GeometryReader { geo in
                        ZStack(alignment: .leading) {
                            Capsule().fill(.quaternary.opacity(0.5))
                            Capsule().fill(.tint)
                                .frame(width: geo.size.width * CGFloat(v) / 20.0)
                        }
                    }
                    .frame(height: 6)
                    Text("\(v)").font(.caption2.monospacedDigit()).foregroundStyle(.secondary)
                        .frame(width: 18, alignment: .trailing)
                }
            }
        }
        .padding(10)
        .background(.quaternary.opacity(0.2), in: RoundedRectangle(cornerRadius: 12))
    }

    private func polish(_ r: MakeResult) {
        guard let token = r.editToken, !isPolishing else { return }
        errorMessage = nil
        isPolishing = true
        Task {
            do {
                let res = try await MUAPI.polish(sku: r.sku, editToken: token)
                Analytics.track("make_polish", ["sku": r.sku, "improved": res.improved])
                await MainActor.run {
                    withAnimation {
                        if res.improved, let url = res.designURL {
                            polishedURL = url
                            score = res.after
                        } else {
                            // 据え置き: 現状スコアは before(=after の最高試行) を見せる
                            score = res.after ?? res.before
                        }
                        polishNote = res.note
                    }
                    isPolishing = false
                }
            } catch {
                await MainActor.run {
                    errorMessage = error.localizedDescription
                    isPolishing = false
                }
            }
        }
    }

    // 作った直後に1回だけ「通知オン?」を促す (ドロップ/売れた通知に繋ぐ)。
    private func maybePromptPush() {
        guard !didPromptPush else { return }
        didPromptPush = true
        Task {
            if await PushManager.status() == .notDetermined {
                let ok = await PushManager.enable()
                Analytics.track("push_enable", ["ok": ok, "at": "after_make"])
            }
        }
    }

    private func make() {
        let text = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        promptFocused = false
        errorMessage = nil
        isMaking = true
        Task {
            do {
                let r = try await MUAPI.make(prompt: text, kind: kind, apiKey: session.apiKey)
                Analytics.track("make_create", ["kind": r.kind, "sku": r.sku])
                await MainActor.run {
                    withAnimation {
                        // 新しい作品 → 磨き状態をリセット
                        polishedURL = nil
                        score = nil
                        polishNote = nil
                        result = r
                    }
                    isMaking = false
                    maybePromptPush()
                }
            } catch {
                await MainActor.run {
                    errorMessage = error.localizedDescription
                    isMaking = false
                }
            }
        }
    }
}
