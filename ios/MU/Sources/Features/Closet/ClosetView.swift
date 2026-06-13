import SwiftUI
import UIKit

// 👤 Closet — ログイン (メール → 6桁コード → api_key) と自分の売上/ストア。
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
            .navigationTitle(String(localized: "tab.closet"))
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
    @State private var showMypage = false
    @State private var showPrivacy = false
    @State private var confirmDelete = false
    @State private var deleteError: String?
    @State private var pushOn = false
    @State private var pushDenied = false

    var body: some View {
        List {
            Section {
                LabeledContent(String(localized: "account.email"), value: session.email ?? "—")
            }
            // 通知: ドロップ/売れた時に届く。許可導線をアカウントに置く。
            Section {
                if pushOn {
                    Label(String(localized: "push.on"), systemImage: "bell.fill")
                        .foregroundStyle(.secondary)
                } else if pushDenied {
                    Button {
                        if let url = URL(string: UIApplication.openSettingsURLString) {
                            UIApplication.shared.open(url)
                        }
                    } label: {
                        Label(String(localized: "push.openSettings"), systemImage: "bell.slash")
                    }
                } else {
                    Button {
                        Task {
                            let ok = await PushManager.enable()
                            pushOn = ok
                            pushDenied = !ok
                            Analytics.track("push_enable", ["ok": ok])
                        }
                    } label: {
                        Label(String(localized: "push.enable"), systemImage: "bell.badge")
                    }
                }
            } footer: {
                Text(String(localized: "push.why"))
            }
            if let t = sales?.total {
                Section(String(localized: "account.sales")) {
                    // 「あなたの作品が世界で N回 着られている」— 印税を誇りに変える
                    if let n = t.orderCount, n > 0 {
                        HStack {
                            Image(systemName: "globe.asia.australia.fill").foregroundStyle(.tint)
                            Text(String(format: String(localized: "account.worn"), n))
                                .font(.subheadline.weight(.medium))
                        }
                    }
                    LabeledContent(String(localized: "account.orders"), value: "\(t.orderCount ?? 0)")
                    LabeledContent(String(localized: "account.revenue"), value: "¥\((t.revenueJpy ?? 0).formatted())")
                }
            }
            Section {
                Button {
                    showMypage = true
                } label: {
                    Label(String(localized: "account.mypage"), systemImage: "safari")
                }
            }
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
        .task {
            if let key = session.apiKey {
                sales = try? await MUAPI.sales(apiKey: key)
            }
            let st = await PushManager.status()
            pushOn = st == .authorized
            pushDenied = st == .denied
        }
        .sheet(isPresented: $showMypage) {
            SafariView(url: URL(string: "https://wearmu.com/mypage")!).ignoresSafeArea()
        }
    }
}
