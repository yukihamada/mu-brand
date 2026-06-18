import SwiftUI

// 👤 Closet — ログイン (メール → 6桁コード → api_key) と自分の売上。
// アカウント削除 (App Store 5.1.1) も既存 API でここに。
struct ClosetView: View {
    @EnvironmentObject private var session: Session

    var body: some View {
        NavigationStack {
            Group {
                if session.isLoggedIn {
                    AccountView()
                } else {
                    AuthView()
                }
            }
            .scrollContentBackground(.hidden)
            .background(Color.muBg)
            .navigationTitle(String(localized: "tab.closet"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(Color.muBg, for: .navigationBar)
        }
    }
}

struct AuthView: View {
    @EnvironmentObject private var session: Session
    @State private var email = ""
    @State private var code = ""
    @State private var codeSent = false
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        Form {
            Section {
                Text(String(localized: "auth.lead"))
                    .font(.subheadline)
                    .foregroundStyle(Color.muMute)
            }
            .listRowBackground(Color.muCard)
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
            .listRowBackground(Color.muCard)
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
            .listRowBackground(Color.muCard)
            if let error {
                Section {
                    Text(error).font(.footnote).foregroundStyle(.red)
                }
                .listRowBackground(Color.muCard)
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
            } else {
                try await MUAPI.register(email: email)
                codeSent = true
            }
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }
}

struct AccountView: View {
    @EnvironmentObject private var session: Session
    @State private var sales: SalesResponse?
    @State private var salesError: String?
    @State private var showMypage = false
    @State private var showPrivacy = false
    @State private var confirmDelete = false
    @State private var deleteError: String?

    var body: some View {
        List {
            Section {
                LabeledContent(String(localized: "account.email"), value: session.email ?? "—")
            }
            .listRowBackground(Color.muCard)
            Section(String(localized: "account.sales")) {
                if let t = sales?.total {
                    LabeledContent(String(localized: "account.orders")) {
                        Text("\(t.orderCount ?? 0)").monospacedDigit()
                    }
                    LabeledContent(String(localized: "account.revenue")) {
                        Text(Fmt.yen(t.revenueJpy ?? 0))
                            .monospacedDigit()
                            .foregroundStyle(Color.muGold)
                    }
                } else if let salesError {
                    Text(salesError).font(.footnote).foregroundStyle(Color.muMute)
                } else {
                    HStack {
                        ProgressView()
                        Text(String(localized: "account.loadingSales"))
                            .font(.footnote)
                            .foregroundStyle(Color.muMute)
                    }
                }
            }
            .listRowBackground(Color.muCard)
            Section {
                Button {
                    showMypage = true
                } label: {
                    Label(String(localized: "account.mypage"), systemImage: "safari")
                }
            }
            .listRowBackground(Color.muCard)
            Section {
                Button(String(localized: "account.logout"), role: .destructive) {
                    session.logOut()
                }
                Button(String(localized: "account.delete"), role: .destructive) {
                    confirmDelete = true
                }
            } footer: {
                VStack(alignment: .leading, spacing: 4) {
                    if let deleteError {
                        Text(deleteError).foregroundStyle(.red)
                    }
                    Button(String(localized: "account.privacy")) { showPrivacy = true }
                        .font(.footnote)
                }
            }
            .listRowBackground(Color.muCard)
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
        .sheet(isPresented: $showPrivacy) {
            SafariView(url: URL(string: "https://wearmu.com/privacy")!).ignoresSafeArea()
        }
        .sheet(isPresented: $showMypage) {
            SafariView(url: URL(string: "https://wearmu.com/mypage")!).ignoresSafeArea()
        }
        .task {
            guard let key = session.apiKey else { return }
            do {
                sales = try await MUAPI.sales(apiKey: key)
            } catch {
                salesError = error.localizedDescription
            }
        }
    }
}
