import SwiftUI

// 🤖 エージェント — アプリ内 AI。会話で MU を操作する(LLMが意図判定→MUアクション実行)。
// 「柔術の黒Tつくって」→ 作る / 「今月いくら売れた?」→ 売上 / 雑談もOK。
struct AgentView: View {
    @EnvironmentObject private var session: Session
    @StateObject private var voice = VoiceInput()
    @State private var input = ""
    @State private var voiceBase = ""
    @State private var messages: [ChatMessage] = []
    @State private var sending = false
    @State private var showCheckout: String?
    @State private var showAIConsent = false
    @FocusState private var focused: Bool

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 12) {
                            if messages.isEmpty { intro }
                            ForEach(messages) { m in bubble(m).id(m.id) }
                            if sending { typing }
                        }
                        .padding()
                    }
                    .onChange(of: messages.count) { _, _ in
                        if let last = messages.last { withAnimation { proxy.scrollTo(last.id, anchor: .bottom) } }
                    }
                }
                inputBar
                Text(String(localized: "make.creationCost"))
                    .font(.caption2).foregroundStyle(.secondary).padding(.horizontal)
            }
            .navigationTitle(String(localized: "tab.agent"))
            .task { Analytics.track("view_agent") }
            .onDisappear { voice.stop() }
            // Every authentication transition, including the first login
            // (nil -> email), rotates the generation. Clearing on the generation
            // rather than on the email keeps `sending` from getting stuck: a reply
            // captured before the change is always rejected, so nothing else can
            // clear the flag.
            .onChange(of: session.identityGeneration) { resetForNewIdentity() }
            .onChange(of: voice.transcript) { _, t in if !t.isEmpty { input = voiceBase + t } }
            .onChange(of: voice.isRecording) { _, rec in if rec { voiceBase = input.isEmpty ? "" : input + " " } }
            .sheet(item: Binding(get: { showCheckout.map { IdentifiedURL(url: $0) } },
                                 set: { showCheckout = $0?.url })) { item in
                if let url = URL(string: item.url) { SafariView(url: url).ignoresSafeArea() }
            }
            .aiConsentAlert(isPresented: $showAIConsent) { performSend() }
        }
    }

    private var intro: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(String(localized: "agent.intro.title")).font(.title3.bold())
            Text(String(localized: "agent.intro.sub")).font(.subheadline).foregroundStyle(.secondary)
            ForEach(["agent.ex1", "agent.ex2", "agent.ex3"], id: \.self) { key in
                Button {
                    input = String(localized: String.LocalizationValue(key))
                    send()
                } label: {
                    HStack { Image(systemName: "sparkles").font(.caption)
                        Text(String(localized: String.LocalizationValue(key))); Spacer() }
                    .font(.subheadline).padding(12)
                    .background(.quaternary.opacity(0.3), in: RoundedRectangle(cornerRadius: 12))
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.top, 20)
    }

    private var typing: some View {
        HStack { ProgressView(); Text(String(localized: "agent.thinking")).font(.footnote).foregroundStyle(.secondary) }
    }

    @ViewBuilder
    private func bubble(_ m: ChatMessage) -> some View {
        if m.role == .user {
            HStack { Spacer()
                Text(m.text).padding(10)
                    .background(.tint, in: RoundedRectangle(cornerRadius: 14)).foregroundStyle(.black)
            }
        } else {
            VStack(alignment: .leading, spacing: 8) {
                if !m.text.isEmpty {
                    Text(m.text).padding(10)
                        .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 14))
                }
                if let p = m.product { productCard(p) }
                if let s = m.sales { salesCard(s) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func productCard(_ r: MakeResult) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            AsyncImage(url: r.designURL) { phase in
                switch phase { case .success(let img): img.resizable().scaledToFit()
                default: Rectangle().fill(.quaternary).frame(height: 200) }
            }
            .frame(maxWidth: .infinity).clipShape(RoundedRectangle(cornerRadius: 12))
            Text(r.hook).font(.subheadline.weight(.medium))
            HStack {
                Text(r.priceLabel).font(.headline)
                Spacer()
                if let c = r.checkoutUrl {
                    Button(String(localized: "pdp.buy")) { showCheckout = c }
                        .buttonStyle(.borderedProminent).foregroundStyle(.black).controlSize(.small)
                }
            }
        }
        .padding(10).background(.quaternary.opacity(0.2), in: RoundedRectangle(cornerRadius: 14))
    }

    private func salesCard(_ s: SalesResponse) -> some View {
        HStack(spacing: 16) {
            VStack { Text("\(s.total?.orderCount ?? 0)").font(.title2.bold())
                Text(String(localized: "account.orders")).font(.caption2).foregroundStyle(.secondary) }
            VStack { Text("¥\((s.total?.revenueJpy ?? 0).formatted())").font(.title2.bold())
                Text(String(localized: "account.revenue")).font(.caption2).foregroundStyle(.secondary) }
        }
        .frame(maxWidth: .infinity).padding(14)
        .background(.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 14))
    }

    private var inputBar: some View {
        HStack(spacing: 8) {
            TextField(String(localized: "agent.placeholder"), text: $input, axis: .vertical)
                .lineLimit(1...4).padding(10)
                .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 18))
                .focused($focused).disabled(sending)
                .accessibilityIdentifier("agent.input")
            Button { Task { await voice.toggle() } } label: {
                Image(systemName: voice.isRecording ? "waveform.circle.fill" : "mic.circle.fill")
                    .font(.system(size: 30)).foregroundStyle(voice.isRecording ? AnyShapeStyle(.red) : AnyShapeStyle(.tint))
            }
            Button { send() } label: { Image(systemName: "arrow.up.circle.fill").font(.system(size: 30)) }
                .disabled(sending || input.trimmingCharacters(in: .whitespaces).isEmpty)
                .accessibilityIdentifier("agent.send")
        }
        .padding(.horizontal).padding(.vertical, 8)
        .background(.bar)
    }

    private func send() {
        let text = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !sending else { return }
        // 入力テキストは意図判定のためGemini(AI)へ送信される。初回のみ同意を取る。
        guard AIConsent.given else { showAIConsent = true; return }
        performSend()
    }

    private func performSend() {
        let text = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !sending else { return }
        if voice.isRecording { voice.stop() }
        focused = false
        messages.append(ChatMessage(role: .user, text: text))
        input = ""
        sending = true
        let history = messages.suffix(8).map { ["role": $0.role == .user ? "user" : "assistant", "content": $0.text] }
        // Snapshot the key and the account at send time. A reply that arrives after
        // the account changed belongs to the old account and must be discarded.
        let apiKey = session.apiKey
        // Generation, not the email: A -> B -> A must not accept an old reply.
        let sentGeneration = session.identityGeneration
        Task {
            do {
                let res = try await MUAPI.agentChat(message: text, history: history, apiKey: apiKey)
                guard await acceptsGeneration(sentGeneration) else { return }
                Analytics.track("agent_chat", ["action": res.action])
                await execute(res, apiKey: apiKey, sentGeneration: sentGeneration)
            } catch {
                guard await acceptsGeneration(sentGeneration) else { return }
                await MainActor.run {
                    messages.append(ChatMessage(role: .assistant, text: error.localizedDescription))
                    sending = false
                }
            }
        }
    }

    // 意図に応じて MU アクションを実行(= MCP と同じ操作群)。
    private func execute(_ res: AgentChatResponse, apiKey: String?, sentGeneration: UUID) async {
        guard await acceptsGeneration(sentGeneration) else { return }
        await MainActor.run { messages.append(ChatMessage(role: .assistant, text: res.reply)) }
        switch res.action {
        case "make":
            let prompt = res.args?.prompt ?? ""
            guard !prompt.isEmpty else { break }
            let kind = res.args?.kind ?? ""
            let royalty = res.args?.royalty ?? 10
            let kinds = (try? await MUAPI.makeKinds()) ?? MakeKindOption.fallback
            guard await acceptsGeneration(sentGeneration) else { return }
            // Never silently turn an unsupported explicit kind into an auto-picked product.
            guard MakeKindOption.isAvailable(kind, in: kinds) else {
                await MainActor.run {
                    let reason = kinds.first { $0.kind == kind }?.unavailableMessage()
                    messages.append(ChatMessage(role: .assistant, text: reason ?? String(localized: "make.kind.unavailableHint")))
                }
                break
            }
            do {
                // Snapshot key: never create under a key read after the account changed.
                let r = try await MUAPI.make(prompt: prompt, kind: kind, royalty: royalty, apiKey: apiKey)
                guard await acceptsGeneration(sentGeneration) else { return }
                await MainActor.run { messages.append(ChatMessage(role: .assistant, text: "", product: r)) }
            } catch {
                guard await acceptsGeneration(sentGeneration) else { return }
                await MainActor.run { messages.append(ChatMessage(role: .assistant, text: error.localizedDescription)) }
            }
        case "sales":
            if let key = apiKey, let s = try? await MUAPI.sales(apiKey: key) {
                guard await acceptsGeneration(sentGeneration) else { return }
                await MainActor.run { messages.append(ChatMessage(role: .assistant, text: "", sales: s)) }
            } else {
                guard await acceptsGeneration(sentGeneration) else { return }
                await MainActor.run { messages.append(ChatMessage(role: .assistant, text: String(localized: "agent.loginNeeded"))) }
            }
        default:
            break
        }
        guard await acceptsGeneration(sentGeneration) else { return }
        await MainActor.run { sending = false }
    }

    /// Drops everything owned by the previous identity: transcript, pending
    /// consent, in-progress send state and voice capture.
    private func resetForNewIdentity() {
        voice.stop()
        voiceBase = ""
        messages.removeAll()
        input = ""
        sending = false
        showCheckout = nil
        showAIConsent = false
    }

    /// Rejects anything started under a previous identity generation. The first
    /// login (nil -> an email) keeps working because it never had prior state.
    private func acceptsGeneration(_ sentGeneration: UUID) async -> Bool {
        await MainActor.run { session.identityGeneration == sentGeneration }
    }
}

struct ChatMessage: Identifiable {
    enum Role { case user, assistant }
    let id = UUID()
    let role: Role
    var text: String
    var product: MakeResult? = nil
    var sales: SalesResponse? = nil
}

private struct IdentifiedURL: Identifiable { let url: String; var id: String { url } }
