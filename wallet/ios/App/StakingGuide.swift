import SwiftUI

/// Static, illustrated guide explaining how staking and node rewards work.
struct StakingGuideView: View {
    @ObservedObject private var loc = Localizer.shared
    var body: some View {
        ZStack {
            BrandBackground()
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    header

                    section(icon: "info.circle.fill", title: loc.t("guide.whatTitle"), color: Brand.pink) {
                        Text(loc.t("guide.whatBody"))
                    }

                    section(icon: "lock.circle.fill", title: loc.t("guide.howTitle"), color: Brand.pink) {
                        VStack(alignment: .leading, spacing: 10) {
                            step(1, loc.t("guide.howStep1"))
                            step(2, loc.t("guide.howStep2"))
                            step(3, loc.t("guide.howStep3"))
                            step(4, loc.t("guide.howStep4"))
                            step(5, loc.t("guide.howStep5"))
                        }
                    }

                    section(icon: "server.rack", title: loc.t("staking.nodeOperatorRewards"), color: Brand.blue) {
                        VStack(alignment: .leading, spacing: 10) {
                            step(1, loc.t("guide.nodeStep1"))
                            step(2, loc.t("guide.nodeStep2"))
                            step(3, loc.t("guide.nodeStep3"))
                            step(4, loc.t("guide.nodeStep4"))
                        }
                    }

                    section(icon: "exclamationmark.triangle.fill", title: loc.t("guide.cautionTitle"), color: .orange) {
                        VStack(alignment: .leading, spacing: 8) {
                            bullet(loc.t("guide.caution1"))
                            bullet(loc.t("guide.caution2"))
                            bullet(loc.t("guide.caution3"))
                            bullet(loc.t("guide.caution4"))
                        }
                    }
                }
                .padding(20)
            }
        }
        .navigationTitle(loc.t("guide.navTitle"))
        .navigationBarTitleDisplayMode(.inline)
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(loc.t("guide.headerTitle"))
                .font(.system(.title2, design: .rounded).weight(.bold))
                .foregroundStyle(Brand.gradient)
            Text(loc.t("guide.headerSubtitle"))
                .font(.footnote).foregroundStyle(Brand.textSecondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func section<Content: View>(icon: String, title: String, color: Color, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Label(title, systemImage: icon)
                .font(.system(.headline, design: .rounded))
                .foregroundStyle(color)
            content()
                .font(.subheadline)
                .foregroundStyle(Brand.textPrimary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .brandCard()
    }

    private func step(_ n: Int, _ text: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Text("\(n)")
                .font(.caption.weight(.bold))
                .foregroundStyle(.white)
                .frame(width: 22, height: 22)
                .background(Brand.gradient, in: Circle())
            Text(.init(text)).foregroundStyle(Brand.textPrimary)
            Spacer(minLength: 0)
        }
    }

    private func bullet(_ text: String) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Text("•").foregroundStyle(Brand.pink)
            Text(.init(text)).foregroundStyle(Brand.textPrimary)
            Spacer(minLength: 0)
        }
    }
}
