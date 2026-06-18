import SwiftUI

// 👤 Mine — この端末で作ったもの (ローカル台帳) + ログイン後は自ストア/売上/アカウント。
struct MineView: View {
    let switchTab: (AppTab) -> Void

    @EnvironmentObject private var session: Session
    @EnvironmentObject private var history: MakeHistory
    @State private var safariURL: URL?
    @State private var showAuth = false

    var body: some View {
        NavigationStack {
            ZStack {
                MUTheme.bg.ignoresSafeArea()
                ScrollView {
                    VStack(alignment: .leading, spacing: 24) {
                        madeSection
                        if session.isLoggedIn {
                            AccountSection(safariURL: $safariURL)
                        } else {
                            loginCard
                        }
                    }
                    .padding(.horizontal, 20)
                    .padding(.bottom, 40)
                }
            }
            .navigationTitle(String(localized: "tab.mine"))
        }
        .sheet(item: $safariURL) { url in
            SafariView(url: url).ignoresSafeArea()
        }
        .sheet(isPresented: $showAuth) {
            AuthSheet()
                .presentationDetents([.medium, .large])
        }
    }

    // MARK: - 作ったもの (ローカル台帳 = 実データ)

    private var madeSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(String(localized: "mine.madeSection"))
                .font(.headline)
            if history.creations.isEmpty {
                VStack(spacing: 14) {
                    StateBanner(kind: .empty(String(localized: "mine.empty")))
                    Button {
                        Haptics.tap()
                        switchTab(.make)
                    } label: {
                        Label(String(localized: "mine.makeCTA"), systemImage: "sparkles")
                            .font(.subheadline.weight(.semibold))
                            .padding(.horizontal, 20)
                            .padding(.vertical, 11)
                            .background(MUTheme.goldGradient, in: Capsule())
                            .foregroundStyle(.black)
                    }
                    .padding(.bottom, 8)
                }
                .frame(maxWidth: .infinity)
            } else {
                ForEach(history.creations) { c in
                    CreationRow(creation: c) { url in
                        safariURL = url
                    }
                }
            }
        }
    }

    private var loginCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            Label(String(localized: "mine.loginTitle"), systemImage: "person.crop.circle.badge.plus")
                .font(.subheadline.weight(.semibold))
            Text(String(localized: "mine.loginLead"))
                .font(.footnote)
                .foregroundStyle(.secondary)
            Button {
                Haptics.tap()
                showAuth = true
            } label: {
                Text(String(localized: "auth.sendCode.cta"))
                    .font(.subheadline.weight(.semibold))
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 12)
                    .background(MUTheme.gold, in: Capsule())
                    .foregroundStyle(.black)
            }
            .padding(.top, 4)
        }
        .padding(16)
        .background(MUTheme.card, in: RoundedRectangle(cornerRadius: 18))
        .overlay(RoundedRectangle(cornerRadius: 18).strokeBorder(MUTheme.cardBorder, lineWidth: 1))
    }
}

struct CreationRow: View {
    let creation: LocalCreation
    let openURL: (URL) -> Void

    var body: some View {
        HStack(spacing: 12) {
            MUAsyncImage(url: creation.imageURL)
                .frame(width: 72, height: 72)
                .clipShape(RoundedRectangle(cornerRadius: 12))
            VStack(alignment: .leading, spacing: 4) {
                Text(creation.prompt)
                    .font(.subheadline)
                    .lineLimit(2)
                HStack(spacing: 8) {
                    Text(creation.priceLabel)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(MUTheme.gold)
                    Text(creation.kind.uppercased())
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                    if creation.status != "live" {
                        Text(String(localized: "result.reviewTitle"))
                            .font(.caption2)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(MUTheme.card, in: Capsule())
                            .overlay(Capsule().strokeBorder(MUTheme.cardBorder, lineWidth: 1))
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Text(creation.createdAt, style: .relative)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {
            Haptics.tap()
            if let url = URL(string: creation.pdpUrl) { openURL(url) }
        }
        .contextMenu {
            if let url = URL(string: creation.pdpUrl) {
                Button { openURL(url) } label: {
                    Label(String(localized: "result.openPDP"), systemImage: "safari")
                }
            }
            if let url = URL(string: creation.editUrl) {
                Button { openURL(url) } label: {
                    Label(String(localized: "result.edit"), systemImage: "slider.horizontal.3")
                }
            }
            if let checkout = creation.checkoutUrl, let url = URL(string: checkout) {
                Button { openURL(url) } label: {
                    Label(String(localized: "result.buy"), systemImage: "bag")
                }
            }
        }
    }
}

// ── ログイン後: 売上 + 自ストア商品 + アカウント操作 (全部 実API・Bearer) ──
struct AccountSection: View {
    @Binding var safariURL: URL?

    @EnvironmentObject private var session: Session
    @State private var sales: SalesResponse?
    @State private var storeProducts: [AgentProduct] = []
    @State private var loading = false
    @State private var failed = false
    @State private var confirmDelete = false
    @State private var deleteError: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(String(localized: "mine.accountSection"))
                .font(.headline)

            VStack(alignment: .leading, spacing: 14) {
                LabeledContent(String(localized: "account.email"), value: session.email ?? "—")
                    .font(.subheadline)
                if let t = sales?.total {
                    Divider().overlay(MUTheme.cardBorder)
                    HStack(spacing: 24) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(String(localized: "account.orders"))
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                            Text("\(t.orderCount ?? 0)")
                                .font(.title3.bold())
                        }
                        VStack(alignment: .leading, spacing: 2) {
                            Text(String(localized: "account.revenue"))
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                            Text("¥\((t.revenueJpy ?? 0).formatted())")
                                .font(.title3.bold())
                                .foregroundStyle(MUTheme.gold)
                        }
                    }
                }
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(MUTheme.card, in: RoundedRectangle(cornerRadius: 18))

            if loading {
                StateBanner(kind: .loading)
            } else if failed {
                StateBanner(kind: .error(String(localized: "common.error"), retry: {
                    Task { await load() }
                }))
            } else if !storeProducts.isEmpty {
                Text(String(localized: "mine.storeSection"))
                    .font(.subheadline.weight(.semibold))
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 12) {
                        ForEach(storeProducts) { p in
                            Button {
                                Haptics.tap()
                                if let s = p.pdpUrl, let url = URL(string: s) { safariURL = url }
                            } label: {
                                VStack(alignment: .leading, spacing: 6) {
                                    MUAsyncImage(url: p.imageURL)
                                        .frame(width: 110, height: 110)
                                        .clipShape(RoundedRectangle(cornerRadius: 12))
                                    Text(p.label ?? p.sku)
                                        .font(.caption2)
                                        .lineLimit(1)
                                        .foregroundStyle(.primary)
                                    Text("¥\((p.retailPriceJpy ?? 0).formatted())")
                                        .font(.caption2.weight(.semibold))
                                        .foregroundStyle(MUTheme.gold)
                                }
                                .frame(width: 110)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }

            VStack(spacing: 0) {
                Button {
                    Haptics.tap()
                    session.logOut()
                } label: {
                    Text(String(localized: "account.logout"))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 12)
                }
                .foregroundStyle(.red)
                Divider().overlay(MUTheme.cardBorder)
                Button {
                    confirmDelete = true
                } label: {
                    Text(String(localized: "account.delete"))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 12)
                }
                .foregroundStyle(.red)
            }
            .font(.subheadline)
            .background(MUTheme.card, in: RoundedRectangle(cornerRadius: 18))

            if let deleteError {
                Text(deleteError).font(.footnote).foregroundStyle(.red)
            }

            Button {
                safariURL = URL(string: "https://wearmu.com/privacy")
            } label: {
                Text(String(localized: "account.privacy"))
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .confirmationDialog(
            String(localized: "account.deleteConfirmTitle"),
            isPresented: $confirmDelete,
            titleVisibility: .visible
        ) {
            Button(String(localized: "account.deleteConfirmAction"), role: .destructive) {
                Task {
                    do {
                        if let key = session.apiKey {
                            try await MUAPI.deleteAccount(apiKey: key)
                        }
                        session.logOut()
                    } catch {
                        deleteError = error.localizedDescription
                    }
                }
            }
        } message: {
            Text(String(localized: "account.deleteConfirmBody"))
        }
        .task { await load() }
    }

    private func load() async {
        guard let key = session.apiKey else { return }
        loading = true
        defer { loading = false }
        do {
            async let s = MUAPI.sales(apiKey: key)
            async let p = MUAPI.myProducts(apiKey: key)
            sales = try await s
            storeProducts = try await p
            failed = false
        } catch {
            failed = true
        }
    }
}

// メール → 6桁コード → api_key (既存 MU アプリと同じ実APIフロー)
struct AuthSheet: View {
    @EnvironmentObject private var session: Session
    @Environment(\.dismiss) private var dismiss
    @State private var email = ""
    @State private var code = ""
    @State private var codeSent = false
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text(String(localized: "auth.lead"))
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                Section {
                    TextField(String(localized: "auth.email"), text: $email)
                        .keyboardType(.emailAddress)
                        .textContentType(.emailAddress)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .disabled(codeSent)
                    if codeSent {
                        TextField(String(localized: "auth.code"), text: $code)
                            .keyboardType(.numberPad)
                            .textContentType(.oneTimeCode)
                    }
                }
                Section {
                    Button {
                        Task { await submit() }
                    } label: {
                        if busy {
                            ProgressView().frame(maxWidth: .infinity)
                        } else {
                            Text(codeSent ? String(localized: "auth.verify") : String(localized: "auth.sendCode"))
                                .frame(maxWidth: .infinity)
                        }
                    }
                    .disabled(busy || email.isEmpty || (codeSent && code.isEmpty))
                    if codeSent {
                        Button(String(localized: "auth.restart")) {
                            codeSent = false
                            code = ""
                            error = nil
                        }
                        .font(.footnote)
                    }
                }
                if let error {
                    Section { Text(error).font(.footnote).foregroundStyle(.red) }
                }
            }
            .scrollContentBackground(.hidden)
            .background(MUTheme.bg)
            .navigationTitle(String(localized: "auth.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(String(localized: "common.cancel")) { dismiss() }
                }
            }
        }
    }

    private func submit() async {
        busy = true
        defer { busy = false }
        do {
            if codeSent {
                let key = try await MUAPI.verify(email: email, code: code)
                session.logIn(email: email, apiKey: key)
                Haptics.success()
                dismiss()
            } else {
                try await MUAPI.register(email: email)
                codeSent = true
                Haptics.tap()
            }
            error = nil
        } catch {
            self.error = error.localizedDescription
            Haptics.failure()
        }
    }
}
