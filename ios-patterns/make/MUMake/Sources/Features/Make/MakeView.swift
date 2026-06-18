import SwiftUI

// ✨ Make — このアプリの主役。画面まるごとが「何が欲しい?」の入力。
// 言葉 → /api/make (実API・匿名可) → 数十秒で買える一点ものが棚に並ぶ。
struct MakeView: View {
    let switchTab: (AppTab) -> Void

    @EnvironmentObject private var session: Session
    @EnvironmentObject private var history: MakeHistory
    @StateObject private var flow = MakeFlow()

    @State private var prompt = ""
    @State private var kind: MakeKind = .auto
    @State private var recent: [RecentMake] = []
    @State private var recentFailed = false
    @State private var recentLoading = false
    @State private var recentDetail: RecentMake?
    @FocusState private var promptFocused: Bool

    private var canMake: Bool {
        !prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !flow.isGenerating
    }

    var body: some View {
        NavigationStack {
            ZStack {
                MUTheme.bg.ignoresSafeArea()
                ScrollView {
                    VStack(alignment: .leading, spacing: 22) {
                        header
                        promptCard
                        kindChips
                        suggestionChips
                        makeButton
                        recentRail
                    }
                    .padding(.horizontal, 20)
                    .padding(.top, 8)
                    .padding(.bottom, 32)
                }
                .scrollDismissesKeyboard(.interactively)
                .refreshable { await loadRecent() }

                if flow.isGenerating {
                    GeneratingView(prompt: flow.lastPrompt) { flow.cancel() }
                        .transition(.opacity)
                        .zIndex(1)
                }
            }
            .animation(.easeInOut(duration: 0.3), value: flow.isGenerating)
            .toolbar(flow.isGenerating ? .hidden : .visible, for: .tabBar)
        }
        .fullScreenCover(item: $flow.result) { result in
            MakeResultView(result: result, prompt: flow.lastPrompt) {
                flow.result = nil
            }
        }
        .alert(
            String(localized: "make.failedTitle"),
            isPresented: Binding(
                get: { if case .failed = flow.phase { return true } else { return false } },
                set: { if !$0 { flow.phase = .idle } }
            )
        ) {
            Button(String(localized: "common.ok"), role: .cancel) { flow.phase = .idle }
        } message: {
            if case .failed(let message) = flow.phase { Text(message) }
        }
        .task {
            await loadRecent()
            consumeAutoPrompt()
        }
    }

    // MARK: - sections

    private var header: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 6) {
                Image(systemName: "moon.fill")
                    .font(.caption2)
                Text("MU MAKE")
                    .font(.caption.weight(.bold))
                    .kerning(2.5)
            }
            .foregroundStyle(MUTheme.goldDim)
            Text(String(localized: "make.headline"))
                .font(.system(size: 42, weight: .bold, design: .rounded))
                .foregroundStyle(MUTheme.goldGradient)
            Text(String(localized: "make.subheadline"))
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .padding(.top, 12)
    }

    private var promptCard: some View {
        VStack(alignment: .leading, spacing: 0) {
            TextField(
                String(localized: "make.placeholder"),
                text: $prompt,
                axis: .vertical
            )
            .lineLimit(3...6)
            .font(.body)
            .focused($promptFocused)
            .submitLabel(.done)
            .padding(16)
        }
        .background(MUTheme.card, in: RoundedRectangle(cornerRadius: 18))
        .overlay(
            RoundedRectangle(cornerRadius: 18)
                .strokeBorder(
                    promptFocused ? AnyShapeStyle(MUTheme.goldGradient) : AnyShapeStyle(MUTheme.cardBorder),
                    lineWidth: promptFocused ? 1.5 : 1
                )
        )
        .shadow(color: promptFocused ? MUTheme.gold.opacity(0.25) : .clear, radius: 14)
        .animation(.spring(duration: 0.35), value: promptFocused)
    }

    private var kindChips: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(MakeKind.allCases) { k in
                    Button {
                        Haptics.tap()
                        withAnimation(.spring(duration: 0.3)) { kind = k }
                    } label: {
                        Label(k.label, systemImage: k.icon)
                            .font(.footnote.weight(.medium))
                            .padding(.horizontal, 13)
                            .padding(.vertical, 7)
                            .background(
                                kind == k ? AnyShapeStyle(MUTheme.gold) : AnyShapeStyle(MUTheme.card),
                                in: Capsule()
                            )
                            .overlay(Capsule().strokeBorder(kind == k ? .clear : MUTheme.cardBorder, lineWidth: 1))
                            .foregroundStyle(kind == k ? .black : .primary)
                    }
                }
            }
        }
    }

    private var suggestionChips: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(String(localized: "make.suggestTitle"))
                .font(.caption.weight(.semibold))
                .foregroundStyle(.tertiary)
            FlowChips(items: [
                String(localized: "make.suggest.1"),
                String(localized: "make.suggest.2"),
                String(localized: "make.suggest.3"),
                String(localized: "make.suggest.4"),
                String(localized: "make.suggest.5"),
                String(localized: "make.suggest.6"),
            ]) { text in
                Haptics.tap()
                withAnimation(.spring(duration: 0.3)) { prompt = text }
                promptFocused = true
            }
        }
    }

    private var makeButton: some View {
        Button {
            promptFocused = false
            flow.start(prompt: prompt, kind: kind, apiKey: session.apiKey, history: history)
        } label: {
            HStack(spacing: 8) {
                Image(systemName: "sparkles")
                Text(String(localized: "make.button"))
            }
            .font(.headline)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 16)
            .background(canMake ? AnyShapeStyle(MUTheme.goldGradient) : AnyShapeStyle(MUTheme.card), in: Capsule())
            .foregroundStyle(canMake ? .black : .secondary)
        }
        .disabled(!canMake)
        .animation(.easeInOut(duration: 0.2), value: canMake)
    }

    private var recentRail: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(String(localized: "make.recentTitle"))
                .font(.headline)
            if recentLoading && recent.isEmpty {
                StateBanner(kind: .loading)
            } else if recentFailed && recent.isEmpty {
                StateBanner(kind: .error(String(localized: "common.error"), retry: {
                    Task { await loadRecent() }
                }))
            } else if recent.isEmpty {
                StateBanner(kind: .empty(String(localized: "make.recentEmpty")))
            } else {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 12) {
                        ForEach(recent) { item in
                            Button {
                                Haptics.tap()
                                recentDetail = item
                            } label: {
                                VStack(alignment: .leading, spacing: 6) {
                                    MUAsyncImage(url: item.imgURL)
                                        .frame(width: 132, height: 132)
                                        .clipShape(RoundedRectangle(cornerRadius: 12))
                                    Text(item.title)
                                        .font(.caption)
                                        .lineLimit(1)
                                        .foregroundStyle(.primary)
                                    Text("¥\(item.price.formatted())")
                                        .font(.caption2.weight(.semibold))
                                        .foregroundStyle(MUTheme.gold)
                                }
                                .frame(width: 132)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }
        }
        .padding(.top, 8)
        .sheet(item: $recentDetail) { item in
            if let url = item.pdpURL {
                SafariView(url: url).ignoresSafeArea()
            }
        }
    }

    // MARK: - data

    private func loadRecent() async {
        recentLoading = true
        defer { recentLoading = false }
        do {
            recent = try await MUAPI.recentMakes()
            recentFailed = false
        } catch {
            recentFailed = true
        }
    }

    // 起動引数 `-autoprompt "<text>"` で入力→生成まで自走 (E2E 検証・URL スキーム代替)
    private func consumeAutoPrompt() {
        guard let auto = UserDefaults.standard.string(forKey: "autoprompt"), !auto.isEmpty else { return }
        UserDefaults.standard.removeObject(forKey: "autoprompt")
        prompt = auto
        flow.start(prompt: auto, kind: kind, apiKey: session.apiKey, history: history)
    }
}

// 折返しチップ (シンプルな自前フローレイアウト)
struct FlowChips: View {
    let items: [String]
    let onTap: (String) -> Void

    var body: some View {
        FlowLayout(spacing: 8) {
            ForEach(items, id: \.self) { text in
                Button {
                    onTap(text)
                } label: {
                    Text(text)
                        .font(.footnote)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 7)
                        .background(MUTheme.card, in: Capsule())
                        .overlay(Capsule().strokeBorder(MUTheme.cardBorder, lineWidth: 1))
                        .foregroundStyle(.primary)
                }
            }
        }
    }
}

struct FlowLayout: Layout {
    var spacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = computeRows(proposal: proposal, subviews: subviews)
        let width = proposal.width ?? rows.map(\.width).max() ?? 0
        let height = rows.reduce(0) { $0 + $1.height } + CGFloat(max(0, rows.count - 1)) * spacing
        return CGSize(width: width, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let rows = computeRows(proposal: proposal, subviews: subviews)
        var y = bounds.minY
        for row in rows {
            var x = bounds.minX
            for index in row.indices {
                let size = subviews[index].sizeThatFits(.unspecified)
                subviews[index].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
                x += size.width + spacing
            }
            y += row.height + spacing
        }
    }

    private struct Row {
        var indices: [Int] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    private func computeRows(proposal: ProposedViewSize, subviews: Subviews) -> [Row] {
        let maxWidth = proposal.width ?? .infinity
        var rows: [Row] = []
        var current = Row()
        for (i, sub) in subviews.enumerated() {
            let size = sub.sizeThatFits(.unspecified)
            let needed = current.indices.isEmpty ? size.width : current.width + spacing + size.width
            if needed > maxWidth, !current.indices.isEmpty {
                rows.append(current)
                current = Row()
            }
            current.width = current.indices.isEmpty ? size.width : current.width + spacing + size.width
            current.height = max(current.height, size.height)
            current.indices.append(i)
        }
        if !current.indices.isEmpty { rows.append(current) }
        return rows
    }
}
