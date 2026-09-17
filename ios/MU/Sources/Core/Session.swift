import Foundation
import Security

// ログイン状態。api_key は Keychain のみ (UserDefaults 平文は禁止 — パシャ監査の教訓)。
@MainActor
final class Session: ObservableObject {
    @Published var email: String?
    @Published var isLoggedIn = false
    /// Identifies *which* account is signed in. Results and in-flight work belong to
    /// one identity only; when it changes, screens must drop the previous account's
    /// state instead of showing it to the next user.
    @Published private(set) var identity: String?
    /// Rotated on every identity change, including B -> A. Comparing identities as
    /// strings would accept a reply from an earlier visit to the same account, so
    /// in-flight work captures this generation and compares it instead.
    @Published private(set) var identityGeneration = UUID()

    private static let service = "com.wearmu.mu.session"
    private static let keyAccount = "api_key"
    private static let emailAccount = "email"

    // API キーのメモリキャッシュ。Analytics/Push が高頻度で読むため、
    // 毎回 Keychain を叩かない(login/logout で更新)。
    private static var cachedKey: String?? = nil  // nil=未読込, .some(nil)=未ログイン

    init() {
        #if DEBUG
        if MakeUITestFixture.enabled {
            Self.cachedKey = MakeUITestFixture.startsSignedOut ? .some(nil) : .some("offline-fixture")
            email = MakeUITestFixture.startsSignedOut ? nil : "fixture@example.invalid"
            isLoggedIn = !MakeUITestFixture.startsSignedOut
            identity = MakeUITestFixture.startsSignedOut ? nil : "fixture@example.invalid"
            return
        }
        #endif
        let key = Self.keychainRead(Self.keyAccount)
        Self.cachedKey = .some(key)
        if let key, !key.isEmpty {
            isLoggedIn = true
            email = Self.keychainRead(Self.emailAccount)
            identity = email
        }
    }

    var apiKey: String? { Self.currentAPIKey() }

    // AppDelegate / Analytics などインスタンス外から鍵を読むための静的アクセサ(キャッシュ優先)。
    static func currentAPIKey() -> String? {
        if let cached = cachedKey { return cached }
        let key = keychainRead(keyAccount)
        cachedKey = .some(key)
        return key
    }

    func logIn(email: String, apiKey: String) {
        Self.keychainWrite(Self.keyAccount, value: apiKey)
        Self.keychainWrite(Self.emailAccount, value: email)
        Self.cachedKey = .some(apiKey)
        self.email = email
        self.isLoggedIn = true
        setIdentity(email)
    }

    func logOut() {
        Self.keychainDelete(Self.keyAccount)
        Self.keychainDelete(Self.emailAccount)
        Self.cachedKey = .some(nil)
        email = nil
        isLoggedIn = false
        setIdentity(nil)
    }

    // MARK: - Offline UI-test account switching (DEBUG + fixture only)

    /// Unit tests cannot set a launch environment variable, so they opt in through
    /// `enableFixtureForUnitTest()`. Still DEBUG-only and memory-only: no Keychain write.
    #if DEBUG
    private var _fixtureOverride = false
    private var fixtureOverride: Bool { _fixtureOverride || MakeUITestFixture.enabled }
    func enableFixtureForUnitTest() { _fixtureOverride = true }
    #endif

    /// Publishes the new identity and rotates its generation, so work started under
    /// a previous generation is rejected even when the email is identical (A -> B -> A).
    private func setIdentity(_ next: String?) {
        identity = next
        identityGeneration = UUID()
    }

    /// Changes the in-memory identity without touching the Keychain, so a UI test
    /// can exercise sign-out and account switching offline. Never compiled into
    /// Release and never available to a normal device login.
    #if DEBUG
    func logInForUITest(email: String) {
        guard fixtureOverride else { return }
        Self.cachedKey = .some("offline-fixture-\(email)")
        self.email = email
        isLoggedIn = true
        setIdentity(email)
    }

    func logOutForUITest() {
        guard fixtureOverride else { return }
        Self.cachedKey = .some(nil)
        email = nil
        isLoggedIn = false
        setIdentity(nil)
    }
    #endif

    // MARK: - Keychain (kSecClassGenericPassword / AfterFirstUnlockThisDeviceOnly)

    private static func keychainWrite(_ account: String, value: String) {
        let data = Data(value.utf8)
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        let attrs: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        let status = SecItemUpdate(query as CFDictionary, attrs as CFDictionary)
        if status == errSecItemNotFound {
            var add = query
            add.merge(attrs) { _, new in new }
            SecItemAdd(add as CFDictionary, nil)
        }
    }

    private static func keychainRead(_ account: String) -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    private static func keychainDelete(_ account: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(query as CFDictionary)
    }
}
