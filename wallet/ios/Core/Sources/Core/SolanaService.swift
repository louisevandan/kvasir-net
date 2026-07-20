import Foundation
import SolanaSwift

/// Thin wrapper over SolanaSwift for the operations the Kvasir wallet needs:
/// read SOL + SPL balances, list recent transactions, and send SOL / SPL tokens
/// with **local** signing (non-custodial). All network calls hit the configured RPC.
public actor SolanaService {
    public let endpoint: APIEndPoint
    private let apiClient: JSONRPCAPIClient

    public init(rpcURL: String, network: Network = .devnet) {
        let ep = APIEndPoint(address: rpcURL, network: network)
        self.endpoint = ep
        self.apiClient = JSONRPCAPIClient(endpoint: ep)
    }

    /// Build a service from the bundled shared-spec (`token.devnet.json`).
    public static func fromSharedSpec() throws -> (service: SolanaService, spec: TokenDevnetSpec) {
        let spec = try SharedSpec.loadToken()
        let network: Network = spec.cluster == "mainnet-beta" ? .mainnetBeta
            : (spec.cluster == "testnet" ? .testnet : .devnet)
        return (SolanaService(rpcURL: spec.rpcUrl, network: network), spec)
    }

    // MARK: - Reads

    /// Native SOL balance of `owner`.
    public func solBalance(owner: String) async throws -> AssetBalance {
        let lamports = try await apiClient.getBalance(account: owner, commitment: "confirmed")
        return AssetBalance(symbol: "SOL", raw: lamports, decimals: 9)
    }

    /// SPL token balance of `owner` for a given `mint`. Sums all matching token
    /// accounts (normally one ATA). Returns zero if the owner has no token account.
    public func tokenBalance(owner: String, mint: String, symbol: String, decimals: Int) async throws -> AssetBalance {
        let accounts = try await apiClient.getTokenAccountsByOwner(
            pubkey: owner,
            params: OwnerInfoParams(mint: mint, programId: nil),
            configs: RequestConfiguration(encoding: "base64")
        )
        let raw = accounts.reduce(UInt64(0)) { $0 + $1.account.data.lamports }
        return AssetBalance(symbol: symbol, raw: raw, decimals: decimals)
    }

    /// Recent transaction signatures touching `owner`.
    public func recentTransactions(owner: String, limit: Int = 20) async throws -> [TxRef] {
        let sigs = try await apiClient.getSignaturesForAddress(
            address: owner,
            configs: RequestConfiguration(limit: limit)
        )
        return sigs.map {
            TxRef(
                signature: $0.signature,
                slot: $0.slot,
                failed: $0.err != nil,
                blockTime: $0.blockTime.map(Int64.init)
            )
        }
    }

    // MARK: - Sends (local signing)
    //
    // SolanaSwift 5.0.0's high-level helpers (prepareSendingNativeSOL /
    // prepareSendingSPLTokens → prepareTransaction) call `getRecentBlockhash` AND
    // `getFees` — both REMOVED from today's Solana RPC. Under the load-balanced public
    // devnet RPC this surfaces INTERMITTENTLY as -32601 (getRecentBlockhash) / -32602
    // ("recent" commitment). So we build + sign transactions ourselves using only pure
    // program-instruction builders, inject a `getLatestBlockhash` value, and raw-send
    // with the supported `sendTransaction` method (with retry). No deprecated RPC.

    /// Send native SOL. Returns the transaction signature.
    public func sendSOL(from: KeyPair, toAddress: String, sol: Double) async throws -> String {
        let lamports = UInt64((sol * 1e9).rounded())
        let ix = SystemProgram.transferInstruction(
            from: from.publicKey, to: try PublicKey(string: toAddress), lamports: lamports)
        let tx = Transaction(instructions: [ix], recentBlockhash: nil, feePayer: from.publicKey)
        return try await signAndSend(tx, signer: from)
    }

    /// Send an SPL token (e.g. KVR). Creates the recipient ATA if missing.
    /// `amount` is in human units; converted to base units via `decimals`.
    public func sendToken(
        from: KeyPair,
        mint: String,
        decimals: Int,
        toAddress: String,
        amount: Double
    ) async throws -> String {
        let mintPk = try PublicKey(string: mint)
        let fromAta = try PublicKey.associatedTokenAddress(
            walletAddress: from.publicKey, tokenMintAddress: mintPk, tokenProgramId: TokenProgram.id)
        let baseUnits = UInt64((amount * pow(10.0, Double(decimals))).rounded())

        // Resolve the recipient's token account + whether it must be created. Uses only
        // getAccountInfo (a supported RPC method) — no deprecated calls.
        let dest = try await apiClient.findSPLTokenDestinationAddress(
            mintAddress: mint, destinationAddress: toAddress, tokenProgramId: TokenProgram.id)

        var instructions: [TransactionInstruction] = []
        if dest.isUnregisteredAsocciatedToken {
            instructions.append(try AssociatedTokenProgram.createAssociatedTokenAccountInstruction(
                mint: mintPk, owner: try PublicKey(string: toAddress),
                payer: from.publicKey, tokenProgramId: TokenProgram.id))
        }
        // SPL Token `transferChecked` (instruction #12): [source, mint, dest, owner].
        instructions.append(TransactionInstruction(
            keys: [
                AccountMeta(publicKey: fromAta, isSigner: false, isWritable: true),
                AccountMeta(publicKey: mintPk, isSigner: false, isWritable: false),
                AccountMeta(publicKey: dest.destination, isSigner: false, isWritable: true),
                AccountMeta(publicKey: from.publicKey, isSigner: true, isWritable: false),
            ],
            programId: TokenProgram.id,
            data: [UInt8(12), baseUnits, UInt8(decimals)]))

        let tx = Transaction(instructions: instructions, recentBlockhash: nil, feePayer: from.publicKey)
        return try await signAndSend(tx, signer: from)
    }

    // MARK: - Modern-RPC transport

    private struct BlockhashResult: Decodable {
        struct Value: Decodable { let blockhash: String }
        let value: Value
    }
    private struct SigStatusResult: Decodable {
        struct Status: Decodable { let confirmationStatus: String? }
        let value: [Status?]
    }

    /// Inject a fresh finalized blockhash, sign locally, and raw-send. The public devnet
    /// RPC load-balances, so a just-fetched blockhash is occasionally "not found" during
    /// preflight — refetch and retry a few times (mirrors the Android client).
    private func signAndSend(_ transaction: Transaction, signer: KeyPair) async throws -> String {
        var lastError: Error?
        for attempt in 0..<4 {
            do {
                var tx = transaction
                tx.recentBlockhash = try await latestBlockhash()
                try tx.sign(signers: [signer])
                let serialized = try tx.serialize().base64EncodedString()
                let sig: String = try await apiClient.request(
                    method: "sendTransaction",
                    params: [serialized, RequestConfiguration(encoding: "base64")!])
                await confirm(signature: sig)
                return sig
            } catch {
                lastError = error
                if attempt < 3 { try? await Task.sleep(nanoseconds: 700_000_000) }
            }
        }
        throw lastError ?? StakingError.badURL
    }

    private func latestBlockhash() async throws -> String {
        // Preflight validates the blockhash at the finalized commitment.
        let r: BlockhashResult = try await apiClient.request(
            method: "getLatestBlockhash",
            params: [RequestConfiguration(commitment: "finalized")!])
        return r.value.blockhash
    }

    /// Poll until the tx reaches at least `confirmed` so the gateway can verify the
    /// payment on-chain. Best-effort: returns after the timeout regardless.
    private func confirm(signature: String, timeoutSec: Int = 30) async {
        for _ in 0..<timeoutSec {
            if let r: SigStatusResult = try? await apiClient.request(
                method: "getSignatureStatuses", params: [[signature]]
            ), let first = r.value.first, let s = first, let cs = s.confirmationStatus,
               cs == "confirmed" || cs == "finalized" {
                return
            }
            try? await Task.sleep(nanoseconds: 1_000_000_000)
        }
    }
}
