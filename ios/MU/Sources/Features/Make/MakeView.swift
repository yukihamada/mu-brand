import SwiftUI
import UIKit
import StoreKit

// 「言えば、作れる」— MU の背骨。ひとこと打つと AI がデザインを起こし、即棚に並ぶ。
// 作ると複数案を生成し、スワイプで見比べて選べる。生成は POST /api/make。
@MainActor
struct MakeView: View {
    @EnvironmentObject private var session: Session
    @EnvironmentObject private var app: AppState
    @Environment(\.requestReview) private var requestReview
    @Environment(\.openURL) private var openURL

    // 「自分で着る/友達に贈る」の言語化バナーは、はじめての一着だけ出す。
    @AppStorage("mu.hasCreatedFirstPiece") private var hasCreatedFirstPiece = false

    @StateObject private var voice = VoiceInput()
    @State private var voiceBasePrompt = ""   // 録音開始時の入力(認識結果を追記する土台)
    @State private var prompt = ""
    @State private var kind = ""
    @State private var kindCatalog = MakeKindCatalog()
    private var kinds: [MakeKindOption] { kindCatalog.items }
    private var kindsLoaded: Bool { kindCatalog.loaded }
    private var kindsUnavailable: Bool { kindCatalog.refreshFailed }
    @State private var showKinds = false
    @State private var kindQuery = ""
    @State private var royalty = 10          // 印税 10〜50%(価格は自動調整)
    @State private var isMaking = false
    @State private var errorMessage: String?
    @State private var showCheckout = false
    @State private var showGift = false
    @State private var showAIConsent = false
    // 2026-08-28: 作るには登録必須。未ログインで作ろうとしたら登録シートを開き、
    // 完了したら自動でもう一度 performMake() を叩く(onDismiss)。
    @State private var showAuthGate = false
    @State private var pendingIntent: MakeIntent?
    @FocusState private var promptFocused: Bool

    // デザイン依頼: このお題を誰かに頼む(相手が作る→自分が受け取る→作手に印税)。
    @State private var askEmail = false
    @State private var requestEmail = ""
    @State private var requestBusy = false
    @State private var requestLink: String?
    @State private var requestStatusURL: String?

    // 複数案 + スワイプ
    @State private var variants: [DesignVariant] = []
    @State private var current = 0           // スワイプ中の案
    @State private var addingVariant = false  // 「もう1案」生成中
    @State private var scope = MakeRequestScope()
    @State private var workTasks: [String: Task<Void, Never>] = [:]
    @State private var activeInput: MakeInput?
    @State private var priceTarget: String?
    @State private var polishingSKU: String?

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
                            TextField(promptPlaceholder, text: $prompt, axis: .vertical)
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

                        kindPicker

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
                        .disabled(isMaking || !kindsLoaded || !selectedKindAvailable || prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

                        // このお題を、誰かにデザインしてもらう。
                        Button(action: { startDesignRequest() }) {
                            HStack {
                                if requestBusy { ProgressView().controlSize(.small) }
                                Image(systemName: "gift")
                                Text("このお題を誰かに頼む")
                            }
                            .font(.subheadline)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 4)
                        }
                        .buttonStyle(.bordered)
                        .disabled(isMaking || requestBusy || !selectedKindAvailable || prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    }
                    .disabled(isMaking)
                    .opacity(isMaking ? 0.5 : 1)

                    if let link = requestLink {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("🎁 依頼リンクができました")
                                .font(.subheadline.weight(.semibold))
                            Text("このリンクを送ると、相手がデザインして、できあがったらあなたが受け取れます。")
                                .font(.caption).foregroundStyle(.secondary)
                            if let url = URL(string: link) {
                                ShareLink(item: url) {
                                    Label("リンクを送る", systemImage: "square.and.arrow.up")
                                        .frame(maxWidth: .infinity)
                                }
                                .buttonStyle(.borderedProminent)
                            }
                            if let s = requestStatusURL, let surl = URL(string: s) {
                                Link("自分の受け取りページを見る →", destination: surl)
                                    .font(.caption)
                            }
                        }
                        .padding(12)
                        .background(.green.opacity(0.12), in: RoundedRectangle(cornerRadius: 10))
                    }

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
                await loadKinds()
                guard !Task.isCancelled else { return }
                for variant in variants where variant.previewPending || !variant.result.isLive {
                    startPolling(sku: variant.id)
                }
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
            .sheet(isPresented: $showKinds) { allKindsPicker }
            .alert(String(localized: "make.editPrice"), isPresented: $showPriceEdit) {
                TextField("¥", text: $priceInput).keyboardType(.numberPad)
                Button(String(localized: "make.priceSave")) { savePrice() }
                Button(String(localized: "make.cancel"), role: .cancel) {}
            } message: {
                Text(String(localized: "make.priceHint"))
            }
            .alert("あなたのメール", isPresented: $askEmail) {
                TextField("you@example.com", text: $requestEmail)
                    .keyboardType(.emailAddress)
                    .textInputAutocapitalization(.never)
                Button("リンクを作る") { Task { await createRequest(email: requestEmail) } }
                Button(String(localized: "make.cancel"), role: .cancel) {}
            } message: {
                Text("完成のお知らせ・受け取りに使います")
            }
            .aiConsentAlert(isPresented: $showAIConsent) { resumeIntent() }
            .sheet(isPresented: $showAuthGate, onDismiss: {
                if session.isLoggedIn { resumeIntent() } else { pendingIntent = nil }
            }) {
                AuthGateSheet()
            }
            .onDisappear { cancelWork() }
        }
    }

    // checkout_url にギフトフラグを安全に足す(`?` の有無で区切りを選ぶ)。
    private func giftURL(_ checkout: String) -> URL? {
        let sep = checkout.contains("?") ? "&" : "?"
        return URL(string: checkout + sep + "gift=1")
    }

    // このお題を誰かに頼む: ログイン中ならそのメール、なければ入力を促す。
    private func startDesignRequest() {
        guard validateKind(kind) else { return }
        guard !prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        if let e = session.email, e.contains("@") {
            Task { await createRequest(email: e) }
        } else {
            if requestEmail.isEmpty { requestEmail = session.email ?? "" }
            askEmail = true
        }
    }

    private func createRequest(email: String) async {
        guard validateKind(kind) else { return }
        let brief = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        let e = email.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !brief.isEmpty, e.contains("@") else {
            errorMessage = String(localized: "メールとお題を入れてください"); return
        }
        requestBusy = true; errorMessage = nil
        let generation = scope.generation
        let operation = scope.begin("request")
        Analytics.track("design_request_create")
        let kindArg = kind.isEmpty ? nil : kind
        do {
            let (link, status) = try await MUAPI.createDesignRequest(email: e, brief: brief, kind: kindArg)
            guard accepts(generation, "request", operation) else { return }
            requestLink = link
            requestStatusURL = status
        } catch let APIError.message(m) {
            guard accepts(generation, "request", operation) else { return }
            errorMessage = m
        } catch {
            guard accepts(generation, "request", operation) else { return }
            errorMessage = String(localized: "リンクを作成できませんでした")
        }
        requestBusy = false
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
            Group {
                if let price = selectedKind.estimatedPrice(royalty: royalty) {
                    Text(String(format: String(localized: "make.priceLine"), "¥\(price.formatted())"))
                    Text(String(format: String(localized: "make.earnLine"), "¥\((price * royalty / 100).formatted())", unitFor(selectedKind)))
                        .foregroundStyle(.tint)
                    Text(String(localized: "make.estimate")).foregroundStyle(.tertiary)
                } else {
                    Text(String(localized: "make.price.afterKind"))
                }
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .padding(10)
        .background(.quaternary.opacity(0.25), in: RoundedRectangle(cornerRadius: 12))
    }

    // 商品ごとの助数詞(着/枚/個…)。日本語は items.counter で出し分け。
    private func unitFor(_ k: MakeKindOption) -> String {
        guard Locale.current.language.languageCode?.identifier == "ja" else {
            return k.kind.isEmpty ? String(localized: "unit.generic") : k.label
        }
        if k.category == "wear" { return String(localized: "unit.apparel") }
        if k.kind == "sticker" { return String(localized: "unit.sticker") }
        return String(localized: "unit.generic")
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
            while isMaking && !Task.isCancelled {
                do { try await Task.sleep(nanoseconds: 1_400_000_000) } catch { return }
                withAnimation { makingStep += 1 }
            }
        }
    }

    private var selectedKind: MakeKindOption { kinds.first { $0.kind == kind } ?? .auto }
    private var selectedKindAvailable: Bool { MakeKindOption.isAvailable(kind, in: kinds) }
    private func unavailableMessage(_ kind: String) -> String {
        kinds.first { $0.kind == kind }?.unavailableMessage() ?? String(localized: "make.kind.unavailableHint")
    }
    private func validateKind(_ kind: String) -> Bool {
        guard MakeKindOption.isAvailable(kind, in: kinds) else {
            errorMessage = unavailableMessage(kind)
            return false
        }
        return true
    }
    private var promptPlaceholder: String {
        kind.isEmpty ? String(localized: "make.placeholder") :
            String(format: String(localized: "make.placeholder.kind"), selectedKind.label)
    }

    private func loadKinds() async {
        do {
            let loaded = try await MUAPI.makeKinds()
            guard !Task.isCancelled else { return }
            kindCatalog.received(loaded)
        } catch {
            guard !Task.isCancelled else { return }
            kindCatalog.failed()
        }
        // Never silently replace a now-unavailable explicit choice with Auto.
        if !selectedKindAvailable { errorMessage = unavailableMessage(kind) }
    }

    private var kindPicker: some View {
        VStack(alignment: .leading, spacing: 8) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(kinds.filter { ["", "tee", "rashguard_ls", "tote", "mug"].contains($0.kind) }) { chip($0) }
                }
            }
            Button { showKinds = true } label: {
                Label(String(format: String(localized: "make.kinds.choose"), selectedKind.label), systemImage: "square.grid.2x2")
            }
            .disabled(!kindsLoaded)
            .accessibilityIdentifier("make.kindPicker")
            .accessibilityValue(kind.isEmpty ? "auto" : kind)
            if !selectedKindAvailable {
                Text(unavailableMessage(kind)).font(.caption).foregroundStyle(.secondary)
            }
            if kindsUnavailable {
                Text(String(localized: "make.kinds.unavailable")).font(.caption).foregroundStyle(.secondary)
                    .accessibilityIdentifier("make.kindsUnavailable")
                Button(String(localized: "make.retry")) { Task { await loadKinds() } }.font(.caption)
            } else if !kindsLoaded { ProgressView() }
        }
    }

    private var allKindsPicker: some View {
        let filtered = kinds.filter { $0.matches(kindQuery) }
        let categories = Array(Set(filtered.map(\.category))).sorted()
        return NavigationStack {
            List {
                ForEach(categories, id: \.self) { category in
                    Section(categoryLabel(category)) {
                        ForEach(filtered.filter { $0.category == category }) { option in
                            Button {
                                guard option.isAvailable else { return }
                                kind = option.kind
                                showKinds = false
                            } label: {
                                HStack {
                                    VStack(alignment: .leading, spacing: 4) {
                                        Text(option.label).foregroundStyle(.primary)
                                        if !option.isAvailable {
                                            Text(unavailableMessage(option.kind))
                                                .font(.caption).foregroundStyle(.secondary)
                                        }
                                    }
                                    Spacer()
                                    if let price = option.estimatedPrice(royalty: royalty) {
                                        Text("¥\(price.formatted())〜").font(.caption).foregroundStyle(.secondary)
                                    }
                                    if option.kind == kind { Image(systemName: "checkmark") }
                                }
                            }
                            .disabled(!option.isAvailable)
                            .accessibilityIdentifier("make.kind.option.\(option.kind.isEmpty ? "auto" : option.kind)")
                        }
                    }
                }
            }
            .searchable(text: $kindQuery, prompt: String(localized: "make.kinds.search"))
            .navigationTitle(String(localized: "make.kinds.title"))
            .refreshable { await loadKinds() }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(String(localized: "make.cancel")) { showKinds = false }
                }
            }
        }
    }

    private func categoryLabel(_ category: String) -> String {
        switch category {
        case "common": return String(localized: "make.category.common")
        case "wear", "apparel": return String(localized: "make.category.wear")
        case "carry", "accessories": return String(localized: "make.category.carry")
        case "home", "lifestyle": return String(localized: "make.category.home")
        case "pet", "pets": return String(localized: "make.category.pet")
        default: return category
        }
    }

    private func chip(_ k: MakeKindOption) -> some View {
        let selected = k.kind == kind
        return Button {
            guard k.isAvailable else { return }
            kind = k.kind
        } label: {
            Text(k.isAvailable ? k.label : k.label + " · " + String(localized: "make.kind.unavailable"))
                .font(.subheadline.weight(selected ? .bold : .regular))
                .padding(.horizontal, 14).padding(.vertical, 8)
                .background(selected ? AnyShapeStyle(.tint) : AnyShapeStyle(.quaternary.opacity(0.4)),
                            in: Capsule())
                .foregroundStyle(selected ? .black : .primary)
        }
        .disabled(isMaking || !k.isAvailable)
        .accessibilityHint(k.unavailableMessage() ?? "")
    }

    // 複数案をスワイプで見比べ。画像をページング、下の操作は現在の案に連動。
    private var variantPager: some View {
        VStack(alignment: .leading, spacing: 14) {
            Label(String(localized: "make.created"), systemImage: "checkmark.seal.fill")
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
            .disabled(addingVariant || isPolishing || isRemixing)

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
            .opacity(isPolishing && v.id == polishingSKU ? 0.4 : 1)
            if isPolishing && v.id == polishingSKU {
                VStack(spacing: 8) { ProgressView().tint(.white)
                    Text(String(localized: "make.polishing")).font(.footnote).foregroundStyle(.white) }
            }
            if v.shownURL != nil {
                VStack { HStack { Spacer()
                    Text(String(localized: String.LocalizationValue(v.previewLabelKey)))
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
        Text(kinds.first { $0.kind == r.kind }?.label ?? r.kind)
            .font(.caption).foregroundStyle(.secondary)

        if v.previewPending {
            HStack {
                if !v.previewTimedOut { ProgressView().controlSize(.small) }
                Text(String(localized: v.previewTimedOut ? "make.preview.delayed" : "make.preview.pending"))
                    .font(.caption).foregroundStyle(.secondary)
                if v.previewTimedOut {
                    Button(String(localized: "make.retry")) { startPolling(sku: r.sku) }.font(.caption)
                }
            }
        }

        if let s = v.score { scoreView(s) }

        // 価格 + 「変更」(作った後に値段を変えられる)
        HStack {
            Text(r.priceLabel).font(.title2.bold())
            if r.editToken != nil && r.isLive {
                Button(String(localized: "make.editPrice")) {
                    priceInput = String(r.retailJpy)
                    priceTarget = r.sku
                    showPriceEdit = true
                }
                .font(.caption).buttonStyle(.bordered).controlSize(.mini)
                .disabled(savingPrice)
            }
            Spacer()
        }

        if r.isLive, let pct = r.makerPct, let earn = r.makerEarnJpy {
            HStack(alignment: .top, spacing: 8) {
                Image(systemName: "bag.badge.plus").foregroundStyle(.tint)
                Text(String(format: String(localized: "make.listedEarn"), pct, "¥\(earn.formatted())")).font(.footnote)
            }
            .padding(10).frame(maxWidth: .infinity, alignment: .leading)
            .background(.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 12))
        }

        Text(v.polishNote ?? r.note).font(.caption).foregroundStyle(.secondary)

        if r.editToken != nil && r.isLive {
            Button { polish(v) } label: {
                HStack { Image(systemName: "wand.and.stars")
                    Text(isPolishing ? String(localized: "make.polishing") : String(localized: "make.polish")) }
                .font(.headline).frame(maxWidth: .infinity).padding(.vertical, 6)
            }
            .buttonStyle(.bordered).tint(.yellow).disabled(isPolishing || isRemixing)
        }

        // はじめての一着だけ、「自分で着る/友達に贈る」の選択を言語化して見せる。
        // 2つ目以降は既に分かっているので出さない(ノイズにしない)。
        if r.isLive && r.checkoutUrl != nil && !hasCreatedFirstPiece {
            firstPieceBanner
        }

        if r.isLive && r.checkoutUrl != nil {
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
            .onAppear { hasCreatedFirstPiece = true }
        } else {
            Label(String(localized: r.status == "review" ? "make.reviewPending" : "make.notLive"), systemImage: "clock")
                .font(.subheadline).foregroundStyle(.secondary)
        }

        if r.supportsRemix(kinds: kinds) { remixSection(r) }

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

        // 広める=報酬(Web の /make 結果画面と同じ導線)。作者は登録必須化で常に
        // 判明しているので affiliate_link は常に返る。
        if r.autoApproved, let link = r.affiliateLink, let url = URL(string: link) {
            VStack(alignment: .leading, spacing: 4) {
                Text(String(localized: "make.spread.line"))
                    .font(.caption).foregroundStyle(.secondary)
                Button {
                    Analytics.track("make_spread_link_tap", ["sku": r.sku])
                    openURL(url)
                } label: {
                    Text(String(localized: "make.spread.cta"))
                        .font(.caption.weight(.semibold))
                }
                .buttonStyle(.plain)
            }
            .padding(.top, 2)
        }
    }

    // 「言った瞬間に、もう選べる」— はじめての一着でだけ「自分で着る/友達に贈る」を
    // 明文化する。IKEA効果(自作品への愛着)が一番強い瞬間に、次の一歩を具体的に示す。
    private var firstPieceBanner: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("🎉 " + String(localized: "make.firstPiece.title"))
                .font(.subheadline.weight(.bold))
            Text(String(localized: "make.firstPiece.sub"))
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.yellow.opacity(0.12), in: RoundedRectangle(cornerRadius: 12))
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
                guard validateKind("rashguard_ls") else { return }
                kind = "rashguard_ls"
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
                    .disabled(isRemixing || isPolishing || remixWords.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || remixWords.count > 120)
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
        guard !words.isEmpty, words.count <= 120, !isRemixing, !isPolishing,
              r.supportsRemix(kinds: kinds) else { return }
        authorize(.remix(sku: r.sku, words: words))
    }

    private func performRemix(sku: String, words: String) {
        guard let i = variantIndex(sku), variants[i].result.supportsRemix(kinds: kinds), !isRemixing else { return }
        guard validateKind(variants[i].result.kind) else { return }
        let generation = scope.generation
        let token = scope.begin("remix")
        let apiKey = session.apiKey
        isRemixing = true
        errorMessage = nil
        workTasks["remix"] = Task {
            do {
                let nr = try await MUAPI.remix(sku: sku, words: words, apiKey: apiKey)
                guard accepts(generation, "remix", token) else { return }
                Analytics.track("make_remix", ["from": sku, "to": nr.sku])
                isRemixing = false; showRemix = false; remixWords = ""
                withAnimation(.spring(response: 0.5, dampingFraction: 0.7)) {
                    variants.append(DesignVariant(result: nr))
                    current = variants.count - 1
                }
                successHaptic()
                startPolling(sku: nr.sku)
            } catch APIError.needRegister {
                guard accepts(generation, "remix", token) else { return }
                isRemixing = false
                pendingIntent = .remix(sku: sku, words: words)
                showAuthGate = true
            } catch {
                guard accepts(generation, "remix", token) else { return }
                errorMessage = error.localizedDescription; isRemixing = false
            }
        }
    }

    private func polish(_ v: DesignVariant) {
        let r = v.result
        guard let token = r.editToken, !isPolishing else { return }
        let generation = scope.generation
        let key = "polish:\(r.sku)"
        let operation = scope.begin(key)
        let design = r.designUrl
        // Stop pre-polish peek, including a response already in flight.
        cancelOperation("peek:\(r.sku)")
        errorMessage = nil
        isPolishing = true
        polishingSKU = r.sku
        workTasks[key] = Task {
            do {
                let res = try await MUAPI.polish(sku: r.sku, editToken: token)
                guard accepts(generation, key, operation), let i = variantIndex(r.sku),
                      variants[i].result.designUrl == design else { return }
                Analytics.track("make_polish", ["sku": r.sku, "improved": res.improved])
                withAnimation { variants[i].applyPolish(res) }
                isPolishing = false; polishingSKU = nil
                startPolling(sku: r.sku)
            } catch {
                guard accepts(generation, key, operation) else { return }
                errorMessage = error.localizedDescription; isPolishing = false; polishingSKU = nil
                startPolling(sku: r.sku)
            }
        }
    }

    // 値段を作った後に変更(/api/make/edit)。
    private func savePrice() {
        guard let sku = priceTarget, let index = variantIndex(sku) else { return }
        let r = variants[index].result
        guard let token = r.editToken,
              let yen = Int(priceInput.trimmingCharacters(in: .whitespacesAndNewlines)), yen > 0, !savingPrice else {
            errorMessage = String(localized: "make.price.invalid"); return
        }
        let generation = scope.generation
        let operation = scope.begin("price")
        savingPrice = true
        workTasks["price"] = Task {
            do {
                let saved = try await MUAPI.editPrice(sku: r.sku, editToken: token, priceJpy: yen)
                guard accepts(generation, "price", operation) else { return }
                Analytics.track("make_price_edit", ["sku": r.sku, "price": saved.priceJpy])
                if let i = variantIndex(r.sku) { variants[i].result.applyPrice(saved) }
                savingPrice = false; showPriceEdit = false
            } catch {
                guard accepts(generation, "price", operation) else { return }
                errorMessage = error.localizedDescription; savingPrice = false
            }
        }
    }

    // もう1案つくる(同じ依頼でバリエーションを追加 → スワイプで見比べ)。
    private func addVariation() {
        guard !addingVariant, !isRemixing, let input = activeInput else { return }
        authorize(.variation(input))
    }

    private func performVariation(_ input: MakeInput, automatic: Bool = false) {
        guard !addingVariant, !variants.isEmpty else { return }
        guard validateKind(input.kind) else { return }
        let generation = scope.generation
        let operation = scope.begin("variation")
        let apiKey = session.apiKey
        addingVariant = true
        workTasks["variation"] = Task {
            do {
                let r = try await MUAPI.make(prompt: input.prompt, kind: input.kind,
                                             royalty: input.royalty, apiKey: apiKey)
                guard accepts(generation, "variation", operation) else { return }
                Analytics.track("make_variation", ["sku": r.sku])
                withAnimation(.spring(response: 0.5, dampingFraction: 0.7)) {
                    variants.append(DesignVariant(result: r))
                    if !automatic { current = variants.count - 1 }
                }
                addingVariant = false
                startPolling(sku: r.sku)
            } catch APIError.needRegister {
                guard accepts(generation, "variation", operation) else { return }
                addingVariant = false
                pendingIntent = .variation(input)
                showAuthGate = true
            } catch {
                guard accepts(generation, "variation", operation) else { return }
                errorMessage = error.localizedDescription; addingVariant = false
            }
        }
    }

    private func accepts(_ generation: UUID, _ key: String, _ token: UUID) -> Bool {
        !Task.isCancelled && scope.accepts(generation, key: key, token: token)
    }

    private func cancelOperation(_ key: String) {
        workTasks.removeValue(forKey: key)?.cancel()
        _ = scope.begin(key)
    }

    private func cancelWork() {
        scope.reset()
        workTasks.values.forEach { $0.cancel() }
        workTasks.removeAll()
        isMaking = false; addingVariant = false; isRemixing = false
        isPolishing = false; savingPrice = false; polishingSKU = nil
        requestBusy = false
        pendingIntent = nil
    }

    private func resetAll() {
        cancelWork()
        activeInput = nil; priceTarget = nil
        showRemix = false; remixWords = ""; errorMessage = nil
        requestLink = nil; requestStatusURL = nil
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

    // レビュー促進: デザインが生まれた高揚の瞬間に、控えめに(2/8/25回目のみ)。
    // 初回はpush許可プロンプトと重なるので出さない。OSが年3回までに間引く。
    private func maybeRequestReview() {
        let key = "mu.makeSuccessCount"
        let n = UserDefaults.standard.integer(forKey: key) + 1
        UserDefaults.standard.set(n, forKey: key)
        guard [2, 8, 25].contains(n) else { return }
        let generation = scope.generation
        let operation = scope.begin("review")
        workTasks["review"] = Task {
            do { try await Task.sleep(nanoseconds: 2_500_000_000) } catch { return }
            guard accepts(generation, "review", operation) else { return }
            requestReview()
            Analytics.track("review_prompt", ["at": "after_make", "n": n])
        }
    }

    private func make() {
        // 二重発火ガード(オンボーディング受け渡し+連打)。課金が二重に走るのを防ぐ。
        guard !isMaking else { return }
        guard validateKind(kind) else { return }
        let text = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        authorize(.create(MakeInput(prompt: text, kind: kind, royalty: royalty)))
    }

    private func authorize(_ intent: MakeIntent) {
        pendingIntent = intent
        guard AIConsent.given else { showAIConsent = true; return }
        guard session.isLoggedIn else { showAuthGate = true; return }
        resumeIntent()
    }

    private func resumeIntent() {
        guard let intent = pendingIntent else { return }
        guard AIConsent.given else { showAIConsent = true; return }
        guard session.isLoggedIn else { showAuthGate = true; return }
        pendingIntent = nil
        switch intent {
        case .create(let input): performMake(input)
        case .variation(let input): performVariation(input)
        case .remix(let sku, let words): performRemix(sku: sku, words: words)
        }
    }

    private func performMake(_ input: MakeInput) {
        guard !isMaking, !input.prompt.isEmpty else { return }
        guard validateKind(input.kind) else { return }
        promptFocused = false
        errorMessage = nil
        resetAll()
        activeInput = input
        let generation = scope.generation
        let operation = scope.begin("make")
        let apiKey = session.apiKey
        makingStep = 0
        isMaking = true
        workTasks["make"] = Task {
            do {
                let r = try await MUAPI.make(prompt: input.prompt, kind: input.kind, royalty: input.royalty, apiKey: apiKey)
                guard accepts(generation, "make", operation) else { return }
                Analytics.track("make_create", ["kind": r.kind, "sku": r.sku, "royalty": input.royalty])
                isMaking = false
                withAnimation(.spring(response: 0.5, dampingFraction: 0.7)) {
                    variants = [DesignVariant(result: r)]
                    current = 0
                }
                successHaptic()
                withAnimation(.spring(response: 0.6, dampingFraction: 0.6).delay(0.05)) { revealed = true }
                maybePromptPush()
                maybeRequestReview()
                startPolling(sku: r.sku)
                performVariation(input, automatic: true)
            } catch APIError.needRegister {
                guard accepts(generation, "make", operation) else { return }
                isMaking = false
                pendingIntent = .create(input)
                showAuthGate = true
            } catch {
                guard accepts(generation, "make", operation) else { return }
                errorMessage = error.localizedDescription; isMaking = false
            }
        }
    }

    // One polling task per SKU, bounded at five minutes (50 x 6 seconds).
    private func startPolling(sku: String) {
        let key = "peek:\(sku)"
        cancelOperation(key)
        guard let i = variantIndex(sku) else { return }
        variants[i].previewPending = true
        variants[i].previewTimedOut = false
        let generation = scope.generation
        let operation = scope.begin(key)
        workTasks[key] = Task {
            for _ in 0..<50 {
                do { try await Task.sleep(nanoseconds: 6_000_000_000) } catch { return }
                guard accepts(generation, key, operation), variantIndex(sku) != nil else { return }
                do {
                    let peek = try await MUAPI.peek(sku: sku)
                    guard accepts(generation, key, operation), let i = variantIndex(sku) else { return }
                    let complete = variants[i].applyPeek(peek)
                    // Review status may become live after the preview is ready.
                    if complete && variants[i].result.isLive { return }
                } catch {
                    guard accepts(generation, key, operation) else { return }
                    // Read-only polling can tolerate transient failure until the deadline.
                }
            }
            guard accepts(generation, key, operation), let i = variantIndex(sku) else { return }
            variants[i].previewTimedOut = variants[i].previewPending
        }
    }

    // 成功ハプティクスは prepare してから発火(不発を避ける)。
    private func successHaptic() {
        let gen = UINotificationFeedbackGenerator()
        gen.prepare()
        gen.notificationOccurred(.success)
    }
}

// 2026-08-28: 作るには登録必須(無断使用/著作権侵害の防止・1人1日5点まで)。
// 既存の ClosetView.AuthView(メール→6桁コード)をそのまま再利用し、
// ログインが成立したら自動で閉じる(呼び出し側の onDismiss が作成を再試行)。
private struct AuthGateSheet: View {
    @EnvironmentObject private var session: Session
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 4) {
                Text(String(localized: "make.registerGate.title"))
                    .font(.headline)
                Text(String(localized: "make.registerGate.subtitle"))
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            .padding([.horizontal, .top])
            AuthView()
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button(String(localized: "make.cancel")) { dismiss() }
                    }
                }
        }
        .onChange(of: session.isLoggedIn) { _, loggedIn in if loggedIn { dismiss() } }
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
