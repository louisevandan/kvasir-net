import SwiftUI

struct OnboardingView: View {
    @ObservedObject private var loc = Localizer.shared
    var body: some View {
        NavigationStack {
            ZStack {
                BrandBackground()
                VStack(spacing: 20) {
                    Spacer()
                    Image("BrandLogo")
                        .resizable()
                        .scaledToFill()
                        .frame(width: 132, height: 132)
                        .clipShape(RoundedRectangle(cornerRadius: 32, style: .continuous))
                        .overlay(RoundedRectangle(cornerRadius: 32, style: .continuous).stroke(.white.opacity(0.6), lineWidth: 1))
                        .shadow(color: Brand.pink.opacity(0.4), radius: 24, y: 12)

                    VStack(spacing: 8) {
                        Text("Kvasir Wallet").brandTitle()
                        Text(loc.t("onboarding.subtitle"))
                            .font(.system(.subheadline, design: .rounded))
                            .foregroundStyle(Brand.textSecondary)
                    }
                    Spacer()
                    VStack(spacing: 12) {
                        NavigationLink { CreateWalletView() } label: { Text(loc.t("onboarding.create")) }
                            .buttonStyle(.brandPrimary)
                        NavigationLink { ImportWalletView() } label: { Text(loc.t("onboarding.import")) }
                            .buttonStyle(.brandSecondary)
                    }
                }
                .padding(28)
            }
        }
        .tint(Brand.pink)
    }
}

/// Generate a fresh mnemonic, show it, require the user to confirm they saved it.
struct CreateWalletView: View {
    @EnvironmentObject var store: WalletStore
    @ObservedObject private var loc = Localizer.shared
    @State private var phrase: [String] = []
    @State private var saved = false
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        ZStack {
            BrandBackground()
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(loc.t("create.phraseHeader"))
                            .font(.system(.title3, design: .rounded).weight(.bold))
                            .foregroundStyle(Brand.textPrimary)
                        Text(loc.t("create.phraseWarning"))
                            .font(.footnote)
                            .foregroundStyle(Brand.textSecondary)
                    }

                    LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 10), count: 3), spacing: 10) {
                        ForEach(Array(phrase.enumerated()), id: \.offset) { idx, word in
                            HStack(spacing: 6) {
                                Text("\(idx + 1)")
                                    .font(.caption2.weight(.bold))
                                    .foregroundStyle(Brand.pink)
                                Text(word)
                                    .font(.system(.callout, design: .rounded))
                                    .foregroundStyle(Brand.textPrimary)
                                Spacer(minLength: 0)
                            }
                            .padding(.vertical, 10).padding(.horizontal, 12)
                            .background(Brand.softGradient, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                        }
                    }
                    .brandCard(padding: 16)

                    Toggle(isOn: $saved) {
                        Text(loc.t("create.savedToggle"))
                            .font(.subheadline)
                            .foregroundStyle(Brand.textPrimary)
                    }
                    .tint(Brand.pink)

                    if let error {
                        Text(error).font(.footnote).foregroundStyle(.red)
                    }

                    Button { Task { await save() } } label: {
                        if busy { ProgressView().tint(.white) } else { Text(loc.t("create.start")) }
                    }
                    .buttonStyle(.brandPrimary)
                    .disabled(!saved || busy)
                    .opacity(saved ? 1 : 0.5)
                }
                .padding(20)
            }
        }
        .navigationTitle(loc.t("create.title"))
        .navigationBarTitleDisplayMode(.inline)
        .tint(Brand.pink)
        .onAppear { if phrase.isEmpty { phrase = store.newMnemonic(wordCount: 12) } }
    }

    private func save() async {
        busy = true; defer { busy = false }
        do { try await store.saveAndActivate(phrase: phrase) }
        catch { self.error = String(describing: error) }
    }
}

/// Import an existing 12/24-word mnemonic.
struct ImportWalletView: View {
    @EnvironmentObject var store: WalletStore
    @ObservedObject private var loc = Localizer.shared
    @State private var text = ""
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        ZStack {
            BrandBackground()
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Text(loc.t("import.phraseHeader"))
                        .font(.system(.title3, design: .rounded).weight(.bold))
                        .foregroundStyle(Brand.textPrimary)
                    Text(loc.t("import.instruction"))
                        .font(.footnote).foregroundStyle(Brand.textSecondary)

                    TextEditor(text: $text)
                        .frame(minHeight: 140)
                        .scrollContentBackground(.hidden)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .font(.system(.callout, design: .monospaced))
                        .foregroundStyle(Brand.textPrimary)
                        .brandCard(padding: 12)

                    if let error { Text(error).font(.footnote).foregroundStyle(.red) }

                    Button { Task { await restore() } } label: {
                        if busy { ProgressView().tint(.white) } else { Text(loc.t("import.restore")) }
                    }
                    .buttonStyle(.brandPrimary)
                    .disabled(busy || text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
                .padding(20)
            }
        }
        .navigationTitle(loc.t("import.title"))
        .navigationBarTitleDisplayMode(.inline)
        .tint(Brand.pink)
    }

    private func restore() async {
        busy = true; defer { busy = false }
        let words = text.lowercased().split(whereSeparator: { $0.isWhitespace }).map(String.init)
        do { try await store.saveAndActivate(phrase: words) }
        catch { self.error = loc.t("import.invalidPhrase") }
    }
}
