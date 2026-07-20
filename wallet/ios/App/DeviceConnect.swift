import SwiftUI
import UIKit

/// Identity of the current iOS device when connected as a node.
enum DeviceInfo {
    static var nodeId: String {
        let idfv = UIDevice.current.identifierForVendor?.uuidString.prefix(8).lowercased() ?? "device"
        return "ios-\(idfv)"
    }
    @MainActor static var label: String {
        let n = UIDevice.current.name
        return n.isEmpty ? Localizer.shared.t("device.iosDevice") : n
    }
    static var kind: String { UIDevice.current.userInterfaceIdiom == .pad ? "tablet" : "phone" }
}

/// Shows the account (wallet) address and how to connect compute devices to it.
struct DeviceConnectView: View {
    @ObservedObject var staking: StakingStore
    @ObservedObject private var loc = Localizer.shared

    private var command: String {
        "LINKCPP_SERVICE=\(staking.serviceURL) LINKCPP_OWNER=\(staking.owner ?? loc.t("device.accountAddressPlaceholder")) node connect.js"
    }

    var body: some View {
        ZStack {
            BrandBackground()
            ScrollView {
                VStack(spacing: 16) {
                    accountCard
                    thisDeviceCard
                    otherDeviceCard
                    if let msg = staking.message {
                        Text(msg).font(.caption).foregroundStyle(.red)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(20)
            }
        }
        .navigationTitle(loc.t("device.navTitle"))
        .navigationBarTitleDisplayMode(.inline)
    }

    private var accountCard: some View {
        VStack(spacing: 12) {
            Label(loc.t("device.myAccount"), systemImage: "person.crop.circle.fill")
                .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.pink)
                .frame(maxWidth: .infinity, alignment: .leading)
            Text(loc.t("device.myAccountDesc"))
                .font(.footnote).foregroundStyle(Brand.textSecondary)
                .frame(maxWidth: .infinity, alignment: .leading)
            if let owner = staking.owner {
                if let qr = QR.image(from: owner) {
                    Image(uiImage: qr).interpolation(.none).resizable()
                        .frame(width: 176, height: 176)
                        .padding(14)
                        .background(.white, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                }
                Text(owner)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(Brand.textPrimary)
                    .multilineTextAlignment(.center).textSelection(.enabled)
                CopyButton(text: owner).tint(Brand.pink)
            }
        }
        .frame(maxWidth: .infinity)
        .brandCard()
    }

    private var thisDeviceCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label(loc.t("device.connectThis"), systemImage: "iphone")
                .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.blue)
            Text(loc.t("device.connectThisDesc", DeviceInfo.kind == "tablet" ? "iPad" : "iPhone"))
                .font(.footnote).foregroundStyle(Brand.textSecondary)
            Button { Task { await staking.connectThisDevice() } } label: {
                if staking.busy { ProgressView().tint(.white) } else { Text(loc.t("device.connectThis")) }
            }
            .buttonStyle(.brandPrimary)
            .disabled(staking.busy)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private var otherDeviceCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label(loc.t("device.connectOther"), systemImage: "desktopcomputer")
                .font(.system(.headline, design: .rounded)).foregroundStyle(Brand.blue)
            Text(loc.t("device.connectOtherDesc"))
                .font(.footnote).foregroundStyle(Brand.textSecondary)
            Text(command)
                .font(.system(.caption2, design: .monospaced))
                .foregroundStyle(Brand.textPrimary)
                .textSelection(.enabled)
                .padding(12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Brand.softGradient, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            CopyButton(text: command, labelKey: "device.copyCommand").tint(Brand.pink)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }
}
