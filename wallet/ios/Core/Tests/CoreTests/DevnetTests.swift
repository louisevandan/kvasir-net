import XCTest
@testable import Core

/// Integration tests that hit Solana **devnet**. They verify SolanaService reads
/// against the real Kvasir KVR mint. Require network; skip logic keeps failures
/// readable if devnet is unreachable.
final class DevnetTests: XCTestCase {

    // Known devnet addresses from Phase 0.
    static let treasuryOwner = "8uu2gDKFVtNS79yqYyztJeerEKAh4cnZGdQytCjsYNfF"
    static let testWallet = "2XvSvZuNnrbGmDxUDbC2VjfCDXZnVApvLY8bxQJebeU7" // funded with 100 KVR

    func makeService() throws -> (SolanaService, TokenDevnetSpec) {
        try SolanaService.fromSharedSpec()
    }

    func testTreasuryHoldsBulkOfSupply() async throws {
        let (service, spec) = try makeService()
        let bal = try await service.tokenBalance(
            owner: Self.treasuryOwner, mint: spec.token.mint,
            symbol: spec.token.symbol, decimals: spec.token.decimals
        )
        XCTAssertEqual(bal.symbol, "KVR")
        XCTAssertEqual(bal.decimals, 6)
        // Treasury holds ~1e9 (minus small amounts moved to test wallets).
        XCTAssertGreaterThan(bal.amount, 999_990_000)
    }

    func testFundedTestWalletBalance() async throws {
        let (service, spec) = try makeService()
        let bal = try await service.tokenBalance(
            owner: Self.testWallet, mint: spec.token.mint,
            symbol: spec.token.symbol, decimals: spec.token.decimals
        )
        // We transferred exactly 100 KVR to this wallet in Phase 1 setup.
        XCTAssertEqual(bal.raw, 100_000000)
        XCTAssertEqual(bal.amount, 100, accuracy: 0.0001)
    }

    func testSolBalanceReadable() async throws {
        let (service, _) = try makeService()
        let sol = try await service.solBalance(owner: Self.treasuryOwner)
        XCTAssertEqual(sol.symbol, "SOL")
        XCTAssertGreaterThan(sol.raw, 0) // treasury owner was funded with devnet SOL
    }
}
