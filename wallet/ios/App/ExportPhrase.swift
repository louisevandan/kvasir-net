import SwiftUI

/// Reveals the stored BIP39 recovery phrase so the user can import the SAME
/// account on Android/desktop. Gated behind a biometric check before the words
/// are ever shown; on biometric failure nothing is revealed.
struct ExportPhraseView: View {
    @Environment(\.dismiss) var dismiss
    @ObservedObject private var loc = Localizer.shared
    let wallet: WalletStore

    @State private var words: [String]?
    @State private var checking = false
    @State private var denied = false

    var body: some View {
        NavigationStack {
            ZStack {
                BrandBackground()
                ScrollView {
                    VStack(spacing: 20) {
                        header
                        if let words {
                            revealedCard(words)
                        } else {
                            gateCard
                        }
                    }
                    .padding(20)
                }
            }
            .navigationTitle(loc.t("export.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarLeading) { Button(loc.t("common.close")) { dismiss() } } }
        }
        .tint(Brand.pink)
    }

    private var header: some View {
        VStack(spacing: 10) {
            Image(systemName: "key.horizontal.fill")
                .font(.system(size: 40))
                .foregroundStyle(Brand.gradient)
            Text(loc.t("export.desc"))
                .font(.footnote)
                .foregroundStyle(Brand.textSecondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .padding(.top, 4)
    }

    // MARK: - Locked gate (before biometric)

    private var gateCard: some View {
        VStack(spacing: 14) {
            Text(loc.t("export.warn"))
                .font(.footnote.weight(.medium))
                .foregroundStyle(.red)
                .multilineTextAlignment(.leading)
                .frame(maxWidth: .infinity, alignment: .leading)
            Button(action: reveal) {
                if checking { ProgressView().tint(.white) }
                else { Label(loc.t("export.reveal"), systemImage: "faceid") }
            }
            .buttonStyle(.brandPrimary)
            .disabled(checking)
            if denied {
                Text(loc.t("export.denied"))
                    .font(.caption)
                    .foregroundStyle(.red)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .brandCard()
    }

    // MARK: - Revealed words

    private func revealedCard(_ words: [String]) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(loc.t("export.warn"))
                .font(.caption.weight(.medium))
                .foregroundStyle(.red)
            LazyVGrid(columns: [GridItem(.flexible(), spacing: 10), GridItem(.flexible(), spacing: 10)], spacing: 10) {
                ForEach(Array(words.enumerated()), id: \.offset) { idx, word in
                    HStack(spacing: 8) {
                        Text("\(idx + 1)")
                            .font(.system(.caption, design: .monospaced).weight(.bold))
                            .foregroundStyle(Brand.pink)
                            .frame(width: 22, alignment: .trailing)
                        Text(word)
                            .font(.system(.callout, design: .rounded).weight(.semibold))
                            .foregroundStyle(Brand.textPrimary)
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 10)
                    .background(Brand.softGradient, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                }
            }
            HStack(spacing: 12) {
                CopyButton(text: words.joined(separator: " "), labelKey: "export.copy")
                    .buttonStyle(.brandSecondary)
                Button { withAnimation { self.words = nil } } label: {
                    Label(loc.t("export.hide"), systemImage: "eye.slash")
                }
                .buttonStyle(.brandSecondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private func reveal() {
        guard !checking else { return }
        checking = true; denied = false
        Task {
            let ok = await Biometrics.unlock(reason: loc.t("export.title"))
            if ok {
                withAnimation { words = wallet.revealMnemonic() }
                if words == nil { denied = true }
            } else {
                denied = true
            }
            checking = false
        }
    }
}
