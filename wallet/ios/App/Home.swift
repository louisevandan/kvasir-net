import SwiftUI
import CoreImage.CIFilterBuiltins
import Core

struct HomeView: View {
    @EnvironmentObject var store: WalletStore
    @ObservedObject private var loc = Localizer.shared
    @State private var showReceive = false
    @State private var showSend = false
    @State private var showStaking = false
    @State private var showInference = false
    @State private var showNodeSettings = false
    @State private var showNodeMonitor = false
    @State private var showModels = false
    @State private var showExport = false
    @State private var showAllHistory = false

    var body: some View {
        NavigationStack {
            ZStack {
                BrandBackground()
                ScrollView {
                    VStack(spacing: 18) {
                        balanceHero
                        actionRow
                        if store.network == .devnet {
                            stakingEntry
                            inferenceEntry
                        }
                        nodeSettingsEntry
                        modelsEntry
                        nodeMonitorEntry
                        historySection
                    }
                    .padding(20)
                }
            }
            .navigationTitle(loc.t("home.title"))
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Picker(loc.t("menu.network"), selection: Binding(
                            get: { store.network },
                            set: { store.setNetwork($0) }
                        )) {
                            ForEach(AppNetwork.allCases) { net in
                                Label(net.display, systemImage: "network").tag(net)
                            }
                        }
                        Picker(loc.t("menu.language"), selection: Binding(
                            get: { loc.lang },
                            set: { loc.lang = $0 }
                        )) {
                            ForEach(AppLanguage.allCases) { l in
                                Text(l.displayName).tag(l)
                            }
                        }
                        Divider()
                        Button { Task { await store.refresh() } } label: { Label(loc.t("menu.refresh"), systemImage: "arrow.clockwise") }
                        Button { showExport = true } label: { Label(loc.t("export.title"), systemImage: "key.horizontal") }
                        Button(role: .destructive) { store.logout() } label: { Label(loc.t("menu.deleteWallet"), systemImage: "trash") }
                    } label: { Image(systemName: "ellipsis.circle") }
                }
            }
            .refreshable { await store.refresh() }
            .task { await store.refresh() }
            // Own the node lifecycle app-wide from the home screen: once the operator
            // has left the node live, this resumes participation on cold start even
            // with no staking sheet open, and installs the foreground-resume observer.
            .task { NodePresence.shared.attach(wallet: store) }
            .sheet(isPresented: $showReceive) { ReceiveView() }
            .sheet(isPresented: $showSend) { SendView() }
            .sheet(isPresented: $showStaking) { StakingView(wallet: store) }
            .sheet(isPresented: $showInference) { InferenceView(wallet: store) }
            .sheet(isPresented: $showNodeSettings) { NodeSettingsSheet(wallet: store) }
            .sheet(isPresented: $showNodeMonitor) { NodeMonitorSheet(wallet: store) }
            .sheet(isPresented: $showModels) { ModelsManagerView() }
            .sheet(isPresented: $showExport) { ExportPhraseView(wallet: store) }
            .sheet(isPresented: $showAllHistory) { TransactionHistoryView() }
        }
        .tint(Brand.pink)
    }

    private var balanceHero: some View {
        VStack(spacing: 8) {
            networkBadge
            Text("\(store.hasToken ? store.tokenSymbol : "SOL") \(loc.t("home.balance"))")
                .font(.system(.subheadline, design: .rounded))
                .foregroundStyle(Brand.textSecondary)
            Text(heroAmount)
                .font(.system(size: 46, weight: .heavy, design: .rounded))
                .foregroundStyle(Brand.gradient)
                .minimumScaleFactor(0.5)
                .lineLimit(1)
            heroSecondary
            addressBlock
            if store.isLoading { ProgressView().tint(Brand.pink).padding(.top, 4) }
            if let err = store.errorMessage {
                Text(err).font(.caption2).foregroundStyle(.red).lineLimit(2)
            }
        }
        .frame(maxWidth: .infinity)
        .brandCard(padding: 24)
    }

    private var heroAmount: String {
        if store.hasToken { return store.tokenBalance.map { fmt($0.amount) } ?? "—" }
        return store.solBalance.map { fmt($0.amount) } ?? "—"
    }

    @ViewBuilder private var heroSecondary: some View {
        if store.hasToken {
            HStack(spacing: 6) {
                Image(systemName: "fuelpump.fill").font(.caption2)
                Text("\(store.solBalance.map { fmt($0.amount) } ?? "0") SOL")
            }
            .font(.system(.subheadline, design: .rounded))
            .foregroundStyle(Brand.textSecondary)
        } else {
            Text(loc.t("home.tokenNotIssued", store.tokenSymbol))
                .font(.caption)
                .foregroundStyle(Brand.textSecondary)
        }
    }

    @ViewBuilder private var addressBlock: some View {
        if let addr = store.address {
            Divider().overlay(Brand.stroke).padding(.vertical, 6)
            Text(addr)
                .font(.system(.caption2, design: .monospaced))
                .foregroundStyle(Brand.textSecondary)
                .multilineTextAlignment(.center)
                .textSelection(.enabled)
            CopyButton(text: addr)
                .font(.system(.footnote, design: .rounded).weight(.medium))
                .tint(Brand.pink)
        }
    }

    private var networkBadge: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(store.network == .mainnet ? Color.green : Brand.blue)
                .frame(width: 7, height: 7)
            Text(store.network.display)
                .font(.system(.caption2, design: .rounded).weight(.semibold))
        }
        .padding(.horizontal, 12).padding(.vertical, 5)
        .background(Brand.softGradient, in: Capsule())
        .foregroundStyle(Brand.textPrimary)
    }

    private var actionRow: some View {
        HStack(spacing: 12) {
            Button { showReceive = true } label: {
                Label(loc.t("common.receive"), systemImage: "qrcode")
            }
            .buttonStyle(.brandSecondary)
            Button { showSend = true } label: {
                Label(loc.t("common.send"), systemImage: "paperplane.fill")
            }
            .buttonStyle(.brandPrimary)
        }
    }

    private var stakingEntry: some View {
        Button { showStaking = true } label: {
            HStack(spacing: 12) {
                Image(systemName: "lock.circle.fill")
                    .font(.title2).foregroundStyle(Brand.gradient)
                VStack(alignment: .leading, spacing: 2) {
                    Text(loc.t("home.stakingTitle"))
                        .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.textPrimary)
                    Text(loc.t("home.stakingSubtitle"))
                        .font(.caption).foregroundStyle(Brand.textSecondary)
                }
                Spacer()
                Image(systemName: "chevron.right").foregroundStyle(Brand.textSecondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .brandCard()
        }
        .buttonStyle(.plain)
    }

    private var inferenceEntry: some View {
        Button { showInference = true } label: {
            HStack(spacing: 12) {
                Image(systemName: "sparkles")
                    .font(.title2).foregroundStyle(Brand.gradient)
                VStack(alignment: .leading, spacing: 2) {
                    Text(loc.t("home.inferenceTitle"))
                        .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.textPrimary)
                    Text(loc.t("home.inferenceSubtitle"))
                        .font(.caption).foregroundStyle(Brand.textSecondary)
                }
                Spacer()
                Image(systemName: "chevron.right").foregroundStyle(Brand.textSecondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .brandCard()
        }
        .buttonStyle(.plain)
    }

    private var nodeSettingsEntry: some View {
        entryCard(icon: "iphone.gen3", title: loc.t("home.nodeSettingsTitle"),
                  subtitle: loc.t("home.nodeSettingsSubtitle")) { showNodeSettings = true }
    }

    private var nodeMonitorEntry: some View {
        entryCard(icon: "chart.bar.xaxis", title: loc.t("home.nodeMonitorTitle"),
                  subtitle: loc.t("home.nodeMonitorSubtitle")) { showNodeMonitor = true }
    }

    private var modelsEntry: some View {
        entryCard(icon: "square.stack.3d.up", title: loc.t("models.title"),
                  subtitle: loc.t("models.subtitle")) { showModels = true }
    }

    private func entryCard(icon: String, title: String, subtitle: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Image(systemName: icon).font(.title2).foregroundStyle(Brand.gradient)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.system(.headline, design: .rounded)).foregroundStyle(Brand.textPrimary)
                    Text(subtitle).font(.caption).foregroundStyle(Brand.textSecondary)
                }
                Spacer()
                Image(systemName: "chevron.right").foregroundStyle(Brand.textSecondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .brandCard()
        }
        .buttonStyle(.plain)
    }

    private var historySection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(loc.t("home.history"))
                .font(.system(.headline, design: .rounded))
                .foregroundStyle(Brand.textPrimary)
            if store.transactions.isEmpty {
                Text(loc.t("home.noTransactions"))
                    .font(.footnote).foregroundStyle(Brand.textSecondary)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding(.vertical, 16)
            } else {
                VStack(spacing: 10) {
                    // Home shows only the latest 10; "더보기" opens the full history.
                    ForEach(store.transactions.prefix(10), id: \.signature) { tx in TxRowView(tx: tx) }
                }
                if store.transactions.count > 10 {
                    Button { showAllHistory = true } label: {
                        HStack(spacing: 4) {
                            Text(loc.t("home.more"))
                                .font(.system(.subheadline, design: .rounded).weight(.semibold))
                            Image(systemName: "chevron.right").font(.caption2.weight(.bold))
                        }
                        .foregroundStyle(Brand.pink)
                        .frame(maxWidth: .infinity)
                        .padding(.top, 4)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }
}

/// A single transaction row, shared by the home preview and the full history screen.
struct TxRowView: View {
    @EnvironmentObject var store: WalletStore
    let tx: TxRef

    var body: some View {
        HStack(spacing: 12) {
            ZStack {
                Circle().fill((tx.failed ? Color.red : Brand.pink).opacity(0.15)).frame(width: 34, height: 34)
                Image(systemName: tx.failed ? "xmark" : "checkmark")
                    .font(.caption.weight(.bold))
                    .foregroundStyle(tx.failed ? .red : Brand.pink)
            }
            VStack(alignment: .leading, spacing: 2) {
                Text(shorten(tx.signature))
                    .font(.system(.footnote, design: .monospaced))
                    .foregroundStyle(Brand.textPrimary)
                if let t = tx.blockTime {
                    Text(Date(timeIntervalSince1970: TimeInterval(t)), style: .date)
                        .font(.caption2).foregroundStyle(Brand.textSecondary)
                }
            }
            Spacer()
            if let url = store.explorerURL(sig: tx.signature) {
                Link(destination: url) {
                    Image(systemName: "arrow.up.right.square").foregroundStyle(Brand.blue)
                }
            }
        }
        .padding(.vertical, 4)
    }
}

/// Full transaction history (all fetched signatures), opened from the home "더보기".
struct TransactionHistoryView: View {
    @EnvironmentObject var store: WalletStore
    @ObservedObject private var loc = Localizer.shared
    @Environment(\.dismiss) var dismiss

    var body: some View {
        NavigationStack {
            ZStack {
                BrandBackground()
                ScrollView {
                    if store.transactions.isEmpty {
                        Text(loc.t("home.noTransactions"))
                            .font(.footnote).foregroundStyle(Brand.textSecondary)
                            .frame(maxWidth: .infinity).padding(.top, 60)
                    } else {
                        VStack(spacing: 10) {
                            ForEach(store.transactions, id: \.signature) { tx in TxRowView(tx: tx) }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .brandCard()
                        .padding(20)
                    }
                }
            }
            .navigationTitle(loc.t("history.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button(loc.t("common.close")) { dismiss() } } }
            .refreshable { await store.refresh() }
        }
        .tint(Brand.pink)
    }
}

struct ReceiveView: View {
    @EnvironmentObject var store: WalletStore
    @ObservedObject private var loc = Localizer.shared
    @Environment(\.dismiss) var dismiss

    var body: some View {
        NavigationStack {
            ZStack {
                BrandBackground()
                ScrollView {
                    VStack(spacing: 20) {
                        if let addr = store.address {
                            VStack(spacing: 16) {
                                if let img = QR.image(from: addr) {
                                    Image(uiImage: img)
                                        .interpolation(.none)
                                        .resizable()
                                        .frame(width: 216, height: 216)
                                        .padding(16)
                                        .background(.white, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
                                }
                                Text(addr)
                                    .font(.system(.callout, design: .monospaced))
                                    .foregroundStyle(Brand.textPrimary)
                                    .multilineTextAlignment(.center)
                                    .textSelection(.enabled)
                            }
                            .frame(maxWidth: .infinity)
                            .brandCard(padding: 24)

                            CopyButton(text: addr).buttonStyle(.brandPrimary)
                        }
                    }
                    .padding(20)
                }
            }
            .navigationTitle(loc.t("common.receive"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button(loc.t("common.close")) { dismiss() } } }
        }
        .tint(Brand.pink)
    }
}

/// Presents the mobile-phone node settings in a sheet with its own store.
struct NodeSettingsSheet: View {
    let wallet: WalletStore
    @ObservedObject private var loc = Localizer.shared
    @Environment(\.dismiss) var dismiss
    @StateObject private var staking: StakingStore

    init(wallet: WalletStore) {
        self.wallet = wallet
        _staking = StateObject(wrappedValue: StakingStore(wallet: wallet))
    }

    var body: some View {
        NavigationStack {
            NodeSettingsView(staking: staking)
                .toolbar { ToolbarItem(placement: .topBarTrailing) { Button(loc.t("common.close")) { dismiss() } } }
        }
        .tint(Brand.pink)
    }
}

/// Presents the node monitor (performance & contribution) in a sheet.
struct NodeMonitorSheet: View {
    let wallet: WalletStore
    @ObservedObject private var loc = Localizer.shared
    @Environment(\.dismiss) var dismiss
    @StateObject private var staking: StakingStore

    init(wallet: WalletStore) {
        self.wallet = wallet
        _staking = StateObject(wrappedValue: StakingStore(wallet: wallet))
    }

    var body: some View {
        NavigationStack {
            NodeMonitorView(staking: staking)
                .toolbar { ToolbarItem(placement: .topBarLeading) { Button(loc.t("common.close")) { dismiss() } } }
        }
        .tint(Brand.pink)
    }
}

/// Copies `text` to the clipboard and briefly shows a "복사됨" confirmation.
struct CopyButton: View {
    let text: String
    var labelKey: String = "common.copyAddress"
    @ObservedObject private var loc = Localizer.shared
    @State private var copied = false

    var body: some View {
        Button {
            UIPasteboard.general.string = text
            withAnimation { copied = true }
            Task {
                try? await Task.sleep(for: .seconds(1.5))
                withAnimation { copied = false }
            }
        } label: {
            Label(copied ? loc.t("common.copied") : loc.t(labelKey),
                  systemImage: copied ? "checkmark.circle.fill" : "doc.on.doc")
        }
    }
}

enum QR {
    static func image(from string: String) -> UIImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(string.utf8)
        guard let output = filter.outputImage?.transformed(by: CGAffineTransform(scaleX: 8, y: 8)) else { return nil }
        let context = CIContext()
        guard let cg = context.createCGImage(output, from: output.extent) else { return nil }
        return UIImage(cgImage: cg)
    }
}

func fmt(_ v: Double) -> String {
    let f = NumberFormatter()
    f.numberStyle = .decimal
    f.maximumFractionDigits = 6
    return f.string(from: NSNumber(value: v)) ?? "\(v)"
}

func shorten(_ s: String, head: Int = 6, tail: Int = 6) -> String {
    guard s.count > head + tail + 1 else { return s }
    return "\(s.prefix(head))…\(s.suffix(tail))"
}
