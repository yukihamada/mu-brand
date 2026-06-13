import SwiftUI

// 一発でファンになる初回オンボーディング。スライドで説得するのではなく、
// 「本物の新作」を見せ(=生きている証明)、最後にその場で“最初の一着”を作らせる。
// MU の魔法 = 言えば作れる。そのアハ体験までを 20 秒で届ける。
struct OnboardingView: View {
    @EnvironmentObject private var app: AppState
    @AppStorage("hasOnboarded") private var hasOnboarded = false

    @State private var page = 0
    @State private var hero: [FeedProduct] = []
    @State private var prompt = ""
    @State private var typed = ""   // タイプライター演出用

    // 1ページ目で出す“言ってみて”の例(タップで3ページ目に流し込む)
    private let seeds = [
        "make.example1", "make.example2", "make.example3",
    ]

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
            hero = (try? await MUAPI.feed(page: 1, kind: .all)) ?? []
            Analytics.track("onboarding_open")
            runTypewriter()
        }
        .preferredColorScheme(.dark)
        .tint(gold)
    }

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

            // 入力(タイプライターでお手本が流れる → そのまま作れる)
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
                let final = text.isEmpty ? String(localized: "make.example1") : text
                Analytics.track("onboarding_make", ["seeded": text.isEmpty])
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
            Spacer()
        }
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
        let target = String(localized: "make.example1")
        typed = ""
        Task {
            for ch in target {
                typed.append(ch)
                try? await Task.sleep(nanoseconds: 45_000_000)
            }
        }
    }
}
