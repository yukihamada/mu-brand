import SwiftUI
import UIKit

// 1案ぶんの状態。複数案をスワイプで見比べられるよう、着画/磨き/スコアは案ごとに持つ。
struct DesignVariant: Identifiable {
    var result: MakeResult
    var mockupURL: URL? = nil
    var mockupIsModel = false
    var polishedURL: URL? = nil
    var score: DesignScore? = nil
    var polishNote: String? = nil
    var id: String { result.sku }
    // 着画(モデル) > 磨いた絵 > 元デザイン
    var shownURL: URL? { mockupURL ?? polishedURL ?? result.designURL }
}

// 「言えば、作れる」— MU の背骨。ひとこと打つと AI がデザインを起こし、即棚に並ぶ。
// 作ると複数案を生成し、スワイプで見比べて選べる。生成は POST /api/make。
struct MakeView: View {
    @EnvironmentObject private var session: Session
    @EnvironmentObject private var app: AppState

    @StateObject private var voice = VoiceInput()
    @State private var voiceBasePrompt = ""   // 録音開始時の入力(認識結果を追記する土台)
    @State private var prompt = ""
    @State private var kind: MakeKind = .auto
    @State private var royalty = 10          // 印税 10〜50%(価格は自動調整)
    @State private var isMaking = false
    @State private var errorMessage: String?
    @State private var showCheckout = false
    @State private var showGift = false
    @FocusState private var promptFocused: Bool

    // 複数案 + スワイプ
    @State private var variants: [DesignVariant] = []
    @State private var current = 0           // スワイプ中の案
    @State private var addingVariant = false  // 「もう1案」生成中
    @State private var pollTasks: [Task<Void, Never>] = []

    // 作っている間の演出
    @State private var makingStep = 0
    @State private var revealed = false      // 完成リビールのアニメ

    // リミックス(続きを作る)の状態
    @State private var showRemix = false
    @State private var remixWords = ""
    @State private var isRemixing = false

    // 「磨く」(5軸自己改善) の状態
    @State private var isPolishing = false

    // 価格編集(作った後に値段を変える)
    @State private var showPriceEdit = false
    @State private var priceInput = ""
    @State private var savingPrice = false

    // 現在表示中の案(安全に取り出す)
    private var currentVariant: DesignVariant? {
        variants.indices.contains(current) ? variants[current] : nil
    }

    // 作った直後 = 高意欲の瞬間に1回だけ通知許可を促す
    @AppStorage("didPromptPushAfterMake") private var didPromptPush = false

    @State private var popular: [FeedProduct] = []   // 売れ筋(人気から作る)

    // 作っている間に流す“作ってる感”メッセージ
    private let makingSteps = ["make.step1", "make.step2", "make.step3", "make.step4", "make.step5", "make.step6"]

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
                    } else if !variants.isEmpty {
                        variantPager
                    } else {
                        hints
                    }
                }
                .padding()
            }
            .navigationTitle(String(localized: "tab.make"))
            .task {
                Analytics.track("view_make")
                if popular.isEmpty { popular = (try? await MUAPI.popular()) ?? [] }
            }
            // 声で作る: 認識テキストを「録音開始時の入力 + 認識結果」で追記する
            // (一方的な全消し上書きでユーザーが手で打った文章を失わないように)。
            .onChange(of: voice.transcript) { _, t in
                if !t.isEmpty { prompt = (voiceBasePrompt + t) }
            }
            .onChange(of: voice.isRecording) { _, rec in
                if rec { voiceBasePrompt = prompt.isEmpty ? "" : prompt + " " }
            }
            // オンボーディングからの「最初の一着」を受け取り、その場で自動生成。
            // 受け取りは onChange の1箇所のみ(onAppear と二重に拾うと make が2回
            // 走り課金が二重になる)。make() 冒頭にも二重発火ガードを置く。
            .onChange(of: app.pendingPrompt) { _, new in
                guard let p = new else { return }
                app.pendingPrompt = nil
                prompt = p
                make()
            }
            .sheet(isPresented: $showCheckout) {
                if let s = currentVariant?.result.checkoutUrl, let url = URL(string: s) {
                    SafariView(url: url).ignoresSafeArea()
                }
            }
            .sheet(isPresented: $showGift) {
                if let s = currentVariant?.result.checkoutUrl, let url = giftURL(s) {
                    SafariView(url: url).ignoresSafeArea()
                }
            }
            .alert(String(localized: "make.editPrice"), isPresented: $showPriceEdit) {
                TextField("¥", text: $priceInput).keyboardType(.numberPad)
                Button(String(localized: "make.priceSave")) { savePrice() }
                Button(String(localized: "make.cancel"), role: .cancel) {}
            } message: {
                Text(String(localized: "make.priceHint"))
            }
        }
    }

    // checkout_url にギフトフラグを安全に足す(`?` の有無で区切りを選ぶ)。
    private func giftURL(_ checkout: String) -> URL? {
        let sep = checkout.contains("?") ? "&" : "?"
        return URL(string: checkout + sep + "gift=1")
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
                // 単位は商品によって変える(服=着・シール=枚・マグ=個…)
                Text(String(format: String(localized: "make.earnLine"), "¥\(earn.formatted())", unitFor(kind)))
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

    // 商品ごとの助数詞(着/枚/個…)。日本語は items.counter で出し分け。
    private func unitFor(_ k: MakeKind) -> String {
        switch k {
        case .tee, .hoodie, .rashguard: return String(localized: "unit.apparel")  // 着
        case .sticker: return String(localized: "unit.sticker")                    // 枚
        case .mug: return String(localized: "unit.mug")                            // 個
        case .tote: return String(localized: "unit.bag")                           // 個
        case .auto: return String(localized: "unit.generic")                       // 点
        }
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
        let trimmed = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        return VStack(spacing: 18) {
            // 自分ゴト化: 入力した言葉を「かたちにしています」と返す
            if !trimmed.isEmpty {
                Text(String(format: String(localized: "make.shaping"), trimmed))
                    .font(.headline)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: .infinity)
            }
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
                    // 魅力的なコピーを巡回(工程 + 価値 + ブランドの物語)
                    Text(String(format: String(localized: String.LocalizationValue(makingSteps[makingStep % makingSteps.count])), royalty))
                        .font(.callout.weight(.medium))
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 24)
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

    // 複数案をスワイプで見比べ。画像をページング、下の操作は現在の案に連動。
    private var variantPager: some View {
        VStack(alignment: .leading, spacing: 14) {
            Label(String(localized: "make.done"), systemImage: "checkmark.seal.fill")
                .font(.headline).foregroundStyle(.tint)
                .scaleEffect(revealed ? 1 : 0.6).opacity(revealed ? 1 : 0)

            // 案カウンタ + スワイプ案内
            if variants.count > 1 {
                Text(String(format: String(localized: "make.variantCount"), current + 1, variants.count))
                    .font(.caption).foregroundStyle(.secondary)
            }

            // 画像ページャ(スワイプで案を切替)
            TabView(selection: $current) {
                ForEach(Array(variants.enumerated()), id: \.element.id) { idx, v in
                    variantImage(v).tag(idx)
                }
                if addingVariant {  // 生成中の案をプレースホルダで見せる
                    ZStack { RoundedRectangle(cornerRadius: 16).fill(.quaternary.opacity(0.3)).shimmering()
                        ProgressView().tint(.white)
                    }.tag(variants.count)
                }
            }
            .tabViewStyle(.page(indexDisplayMode: variants.count > 1 ? .always : .never))
            .frame(height: 360)

            if let v = currentVariant { detailsFor(v) }

            // もう1案つくる(バリエーションを増やしてスワイプで見比べ)
            Button {
                addVariation()
            } label: {
                HStack {
                    if addingVariant { ProgressView() } else { Image(systemName: "plus.circle") }
                    Text(String(localized: "make.another"))
                }
                .font(.subheadline.weight(.semibold))
                .frame(maxWidth: .infinity).padding(.vertical, 4)
            }
            .buttonStyle(.bordered)
            .disabled(addingVariant || isPolishing)

            Button(String(localized: "make.again")) {
                resetAll()
                prompt = ""
                promptFocused = true
            }
            .font(.subheadline).frame(maxWidth: .infinity).padding(.top, 2)
        }
        .padding(16)
        .background(.quaternary.opacity(0.25), in: RoundedRectangle(cornerRadius: 20))
    }

    private func variantImage(_ v: DesignVariant) -> some View {
        ZStack {
            AsyncImage(url: v.shownURL) { phase in
                switch phase {
                case .success(let img): img.resizable().scaledToFit()
                default: Rectangle().fill(.quaternary)
                }
            }
            .frame(maxWidth: .infinity)
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .opacity(isPolishing && v.id == currentVariant?.id ? 0.4 : 1)
            if isPolishing && v.id == currentVariant?.id {
                VStack(spacing: 8) { ProgressView().tint(.white)
                    Text(String(localized: "make.polishing")).font(.footnote).foregroundStyle(.white) }
            }
            if v.mockupURL != nil && v.polishedURL == nil {
                VStack { HStack { Spacer()
                    Text(String(localized: v.mockupIsModel ? "make.onbody.model" : "make.onbody"))
                        .font(.caption2.weight(.bold)).padding(.horizontal, 8).padding(.vertical, 4)
                        .background(.black.opacity(0.6), in: Capsule()).foregroundStyle(.white).padding(8)
                }; Spacer() }
            }
        }
    }

    // 現在の案の詳細 + 操作(価格編集・磨く・買う・プレゼント・リミックス・シェア)。
    @ViewBuilder
    private func detailsFor(_ v: DesignVariant) -> some View {
        let r = v.result
        Text(r.display.uppercased()).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
        Text(r.hook).font(.title3.weight(.medium))

        if let s = v.score { scoreView(s) }

        // 価格 + 「変更」(作った後に値段を変えられる)
        HStack {
            Text(r.priceLabel).font(.title2.bold())
            if r.editToken != nil && r.autoApproved {
                Button(String(localized: "make.editPrice")) {
                    priceInput = String(r.retailJpy)
                    showPriceEdit = true
                }
                .font(.caption).buttonStyle(.bordered).controlSize(.mini)
            }
            Spacer()
        }

        if r.autoApproved, let pct = r.makerPct, let earn = r.makerEarnJpy {
            HStack(alignment: .top, spacing: 8) {
                Image(systemName: "bag.badge.plus").foregroundStyle(.tint)
                Text(String(format: String(localized: "make.listedEarn"), pct, "¥\(earn.formatted())")).font(.footnote)
            }
            .padding(10).frame(maxWidth: .infinity, alignment: .leading)
            .background(.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 12))
        }

        Text(v.polishNote ?? r.note).font(.caption).foregroundStyle(.secondary)

        if r.editToken != nil && r.autoApproved {
            Button { polish(v) } label: {
                HStack { Image(systemName: "wand.and.stars")
                    Text(isPolishing ? String(localized: "make.polishing") : String(localized: "make.polish")) }
                .font(.headline).frame(maxWidth: .infinity).padding(.vertical, 6)
            }
            .buttonStyle(.bordered).tint(.yellow).disabled(isPolishing)
        }

        if r.checkoutUrl != nil {
            Button { Analytics.track("make_buy", ["sku": r.sku]); showCheckout = true } label: {
                Label(String(localized: "pdp.buy"), systemImage: "bolt.fill")
                    .font(.headline).frame(maxWidth: .infinity).padding(.vertical, 6)
            }
            .buttonStyle(.borderedProminent).foregroundStyle(.black).disabled(isPolishing)
            Button { Analytics.track("make_gift", ["sku": r.sku]); showGift = true } label: {
                Label(String(localized: "buy.gift"), systemImage: "gift.fill")
                    .font(.subheadline.weight(.semibold)).frame(maxWidth: .infinity).padding(.vertical, 4)
            }
            .buttonStyle(.bordered).disabled(isPolishing)
        } else {
            Label(String(localized: "make.reviewPending"), systemImage: "clock")
                .font(.subheadline).foregroundStyle(.secondary)
        }

        if r.editToken != nil && r.autoApproved { remixSection(r) }

        if let pdp = URL(string: r.pdpUrl) {
            ShareLink(item: pdp, subject: Text(r.display), message: Text(String(localized: "share.message"))) {
                Label(String(localized: "share.cta"), systemImage: "square.and.arrow.up")
                    .font(.subheadline.weight(.semibold)).frame(maxWidth: .infinity).padding(.vertical, 4)
            }
            .buttonStyle(.bordered)
            Link(destination: pdp) {
                Label(String(localized: "pdp.openWeb"), systemImage: "safari")
                    .font(.subheadline).frame(maxWidth: .infinity)
            }
        }
    }

    private var hints: some View {
        VStack(alignment: .leading, spacing: 8) {
            // 売れ筋から作る: 人気デザインを起点にすると「買いたくなる商品」が作りやすい。
            if !popular.isEmpty {
                Text(String(localized: "make.popularTitle"))
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(.secondary)
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(alignment: .top, spacing: 10) {
                        ForEach(popular.prefix(10)) { p in
                            Button {
                                // その商品のコンセプトを起点に(売れ筋ベース=desirable)。
                                let brief = p.description.components(separatedBy: " — ").last ?? p.description
                                prompt = brief
                                promptFocused = true
                                Analytics.track("make_from_popular", ["sku": p.sku])
                            } label: {
                                VStack(alignment: .leading, spacing: 3) {
                                    AsyncImage(url: p.mockupURL) { phase in
                                        switch phase {
                                        case .success(let img): img.resizable().scaledToFill()
                                        default: Rectangle().fill(.quaternary)
                                        }
                                    }
                                    .frame(width: 110, height: 110)
                                    .clipShape(RoundedRectangle(cornerRadius: 10))
                                    Text(p.description).font(.caption2).lineLimit(1).frame(width: 110, alignment: .leading)
                                }
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
                .padding(.bottom, 4)
            }

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

    // sku で案のインデックスを引く(スワイプで current が動いても安全に更新するため)。
    private func variantIndex(_ sku: String) -> Int? { variants.firstIndex { $0.id == sku } }

    private func remix(_ r: MakeResult) {
        let words = remixWords.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !words.isEmpty, !isRemixing else { return }
        isRemixing = true
        Task {
            do {
                let nr = try await MUAPI.remix(sku: r.sku, words: words, apiKey: session.apiKey)
                Analytics.track("make_remix", ["from": r.sku, "to": nr.sku])
                await MainActor.run {
                    isRemixing = false; showRemix = false; remixWords = ""
                    // リミックスは新しい案として追加し、そこへスワイプ。
                    withAnimation(.spring(response: 0.5, dampingFraction: 0.7)) {
                        variants.append(DesignVariant(result: nr))
                        current = variants.count - 1
                    }
                    successHaptic()
                    startPolling(sku: nr.sku)
                }
            } catch {
                await MainActor.run { errorMessage = error.localizedDescription; isRemixing = false }
            }
        }
    }

    private func polish(_ v: DesignVariant) {
        let r = v.result
        guard let token = r.editToken, !isPolishing else { return }
        errorMessage = nil
        isPolishing = true
        Task {
            do {
                let res = try await MUAPI.polish(sku: r.sku, editToken: token)
                Analytics.track("make_polish", ["sku": r.sku, "improved": res.improved])
                await MainActor.run {
                    if let i = variantIndex(r.sku) {
                        withAnimation {
                            if res.improved, let url = res.designURL {
                                variants[i].polishedURL = url
                                variants[i].score = res.after
                                variants[i].mockupURL = nil; variants[i].mockupIsModel = false
                                startPolling(sku: r.sku) // 磨いた絵の着画を取り直す
                            } else {
                                variants[i].score = res.after ?? res.before
                            }
                            variants[i].polishNote = res.note
                        }
                    }
                    isPolishing = false
                }
            } catch {
                await MainActor.run { errorMessage = error.localizedDescription; isPolishing = false }
            }
        }
    }

    // 値段を作った後に変更(/api/make/edit)。
    private func savePrice() {
        guard let r = currentVariant?.result, let token = r.editToken,
              let yen = Int(priceInput.filter(\.isNumber)), yen > 0, !savingPrice else { return }
        savingPrice = true
        Task {
            do {
                let newPrice = try await MUAPI.editPrice(sku: r.sku, editToken: token, priceJpy: yen)
                Analytics.track("make_price_edit", ["sku": r.sku, "price": newPrice])
                await MainActor.run {
                    if let i = variantIndex(r.sku) { variants[i].result.retailJpy = newPrice }
                    savingPrice = false; showPriceEdit = false
                }
            } catch {
                await MainActor.run { errorMessage = error.localizedDescription; savingPrice = false }
            }
        }
    }

    // もう1案つくる(同じ依頼でバリエーションを追加 → スワイプで見比べ)。
    private func addVariation() {
        guard !addingVariant, let base = variants.first?.result else { return }
        let text = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        addingVariant = true
        Task {
            do {
                let r = try await MUAPI.make(prompt: text.isEmpty ? base.hook : text,
                                             kind: kind, royalty: royalty, apiKey: session.apiKey)
                Analytics.track("make_variation", ["sku": r.sku])
                await MainActor.run {
                    withAnimation(.spring(response: 0.5, dampingFraction: 0.7)) {
                        variants.append(DesignVariant(result: r))
                        current = variants.count - 1
                    }
                    addingVariant = false
                    UIImpactFeedbackGenerator(style: .medium).impactOccurred()
                    startPolling(sku: r.sku)
                }
            } catch {
                await MainActor.run { errorMessage = error.localizedDescription; addingVariant = false }
            }
        }
    }

    private func resetAll() {
        pollTasks.forEach { $0.cancel() }; pollTasks.removeAll()
        withAnimation {
            variants.removeAll(); current = 0; revealed = false
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
        // 二重発火ガード(オンボーディング受け渡し+連打)。課金が二重に走るのを防ぐ。
        guard !isMaking else { return }
        let text = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        promptFocused = false
        errorMessage = nil
        resetAll()
        makingStep = 0
        isMaking = true
        Task {
            do {
                let r = try await MUAPI.make(prompt: text, kind: kind, royalty: royalty, apiKey: session.apiKey)
                Analytics.track("make_create", ["kind": r.kind, "sku": r.sku, "royalty": royalty])
                await MainActor.run {
                    isMaking = false
                    withAnimation(.spring(response: 0.5, dampingFraction: 0.7)) {
                        variants = [DesignVariant(result: r)]
                        current = 0
                    }
                    successHaptic()
                    withAnimation(.spring(response: 0.6, dampingFraction: 0.6).delay(0.05)) { revealed = true }
                    maybePromptPush()
                    startPolling(sku: r.sku)
                }
                // いろいろスワイプで見比べられるよう、もう1案を裏で生成して追加。
                // (案を増やしすぎると時間あたりの生成上限に当たるので控えめに。
                //  さらに欲しい人は「もう1案つくる」で追加できる。)
                await autoVariations(prompt: text, count: 1)
            } catch {
                await MainActor.run { errorMessage = error.localizedDescription; isMaking = false }
            }
        }
    }

    // 追加バリエーションを順に生成して追加(スワイプ用)。
    private func autoVariations(prompt text: String, count: Int) async {
        for _ in 0..<count {
            guard let r = try? await MUAPI.make(prompt: text, kind: kind, royalty: royalty, apiKey: session.apiKey)
            else { continue }
            await MainActor.run {
                guard !variants.isEmpty else { return } // 作り直し済みなら捨てる
                variants.append(DesignVariant(result: r))
                startPolling(sku: r.sku)
            }
        }
    }

    // 着画ポーリングを起動(案ごとに1本・配列で保持し、作り直しで全キャンセル)。
    private func startPolling(sku: String) {
        let t = Task { await pollOnbody(sku: sku) }
        pollTasks.append(t)
    }

    // 着画ポーリング: 数秒ごとに peek し、mockup が出たら該当案に反映。
    private func pollOnbody(sku: String) async {
        for _ in 0..<14 {
            try? await Task.sleep(nanoseconds: 5_000_000_000)
            if Task.isCancelled { return }
            if variantIndex(sku) == nil { return }
            if let peek = try? await MUAPI.peek(sku: sku), let url = peek.mockupURL {
                if Task.isCancelled { return }
                await MainActor.run {
                    guard let i = variantIndex(sku) else { return }
                    withAnimation(.spring(response: 0.5, dampingFraction: 0.75)) {
                        variants[i].mockupURL = url
                        variants[i].mockupIsModel = peek.isModel ?? false
                    }
                    if i == current { UIImpactFeedbackGenerator(style: .light).impactOccurred() }
                }
                if peek.isModel == true { return }
            }
        }
    }

    // 成功ハプティクスは prepare してから発火(不発を避ける)。
    private func successHaptic() {
        let gen = UINotificationFeedbackGenerator()
        gen.prepare()
        gen.notificationOccurred(.success)
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
