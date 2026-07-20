import SwiftUI
import Core

struct StakingView: View {
    @Environment(\.dismiss) var dismiss
    @ObservedObject private var loc = Localizer.shared
    let wallet: WalletStore
    @StateObject private var staking: StakingStore

    @State private var stakeAmount = ""
    @State private var nodeId = ""
    @State private var showSettings = false
    @State private var urlText = ""

    init(wallet: WalletStore) {
        self.wallet = wallet
        _staking = StateObject(wrappedValue: StakingStore(wallet: wallet))
    }

    var body: some View {
        NavigationStack {
            ZStack {
                BrandBackground()
                ScrollView {
                    VStack(spacing: 16) {
                        guideLink
                        stakingCard
                        nodeRewardsCard
                        if staking.busy { ProgressView().tint(Brand.pink) }
                        if let sig = staking.lastSignature, let url = staking.explorerURL(sig: sig) {
                            Link(loc.t("staking.recentTx"), destination: url)
                                .font(.footnote).tint(Brand.blue)
                        }
                        if let msg = staking.message {
                            Text(msg).font(.caption).foregroundStyle(.red)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    .padding(20)
                }
            }
            .navigationTitle(loc.t("staking.title"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button(loc.t("common.close")) { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) {
                    Button { urlText = staking.serviceURL; showSettings = true } label: { Image(systemName: "gearshape") }
                }
            }
            .task { await staking.load() }
            .refreshable { await staking.load() }
            .sheet(isPresented: $showSettings) { settingsSheet }
        }
        .tint(Brand.pink)
    }

    private var guideLink: some View {
        NavigationLink { StakingGuideView() } label: {
            HStack(spacing: 10) {
                Image(systemName: "book.circle.fill").font(.title3).foregroundStyle(Brand.gradient)
                Text(loc.t("staking.guideLink"))
                    .font(.system(.subheadline, design: .rounded).weight(.semibold))
                    .foregroundStyle(Brand.textPrimary)
                Spacer()
                Image(systemName: "chevron.right").font(.footnote).foregroundStyle(Brand.textSecondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .brandCard(padding: 14)
        }
        .buttonStyle(.plain)
    }

    // MARK: staking

    private var stakingCard: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Label(loc.t("staking.title"), systemImage: "lock.circle.fill")
                    .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.pink)
                Spacer()
                if let apr = staking.config?.aprPercent {
                    Text("APR \(fmt(apr))%")
                        .font(.system(.subheadline, design: .rounded).weight(.semibold))
                        .foregroundStyle(Brand.textSecondary)
                }
            }
            HStack {
                statTile(loc.t("staking.staked"), staking.position?.principal)
                statTile(loc.t("staking.reward"), staking.position?.rewards)
            }
            HStack {
                TextField(loc.t("common.amount"), text: $stakeAmount)
                    .keyboardType(.decimalPad)
                    .font(.system(.title3, design: .rounded))
                    .foregroundStyle(Brand.textPrimary)
                Text("KVR").font(.system(.headline, design: .rounded)).foregroundStyle(Brand.pink)
            }
            .padding(.vertical, 6).padding(.horizontal, 12)
            .background(Brand.softGradient, in: RoundedRectangle(cornerRadius: 12, style: .continuous))

            // Staking is reserved for online contributing nodes — the backend
            // rejects a stake from an owner with no online node, so gate here too.
            if (staking.nodeStatus?.totals.online ?? 0) == 0 {
                Text("⚠ " + loc.t("staking.needOnline"))
                    .font(.system(.footnote, design: .rounded))
                    .foregroundStyle(Brand.warn)
            }
            Button { Task { let a = Double(stakeAmount) ?? 0; if a > 0 { await staking.stake(amount: a); stakeAmount = "" } } } label: {
                Text(loc.t("staking.stake"))
            }
            .buttonStyle(.brandPrimary)
            .disabled(staking.busy || (Double(stakeAmount) ?? 0) <= 0 || (staking.nodeStatus?.totals.online ?? 0) == 0)

            Button { Task { await staking.unstake(amount: nil) } } label: {
                Text(loc.t("staking.unstakeAll"))
            }
            .buttonStyle(.brandSecondary)
            .disabled(staking.busy || (staking.position?.principal ?? 0) <= 0)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    // MARK: node rewards

    private var nodeRewardsCard: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Label(loc.t("staking.nodeOperatorRewards"), systemImage: "server.rack")
                    .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.blue)
                Spacer()
                if let r = staking.nodeRewards?.rewardPerUnit {
                    Text("\(fmt(r)) KVR/unit").font(.caption).foregroundStyle(Brand.textSecondary)
                }
            }
            statTile(loc.t("staking.claimableRewards"), staking.nodeRewards?.pending)

            if let rawNodes = staking.nodeRewards?.nodes, !rawNodes.isEmpty {
                let nodes = consolidateRewardNodes(rawNodes)
                VStack(spacing: 8) {
                    ForEach(nodes) { n in
                        HStack {
                            Text(n.nodeId).font(.system(.footnote, design: .monospaced)).foregroundStyle(Brand.textPrimary)
                            Spacer()
                            Text("\(fmt(n.pendingRewards)) KVR").font(.footnote).foregroundStyle(Brand.textSecondary)
                        }
                    }
                }
            }

            HStack {
                TextField(loc.t("staking.nodeIdPlaceholder"), text: $nodeId)
                    .autocorrectionDisabled().textInputAutocapitalization(.never)
                    .font(.system(.callout, design: .monospaced)).foregroundStyle(Brand.textPrimary)
                Button(loc.t("staking.register")) { Task { let id = nodeId.trimmingCharacters(in: .whitespaces); if !id.isEmpty { await staking.registerNode(id); nodeId = "" } } }
                    .font(.system(.subheadline, design: .rounded).weight(.semibold))
                    .tint(Brand.pink)
            }
            .padding(.vertical, 6).padding(.horizontal, 12)
            .background(Brand.softGradient, in: RoundedRectangle(cornerRadius: 12, style: .continuous))

            Button { Task { await staking.claim() } } label: { Text(loc.t("staking.claim")) }
                .buttonStyle(.brandPrimary)
                .disabled(staking.busy || (staking.nodeRewards?.pending ?? 0) <= 0)

            NavigationLink { NodeMonitorView(staking: staking) } label: {
                HStack(spacing: 8) {
                    Image(systemName: "chart.bar.xaxis").font(.footnote)
                    Text(loc.t("staking.nodeStatusLink"))
                        .font(.system(.subheadline, design: .rounded).weight(.medium))
                    Spacer()
                    Image(systemName: "chevron.right").font(.footnote)
                }
                .foregroundStyle(Brand.blue)
                .padding(.top, 2)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private func statTile(_ title: String, _ value: Double?) -> some View {
        VStack(spacing: 2) {
            Text(value.map { fmt($0) } ?? "—")
                .font(.system(.title2, design: .rounded).weight(.bold))
                .foregroundStyle(Brand.gradient)
            Text(title).font(.caption2).foregroundStyle(Brand.textSecondary)
        }
        .frame(maxWidth: .infinity)
    }

    // MARK: settings

    private var settingsSheet: some View {
        NavigationStack {
            ZStack {
                BrandBackground()
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        Text(loc.t("staking.serviceUrlTitle"))
                            .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.textPrimary)
                        Text(loc.t("staking.serviceUrlDesc"))
                            .font(.footnote).foregroundStyle(Brand.textSecondary)
                        TextField("https://gate.kvasir-ai.net", text: $urlText)
                            .autocorrectionDisabled().textInputAutocapitalization(.never)
                            .keyboardType(.URL)
                            .font(.system(.callout, design: .monospaced)).foregroundStyle(Brand.textPrimary)
                            .brandCard(padding: 14)
                        Button {
                            staking.updateServiceURL(urlText)
                            showSettings = false
                            Task { await staking.load() }
                        } label: { Text(loc.t("common.save")) }
                        .buttonStyle(.brandPrimary)

                        genesisCard
                    }
                    .padding(20)
                }
            }
            .navigationTitle(loc.t("common.settings"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarLeading) { Button(loc.t("common.close")) { showSettings = false } } }
        }
        .tint(Brand.pink)
    }

    // MARK: genesis gateway info

    /// Info on the network coordination node (the active settlement/inference gateway),
    /// read from its /api/config. Reachability reflects whether that config loaded.
    private var genesisCard: some View {
        let cfg = staking.config
        return VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text(loc.t("genesis.title"))
                    .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.textPrimary)
                Spacer()
                HStack(spacing: 5) {
                    Circle().fill(cfg == nil ? Color.red : Color.green).frame(width: 8, height: 8)
                    Text(cfg == nil ? loc.t("genesis.unreachable") : loc.t("genesis.reachable"))
                        .font(.caption).foregroundStyle(Brand.textSecondary)
                }
            }
            Text(loc.t("genesis.desc")).font(.footnote).foregroundStyle(Brand.textSecondary)
            genesisURLRow(loc.t("genesis.active"), staking.serviceURL, tint: Brand.textPrimary)
            if let pub = cfg?.publicUrl, !pub.isEmpty, pub != staking.serviceURL {
                genesisURLRow(loc.t("genesis.advertised"), pub, tint: Brand.pink)
            }
            if let cfg {
                genesisFact(loc.t("genesis.cluster"), cfg.cluster)
                genesisFact("APR", "\(fmt(cfg.aprPercent))%")
                genesisFact(loc.t("genesis.rewardPerUnit"), "\(fmt(cfg.rewardPerUnit)) \(cfg.symbol)/unit")
                if let bonus = cfg.gatewayBonus, bonus > 1 {
                    genesisFact(loc.t("genesis.gatewayBonus"), "+\(Int((bonus - 1) * 100))%")
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard(padding: 14)
    }

    private func genesisURLRow(_ label: String, _ value: String, tint: Color) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label).font(.caption2).foregroundStyle(Brand.textSecondary)
            Text(value).font(.system(.caption, design: .monospaced)).foregroundStyle(tint)
                .textSelection(.enabled).lineLimit(2).truncationMode(.middle)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func genesisFact(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label).font(.caption).foregroundStyle(Brand.textSecondary)
            Spacer()
            Text(value).font(.system(.caption, design: .rounded).weight(.semibold)).foregroundStyle(Brand.textPrimary)
        }
    }
}

// Fold a hub-qualified expert-work node's claimable reward (`infer-<hubKey>-<nodeId>`)
// into its base phone node (`<nodeId>`), so the phone's earnings list on its own
// row rather than a mystery second one. Non-matching work-nodes stay untouched.
private func consolidateRewardNodes(_ nodes: [NodeReward]) -> [NodeReward] {
    let ids = Set(nodes.map { $0.nodeId })
    func baseId(_ id: String) -> String? {
        guard id.hasPrefix("infer-") else { return nil }
        let rest = id.dropFirst("infer-".count)
        guard let dash = rest.firstIndex(of: "-") else { return nil }
        return String(rest[rest.index(after: dash)...])
    }
    var merged: [String: NodeReward] = [:]
    var order: [String] = []
    for n in nodes where !(baseId(n.nodeId).map { ids.contains($0) } ?? false) {
        merged[n.nodeId] = n
        order.append(n.nodeId)
    }
    for n in nodes {
        guard let base = baseId(n.nodeId), var t = merged[base] else { continue }
        t.contributedUnits += n.contributedUnits
        t.pendingRewards += n.pendingRewards
        merged[base] = t
    }
    return order.compactMap { merged[$0] }
}
