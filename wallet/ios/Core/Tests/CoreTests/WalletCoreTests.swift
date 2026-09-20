import XCTest
@testable import Core

/// Offline tests: shared-spec loading, BIP39, and the cross-implementation
/// derivation vector. No network required.
final class WalletCoreTests: XCTestCase {

    func testSharedSpecLoadsRealMint() throws {
        let spec = try SharedSpec.loadToken()
        XCTAssertEqual(spec.token.symbol, "KVR")
        XCTAssertEqual(spec.token.decimals, 6)
        // The live mint, agreed by three sources that do not copy each other:
        // wallet/shared-spec/token.devnet.json, the explorer link inside it, and
        // what gate.kvasir-ai.net returns from /api/pay/models. The value this
        // test used to carry predates the mint the network actually runs on.
        XCTAssertEqual(spec.token.mint, "6cuJAmqtMuGzJ7s7eWQSqfJvEFRUdTiYR3cuMmiNoCPQ")
        XCTAssertEqual(spec.cluster, "devnet")
        XCTAssertFalse(spec.rpcUrl.isEmpty)
    }

    func testWalletConstantsDerivationPath() throws {
        let constants = try SharedSpec.loadConstants()
        XCTAssertEqual(constants.derivation.path, "m/44'/501'/0'/0'")
        XCTAssertEqual(constants.derivation.coinType, 501)
        XCTAssertEqual(constants.activeCluster, "devnet")
    }

    func testMnemonicGenerateLengths() {
        XCTAssertEqual(WalletMnemonic.generate(wordCount: 12).count, 12)
        XCTAssertEqual(WalletMnemonic.generate(wordCount: 24).count, 24)
    }

    func testMnemonicValidation() {
        let valid = "employ very include battle midnight usual broom school hollow manual embody slam"
        XCTAssertTrue(WalletMnemonic.isValid(WalletMnemonic.words(from: valid)))
        // wrong checksum
        XCTAssertFalse(WalletMnemonic.isValid(WalletMnemonic.words(from:
            "length else orbit dinner cannon dinosaur miss deal reward embody debate axis")))
        // not enough words
        XCTAssertFalse(WalletMnemonic.isValid(["hello", "world"]))
    }

    /// The derivation contract: this exact phrase must derive this exact address,
    /// matching the ecosystem-standard bip39 + ed25519-hd-key path m/44'/501'/0'/0'
    /// (as used by Phantom / @solana/web3.js). Guards against silent derivation drift
    /// and is the same vector the Android wallet must reproduce.
    func testDerivationVectorMatchesEcosystem() async throws {
        let phrase = WalletMnemonic.words(from:
            "employ very include battle midnight usual broom school hollow manual embody slam")
        let address = try await WalletDeriver.address(phrase: phrase)
        XCTAssertEqual(address, "EZFPUWABKyJKrZ5Ubbu5kdbraSkdCdLu9KPDeU3qgqEG")
    }
}
