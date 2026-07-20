import SwiftUI
import Core

/// Operator dashboard: live status of the operator's registered nodes and rewards.
struct NodeMonitorView: View {
    @ObservedObject var staking: StakingStore
    @ObservedObject private var loc = Localizer.shared
    @ObservedObject private var hub = HubParticipation.shared
    @State private var pendingRemove: NodeStatusItem?
    @State private var showRemoveConfirm = false

    var body: some View {
        ZStack {
            BrandBackground()
            ScrollView {
                VStack(spacing: 16) {
                    hubConnectionCard
                    if let s = staking.nodeStatus {
                        summaryCard(s.totals)
                        tierLegend
                        if s.nodes.isEmpty {
                            emptyCard
                        } else {
                            ForEach(consolidateExpertNodes(s.nodes)) { nodeCard($0) }
                        }
                    } else if staking.message == nil {
                        ProgressView().tint(Brand.pink).padding(.top, 60)
                    }
                    if let msg = staking.message {
                        Text(msg).font(.caption).foregroundStyle(.red)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(20)
            }
        }
        .navigationTitle(loc.t("monitor.navTitle"))
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                NavigationLink { DeviceConnectView(staking: staking) } label: {
                    Label(loc.t("monitor.connectDevice"), systemImage: "plus.circle")
                }
            }
        }
        .task { await staking.loadStatus() }
        .refreshable { await staking.loadStatus() }
        .confirmationDialog(
            loc.t("node.removeConfirm"),
            isPresented: $showRemoveConfirm,
            presenting: pendingRemove
        ) { node in
            Button(loc.t("node.remove"), role: .destructive) {
                Task { await staking.removeNode(node.nodeId) }
            }
            Button(loc.t("common.close"), role: .cancel) {}
        } message: { node in
            Text(node.label ?? node.nodeId)
        }
    }

    /// Hub connection status — which known hubs the node auto-connects to on
    /// launch and whether it is currently serving one.
    private var hubConnectionCard: some View {
        let serving = !hub.servingHost.isEmpty
        let dotColor: Color = serving ? .green : (hub.connected ? Brand.pink : Brand.textSecondary)
        let stateText: String = serving
            ? loc.t("monitor.hubServing")
            : (hub.connected ? loc.t("monitor.hubConnectedIdle") : loc.t("monitor.hubDisconnected"))
        return VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Image(systemName: "antenna.radiowaves.left.and.right")
                    .foregroundStyle(Brand.pink)
                Text(loc.t("monitor.hubTitle"))
                    .font(.system(.subheadline, design: .rounded).weight(.bold))
                    .foregroundStyle(Brand.textPrimary)
                Spacer()
                HStack(spacing: 6) {
                    Circle().fill(dotColor).frame(width: 8, height: 8)
                    Text(stateText).font(.caption.weight(.semibold)).foregroundStyle(Brand.textSecondary)
                }
            }
            if hub.hubs.isEmpty {
                Text(loc.t("monitor.hubNone"))
                    .font(.caption2).foregroundStyle(Brand.textSecondary)
            } else {
                ForEach(hub.hubs, id: \.self) { url in
                    HStack(spacing: 6) {
                        let isServed = serving && hubHost(url) == hub.servingHost
                        Circle().fill(isServed ? Color.green : Brand.pink).frame(width: 6, height: 6)
                        Text(hubHost(url))
                            .font(.caption.monospaced()).foregroundStyle(Brand.textPrimary)
                        Spacer()
                        if isServed {
                            Text(hub.lastStatus).font(.caption2).foregroundStyle(Brand.textSecondary)
                                .lineLimit(1).truncationMode(.tail)
                        }
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private func hubHost(_ url: String) -> String { URL(string: url)?.host ?? url }

    private func summaryCard(_ t: NodeStatusTotals) -> some View {
        VStack(spacing: 14) {
            HStack {
                metric("\(t.nodes)", loc.t("monitor.nodes"))
                metric("\(t.online)", loc.t("monitor.online"))
            }
            HStack {
                metric(fmt(t.contributedUnits), loc.t("monitor.rawContribUnit"))
                metric(fmt(t.effectiveUnits ?? t.contributedUnits), loc.t("monitor.effectiveWeighted"))
            }
            HStack {
                metric(fmt(t.lifetimeRewards), loc.t("monitor.lifetimeRewards"))
                metric(fmt(t.pending), loc.t("monitor.claimableKVR"))
            }
        }
        .frame(maxWidth: .infinity)
        .brandCard()
    }

    private var tierLegend: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(loc.t("monitor.tierTitle"))
                .font(.system(.subheadline, design: .rounded).weight(.bold)).foregroundStyle(Brand.textPrimary)
            Text(loc.t("monitor.tierDesc"))
                .font(.caption2).foregroundStyle(Brand.textSecondary)
            tierRow("S", "≥ 90 tok/s", "×1.5")
            tierRow("A", "60–89 tok/s", "×1.25")
            tierRow("B", "30–59 tok/s", "×1.0")
            tierRow("C", "< 30 tok/s", "×0.7")
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private func tierRow(_ tier: String, _ range: String, _ mult: String) -> some View {
        HStack(spacing: 10) {
            tierBadge(tier, size: 24)
            Text(range).font(.caption).foregroundStyle(Brand.textSecondary)
            Spacer()
            Text(mult).font(.system(.subheadline, design: .monospaced).weight(.bold)).foregroundStyle(tierColor(tier))
        }
        .padding(.vertical, 2)
    }

    private func tierBadge(_ tier: String, size: CGFloat) -> some View {
        Text(tier)
            .font(.system(size: size * 0.55, weight: .heavy, design: .rounded))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(tierColor(tier), in: Circle())
    }

    private func tierColor(_ tier: String) -> Color {
        switch tier {
        case "S": return .orange
        case "A": return .green
        case "B": return Brand.blue
        default: return .gray
        }
    }

    private func modeLabel(_ mode: String?) -> String {
        switch mode {
        case "local_shard": return loc.t("mode.localShard")
        case "rpc_worker": return loc.t("mode.rpcWorker")
        default: return mode ?? ""
        }
    }

    private func nodeCard(_ n: NodeStatusItem) -> some View {
        NavigationLink {
            NodeSettingsView(staking: staking)
        } label: {
            nodeCardContent(n)
        }
        .buttonStyle(.plain)
        .accessibilityHint(loc.t("node.openSettings"))
        .overlay(alignment: .bottomTrailing) {
            Button(role: .destructive) {
                pendingRemove = n
                showRemoveConfirm = true
            } label: {
                Image(systemName: "trash")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.red)
                    .frame(width: 36, height: 36)
                    .background(.red.opacity(0.12), in: Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(loc.t("node.remove"))
            .padding(12)
        }
    }

    private func nodeCardContent(_ n: NodeStatusItem) -> some View {
        let tier = n.tier ?? "—"
        let mult = n.perfMultiplier ?? 1.0
        let eff = n.effectiveUnits ?? n.contributedUnits
        return VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                Image(systemName: osIcon(n.os)).font(.title3).foregroundStyle(Brand.blue)
                VStack(alignment: .leading, spacing: 2) {
                    Text(n.label ?? n.nodeId)
                        .font(.system(.subheadline, design: .rounded).weight(.semibold))
                        .foregroundStyle(Brand.textPrimary)
                    Text("\(osLabel(n.os)) · \(accelLabel(n.accelerator))")
                        .font(.caption2).foregroundStyle(Brand.textSecondary)
                }
                Spacer()
                statusPill(n.status)
                Image(systemName: "chevron.right")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(Brand.textSecondary)
            }

            // performance re-scoring panel
            HStack(spacing: 12) {
                tierBadge(tier, size: 36)
                VStack(alignment: .leading, spacing: 2) {
                    Text(loc.t("monitor.perfTier", tier, fmt(mult)))
                        .font(.system(.footnote, design: .rounded).weight(.bold)).foregroundStyle(tierColor(tier))
                    Text(perfSubtitle(n)).font(.caption2).foregroundStyle(Brand.textSecondary)
                }
                Spacer()
            }
            .padding(12)
            .background(tierColor(tier).opacity(0.10), in: RoundedRectangle(cornerRadius: 12, style: .continuous))

            HStack(spacing: 4) {
                Text(loc.t("monitor.rawContribInline", fmt(n.contributedUnits))).font(.caption).foregroundStyle(Brand.textSecondary)
                Text("×\(fmt(mult))").font(.caption.weight(.bold)).foregroundStyle(tierColor(tier))
                Text(loc.t("monitor.effectiveInline", fmt(eff))).font(.caption.weight(.semibold)).foregroundStyle(Brand.textPrimary)
            }

            HStack {
                metric(fmt(eff), loc.t("monitor.effectiveContrib"))
                metric(fmt(n.pendingRewards), loc.t("monitor.claimable"))
                metric(fmt(n.claimedTotal), loc.t("monitor.received"))
            }
            if let last = n.lastReport {
                HStack(spacing: 4) {
                    Image(systemName: "clock").font(.caption2)
                    Text(loc.t("monitor.lastReport"))
                    Text(Date(timeIntervalSince1970: last), style: .relative)
                }
                .font(.caption2).foregroundStyle(Brand.textSecondary)
            } else {
                Text(loc.t("monitor.noReport"))
                    .font(.caption2).foregroundStyle(Brand.textSecondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private var emptyCard: some View {
        VStack(spacing: 8) {
            Image(systemName: "server.rack").font(.largeTitle).foregroundStyle(Brand.textSecondary)
            Text(loc.t("monitor.emptyTitle"))
                .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.textPrimary)
            Text(loc.t("monitor.emptyDesc"))
                .font(.footnote).foregroundStyle(Brand.textSecondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .brandCard(padding: 28)
    }

    private func metric(_ value: String, _ title: String) -> some View {
        VStack(spacing: 2) {
            Text(value)
                .font(.system(.title3, design: .rounded).weight(.bold))
                .foregroundStyle(Brand.gradient)
                .minimumScaleFactor(0.6).lineLimit(1)
            Text(title).font(.caption2).foregroundStyle(Brand.textSecondary)
        }
        .frame(maxWidth: .infinity)
    }

    private func statusPill(_ status: String) -> some View {
        let (color, label) = statusMeta(status)
        return HStack(spacing: 5) {
            Circle().fill(color).frame(width: 7, height: 7)
            Text(label).font(.caption2.weight(.semibold))
        }
        .padding(.horizontal, 10).padding(.vertical, 4)
        .background(color.opacity(0.15), in: Capsule())
        .foregroundStyle(color)
    }

    private func statusMeta(_ s: String) -> (Color, String) {
        switch s {
        case "online": return (.green, loc.t("status.online"))
        case "idle": return (.orange, loc.t("status.idle"))
        case "registered": return (Brand.blue, loc.t("status.registered"))
        default: return (.gray, loc.t("status.offline"))
        }
    }

    private func osIcon(_ os: String?) -> String {
        switch os?.lowercased() {
        case "macos": return "desktopcomputer"
        case "ios": return "iphone"
        case "android": return "candybarphone"
        case "windows": return "pc"
        case "linux": return "terminal"
        default: return "cpu"
        }
    }
    private func osLabel(_ os: String?) -> String {
        switch os?.lowercased() {
        case "macos": return "macOS"
        case "ios": return "iOS"
        case "android": return "Android"
        case "windows": return "Windows"
        case "linux": return "Linux"
        default: return "Unknown"
        }
    }
    private func accelLabel(_ a: String?) -> String { (a ?? "cpu").uppercased() }

    private func perfSubtitle(_ n: NodeStatusItem) -> String {
        var parts: [String] = []
        if let b = n.backend, !b.isEmpty { parts.append(b.uppercased()) }
        if let p = n.perfScore, p > 0 { parts.append("\(Int(p)) tok/s") }
        if let m = n.mode, !m.isEmpty { parts.append(modeLabel(m)) }
        return parts.isEmpty ? loc.t("monitor.noPerfData") : parts.joined(separator: " · ")
    }
}

// The settlement gateway upserts a hub-qualified node (`infer-<hubKey>-<nodeId>`)
// for a phone's expert-shard contribution, separate from the phone's own
// registration node (`<nodeId>`). Fold every such work-node's reward into its
// base node so a phone's earnings show on its own card, not a mystery second one.
// Work-nodes with no matching base (datacenter agents) are left untouched.
private func consolidateExpertNodes(_ nodes: [NodeStatusItem]) -> [NodeStatusItem] {
    let ids = Set(nodes.map { $0.nodeId })
    func baseId(_ id: String) -> String? {
        guard id.hasPrefix("infer-") else { return nil }
        let rest = id.dropFirst("infer-".count)                 // <hubKey>-<baseId>
        guard let dash = rest.firstIndex(of: "-") else { return nil }
        return String(rest[rest.index(after: dash)...])
    }
    var merged: [String: NodeStatusItem] = [:]
    var order: [String] = []
    for n in nodes where !(baseId(n.nodeId).map { ids.contains($0) } ?? false) {
        merged[n.nodeId] = n
        order.append(n.nodeId)
    }
    for n in nodes {
        guard let base = baseId(n.nodeId), var t = merged[base] else { continue }
        t.contributedUnits += n.contributedUnits
        t.effectiveUnits = (t.effectiveUnits ?? 0) + (n.effectiveUnits ?? 0)
        t.pendingRewards += n.pendingRewards
        t.claimedTotal += n.claimedTotal
        merged[base] = t
    }
    return order.compactMap { merged[$0] }
}
