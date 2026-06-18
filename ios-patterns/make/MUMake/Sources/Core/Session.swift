import Foundation
import Security

// ログイン状態。api_key は Keychain のみ (UserDefaults 平文は禁止 — パシャ監査の教訓)。
@MainActor
final class Session: ObservableObject {
    @Published var email: String?
    @Published var isLoggedIn = false

    private static let service = "com.wearmu.mu.make.session"
    private static let keyAccount = "api_key"
    private static let emailAccount = "email"

    init() {
        if let key = Self.keychainRead(Self.keyAccount), !key.isEmpty {
            isLoggedIn = true
            email = Self.keychainRead(Self.emailAccount)
        }
    }

    var apiKey: String? { Self.keychainRead(Self.keyAccount) }

    func logIn(email: String, apiKey: String) {
        Self.keychainWrite(Self.keyAccount, value: apiKey)
        Self.keychainWrite(Self.emailAccount, value: email)
        self.email = email
        self.isLoggedIn = true
    }

    func logOut() {
        Self.keychainDelete(Self.keyAccount)
        Self.keychainDelete(Self.emailAccount)
        email = nil
        isLoggedIn = false
    }

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
