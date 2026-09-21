import SwiftUI

@main
struct LinkcppWalletApp: App {
    @StateObject private var store = WalletStore()
    @Environment(\.scenePhase) private var scenePhase

    init() {
        // BGTaskScheduler requires its handler be registered before launch finishes.
        NodeBackgroundTask.register()
    }

    var body: some Scene {
        WindowGroup {
            RootView().environmentObject(store).environmentObject(Localizer.shared)
        }
        .onChange(of: scenePhase) { phase in
            if phase != .active {
                UIApplication.shared.isIdleTimerDisabled = false
                // If serving, take a short drain assertion so an in-flight request
                // finishes, and schedule the next background compute window.
                if StakingStore.nodeIsLive() { NodeBackgroundTask.beginShortAssertion() }
            } else {
                NodeBackgroundTask.endShortAssertion()
            }
        }
    }
}

/// Routes between onboarding, biometric unlock, and the home screen.
struct RootView: View {
    @EnvironmentObject var store: WalletStore
    @State private var unlocking = false

    var body: some View {
        Group {
            if store.address != nil {
                HomeView()
            } else if store.hasWallet {
                unlockGate
            } else {
                OnboardingView()
            }
        }
        .animation(.default, value: store.address)
    }

    private var unlockGate: some View {
        ZStack {
            BrandBackground()
            VStack(spacing: 16) {
                Image(systemName: "lock.shield.fill")
                    .font(.system(size: 52))
                    .foregroundStyle(Brand.gradient)
                Text("Kvasir Wallet")
                    .font(.system(.headline, design: .rounded))
                    .foregroundStyle(Brand.textPrimary)
                if unlocking { ProgressView().tint(Brand.pink) }
            }
        }
        .task {
            guard !unlocking else { return }
            unlocking = true
            // An iPhone with no passcode protects nothing the app could add a
            // gate in front of — whoever is holding it already has the screen.
            // So an absent device lock lets the app open. Reading the recovery
            // phrase is the opposite case and refuses; see ExportPhrase.
            if await Biometrics.unlock() != .refused {
                await store.restoreIfNeeded()
            }
            unlocking = false
        }
    }
}
