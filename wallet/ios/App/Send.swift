import SwiftUI
import Core

struct SendView: View {
    @EnvironmentObject var store: WalletStore
    @ObservedObject private var loc = Localizer.shared
    @Environment(\.dismiss) var dismiss

    enum Asset: String, CaseIterable, Identifiable { case token, sol; var id: String { rawValue } }

    @State private var asset: Asset = .token
    @State private var toAddress = ""
    @State private var amountText = ""
    @State private var busy = false
    @State private var result: String?
    @State private var error: String?

    private var assetSymbol: String { asset == .token ? store.tokenSymbol : "SOL" }
    private var currentBalance: AssetBalance? { asset == .token ? store.tokenBalance : store.solBalance }

    var body: some View {
        NavigationStack {
            ZStack {
                BrandBackground()
                ScrollView {
                    VStack(spacing: 16) {
                        // Asset picker
                        VStack(alignment: .leading, spacing: 10) {
                            Text(loc.t("send.asset")).font(.system(.subheadline, design: .rounded).weight(.semibold))
                                .foregroundStyle(Brand.textSecondary)
                            if store.hasToken {
                                Picker(loc.t("send.asset"), selection: $asset) {
                                    Text(store.tokenSymbol).tag(Asset.token)
                                    Text("SOL").tag(Asset.sol)
                                }
                                .pickerStyle(.segmented)
                            } else {
                                Text("SOL · \(store.network.display)")
                                    .font(.system(.headline, design: .rounded))
                                    .foregroundStyle(Brand.textPrimary)
                            }
                            if let bal = currentBalance {
                                Text(loc.t("send.balance", fmt(bal.amount), assetSymbol))
                                    .font(.footnote).foregroundStyle(Brand.textSecondary)
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .brandCard()
                        .onAppear { if !store.hasToken { asset = .sol } }

                        // Recipient
                        fieldCard(title: loc.t("send.recipient")) {
                            TextField(loc.t("send.solanaAddress"), text: $toAddress)
                                .autocorrectionDisabled()
                                .textInputAutocapitalization(.never)
                                .font(.system(.callout, design: .monospaced))
                                .foregroundStyle(Brand.textPrimary)
                        }

                        // Amount
                        fieldCard(title: loc.t("common.amount")) {
                            HStack {
                                TextField("0.0", text: $amountText)
                                    .keyboardType(.decimalPad)
                                    .font(.system(.title3, design: .rounded))
                                    .foregroundStyle(Brand.textPrimary)
                                Text(assetSymbol)
                                    .font(.system(.headline, design: .rounded))
                                    .foregroundStyle(Brand.pink)
                            }
                        }

                        if let result {
                            VStack(alignment: .leading, spacing: 8) {
                                Label(loc.t("send.sent"), systemImage: "checkmark.seal.fill")
                                    .font(.system(.subheadline, design: .rounded).weight(.semibold))
                                    .foregroundStyle(Brand.pink)
                                Text(shorten(result)).font(.system(.footnote, design: .monospaced))
                                    .foregroundStyle(Brand.textSecondary)
                                if let url = store.explorerURL(sig: result) {
                                    Link(loc.t("send.viewInExplorer"), destination: url)
                                        .font(.footnote).tint(Brand.blue)
                                }
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .brandCard()
                        }

                        if let error {
                            Text(error).font(.footnote).foregroundStyle(.red)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }

                        Button { Task { await send() } } label: {
                            if busy { ProgressView().tint(.white) } else { Text(loc.t("common.send")) }
                        }
                        .buttonStyle(.brandPrimary)
                        .disabled(!canSend || busy)
                        .opacity(canSend ? 1 : 0.5)
                    }
                    .padding(20)
                }
            }
            .navigationTitle(loc.t("common.send"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarLeading) { Button(loc.t("common.close")) { dismiss() } } }
        }
        .tint(Brand.pink)
    }

    private func fieldCard<Content: View>(title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).font(.system(.subheadline, design: .rounded).weight(.semibold))
                .foregroundStyle(Brand.textSecondary)
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private var canSend: Bool {
        !toAddress.trimmingCharacters(in: .whitespaces).isEmpty && (Double(amountText) ?? 0) > 0
    }

    private func send() async {
        busy = true; defer { busy = false }
        error = nil; result = nil
        guard let amount = Double(amountText), amount > 0 else { error = loc.t("send.invalidAmount"); return }
        let to = toAddress.trimmingCharacters(in: .whitespacesAndNewlines)
        do {
            let sig: String
            switch asset {
            case .token: sig = try await store.sendToken(to: to, amount: amount)
            case .sol:   sig = try await store.sendSOL(to: to, amount: amount)
            }
            result = sig
        } catch {
            self.error = String(describing: error)
        }
    }
}
