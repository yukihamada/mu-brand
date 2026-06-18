import SwiftUI

// Account — 既存 register/verify API ログイン、売上 (api/agent/sales の totals)、
// アカウント削除 (App Store 5.1.1)。佇まいは他画面と同じ静けさで。
struct AccountView: View {
    @EnvironmentObject private var session: Session

    var body: some View {
        NavigationStack {
            ScrollView(showsIndicators: false) {
                VStack(alignment: .leading, spacing: 0) {
                    header
                    if session.isLoggedIn {
                        SignedInView()
                    } else {
                        AuthFormView()
                    }
                }
                .padding(.horizontal, 24)
            }
            .background(Atelier.paper)
            .toolbar(.hidden, for: .navigationBar)
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(verbatim: "MU ATELIER").eyebrow()
            Text("account.title").serif(.largeTitle)
        }
        .padding(.top, 12)
        .padding(.bottom, 30)
    }
}

// MARK: - Auth (メール → 6桁コード → api_key)

private struct AuthFormView: View {
    @EnvironmentObject private var session: Session
    @State private var email = ""
    @State private var code = ""
    @State private var codeSent = false
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 28) {
            Text("auth.lead")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineSpacing(5)

            field(
                label: "auth.email",
                text: $email,
                keyboard: .emailAddress,
                contentType: .emailAddress,
                disabled: codeSent
            )

            if codeSent {
                field(
                    label: "auth.code",
                    text: $code,
                    keyboard: .numberPad,
                    contentType: .oneTimeCode,
                    disabled: false
                )
            }

            if let error {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(Color(red: 0.72, green: 0.22, blue: 0.18))
            }

            VStack(spacing: 14) {
                Button {
                    Task { await submit() }
                } label: {
                    if busy {
                        ProgressView().tint(.white)
                    } else {
                        Text(codeSent ? "auth.verify" : "auth.send")
                    }
                }
                .buttonStyle(PrimaryButtonStyle())
                .disabled(busy || email.isEmpty || (codeSent && code.isEmpty))

                if codeSent {
                    Button {
                        codeSent = false
                        code = ""
                        error = nil
                    } label: {
                        Text("auth.restart")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .underline()
                    }
                }
            }
        }
        .padding(.bottom, 60)
    }

    private func field(
        label: LocalizedStringKey,
        text: Binding<String>,
        keyboard: UIKeyboardType,
        contentType: UITextContentType,
        disabled: Bool
    ) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(label)
                .font(.caption2.weight(.medium))
                .tracking(2.0)
                .foregroundStyle(.secondary)
            TextField("", text: text)
                .font(.body)
                .keyboardType(keyboard)
                .textContentType(contentType)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .disabled(disabled)
                .opacity(disabled ? 0.5 : 1)
            Hairline()
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

// MARK: - Signed in

private struct SignedInView: View {
    @EnvironmentObject private var session: Session
    @State private var sales: SalesResponse?
    @State private var showPrivacy = false
    @State private var confirmDelete = false
    @State private var deleteError: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            row(label: String(localized: "account.email"), value: session.email ?? "—")
            if let t = sales?.total {
                row(label: String(localized: "account.orders"), value: "\(t.orderCount ?? 0)")
                row(label: String(localized: "account.revenue"), value: "¥\((t.revenueJpy ?? 0).formatted())")
            }

            Text("account.salesNote")
                .font(.caption2)
                .foregroundStyle(.tertiary)
                .padding(.top, 14)

            VStack(spacing: 14) {
                Button {
                    showPrivacy = true
                } label: {
                    Text("account.privacy")
                }
                .buttonStyle(HairlineButtonStyle())

                Button {
                    session.logOut()
                } label: {
                    Text("account.logout")
                }
                .buttonStyle(HairlineButtonStyle())

                Button {
                    confirmDelete = true
                } label: {
                    Text("account.delete")
                        .font(.caption)
                        .foregroundStyle(Color(red: 0.72, green: 0.22, blue: 0.18))
                        .underline()
                }
                .padding(.top, 12)

                if let deleteError {
                    Text(deleteError)
                        .font(.caption)
                        .foregroundStyle(Color(red: 0.72, green: 0.22, blue: 0.18))
                }
            }
            .padding(.top, 44)
            .padding(.bottom, 60)
        }
        .task {
            if let key = session.apiKey {
                sales = try? await MUAPI.sales(apiKey: key)
            }
        }
        .sheet(isPresented: $showPrivacy) {
            SafariView(url: URL(string: "https://wearmu.com/privacy")!).ignoresSafeArea()
        }
        .confirmationDialog(
            String(localized: "account.deleteTitle"),
            isPresented: $confirmDelete,
            titleVisibility: .visible
        ) {
            Button(String(localized: "account.deleteConfirm"), role: .destructive) {
                Task { await deleteAccount() }
            }
        } message: {
            Text("account.deleteBody")
        }
    }

    private func row(label: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline) {
                Text(label)
                    .font(.caption2.weight(.medium))
                    .tracking(2.0)
                    .foregroundStyle(.secondary)
                Spacer()
                Text(value)
                    .font(.subheadline)
            }
            Hairline()
        }
        .padding(.top, 18)
    }

    private func deleteAccount() async {
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
