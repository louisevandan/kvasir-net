import Foundation
import BackgroundTasks
import UIKit

/// Keeps the node contributing when the app is backgrounded or the screen locks.
///
/// iOS does not allow an always-on inbound server in the background, so this is
/// best-effort, not on-demand: while charging, iOS grants BGProcessingTask
/// windows (minutes each) in which the RPC worker + agent control server stay
/// alive and reachable. We also take a short UIApplication background assertion
/// on the way out so an in-flight request can finish before suspension.
///
/// v1 reality: a hub can reliably reach the phone while it is foregrounded
/// (screen on) or during a granted background window while charging. Continuous
/// on-demand availability is not possible under iOS app sandboxing.
@MainActor
enum NodeBackgroundTask {
    static let identifier = "ai.banya.linkcpp.wallet.node"
    private static var bgAssertion: UIBackgroundTaskIdentifier = .invalid

    /// Register the launch handler. Call once at app launch, before the app
    /// finishes launching (BGTaskScheduler requires registration up front).
    static func register() {
        BGTaskScheduler.shared.register(forTaskWithIdentifier: identifier, using: nil) { task in
            guard let task = task as? BGProcessingTask else { task.setTaskCompleted(success: false); return }
            Task { @MainActor in handle(task) }
        }
    }

    /// Ask iOS for a future background compute window. Only meaningful while the
    /// node is live; a no-op scheduling error (e.g. simulator) is ignored.
    static func schedule() {
        let request = BGProcessingTaskRequest(identifier: identifier)
        request.requiresExternalPower = true          // contribute only while charging
        request.requiresNetworkConnectivity = true    // the ring/RPC needs the LAN
        request.earliestBeginDate = nil
        do { try BGTaskScheduler.shared.submit(request) }
        catch { /* not fatal: foreground serving still works */ }
    }

    static func cancel() {
        BGTaskScheduler.shared.cancel(taskRequestWithIdentifier: identifier)
    }

    private static func handle(_ task: BGProcessingTask) {
        // Chain the next window so contribution continues across grants.
        schedule()
        let live = StakingStore.nodeIsLive()
        if live { AgentControlServer.shared.resumeForBackground() }

        // Hold the window until iOS reclaims it; end cleanly on expiration.
        task.expirationHandler = {
            Task { @MainActor in task.setTaskCompleted(success: true) }
        }
        // Keep the task open while serving; poll the deadline the system enforces.
        Task { @MainActor in
            while StakingStore.nodeIsLive() {
                try? await Task.sleep(for: .seconds(5))
            }
            task.setTaskCompleted(success: true)
        }
    }

    /// Take a short assertion so an in-flight inference can finish as the app
    /// backgrounds, then schedule the next processing window.
    static func beginShortAssertion() {
        endShortAssertion()
        bgAssertion = UIApplication.shared.beginBackgroundTask(withName: "kvasir-node-drain") {
            endShortAssertion()
        }
        schedule()
    }

    static func endShortAssertion() {
        if bgAssertion != .invalid {
            UIApplication.shared.endBackgroundTask(bgAssertion)
            bgAssertion = .invalid
        }
    }
}
