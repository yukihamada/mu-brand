import SwiftUI

// 一発でファンになる初回オンボーディング。スライドで説得するのではなく、
// 「本物の新作」を見せ(=生きている証明)、最後にその場で“最初の一着”を作らせる。
// MU の魔法 = 言えば作れる。そのアハ体験までを 20 秒で届ける。
struct OnboardingView: View {
    @EnvironmentObject private var app: AppState
    @EnvironmentObject private var session: Session
    @AppStorage("hasOnboarded") private var hasOnboarded = false

    @State private var page = 0
    @State private var hero: [FeedProduct] = []
    @State private var prompt = ""
    @State private var typed = ""   // タイプライター演出用

    // 2026-08-28: /api/make が登録必須になったため、doPage の「作る」を押した
    // 瞬間に登録シートへ中断されないよう、doPage の中で先に済ませてしまう。
    // (サプライズで割り込む登録より、最初から織り込まれた登録の方が離脱しない)
    @State private var obEmail = ""
    @State private var obCode = ""
    @State private var obCodeSent = false
    @State private var obBusy = false
    @State private var obError: String?

    // 最初の一着の“幸せプロンプト”候補(A/B/C)。決め打ちでなくランダム割当で
    // どれが活性化に効くかを計測(科学的にデータで勝者を決める)。
    private let seeds = ["onb.seed.a", "onb.seed.b", "onb.seed.c"]
    // 割当バリアントは端末で固定(同じユーザーは毎回同じ初期値=計測がブレない)。
    @AppStorage("onbSeedVariant") private var seedVariant = -1
    @State private var seededDefault = false

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()

            TabView(selection: $page) {
                hookPage.tag(0)
                proofPage.tag(1)
                doPage.tag(2)
            }
            .tabViewStyle(.page(indexDisplayMode: .always))
            .indexViewStyle(.page(backgroundDisplayMode: .always))

            // スキップ
            VStack {
                HStack {
                    Spacer()
                    Button(String(localized: "onb.skip")) { finish(nil) }
                        .font(.subheadline)
                        .foregroundStyle(.white.opacity(0.6))
                        .padding(.trailing, 18).padding(.top, 8)
                }
                Spacer()
            }
        }
        .task {
            // A/B: 初回だけランダムにバリアント割当(端末固定)。どの“幸せプロンプト”が
            // 活性化に効くかをデータで決める(決め打ちでなく科学的に)。
            if seedVariant < 0 { seedVariant = Int.random(in: 0..<seeds.count) }
            if !seededDefault {
                seededDefault = true
                if prompt.isEmpty { prompt = String(localized: String.LocalizationValue(defaultSeedKey)) }
            }
            hero = (try? await MUAPI.feed(page: 1, kind: .all)) ?? []
            Analytics.track("onboarding_open", ["variant": seedVariant])
            runTypewriter()
        }
        .preferredColorScheme(.dark)
        .tint(gold)
    }

    // 割当バリアントの既定プロンプト(0..<seeds.count に丸める)。
    private var defaultSeedKey: String { seeds[max(0, min(seedVariant, seeds.count - 1))] }

    private let gold = Color(red: 0.90, green: 0.77, blue: 0.29)

    // ── 1. HOOK: 言えば、作れる。 ──
    private var hookPage: some View {
        VStack(spacing: 0) {
            Spacer()
            heroImage(hero.first)
                .frame(height: 300)
                .clipShape(RoundedRectangle(cornerRadius: 24))
                .padding(.horizontal, 28)
                .shadow(color: gold.opacity(0.25), radius: 30)
            Spacer()
            VStack(spacing: 12) {
                Text("MU")
                    .font(.system(size: 18, weight: .black))
                    .tracking(8)
                    .foregroundStyle(gold)
                Text(String(localized: "onb.hook.title"))
                    .font(.system(size: 34, weight: .heavy))
                    .foregroundStyle(.white)
                Text(String(localized: "onb.hook.sub"))
                    .font(.body)
                    .foregroundStyle(.white.opacity(0.7))
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 36)
            }
            Spacer()
            nextButton(to: 1, label: String(localized: "onb.next"))
            Spacer().frame(height: 60)
        }
    }

    // ── 2. PROOF: 毎時、新作が生まれている。 ──
    private var proofPage: some View {
        VStack(spacing: 20) {
            Spacer().frame(height: 60)
            VStack(spacing: 8) {
                Text(String(localized: "onb.proof.title"))
                    .font(.system(size: 28, weight: .heavy))
                    .foregroundStyle(.white)
                    .multilineTextAlignment(.center)
                Text(String(localized: "onb.proof.sub"))
                    .font(.subheadline)
                    .foregroundStyle(.white.opacity(0.7))
            }
            .padding(.horizontal, 28)

            // 本物の新作を流す(生きている証明)
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 12) {
                    ForEach(hero.prefix(8)) { p in
                        heroImage(p)
                            .frame(width: 150, height: 195)
                            .clipShape(RoundedRectangle(cornerRadius: 14))
                    }
                }
                .padding(.horizontal, 28)
            }
            Spacer()
            nextButton(to: 2, label: String(localized: "onb.proof.cta"))
            Spacer().frame(height: 60)
        }
    }

    // ── 3. DO: 最初の一着を、いま。 ──
    private var doPage: some View {
        VStack(spacing: 18) {
            Spacer()
            Text(String(localized: "onb.do.title"))
                .font(.system(size: 30, weight: .heavy))
                .foregroundStyle(.white)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 28)
            Text(String(localized: "onb.do.sub"))
                .font(.subheadline)
                .foregroundStyle(.white.opacity(0.7))
                .multilineTextAlignment(.center)
                .padding(.horizontal, 36)

            if session.isLoggedIn {
                composeFields
            } else {
                registerFields
            }
            Spacer()
        }
    }

    // 作る本体: 入力(タイプライターでお手本が流れる)+お手本チップ+作るボタン。
    @ViewBuilder
    private var composeFields: some View {
        TextField("", text: $prompt, prompt: Text(typed).foregroundColor(.white.opacity(0.4)), axis: .vertical)
            .lineLimit(2...4)
            .font(.body)
            .foregroundStyle(.white)
            .padding(14)
            .background(.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 14))
            .overlay(RoundedRectangle(cornerRadius: 14).stroke(gold.opacity(0.4), lineWidth: 1))
            .padding(.horizontal, 28)

        // お手本チップ
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(seeds, id: \.self) { key in
                    Button {
                        prompt = String(localized: String.LocalizationValue(key))
                        Analytics.track("onboarding_seed_pick", ["seed": key])
                    } label: {
                        Text(String(localized: String.LocalizationValue(key)))
                            .font(.caption)
                            .lineLimit(1)
                            .padding(.horizontal, 12).padding(.vertical, 7)
                            .background(.white.opacity(0.08), in: Capsule())
                            .foregroundStyle(.white.opacity(0.85))
                    }
                }
            }
            .padding(.horizontal, 28)
        }

        Button {
            let text = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
            let final = text.isEmpty ? String(localized: String.LocalizationValue(defaultSeedKey)) : text
            // 計測: どのバリアント/プロンプトで作ったか(後で購入率まで追える)
            Analytics.track("onboarding_make", ["variant": seedVariant, "prompt": final])
            finish(final)
        } label: {
            HStack {
                Image(systemName: "sparkles")
                Text(String(localized: "onb.do.make"))
            }
            .font(.headline)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 8)
        }
        .buttonStyle(.borderedProminent)
        .tint(gold)
        .foregroundStyle(.black)
        .padding(.horizontal, 28)
    }

    // 登録(メール→6桁コード・パスワードなし)。作るボタンの手前で先に済ませ、
    // 押した瞬間に別シートへ中断されないようにする(=離脱ポイントを1つ減らす)。
    @ViewBuilder
    private var registerFields: some View {
        Text(String(localized: "onb.register.why"))
            .font(.caption)
            .foregroundStyle(.white.opacity(0.55))
            .multilineTextAlignment(.center)
            .padding(.horizontal, 32)

        if !obCodeSent {
            TextField(String(localized: "auth.email"), text: $obEmail)
                .keyboardType(.emailAddress)
                .textContentType(.emailAddress)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .foregroundStyle(.white)
                .padding(14)
                .background(.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 14))
                .overlay(RoundedRectangle(cornerRadius: 14).stroke(gold.opacity(0.4), lineWidth: 1))
                .padding(.horizontal, 28)

            Button {
                Task { await obSendCode() }
            } label: {
                if obBusy { ProgressView().tint(.black) }
                else { Text(String(localized: "auth.sendCode")).font(.headline).frame(maxWidth: .infinity).padding(.vertical, 8) }
            }
            .buttonStyle(.borderedProminent)
            .tint(gold)
            .foregroundStyle(.black)
            .padding(.horizontal, 28)
            .disabled(obBusy || obEmail.trimmingCharacters(in: .whitespaces).isEmpty)
        } else {
            TextField(String(localized: "auth.code"), text: $obCode)
                .keyboardType(.numberPad)
                .textContentType(.oneTimeCode)
                .multilineTextAlignment(.center)
                .foregroundStyle(.white)
                .padding(14)
                .background(.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 14))
                .overlay(RoundedRectangle(cornerRadius: 14).stroke(gold.opacity(0.4), lineWidth: 1))
                .padding(.horizontal, 28)

            Button {
                Task { await obVerifyCode() }
            } label: {
                if obBusy { ProgressView().tint(.black) }
                else { Text(String(localized: "auth.verify")).font(.headline).frame(maxWidth: .infinity).padding(.vertical, 8) }
            }
            .buttonStyle(.borderedProminent)
            .tint(gold)
            .foregroundStyle(.black)
            .padding(.horizontal, 28)
            .disabled(obBusy || obCode.trimmingCharacters(in: .whitespaces).isEmpty)

            Button(String(localized: "auth.restart")) {
                obCodeSent = false; obCode = ""; obError = nil
            }
            .font(.footnote)
            .foregroundStyle(.white.opacity(0.6))
        }

        if let obError {
            Text(obError).font(.caption).foregroundStyle(.red.opacity(0.85)).multilineTextAlignment(.center).padding(.horizontal, 28)
        }
    }

    private func obSendCode() async {
        let email = obEmail.trimmingCharacters(in: .whitespacesAndNewlines)
        guard email.contains("@") else { obError = String(localized: "auth.email"); return }
        obBusy = true; obError = nil
        do {
            try await MUAPI.register(email: email)
            obCodeSent = true
            Analytics.track("onboarding_register_code_sent")
        } catch {
            obError = error.localizedDescription
        }
        obBusy = false
    }

    private func obVerifyCode() async {
        let email = obEmail.trimmingCharacters(in: .whitespacesAndNewlines)
        let code = obCode.trimmingCharacters(in: .whitespacesAndNewlines)
        obBusy = true; obError = nil
        do {
            let key = try await MUAPI.verify(email: email, code: code)
            session.logIn(email: email, apiKey: key)
            Analytics.track("onboarding_register_done")
        } catch {
            obError = error.localizedDescription
        }
        obBusy = false
    }

    // ── parts ──
    @ViewBuilder
    private func heroImage(_ p: FeedProduct?) -> some View {
        if let p, let url = p.mockupURL {
            AsyncImage(url: url) { phase in
                switch phase {
                case .success(let img): img.resizable().scaledToFill()
                default: placeholder
                }
            }
        } else {
            placeholder
        }
    }
    private var placeholder: some View {
        ZStack {
            Color.white.opacity(0.06)
            Image(systemName: "tshirt").font(.system(size: 44)).foregroundStyle(.white.opacity(0.25))
        }
    }

    private func nextButton(to target: Int, label: String) -> some View {
        Button {
            withAnimation { page = target }
        } label: {
            Text(label)
                .font(.headline)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 8)
        }
        .buttonStyle(.borderedProminent)
        .tint(gold)
        .foregroundStyle(.black)
        .padding(.horizontal, 28)
    }

    private func finish(_ makePrompt: String?) {
        hasOnboarded = true
        if let p = makePrompt { app.startMake(p) }
        Analytics.track("onboarding_done", ["made": makePrompt != nil])
    }

    // 入力欄プレースホルダのタイプライター演出(お手本が“勝手に書かれていく”)
    private func runTypewriter() {
        let target = String(localized: String.LocalizationValue(defaultSeedKey))
        typed = ""
        Task {
            for ch in target {
                typed.append(ch)
                try? await Task.sleep(nanoseconds: 45_000_000)
            }
        }
    }
}
