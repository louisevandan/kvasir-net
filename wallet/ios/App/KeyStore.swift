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

/// The device's biometric / passcode gate.
///
/// This used to return "allowed" when the device could not authenticate at all,
/// so that a Simulator with nothing enrolled would not block development. That
/// is a fail-open authentication gate, and one of the two things behind it is
/// the recovery phrase: on an iPhone with no passcode set, anyone holding it
/// could read the twelve words and drain the wallet from anywhere, later,
/// permanently. So the answer is now three-valued and each caller decides what
/// an absent device lock means for what it is protecting — which is not the
/// same answer in both places.
enum Biometrics {
    enum Outcome {
        /// The person proved they are the device owner.
        case authenticated
        /// They were asked and failed, or cancelled.
        case refused
        /// The device offers no biometric and no passcode; nobody was asked.
        case unavailable
    }

    /// The UI tests relaunch the app for every case, and every cold launch hits
    /// this gate. XCUITest cannot present a face, so without a way past it the
    /// only screens a device run can reach are the ones in front of the lock —
    /// which is most of what is worth looking at, missed. A debug build started
    /// with this argument skips the gate; a release build has no such path, and
    /// the argument has to be passed deliberately, so nothing changes for anyone
    /// who installs the app.
    static let uiTestBypassArgument = "-kvasir-ui-test-unlocked"

    static func unlock(reason: String = "Unlock your Kvasir wallet") async -> Outcome {
        #if DEBUG
        if CommandLine.arguments.contains(uiTestBypassArgument) { return .authenticated }
        #endif
        let ctx = LAContext()
        var err: NSError?
        guard ctx.canEvaluatePolicy(.deviceOwnerAuthentication, error: &err) else {
            return .unavailable
        }
        return await withCheckedContinuation { cont in
            ctx.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason) { ok, _ in
                cont.resume(returning: ok ? .authenticated : .refused)
            }
        }
    }
}
