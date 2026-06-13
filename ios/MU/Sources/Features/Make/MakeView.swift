import SwiftUI
import UIKit

// 「言えば、作れる」— MU の背骨。ひとこと打つと AI がデザインを起こし、即棚に並ぶ。
// 生成は POST /api/make。結果は design 画像 + 名前 + ひとこと + 購入導線。
struct MakeView: View {
    @EnvironmentObject private var session: Session
    @EnvironmentObject private var app: AppState

    @StateObject private var voice = VoiceInput()
    @State private var prompt = ""
    @State private var kind: MakeKind = .auto
    @State private var royalty = 10          // 印税 10〜50%(価格は自動調整)
    @State private var isMaking = false
    @State private var result: MakeResult?
    @State private var errorMessage: String?
    @State private var showCheckout = false
    @FocusState private var promptFocused: Bool

    // 作っている間の演出
    @State private var makingStep = 0
    @State private var revealed = false      // 完成リビールのアニメ

    // 着画(オンボディ mockup)。ポーリングで出来たら差し替え。
    @State private var mockupURL: URL?

    // リミックス(続きを作る)の状態
    @State private var showRemix = false
    @State private var remixWords = ""
    @State private var isRemixing = false

    // 「磨く」(5軸自己改善) の状態
    @State private var isPolishing = false
    @State private var polishedURL: URL?     // 磨いた後の差し替え画像
    @State private var score: DesignScore?   // 最新スコア (after)
    @State private var polishNote: String?

    // 作った直後 = 高意欲の瞬間に1回だけ通知許可を促す
    @AppStorage("didPromptPushAfterMake") private var didPromptPush = false

    // 作っている間に流す“作ってる感”メッセージ
    private let makingSteps = ["make.step1", "make.step2", "make.step3", "make.step4", "make.step5"]

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    header

                    // 入力
                    VStack(alignment: .leading, spacing: 12) {
                        HStack(alignment: .bottom, spacing: 8) {
                            TextField(String(localized: "make.placeholder"), text: $prompt, axis: .vertical)
                                .lineLimit(2...5)
                                .textFieldStyle(.plain)
                                .padding(12)
                                .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 12))
                                .focused($promptFocused)
                                .disabled(isMaking)
                            // 声で作る
                            Button {
                                Task {
                                    Analytics.track("voice_toggle")
                                    await voice.toggle()
                                }
                            } label: {
                                Image(systemName: voice.isRecording ? "waveform.circle.fill" : "mic.circle.fill")
                                    .font(.system(size: 38))
                                    .foregroundStyle(voice.isRecording ? AnyShapeStyle(.red) : AnyShapeStyle(.tint))
                                    .symbolEffect(.pulse, isActive: voice.isRecording)
                            }
                            .disabled(isMaking)
                        }
                        if voice.isRecording {
                            Label(String(localized: "make.listening"), systemImage: "waveform")
                                .font(.caption).foregroundStyle(.red)
                        } else if voice.denied {
                            Text(String(localized: "make.voiceDenied"))
                                .font(.caption).foregroundStyle(.secondary)
                        }

                        // 種類チップ (おまかせ既定)
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(spacing: 8) {
                                ForEach(MakeKind.allCases) { k in
                                    chip(k)
                                }
                            }
                        }

                        royaltyPicker

                        Button(action: { make() }) {
                            HStack {
                                Image(systemName: "sparkles")
                                Text(String(localized: "make.go"))
                            }
                            .font(.headline)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 6)
                        }
                        .buttonStyle(.borderedProminent)
                        .foregroundStyle(.black)
                        .disabled(isMaking || prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                    .disabled(isMaking)
                    .opacity(isMaking ? 0.5 : 1)

                    if let errorMessage {
                        Label(errorMessage, systemImage: "exclamationmark.triangle")
                            .font(.footnote)
                            .foregroundStyle(.red)
                    }

                    if isMaking {
                        makingView
                    } else if let result {
                        resultCard(result)
                    } else {
                        hints
                    }
                }
                .padding()
            }
            .navigationTitle(String(localized: "tab.make"))
            .task { Analytics.track("view_make") }
            // 声で作る: 認識テキストをそのまま入力に流す
            .onChange(of: voice.transcript) { _, t in
                if !t.isEmpty { prompt = t }
            }
            // オンボーディングからの「最初の一着」を受け取り、その場で自動生成。
            .onChange(of: app.pendingPrompt) { _, new in
                guard let p = new else { return }
                app.pendingPrompt = nil
                prompt = p
                make()
            }
            .onAppear {
                if let p = app.pendingPrompt {
                    app.pendingPrompt = nil
                    prompt = p
                    make()
                }
            }
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

    // 印税 10〜50%。上げるほど価格が自動で上がり、あなたの取り分が増える。
    private var royaltyPicker: some View {
        let base = basePrice(kind)
        let price = adjustedPrice(base, royalty)
        let earn = price * royalty / 100
        return VStack(alignment: .leading, spacing: 6) {
            HStack {
                Label(String(localized: "make.royalty"), systemImage: "yensign.circle")
                    .font(.subheadline.weight(.medium))
                Spacer()
                Text("\(royalty)%").font(.subheadline.bold()).foregroundStyle(.tint)
            }
            Slider(value: Binding(
                get: { Double(royalty) },
                set: { royalty = min(50, max(10, Int(($0 / 10).rounded()) * 10)) }
            ), in: 10...50, step: 10)
            // 価格と取り分は自動連動(おまかせ時は目安)
            HStack(spacing: 4) {
                Text(String(format: String(localized: "make.priceLine"), "¥\(price.formatted())"))
                Text("·").foregroundStyle(.tertiary)
                Text(String(format: String(localized: "make.earnLine"), "¥\(earn.formatted())"))
                    .foregroundStyle(.tint)
                if kind == .auto {
                    Text(String(localized: "make.estimate")).foregroundStyle(.tertiary)
                }
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .padding(10)
        .background(.quaternary.opacity(0.25), in: RoundedRectangle(cornerRadius: 12))
    }

    // 種類別の基準価格(印税プレビュー用。サーバの spec.retail_jpy と一致)。
    private func basePrice(_ k: MakeKind) -> Int {
        switch k {
        case .auto, .tee: return 4900
        case .hoodie: return 8800
        case .sticker: return 800
        case .rashguard: return 9800
        case .tote: return 2900
        case .mug: return 2200
        }
    }
    // サーバ royalty_adjusted_price と同式: base * 0.9 / (1 - pct/100)、100円丸め。
    private func adjustedPrice(_ base: Int, _ pct: Int) -> Int {
        let factor = 0.9 / (1.0 - Double(pct) / 100.0)
        let raw = Int((Double(base) * factor).rounded())
        return min(max(((raw + 50) / 100) * 100, base), 99_000)
    }

    // 作っている間の“作ってる感”。本当に何かが起きている手応えを出す。
    private var makingView: some View {
        VStack(spacing: 18) {
            ZStack {
                RoundedRectangle(cornerRadius: 20)
                    .fill(.quaternary.opacity(0.3))
                    .frame(height: 300)
                    .shimmering()
                VStack(spacing: 14) {
                    Image(systemName: "wand.and.stars")
                        .font(.system(size: 40))
                        .foregroundStyle(.tint)
                        .symbolEffect(.variableColor.iterative, options: .repeating)
                    Text(String(localized: String.LocalizationValue(makingSteps[makingStep % makingSteps.count])))
                        .font(.callout.weight(.medium))
                        .foregroundStyle(.secondary)
                        .transition(.opacity)
                        .id(makingStep)
                }
            }
            // 進捗ドット
            HStack(spacing: 6) {
                ForEach(0..<makingSteps.count, id: \.self) { i in
                    Circle()
                        .fill(i <= makingStep % makingSteps.count ? AnyShapeStyle(.tint) : AnyShapeStyle(.quaternary))
                        .frame(width: 6, height: 6)
                }
            }
        }
        .task {
            // 1.4秒ごとにメッセージを進める(完成まで巡回)
            while isMaking {
                try? await Task.sleep(nanoseconds: 1_400_000_000)
                withAnimation { makingStep += 1 }
            }
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
        // 表示画像: 磨いた絵 > 着画(オンボディ) > 元デザイン
        let shownURL = polishedURL ?? mockupURL ?? r.designURL
        VStack(alignment: .leading, spacing: 14) {
            // 完成バナー(テンション上げ)
            Label(String(localized: "make.done"), systemImage: "checkmark.seal.fill")
                .font(.headline)
                .foregroundStyle(.tint)
                .scaleEffect(revealed ? 1 : 0.6)
                .opacity(revealed ? 1 : 0)

            ZStack {
                AsyncImage(url: shownURL) { phase in
                    switch phase {
                    case .success(let img): img.resizable().scaledToFit()
                    default: Rectangle().fill(.quaternary).frame(height: 320)
                    }
                }
                .frame(maxWidth: .infinity)
                .clipShape(RoundedRectangle(cornerRadius: 16))
                .opacity(isPolishing ? 0.4 : 1)
                .scaleEffect(revealed ? 1 : 0.92)

                if isPolishing {
                    VStack(spacing: 8) {
                        ProgressView().tint(.white)
                        Text(String(localized: "make.polishing"))
                            .font(.footnote).foregroundStyle(.white)
                    }
                }
                // 着画ができたら右上にバッジ
                if mockupURL != nil && polishedURL == nil {
                    VStack { HStack {
                        Spacer()
                        Text(String(localized: "make.onbody"))
                            .font(.caption2.weight(.bold))
                            .padding(.horizontal, 8).padding(.vertical, 4)
                            .background(.black.opacity(0.6), in: Capsule())
                            .foregroundStyle(.white)
                            .padding(8)
                    }; Spacer() }
                }
            }
            .id(shownURL?.absoluteString ?? r.designUrl) // 差し替え時に再描画

            Text(r.display.uppercased())
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            Text(r.hook)
                .font(.title3.weight(.medium))

            // MUスコア (5軸)。磨くと更新される。
            if let s = score {
                scoreView(s)
            }

            HStack {
                Text(r.priceLabel).font(.title2.bold())
                Spacer()
            }

            // 出品済み + 印税: 「もう棚に並んだ。売れたら R% (約¥Y) があなたに」
            if r.autoApproved, let pct = r.makerPct, let earn = r.makerEarnJpy {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: "bag.badge.plus").foregroundStyle(.tint)
                    Text(String(format: String(localized: "make.listedEarn"), pct, "¥\(earn.formatted())"))
                        .font(.footnote)
                }
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 12))
            }

            Text(polishNote ?? r.note)
                .font(.caption)
                .foregroundStyle(.secondary)

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

            // 続きを、誰かと作る(リミックス)。元の作者には印税5%が流れる。
            if r.editToken != nil && r.autoApproved {
                remixSection(r)
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
                    mockupURL = nil
                    score = nil
                    polishNote = nil
                    revealed = false
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
            // 🥋 道場グッズ プリセット(戦略: BJJ垂直の実需。言うだけでチーム公式グッズ)
            Button {
                prompt = String(localized: "make.bjj.template")
                kind = .rashguard
                promptFocused = true
                Analytics.track("make_preset", ["preset": "bjj_dojo"])
            } label: {
                HStack {
                    Text("🥋").font(.title3)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(String(localized: "make.bjj.title")).font(.subheadline.weight(.semibold))
                        Text(String(localized: "make.bjj.sub")).font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Image(systemName: "chevron.right").font(.caption).foregroundStyle(.tertiary)
                }
                .padding(12)
                .frame(maxWidth: .infinity)
                .background(.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 12))
            }
            .buttonStyle(.plain)

            Text(String(localized: "make.examplesTitle"))
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.top, 4)
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

    // 「続きを作る」— 一言足して別バージョンを織る。完成したらそれが新しい result に。
    @ViewBuilder
    private func remixSection(_ r: MakeResult) -> some View {
        VStack(spacing: 8) {
            if showRemix {
                HStack(spacing: 8) {
                    TextField(String(localized: "make.remix.placeholder"), text: $remixWords)
                        .textFieldStyle(.plain)
                        .padding(10)
                        .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 10))
                        .disabled(isRemixing)
                    Button {
                        remix(r)
                    } label: {
                        if isRemixing { ProgressView() }
                        else { Text(String(localized: "make.remix.go")).font(.subheadline.bold()) }
                    }
                    .disabled(isRemixing || remixWords.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                Text(String(localized: "make.remix.royalty"))
                    .font(.caption2).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                Button {
                    withAnimation { showRemix = true }
                } label: {
                    Label(String(localized: "make.remix"), systemImage: "arrow.triangle.branch")
                        .font(.subheadline)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 4)
                }
                .buttonStyle(.bordered)
            }
        }
    }

    private func remix(_ r: MakeResult) {
        let words = remixWords.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !words.isEmpty, !isRemixing else { return }
        isRemixing = true
        Task {
            do {
                let nr = try await MUAPI.remix(sku: r.sku, words: words, apiKey: session.apiKey)
                Analytics.track("make_remix", ["from": r.sku, "to": nr.sku])
                await MainActor.run {
                    isRemixing = false
                    showRemix = false
                    remixWords = ""
                    polishedURL = nil
                    mockupURL = nil
                    score = nil
                    polishNote = nil
                    revealed = false
                    withAnimation(.spring(response: 0.5, dampingFraction: 0.7)) { result = nr }
                    UINotificationFeedbackGenerator().notificationOccurred(.success)
                    withAnimation(.spring(response: 0.6, dampingFraction: 0.6).delay(0.05)) { revealed = true }
                }
                await pollOnbody(sku: nr.sku)
            } catch {
                await MainActor.run {
                    errorMessage = error.localizedDescription
                    isRemixing = false
                }
            }
        }
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
        // 新しい作品 → 全状態リセット
        result = nil
        polishedURL = nil
        mockupURL = nil
        score = nil
        polishNote = nil
        revealed = false
        makingStep = 0
        isMaking = true
        Task {
            do {
                let r = try await MUAPI.make(prompt: text, kind: kind, royalty: royalty, apiKey: session.apiKey)
                Analytics.track("make_create", ["kind": r.kind, "sku": r.sku, "royalty": royalty])
                await MainActor.run {
                    isMaking = false
                    withAnimation(.spring(response: 0.5, dampingFraction: 0.7)) {
                        result = r
                    }
                    // 完成の高揚: 成功ハプティクス + リビールアニメ
                    UINotificationFeedbackGenerator().notificationOccurred(.success)
                    withAnimation(.spring(response: 0.6, dampingFraction: 0.6).delay(0.05)) {
                        revealed = true
                    }
                    maybePromptPush()
                }
                // 着画(オンボディ mockup)が出来たらポーリングで差し替え(最大~70秒)。
                await pollOnbody(sku: r.sku)
            } catch {
                await MainActor.run {
                    errorMessage = error.localizedDescription
                    isMaking = false
                }
            }
        }
    }

    // 着画ポーリング: 数秒ごとに peek し、mockup が出たら反映してハプティクス。
    private func pollOnbody(sku: String) async {
        for _ in 0..<14 {
            try? await Task.sleep(nanoseconds: 5_000_000_000)
            // 別の作品に切り替わっていたら中断
            if result?.sku != sku { return }
            if let peek = try? await MUAPI.peek(sku: sku), let url = peek.mockupURL {
                await MainActor.run {
                    guard result?.sku == sku else { return }
                    withAnimation(.spring(response: 0.5, dampingFraction: 0.75)) {
                        mockupURL = url
                    }
                    UIImpactFeedbackGenerator(style: .light).impactOccurred()
                }
                return
            }
        }
    }
}

// シマー(作ってる感のスケルトン)
private struct Shimmer: ViewModifier {
    @State private var phase: CGFloat = -1
    func body(content: Content) -> some View {
        content.overlay(
            GeometryReader { geo in
                LinearGradient(
                    colors: [.clear, .white.opacity(0.18), .clear],
                    startPoint: .leading, endPoint: .trailing
                )
                .frame(width: geo.size.width * 1.5)
                .offset(x: phase * geo.size.width * 1.5)
            }
            .clipped()
            .allowsHitTesting(false)
        )
        .task {
            while !Task.isCancelled {
                withAnimation(.linear(duration: 1.3)) { phase = 1.2 }
                try? await Task.sleep(nanoseconds: 1_300_000_000)
                phase = -1
            }
        }
    }
}
private extension View {
    func shimmering() -> some View { modifier(Shimmer()) }
}
