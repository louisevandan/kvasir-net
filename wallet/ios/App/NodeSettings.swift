import SwiftUI
import Core

/// Mobile-phone node settings: pick a compute backend (MLX GPU / CPU) and mode,
/// see the estimated resource impact, and run the device live with real-time gauges.
struct NodeSettingsView: View {
    @ObservedObject var staking: StakingStore
    @ObservedObject private var loc = Localizer.shared
    @ObservedObject private var agent = AgentControlServer.shared
    @ObservedObject private var participation = HubParticipation.shared
    @State private var stats = DeviceStats()
    @State private var hubURL = "https://hub.kvasir-ai.net"
    @State private var hubStatus = ""
    @State private var hubBusy = false
    @State private var keyStatus = ""
    @State private var keyBusy = false

    private var profile: NodeProfile { nodeProfile(backend: staking.nodeBackend, mode: staking.nodeMode) }

    var body: some View {
        ZStack {
            BrandBackground()
            ScrollView {
                VStack(spacing: 14) {
                    backendCard
                    modeCard
                    infographicCard
                    policyCard
                    hubCard
                    apiKeyCard
                    if staking.nodeLive { liveCard }
                }
                .padding(20)
            }
        }
        .navigationTitle(loc.t("home.nodeSettingsTitle"))
        .navigationBarTitleDisplayMode(.inline)
        .task(id: staking.nodeLive) { await runLiveLoop() }
    }

    // MARK: backend

    private var backendCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(loc.t("nodeSettings.backendTitle"))
                .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.textPrimary)
            Text(loc.t("nodeSettings.backendDesc"))
                .font(.caption).foregroundStyle(Brand.textSecondary)
            HStack(spacing: 8) {
                ForEach(NodeBackend.all, id: \.id) { b in
                    chip(b.label, selected: staking.nodeBackend == b.id) { staking.nodeBackend = b.id }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    // MARK: connect to a remote hub (SIWS node token -> outbound shard serving)

    // The credit API key is stored on this device and the gateway keeps only its
    // hash, so a key it no longer recognises has to be reminted rather than
    // repaired. This is the way out of "invalid API key" from inside the app.
    private var apiKeyCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(loc.t("apiKey.title"))
                .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.textPrimary)
            Text(loc.t("apiKey.desc"))
                .font(.caption).foregroundStyle(Brand.textSecondary)
            Button {
                guard !keyBusy else { return }
                keyBusy = true
                keyStatus = loc.t("apiKey.working")
                Task {
                    let r = await staking.reissueCreditApiKey()
                    keyStatus = r; keyBusy = false
                }
            } label: {
                Text(keyBusy ? loc.t("apiKey.working") : loc.t("apiKey.reissue"))
                    .fontWeight(.semibold).foregroundStyle(.white)
                    .frame(maxWidth: .infinity).padding(.vertical, 12)
                    .background(Brand.pink.opacity(keyBusy ? 0.4 : 1),
                                in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            }
            .disabled(keyBusy)
            if !keyStatus.isEmpty {
                Text(keyStatus).font(.caption2)
                    .foregroundStyle(keyStatus == loc.t("apiKey.ok") ? Color.green : Brand.textSecondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private var hubCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(loc.t("nodeSettings.hubConnectTitle"))
                .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.textPrimary)
            Text(loc.t("nodeSettings.hubConnectDesc"))
                .font(.caption).foregroundStyle(Brand.textSecondary)
            TextField(loc.t("nodeSettings.hubUrl"), text: $hubURL)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled(true)
                .keyboardType(.URL)
                .font(.callout)
                .padding(12)
                .background(Brand.stroke, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                .foregroundStyle(Brand.textPrimary)
            Button {
                guard !hubBusy else { return }
                hubBusy = true
                hubStatus = loc.t("nodeSettings.hubSigning")
                Task {
                    let r = await staking.connectHub(url: hubURL)
                    hubStatus = r; hubBusy = false
                }
            } label: {
                Text(hubBusy ? loc.t("nodeSettings.hubConnecting") : loc.t("nodeSettings.hubConnectBtn"))
                    .fontWeight(.semibold).foregroundStyle(.white)
                    .frame(maxWidth: .infinity).padding(.vertical, 12)
                    .background(Brand.pink.opacity(hubBusy ? 0.4 : 1),
                                in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            }
            .disabled(hubBusy)
            if !hubStatus.isEmpty {
                Text(hubStatus).font(.caption2.monospaced())
                    .foregroundStyle(hubStatus.hasPrefix(loc.t("nodeSettings.hubConnected")) ? Color.green : Brand.textSecondary)
            }
            ForEach(participation.hubs, id: \.self) { h in
                Text("• \(h)").font(.caption2).foregroundStyle(Brand.textSecondary)
            }
            if !participation.lastStatus.isEmpty {
                Text(participation.lastStatus).font(.caption2).foregroundStyle(Brand.textSecondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    // MARK: mode

    private var modeCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(loc.t("nodeSettings.modeTitle"))
                .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.textPrimary)
            modeRow(loc.t("nodeSettings.modeLocalTitle"), loc.t("nodeSettings.modeLocalDesc"), id: "local_shard")
            modeRow(loc.t("mode.rpcWorker"), loc.t("nodeSettings.modeRpcDesc"), id: "rpc_worker")
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private func modeRow(_ title: String, _ desc: String, id: String) -> some View {
        let selected = staking.nodeMode == id
        return Button { staking.nodeMode = id } label: {
            HStack(spacing: 12) {
                ZStack {
                    Circle().fill(selected ? Brand.pink : Brand.stroke).frame(width: 22, height: 22)
                    if selected { Image(systemName: "checkmark").font(.caption2.weight(.bold)).foregroundStyle(.white) }
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.system(.subheadline, design: .rounded).weight(.semibold)).foregroundStyle(Brand.textPrimary)
                    Text(desc).font(.caption2).foregroundStyle(Brand.textSecondary)
                }
                Spacer()
            }
            .padding(12)
            .background(selected ? Brand.pink.opacity(0.10) : .clear, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        }
        .buttonStyle(.plain)
    }

    // MARK: infographic

    private var infographicCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top) {
                Text(loc.t("nodeSettings.resourceTitle"))
                    .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.textPrimary)
                Spacer()
                VStack(alignment: .trailing, spacing: 0) {
                    Text("\(profile.tokPerSec)")
                        .font(.system(size: 24, weight: .heavy, design: .rounded))
                        .foregroundStyle(Brand.gradient)
                    Text(loc.t("nodeSettings.tokPerSecCaption")).font(.system(size: 10)).foregroundStyle(Brand.textSecondary)
                }
            }
            Text(profile.computeUnit).font(.system(.footnote, design: .rounded).weight(.semibold)).foregroundStyle(Brand.blue)
            metricBar(loc.t("nodeSettings.memImpact"), profile.memImpact, "\(Int(profile.memImpact * 100))%", Brand.blue)
            metricBar(loc.t("nodeSettings.thermal"), profile.thermal, thermalWord(profile.thermal), .orange)
            metricBar(loc.t("nodeSettings.performance"), profile.performance, "\(Int(profile.performance * 100))%", .green)
            Text("※ \(profile.note)").font(.caption2).foregroundStyle(Brand.textSecondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private func metricBar(_ label: String, _ value: Float, _ valueText: String, _ color: Color) -> some View {
        VStack(spacing: 4) {
            HStack {
                Text(label).font(.caption).foregroundStyle(Brand.textSecondary)
                Spacer()
                Text(valueText).font(.caption.weight(.semibold)).foregroundStyle(Brand.textPrimary)
            }
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    Capsule().fill(Brand.stroke).frame(height: 8)
                    Capsule().fill(color).frame(width: geo.size.width * CGFloat(min(max(value, 0), 1)), height: 8)
                }
            }
            .frame(height: 8)
        }
        .padding(.vertical, 3)
    }

    // MARK: policy + live

    private var policyCard: some View {
        VStack(spacing: 10) {
            Toggle(loc.t("nodeSettings.chargingOnly"), isOn: $staking.nodeChargingOnly)
                .tint(Brand.pink).foregroundStyle(Brand.textPrimary)
                .font(.system(.subheadline, design: .rounded))
            Toggle(loc.t("nodeSettings.runLive"), isOn: $staking.nodeLive)
                .tint(Brand.pink).foregroundStyle(Brand.textPrimary)
                .font(.system(.subheadline, design: .rounded))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private var liveCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Circle().fill(.green).frame(width: 9, height: 9)
                Text(loc.t("nodeSettings.liveGauges")).font(.system(.headline, design: .rounded)).foregroundStyle(Brand.textPrimary)
                Spacer()
                Text(stats.charging ? loc.t("nodeSettings.charging") : loc.t("nodeSettings.battery")).font(.caption).foregroundStyle(Brand.textSecondary)
            }
            if staking.nodeMode == "rpc_worker" {
                VStack(alignment: .leading, spacing: 3) {
                    if let paused = staking.nodePausedReason {
                        Label(paused, systemImage: "pause.circle.fill")
                            .font(.caption).foregroundStyle(.orange)
                    } else {
                        Text("RPC worker \(agent.running ? "listening" : "starting") · agent :\(agent.agentPort) · rpc :\(agent.rpcPort)")
                            .font(.system(.caption, design: .monospaced)).foregroundStyle(agent.running ? .green : Brand.textSecondary)
                    }
                    if let c = agent.boundController {
                        Text("bound to \(c)").font(.caption2).foregroundStyle(Brand.textSecondary)
                    }
                    if !agent.lastEvent.isEmpty && staking.nodePausedReason == nil {
                        Text(agent.lastEvent).font(.caption2).foregroundStyle(Brand.textSecondary).lineLimit(2)
                    }
                }
                Label(loc.t("nodeSettings.screenOnHint"), systemImage: "bolt.fill")
                    .font(.caption2).foregroundStyle(Brand.textSecondary)
            }
            gauge("RAM", ramFrac, String(format: "%.1f / %.1f GB", Double(stats.ramUsedMb) / 1024, Double(stats.ramTotalMb) / 1024), Brand.blue, live: true)
            gauge(loc.t("nodeSettings.cpuLoad"), stats.cpuLoad, "\(Int(stats.cpuLoad * 100))%", .green, live: true)
            gauge(loc.t("nodeSettings.thermalState"), stats.thermal, loc.t(stats.thermalWord), .orange, live: true)
            gauge("GPU (\(staking.nodeBackend.uppercased()))", profile.performance, loc.t("nodeSettings.estimated"), Brand.pink, live: false)
            Text(loc.t("nodeSettings.gaugeNote")).font(.system(size: 10)).foregroundStyle(Brand.textSecondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private var ramFrac: Float {
        stats.ramTotalMb > 0 ? Float(stats.ramUsedMb) / Float(stats.ramTotalMb) : 0
    }

    private func gauge(_ label: String, _ value: Float, _ valueText: String, _ color: Color, live: Bool) -> some View {
        VStack(spacing: 5) {
            HStack(spacing: 6) {
                Text(label).font(.system(.subheadline, design: .rounded).weight(.medium)).foregroundStyle(Brand.textPrimary)
                if !live {
                    Text(loc.t("nodeSettings.estimated")).font(.system(size: 9)).foregroundStyle(Brand.textSecondary)
                        .padding(.horizontal, 4).padding(.vertical, 1)
                        .background(Brand.stroke, in: RoundedRectangle(cornerRadius: 4))
                }
                Spacer()
                Text(valueText).font(.system(.subheadline, design: .monospaced).weight(.bold)).foregroundStyle(color)
            }
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    Capsule().fill(Brand.stroke).frame(height: 10)
                    Capsule().fill(color).frame(width: geo.size.width * CGFloat(min(max(value, 0), 1)), height: 10)
                }
            }
            .frame(height: 10)
        }
        .padding(.vertical, 6)
    }

    // MARK: helpers

    private func chip(_ label: String, selected: Bool, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Text(label)
                .font(.caption.weight(.semibold))
                .foregroundStyle(selected ? .white : Brand.pink)
                .padding(.horizontal, 12).padding(.vertical, 8)
                .background(selected ? Brand.pink : Brand.pink.opacity(0.12), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        }
        .buttonStyle(.plain)
    }

    private func thermalWord(_ v: Float) -> String {
        loc.t(v >= 0.75 ? "thermal.high" : (v >= 0.5 ? "thermal.medium" : "thermal.low"))
    }

    /// While live, register with the current profile then heartbeat + refresh gauges.
    /// The rpc_worker data plane is NOT tied to this view — StakingStore owns the
    /// worker's lifecycle so it survives leaving the screen.
    private func runLiveLoop() async {
        guard staking.nodeLive else { return }
        // Registration, heartbeat and charge-driven pause/resume are owned app-wide
        // by StakingStore (so the node stays online off this screen and across
        // background/foreground); this view loop only refreshes the on-screen gauges.
        while staking.nodeLive && !Task.isCancelled {
            stats = NodeTelemetry.shared.read()
            try? await Task.sleep(for: .seconds(1.5))
        }
    }
}
