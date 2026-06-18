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
            }
            .navigationTitle(String(localized: "tab.agent"))
            .task { Analytics.track("view_agent") }
            .onChange(of: voice.transcript) { _, t in if !t.isEmpty { input = voiceBase + t } }
            .onChange(of: voice.isRecording) { _, rec in if rec { voiceBase = input.isEmpty ? "" : input + " " } }
            .sheet(item: Binding(get: { showCheckout.map { IdentifiedURL(url: $0) } },
                                 set: { showCheckout = $0?.url })) { item in
                if let url = URL(string: item.url) { SafariView(url: url).ignoresSafeArea() }
            }
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
                if let ps = m.products { productsCard(ps) }
                if let me = m.me { meCard(me) }
                if let a = m.affiliate { affiliateCard(a) }
                if let sh = m.ship { shipCard(sh) }
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

    // list_mine — 自分が作った商品の一覧 (上位を縦に)。
    private func productsCard(_ r: ProductsResponse) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("\(r.count ?? r.products.count)").font(.title2.bold())
                + Text(" " + String(localized: "agent.products.count")).font(.caption).foregroundColor(.secondary)
            ForEach(r.products.prefix(10)) { p in
                Button {
                    if let u = p.pdpUrl, !u.isEmpty { showCheckout = u }
                } label: {
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(p.label ?? p.sku).font(.subheadline.weight(.medium)).lineLimit(1)
                            Text(p.sku).font(.caption2).foregroundStyle(.secondary)
                        }
                        Spacer()
                        VStack(alignment: .trailing, spacing: 2) {
                            if !p.priceLabel.isEmpty { Text(p.priceLabel).font(.subheadline) }
                            if let s = p.status { Text(s).font(.caption2).foregroundStyle(.secondary) }
                        }
                    }
                }
                .buttonStyle(.plain)
                .padding(.vertical, 4)
                if p.id != r.products.prefix(10).last?.id { Divider() }
            }
        }
        .padding(12).frame(maxWidth: .infinity, alignment: .leading)
        .background(.quaternary.opacity(0.2), in: RoundedRectangle(cornerRadius: 14))
    }

    // status — アカウント状態。
    private func meCard(_ m: MeResponse) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            if let e = m.email { Text(e).font(.subheadline.weight(.medium)) }
            HStack(spacing: 16) {
                VStack { Text("\(m.muCreditsBalance ?? 0)").font(.title3.bold())
                    Text(String(localized: "agent.credits")).font(.caption2).foregroundStyle(.secondary) }
                VStack { Text("\(m.stores?.count ?? 0)").font(.title3.bold())
                    Text(String(localized: "agent.stores")).font(.caption2).foregroundStyle(.secondary) }
            }
        }
        .padding(14).frame(maxWidth: .infinity, alignment: .leading)
        .background(.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 14))
    }

    // affiliate — 紹介リンクと実績。
    private func affiliateCard(_ a: AffiliateResponse) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            if let link = a.link, !link.isEmpty {
                Button { showCheckout = link } label: {
                    HStack { Image(systemName: "link"); Text(link).lineLimit(1) }.font(.footnote)
                }.buttonStyle(.plain)
            } else if let code = a.code {
                Text(code).font(.subheadline.weight(.medium))
            }
            HStack(spacing: 16) {
                VStack { Text("\(a.clicks ?? 0)").font(.headline)
                    Text(String(localized: "agent.clicks")).font(.caption2).foregroundStyle(.secondary) }
                VStack { Text("\(a.uses ?? 0)").font(.headline)
                    Text(String(localized: "agent.uses")).font(.caption2).foregroundStyle(.secondary) }
                VStack { Text("¥\((a.earnedJpy ?? 0).formatted())").font(.headline)
                    Text(String(localized: "agent.earned")).font(.caption2).foregroundStyle(.secondary) }
            }
        }
        .padding(14).frame(maxWidth: .infinity, alignment: .leading)
        .background(.tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 14))
    }

    // ship — 配送状況 (PIIマスク済み: sku/状態/配送業者/追跡/金額のみ)。
    private func shipCard(_ s: ShipOrdersResponse) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("\(s.count ?? s.orders.count)").font(.title2.bold())
                + Text(" " + String(localized: "agent.ship.count")).font(.caption).foregroundColor(.secondary)
            ForEach(s.orders.prefix(10)) { o in
                VStack(alignment: .leading, spacing: 2) {
                    HStack {
                        Text(o.sku ?? "—").font(.subheadline.weight(.medium)).lineLimit(1)
                        Spacer()
                        if !o.amountLabel.isEmpty { Text(o.amountLabel).font(.subheadline) }
                    }
                    HStack(spacing: 8) {
                        if let st = o.shipStatus { Text(st).font(.caption2).foregroundStyle(.secondary) }
                        if let c = o.courier, !c.isEmpty { Text(c).font(.caption2).foregroundStyle(.secondary) }
                        if let t = o.tracking, !t.isEmpty { Text(t).font(.caption2).foregroundStyle(.secondary).lineLimit(1) }
                    }
                }
                .padding(.vertical, 4)
                if o.id != s.orders.prefix(10).last?.id { Divider() }
            }
        }
        .padding(12).frame(maxWidth: .infinity, alignment: .leading)
        .background(.quaternary.opacity(0.2), in: RoundedRectangle(cornerRadius: 14))
    }

    private func loginNeeded() async {
        await MainActor.run { messages.append(ChatMessage(role: .assistant, text: String(localized: "agent.loginNeeded"))) }
    }

    private var inputBar: some View {
        HStack(spacing: 8) {
            TextField(String(localized: "agent.placeholder"), text: $input, axis: .vertical)
                .lineLimit(1...4).padding(10)
                .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 18))
                .focused($focused).disabled(sending)
            Button { Task { await voice.toggle() } } label: {
                Image(systemName: voice.isRecording ? "waveform.circle.fill" : "mic.circle.fill")
                    .font(.system(size: 30)).foregroundStyle(voice.isRecording ? AnyShapeStyle(.red) : AnyShapeStyle(.tint))
            }
            Button { send() } label: { Image(systemName: "arrow.up.circle.fill").font(.system(size: 30)) }
                .disabled(sending || input.trimmingCharacters(in: .whitespaces).isEmpty)
        }
        .padding(.horizontal).padding(.vertical, 8)
        .background(.bar)
    }

    private func send() {
        let text = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !sending else { return }
        if voice.isRecording { voice.stop() }
        focused = false
        messages.append(ChatMessage(role: .user, text: text))
        input = ""
        sending = true
        let history = messages.suffix(8).map { ["role": $0.role == .user ? "user" : "assistant", "content": $0.text] }
        Task {
            do {
                let res = try await MUAPI.agentChat(message: text, history: history, apiKey: session.apiKey)
                Analytics.track("agent_chat", ["action": res.action])
                await execute(res)
            } catch {
                await MainActor.run {
                    messages.append(ChatMessage(role: .assistant, text: error.localizedDescription))
                    sending = false
                }
            }
        }
    }

    // 意図に応じて MU アクションを実行(= MCP と同じ操作群)。
    private func execute(_ res: AgentChatResponse) async {
        await MainActor.run { messages.append(ChatMessage(role: .assistant, text: res.reply)) }
        switch res.action {
        case "make":
            let prompt = res.args?.prompt ?? ""
            guard !prompt.isEmpty else { break }
            let kind = MakeKind(rawValue: res.args?.kind ?? "") ?? .auto
            let royalty = res.args?.royalty ?? 10
            if let r = try? await MUAPI.make(prompt: prompt, kind: kind, royalty: royalty, apiKey: session.apiKey) {
                await MainActor.run { messages.append(ChatMessage(role: .assistant, text: "", product: r)) }
            } else {
                await MainActor.run { messages.append(ChatMessage(role: .assistant, text: String(localized: "agent.makeFail"))) }
            }
        case "polish":
            guard let key = session.apiKey else { await loginNeeded(); break }
            let sku = res.args?.sku ?? ""
            guard !sku.isEmpty else { break }
            // polish はサーバ側で edit token を要求する。アプリには無いので、
            // reply の案内文に委ねる (即時実行はしない)。
            _ = key
        case "remix":
            guard let key = session.apiKey else { await loginNeeded(); break }
            let sku = res.args?.sku ?? ""
            let words = res.args?.words ?? ""
            guard !sku.isEmpty, !words.isEmpty else { break }
            if let r = try? await MUAPI.remix(sku: sku, words: words, apiKey: key) {
                await MainActor.run { messages.append(ChatMessage(role: .assistant, text: "", product: r)) }
            } else {
                await MainActor.run { messages.append(ChatMessage(role: .assistant, text: String(localized: "agent.makeFail"))) }
            }
        case "sales":
            guard let key = session.apiKey else { await loginNeeded(); break }
            if let s = try? await MUAPI.sales(apiKey: key) {
                await MainActor.run { messages.append(ChatMessage(role: .assistant, text: "", sales: s)) }
            }
        case "list_mine":
            guard let key = session.apiKey else { await loginNeeded(); break }
            if let ps = try? await MUAPI.listMine(apiKey: key) {
                await MainActor.run { messages.append(ChatMessage(role: .assistant, text: "", products: ps)) }
            }
        case "status":
            guard let key = session.apiKey else { await loginNeeded(); break }
            if let me = try? await MUAPI.status(apiKey: key) {
                await MainActor.run { messages.append(ChatMessage(role: .assistant, text: "", me: me)) }
            }
        case "affiliate":
            guard let key = session.apiKey else { await loginNeeded(); break }
            if let a = try? await MUAPI.affiliate(apiKey: key) {
                await MainActor.run { messages.append(ChatMessage(role: .assistant, text: "", affiliate: a)) }
            }
        case "ship":
            guard let key = session.apiKey else { await loginNeeded(); break }
            if let sh = try? await MUAPI.shipOrders(apiKey: key) {
                await MainActor.run { messages.append(ChatMessage(role: .assistant, text: "", ship: sh)) }
            }
        default:
            break
        }
        await MainActor.run { sending = false }
    }
}

struct ChatMessage: Identifiable {
    enum Role { case user, assistant }
    let id = UUID()
    let role: Role
    var text: String
    var product: MakeResult? = nil
    var sales: SalesResponse? = nil
    var products: ProductsResponse? = nil
    var me: MeResponse? = nil
    var affiliate: AffiliateResponse? = nil
    var ship: ShipOrdersResponse? = nil
}

private struct IdentifiedURL: Identifiable { let url: String; var id: String { url } }
