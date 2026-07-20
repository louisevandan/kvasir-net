import Foundation
import Security
import LocalAuthentication

/// Non-custodial key storage: the BIP39 mnemonic lives only in the iOS Keychain
/// (device-only accessibility). Keys never leave the device; signing happens
/// locally via Core.SolanaService.
enum KeyStore {
    private static let service = "ai.banya.linkcpp.wallet"
    private static let account = "mnemonic"

    enum KeyStoreError: Error { case keychain(OSStatus) }

    static func save(mnemonic: [String]) throws {
        let data = Data(mnemonic.joined(separator: " ").utf8)
        SecItemDelete(baseQuery() as CFDictionary)
        var attrs = baseQuery()
        attrs[kSecValueData as String] = data
        attrs[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        let status = SecItemAdd(attrs as CFDictionary, nil)
        guard status == errSecSuccess else { throw KeyStoreError.keychain(status) }
    }

    static func loadMnemonic() -> [String]? {
        var q = baseQuery()
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &out)
        guard status == errSecSuccess, let data = out as? Data,
              let str = String(data: data, encoding: .utf8) else { return nil }
        return str.split(separator: " ").map(String.init)
    }

    static func exists() -> Bool { loadMnemonic() != nil }

    static func delete() { SecItemDelete(baseQuery() as CFDictionary) }

    // MARK: - Gateway credit API key (per wallet)
    //
    // A prepaid-credit API key is a bearer credential for the wallet's credit
    // balance, so it lives in the Keychain too (never in plaintext defaults). It is
    // keyed by wallet address and is re-mintable, so losing it is recoverable.

    static func saveApiKey(_ key: String, wallet: String) throws {
        let q = apiKeyQuery(wallet)
        SecItemDelete(q as CFDictionary)
        var attrs = q
        attrs[kSecValueData as String] = Data(key.utf8)
        attrs[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        let status = SecItemAdd(attrs as CFDictionary, nil)
        guard status == errSecSuccess else { throw KeyStoreError.keychain(status) }
    }

    static func loadApiKey(wallet: String) -> String? {
        var q = apiKeyQuery(wallet)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess,
              let data = out as? Data, let s = String(data: data, encoding: .utf8) else { return nil }
        return s
    }

    static func deleteApiKey(wallet: String) { SecItemDelete(apiKeyQuery(wallet) as CFDictionary) }

    private static func apiKeyQuery(_ wallet: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: "credit-apikey:\(wallet)",
        ]
    }

    private static func baseQuery() -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
    }
}

/// Best-effort biometric / passcode gate. On a Simulator with no enrolled
/// biometrics it allows access so development flows aren't blocked.
enum Biometrics {
    static func unlock(reason: String = "Unlock your Kvasir wallet") async -> Bool {
        let ctx = LAContext()
        var err: NSError?
        guard ctx.canEvaluatePolicy(.deviceOwnerAuthentication, error: &err) else {
            return true
        }
        return await withCheckedContinuation { cont in
            ctx.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason) { ok, _ in
                cont.resume(returning: ok)
            }
        }
    }
}
