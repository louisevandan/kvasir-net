import SwiftUI
import Foundation
import Core

/// Chat-style AI inference on Kvasir: each message pays KVR on-chain, then runs
/// the inference and renders the Markdown reply (Phase 2).
struct InferenceView: View {
    @Environment(\.dismiss) var dismiss
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @ObservedObject private var loc = Localizer.shared
    let wallet: WalletStore
    @StateObject private var store: InferenceStore
    @State private var input = ""
    @FocusState private var inputFocused: Bool
    @State private var showTopUp = false

    init(wallet: WalletStore) {
        self.wallet = wallet
        _store = StateObject(wrappedValue: InferenceStore(wallet: wallet))
    }

    var body: some View {
        NavigationStack {
            ZStack {
                BrandBackground()
                VStack(spacing: 0) {
                    messageList
                    composer
                }
            }
            .navigationTitle(loc.t("home.inferenceTitle"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button(loc.t("common.close")) { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) { creditChip }
                ToolbarItem(placement: .topBarTrailing) {
                    if !store.messages.isEmpty {
                        Button(role: .destructive) { store.clearHistory() } label: {
                            Label(loc.t("inference.clear"), systemImage: "trash")
                        }
                    }
                }
            }
            .task { await store.loadModels() }
            .sheet(isPresented: $showTopUp) { TopUpSheet(store: store) }
        }
        .tint(Brand.pink)
    }

    /// Prepaid-credit balance chip → opens the top-up sheet. Shows the balance once
    /// the wallet has an API key; otherwise a plain "credits" affordance.
    private var creditChip: some View {
        Button { showTopUp = true } label: {
            HStack(spacing: 4) {
                Image(systemName: "creditcard")
                if let b = store.creditBalance {
                    Text("\(b, specifier: "%.2f") \("KVR")")
                        .font(.caption.weight(.semibold)).monospacedDigit()
                } else {
                    Text(loc.t("inference.credits")).font(.caption.weight(.semibold))
                }
            }
        }
    }

    // MARK: - Message list

    private var messageList: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 16) {
                    if store.messages.isEmpty { emptyState }
                    ForEach(store.messages) { m in
                        MessageRow(message: m).id(m.id)
                    }
                    Color.clear.frame(height: 1).id(Self.bottomAnchor)
                }
                .padding(20)
            }
            .scrollDismissesKeyboard(.interactively)
            .onChange(of: store.messages.count) { _ in scrollToBottom(proxy) }
            .onChange(of: store.busy) { _ in scrollToBottom(proxy) }
        }
    }

    private static let bottomAnchor = "chat.bottom"

    private func scrollToBottom(_ proxy: ScrollViewProxy) {
        if reduceMotion {
            proxy.scrollTo(Self.bottomAnchor, anchor: .bottom)
        } else {
            withAnimation(.easeOut(duration: 0.25)) { proxy.scrollTo(Self.bottomAnchor, anchor: .bottom) }
        }
    }

    private var emptyState: some View {
        VStack(spacing: 10) {
            Text("✨").font(.system(size: 44))
            Text(loc.t("home.inferenceTitle"))
                .font(.system(.headline, design: .rounded))
                .foregroundStyle(Brand.textPrimary)
            Text(loc.t("inference.emptyNote"))
                .font(.footnote)
                .foregroundStyle(Brand.textSecondary)
                .multilineTextAlignment(.center)
            if let err = store.loadError {
                Text(err).font(.caption2).foregroundStyle(.red)
                    .multilineTextAlignment(.center)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.top, 60)
    }

    // MARK: - Composer

    private var canSend: Bool {
        !input.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && !store.selectedModel.isEmpty && !store.busy
    }

    private var composer: some View {
        VStack(spacing: 12) {
            if !store.options.isEmpty {
                Menu {
                    ForEach(store.options) { opt in
                        Button {
                            store.selectedModel = opt.id
                        } label: {
                            let title = opt.isLocal ? "\(opt.name) · \(loc.t("models.onDevice"))" : opt.name
                            if store.selectedModel == opt.id {
                                Label(title, systemImage: "checkmark")
                            } else if opt.isLocal {
                                Label(title, systemImage: "iphone")
                            } else {
                                Text(title)
                            }
                        }
                    }
                } label: {
                    HStack(spacing: 8) {
                        let sel = store.options.first(where: { $0.id == store.selectedModel }) ?? store.options.first
                        if sel?.isLocal == true {
                            Image(systemName: "iphone").font(.caption2).foregroundStyle(Brand.blue)
                        }
                        Text(sel.map { $0.isLocal ? "\($0.name) · \(loc.t("models.onDevice"))" : $0.name } ?? "")
                            .font(.system(.footnote, design: .rounded).weight(.semibold))
                            .foregroundStyle(Brand.textPrimary)
                            .lineLimit(1)
                        Spacer()
                        Image(systemName: "chevron.down")
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(Brand.textSecondary)
                    }
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .frame(maxWidth: .infinity)
                    .background(RoundedRectangle(cornerRadius: 12).fill(Brand.card))
                    .overlay(RoundedRectangle(cornerRadius: 12).stroke(Brand.stroke, lineWidth: 1))
                }
            }
            HStack(alignment: .bottom, spacing: 10) {
                TextField(loc.t("inference.messagePlaceholder"), text: $input, axis: .vertical)
                    .lineLimit(1...5)
                    .font(.callout)
                    .foregroundStyle(Brand.textPrimary)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 11)
                    .background(Brand.softGradient, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
                    .focused($inputFocused)

                Button(action: sendCurrent) {
                    if store.busy {
                        ProgressView().tint(Brand.pink).frame(width: 34, height: 34)
                    } else {
                        Image(systemName: "arrow.up.circle.fill")
                            .font(.system(size: 34))
                            .foregroundStyle(canSend
                                ? AnyShapeStyle(Brand.gradient)
                                : AnyShapeStyle(Brand.textSecondary.opacity(0.4)))
                    }
                }
                .disabled(!canSend)
                .accessibilityLabel(loc.t("inference.send"))
            }
        }
        .padding(16)
        .background(
            Brand.card
                .overlay(Rectangle().fill(Brand.stroke).frame(height: 1), alignment: .top)
                .ignoresSafeArea(edges: .bottom)
        )
    }

    private func sendCurrent() {
        let text = input
        input = ""
        inputFocused = false   // dismiss the keyboard on send (parity with Android)
        Task { await store.send(text) }
    }
}

// MARK: - Model selector chip

/// Credit top-up: transfer KVR on-chain to the gateway vault and credit it to the
/// prepaid balance that networked (streaming) inference debits.
private struct TopUpSheet: View {
    @ObservedObject var store: InferenceStore
    @ObservedObject private var loc = Localizer.shared
    @Environment(\.dismiss) private var dismiss
    @State private var amount = "10"
    @State private var message: String?

    private let presets = ["10", "50", "100"]

    var body: some View {
        NavigationStack {
            ZStack {
                BrandBackground()
                VStack(alignment: .leading, spacing: 18) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(loc.t("inference.currentBalance")).font(.caption).foregroundStyle(Brand.textSecondary)
                        Text(store.creditBalance.map { "\(String(format: "%.4f", $0)) \("KVR")" } ?? "—")
                            .font(.system(.title2, design: .rounded).weight(.bold)).foregroundStyle(Brand.textPrimary)
                    }
                    Text(loc.t("inference.topUpNote")).font(.footnote).foregroundStyle(Brand.textSecondary)
                    HStack(spacing: 8) {
                        ForEach(presets, id: \.self) { p in
                            Button { amount = p } label: {
                                Text("\(p) \("KVR")")
                                    .font(.footnote.weight(.semibold))
                                    .padding(.horizontal, 12).padding(.vertical, 7)
                                    .background(Capsule().fill(amount == p ? Brand.pink.opacity(0.2) : Color.clear))
                                    .overlay(Capsule().stroke(Brand.pink.opacity(0.5)))
                            }.foregroundStyle(Brand.pink)
                        }
                    }
                    TextField(loc.t("inference.topUpAmount"), text: $amount)
                        .keyboardType(.decimalPad)
                        .padding(12).background(RoundedRectangle(cornerRadius: 12).fill(Brand.card))
                    if let m = message { Text(m).font(.caption).foregroundStyle(Brand.textSecondary) }
                    Button {
                        Task {
                            message = await store.topUp(amount: Double(amount) ?? 0)
                            if store.creditBalance != nil, message == loc.t("inference.topUpOk") { dismiss() }
                        }
                    } label: {
                        HStack { if store.toppingUp { ProgressView().tint(.white) }
                            Text(loc.t("inference.topUpConfirm")).fontWeight(.semibold) }
                            .frame(maxWidth: .infinity).padding(.vertical, 14)
                            .background(Brand.gradient).foregroundStyle(.white).clipShape(RoundedRectangle(cornerRadius: 14))
                    }.disabled(store.toppingUp)
                    Spacer()
                }
                .padding(20)
            }
            .navigationTitle(loc.t("inference.topUpTitle"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button(loc.t("common.close")) { dismiss() } } }
        }
        .tint(Brand.pink)
        .task { await store.refreshBalance() }
    }
}

private struct ModelChip: View {
    let name: String
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(name)
                .font(.system(.footnote, design: .rounded).weight(.semibold))
                .foregroundStyle(selected ? .white : Brand.pink)
                .padding(.horizontal, 14)
                .padding(.vertical, 7)
                .background {
                    if selected {
                        Capsule().fill(Brand.gradient)
                    } else {
                        Capsule().fill(Brand.pink.opacity(0.12))
                    }
                }
        }
        .buttonStyle(.plain)
    }
}

// MARK: - One chat bubble

private struct MessageRow: View {
    @ObservedObject private var loc = Localizer.shared
    let message: ChatMessage

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            if message.role == .user {
                Spacer(minLength: 36)
                bubble
            } else {
                avatar
                VStack(alignment: .leading, spacing: 6) {
                    if let model = message.model, !model.isEmpty, !message.isError {
                        Text(model)
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(Brand.pink)
                            .padding(.leading, 4)
                    }
                    bubble
                    if let u = message.usage { footnote(u) }
                }
                Spacer(minLength: 36)
            }
        }
    }

    private var avatar: some View {
        Text("✨")
            .font(.system(size: 16))
            .frame(width: 32, height: 32)
            .background(Brand.softGradient, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).stroke(Brand.stroke, lineWidth: 1))
    }

    @ViewBuilder private var bubble: some View {
        if message.role == .user {
            Text(message.text)
                .font(.callout)
                .foregroundStyle(.white)
                .textSelection(.enabled)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background(Brand.gradient, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
        } else {
            assistantBubble
                .padding(.horizontal, 14)
                .padding(.vertical, 11)
                .background(Brand.card, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 18, style: .continuous).stroke(Brand.stroke, lineWidth: 1))
        }
    }

    @ViewBuilder private var assistantBubble: some View {
        if message.thinking {
            ThinkingIndicator()
        } else if message.isError {
            Text(message.text)
                .font(.callout)
                .foregroundStyle(.red)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        } else {
            MarkdownView(text: message.text)
                .textSelection(.enabled)
        }
    }

    private func footnote(_ u: TokenUsage) -> some View {
        let base = "\(loc.t("inference.actualTokens")): \(u.totalTokens) tok"
        let text = u.costToken.map { "\(base) · \(fmt($0)) KVR" } ?? base
        return Text(text)
            .font(.caption2)
            .foregroundStyle(Brand.textSecondary)
            .padding(.leading, 4)
    }
}

// MARK: - Animated "thinking…" indicator

private struct ThinkingIndicator: View {
    @ObservedObject private var loc = Localizer.shared
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var animating = false

    var body: some View {
        HStack(spacing: 8) {
            Text(loc.t("inference.thinking"))
                .font(.callout)
                .foregroundStyle(Brand.textSecondary)
            HStack(spacing: 4) {
                ForEach(0..<3, id: \.self) { i in
                    Circle()
                        .fill(Brand.pink)
                        .frame(width: 6, height: 6)
                        .opacity(reduceMotion ? 0.6 : (animating ? 1 : 0.25))
                        .animation(
                            reduceMotion ? nil
                            : .easeInOut(duration: 0.6).repeatForever().delay(Double(i) * 0.2),
                            value: animating)
                }
            }
        }
        .onAppear { animating = true }
    }
}

// MARK: - Lightweight block-level Markdown renderer

/// Renders assistant replies as formatted text: splits on lines and styles
/// headings, bullet/numbered lists, blockquotes, and fenced code blocks. Inline
/// emphasis/code/links are rendered via `AttributedString(markdown:)`.
struct MarkdownView: View {
    let text: String

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            let blocks = MarkdownParser.parse(text)
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                view(for: block)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder private func view(for block: MarkdownParser.Block) -> some View {
        switch block {
        case let .heading(level, content):
            Text(inline(content))
                .font(.system(headingStyle(level), design: .rounded).weight(.bold))
                .foregroundStyle(Brand.textPrimary)
                .fixedSize(horizontal: false, vertical: true)
        case let .paragraph(content):
            Text(inline(content))
                .font(.callout)
                .foregroundStyle(Brand.textPrimary)
                .fixedSize(horizontal: false, vertical: true)
        case let .bullet(content):
            HStack(alignment: .top, spacing: 8) {
                Text("•").font(.callout.weight(.bold)).foregroundStyle(Brand.pink)
                Text(inline(content)).font(.callout).foregroundStyle(Brand.textPrimary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        case let .numbered(number, content):
            HStack(alignment: .top, spacing: 8) {
                Text(number)
                    .font(.system(.callout, design: .rounded).weight(.semibold))
                    .foregroundStyle(Brand.pink)
                Text(inline(content)).font(.callout).foregroundStyle(Brand.textPrimary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        case let .quote(content):
            HStack(spacing: 10) {
                RoundedRectangle(cornerRadius: 2).fill(Brand.pink).frame(width: 3)
                Text(inline(content))
                    .font(.callout).italic()
                    .foregroundStyle(Brand.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        case let .code(content):
            Text(content)
                .font(.system(.footnote, design: .monospaced))
                .foregroundStyle(Brand.textPrimary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(10)
                .background(Brand.softGradient, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        }
    }

    private func headingStyle(_ level: Int) -> Font.TextStyle {
        switch level {
        case 1: return .title2
        case 2: return .title3
        default: return .headline
        }
    }

    /// Inline Markdown (bold/italic/code/links) → AttributedString, preserving
    /// the plain text if parsing fails.
    private func inline(_ s: String) -> AttributedString {
        (try? AttributedString(
            markdown: s,
            options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        )) ?? AttributedString(s)
    }
}

/// Splits Markdown source into block-level elements for `MarkdownView`.
enum MarkdownParser {
    enum Block {
        case heading(level: Int, content: String)
        case paragraph(content: String)
        case bullet(content: String)
        case numbered(number: String, content: String)
        case quote(content: String)
        case code(content: String)
    }

    static func parse(_ text: String) -> [Block] {
        var blocks: [Block] = []
        var paragraph: [String] = []
        let lines = text.components(separatedBy: "\n")
        var i = 0

        func flush() {
            if !paragraph.isEmpty {
                blocks.append(.paragraph(content: paragraph.joined(separator: "\n")))
                paragraph = []
            }
        }

        while i < lines.count {
            let line = lines[i]
            let trimmed = line.trimmingCharacters(in: .whitespaces)

            if trimmed.hasPrefix("```") {                       // fenced code block
                flush()
                var code: [String] = []
                i += 1
                while i < lines.count,
                      !lines[i].trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                    code.append(lines[i]); i += 1
                }
                i += 1                                          // skip closing fence
                blocks.append(.code(content: code.joined(separator: "\n")))
                continue
            }

            if trimmed.isEmpty { flush(); i += 1; continue }

            if let level = headingLevel(trimmed) {
                flush()
                let content = String(trimmed.drop { $0 == "#" }).trimmingCharacters(in: .whitespaces)
                blocks.append(.heading(level: level, content: content))
            } else if trimmed.hasPrefix(">") {
                flush()
                blocks.append(.quote(content: String(trimmed.dropFirst()).trimmingCharacters(in: .whitespaces)))
            } else if trimmed.hasPrefix("- ") || trimmed.hasPrefix("* ") {
                flush()
                blocks.append(.bullet(content: String(trimmed.dropFirst(2)).trimmingCharacters(in: .whitespaces)))
            } else if let (number, rest) = numberedItem(trimmed) {
                flush()
                blocks.append(.numbered(number: number, content: rest))
            } else {
                paragraph.append(line)
            }
            i += 1
        }
        flush()
        return blocks
    }

    private static func headingLevel(_ s: String) -> Int? {
        let hashes = s.prefix { $0 == "#" }.count
        guard hashes >= 1, hashes <= 6, s.count > hashes else { return nil }
        return s[s.index(s.startIndex, offsetBy: hashes)] == " " ? hashes : nil
    }

    private static func numberedItem(_ s: String) -> (String, String)? {
        let parts = s.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: false)
        guard let marker = parts.first, marker.hasSuffix("."),
              !marker.dropLast().isEmpty, marker.dropLast().allSatisfy(\.isNumber) else { return nil }
        let rest = parts.count > 1 ? String(parts[1]) : ""
        return (String(marker), rest)
    }
}
