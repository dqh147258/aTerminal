import Foundation
import Security

enum PairingStore {
    private static let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
                                              kSecAttrService as String: "dev.aiterminal.pairing",
                                              kSecAttrAccount as String: "desktop"]
    private static var accountQuery: [String: Any] { var value = query; value[kSecAttrService as String] = "dev.aiterminal.account"; value[kSecAttrAccount as String] = "session"; return value }
    static func saveAccount(_ value: String) throws { try save(value, query: accountQuery) }
    static func loadAccount() -> String? { load(query: accountQuery) }
    static func clearAccount() throws { let status = SecItemDelete(accountQuery as CFDictionary); if status != errSecSuccess && status != errSecItemNotFound { throw CocoaError(.fileWriteNoPermission) } }
    static func save(_ value: String) throws { try save(value, query: query) }
    private static func save(_ value: String, query: [String: Any]) throws {
        let data = Data(value.utf8)
        let status = SecItemUpdate(query as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if status == errSecItemNotFound {
            var item = query
            item[kSecValueData as String] = data
            item[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
            guard SecItemAdd(item as CFDictionary, nil) == errSecSuccess else { throw CocoaError(.fileWriteNoPermission) }
        } else if status != errSecSuccess { throw CocoaError(.fileWriteNoPermission) }
    }
    static func load() -> String? { load(query: query) }
    private static func load(query: [String: Any]) -> String? {
        var item = query
        item[kSecReturnData as String] = true
        item[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        guard SecItemCopyMatching(item as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }
}
