import Foundation
import SwiftUI
import Core
import SolanaSwift

/// Selectable Solana network. The wallet address is network-independent, so
/// switching only changes the RPC endpoint and which token is available.
enum AppNetwork: String, CaseIterable, Identifiable {
    case devnet = "devnet"
    case mainnet = "mainnet-beta"
    var id: String { rawValue }
    var display: String { self == .devnet ? "Devnet" : "Mainnet" }
    var solana: Network { self == .devnet ? .devnet : .mainnetBeta }
}

struct TokenInfo: Equatable {
    let mint: String
    let symbol: String
    let decimals: Int
}

/// App-wide wallet state: derives the account from the stored mnemonic, tracks the
/// selected network, reads balances/history, and sends transactions (local signing).
@MainActor
final class WalletStore: ObservableObject {
    @Published var address: String?
    @Published var solBalance: AssetBalance?
    @Published var tokenBalance: AssetBalance?
    @Published var transactions: [TxRef] = []
    @Published var isLoading = false
    @Published var errorMessage: String?
    @Published private(set) var network: AppNetwork

    private let constants: WalletConstants
    private let devnetToken: TokenInfo
    private var service: SolanaService
    private var keyPair: KeyPair?

    private static let networkKey = "linkcpp.network"
    private static let stakingURLKey = "linkcpp.stakingURL"

    init() {
        guard let constants = try? SharedSpec.loadConstants(),
              let tokenSpec = try? SharedSpec.loadToken() else {
            fatalError("shared-spec missing from bundle")
        }
        self.constants = constants
        self.devnetToken = TokenInfo(mint: tokenSpec.token.mint,
                                     symbol: tokenSpec.token.symbol,
                                     decimals: tokenSpec.token.decimals)
        let saved = UserDefaults.standard.string(forKey: Self.networkKey)
            .flatMap(AppNetwork.init(rawValue:)) ?? .devnet
        self.network = saved
        self.service = WalletStore.makeService(network: saved, constants: constants)
    }

    private static func makeService(network: AppNetwork, constants: WalletConstants) -> SolanaService {
        let fallback = network == .devnet
            ? "https://api.devnet.solana.com" : "https://api.mainnet-beta.solana.com"
        let rpc = constants.clusters[network.rawValue]?.rpcUrl ?? fallback
        return SolanaService(rpcURL: rpc, network: network.solana)
    }

    // MARK: - Derived config

    /// The token available on the current network. KVR exists on devnet only for now.
    var token: TokenInfo? { network == .devnet ? devnetToken : nil }
    var hasToken: Bool { token != nil }
    var tokenSymbol: String { (token ?? devnetToken).symbol }
    var explorerCluster: String {
        constants.clusters[network.rawValue]?.explorerCluster ?? network.rawValue
    }
    var hasWallet: Bool { KeyStore.exists() }

    /// Base URL of the off-chain staking settlement service. Defaults to the
    /// shared-spec value (the genesis gateway), overridable in-app (per device / LAN).
    var stakingServiceURL: String {
        UserDefaults.standard.string(forKey: Self.stakingURLKey) ?? defaultStakingServiceURL
    }
    /// The shipped default (genesis gateway) from shared-spec.
    var defaultStakingServiceURL: String { constants.stakingServiceUrl ?? "" }
    /// True once the user pinned their own settlement URL — genesis discovery must
    /// not override a manual choice (e.g. a temporary LAN/IP override).
    var hasCustomStakingURL: Bool {
        UserDefaults.standard.string(forKey: Self.stakingURLKey) != nil
    }
    func setStakingServiceURL(_ url: String) {
        UserDefaults.standard.set(url.trimmingCharacters(in: .whitespacesAndNewlines), forKey: Self.stakingURLKey)
    }

    func setNetwork(_ net: AppNetwork) {
        guard net != network else { return }
        network = net
        UserDefaults.standard.set(net.rawValue, forKey: Self.networkKey)
        service = WalletStore.makeService(network: net, constants: constants)
        tokenBalance = nil
        solBalance = nil
        transactions = []
        Task { await refresh() }
    }

    // MARK: - Wallet lifecycle

    func newMnemonic(wordCount: Int = 12) -> [String] { WalletMnemonic.generate(wordCount: wordCount) }

    func restoreIfNeeded() async {
        guard address == nil, let phrase = KeyStore.loadMnemonic() else { return }
        await activate(phrase: phrase)
    }

    func saveAndActivate(phrase: [String]) async throws {
        let normalized = phrase.map { $0.lowercased() }
        guard WalletMnemonic.isValid(normalized) else { throw WalletError.invalidMnemonic }
        try KeyStore.save(mnemonic: normalized)
        await activate(phrase: normalized)
    }

    private func activate(phrase: [String]) async {
        do {
            // The address is the same on every network; derive once.
            let kp = try await WalletDeriver.keyPair(phrase: phrase, network: .devnet)
            self.keyPair = kp
            self.address = kp.publicKey.base58EncodedString
            await refresh()
        } catch {
            self.errorMessage = String(describing: error)
        }
    }

    func refresh() async {
        guard let address else { return }
        isLoading = true
        defer { isLoading = false }
        do {
            self.solBalance = try await service.solBalance(owner: address)
            self.transactions = try await service.recentTransactions(owner: address, limit: 50)
            if let t = token {
                self.tokenBalance = try await service.tokenBalance(
                    owner: address, mint: t.mint, symbol: t.symbol, decimals: t.decimals)
            } else {
                self.tokenBalance = nil
            }
            self.errorMessage = nil
        } catch {
            self.errorMessage = String(describing: error)
        }
    }

    func sendToken(to: String, amount: Double) async throws -> String {
        guard let keyPair, let t = token else { throw WalletError.invalidMnemonic }
        let sig = try await service.sendToken(
            from: keyPair, mint: t.mint, decimals: t.decimals, toAddress: to, amount: amount)
        await refresh()
        return sig
    }

    func sendSOL(to: String, amount: Double) async throws -> String {
        guard let keyPair else { throw WalletError.invalidMnemonic }
        let sig = try await service.sendSOL(from: keyPair, toAddress: to, sol: amount)
        await refresh()
        return sig
    }

    /// The stored BIP39 mnemonic, for backing up / importing the same account on
    /// another device. Callers must gate this behind a biometric check.
    func revealMnemonic() -> [String]? { KeyStore.loadMnemonic() }

    func logout() {
        KeyStore.delete()
        address = nil; solBalance = nil; tokenBalance = nil
        transactions = []; keyPair = nil
    }

    func explorerURL(sig: String) -> URL? {
        URL(string: "https://explorer.solana.com/tx/\(sig)?cluster=\(explorerCluster)")
    }
}
