import XCTest

/// A walk through the screens a node operator actually uses, on a real device,
/// against the live gateway.
///
/// This exists because a physical iPhone cannot be driven or photographed from
/// a shell — there is no `devicectl` screenshot and no equivalent of
/// `adb shell input` — and because the defects worth catching here are the ones
/// a build cannot see. The Android pass found two: a balance that wrapped
/// mid-number so the screen showed a lone digit on its own line, and a retired
/// host still listed as a connected bridge. Both compiled, both passed the unit
/// tests, and both were obvious the moment a screen showed them.
///
/// Every step attaches a screenshot, so a failure comes with the picture of it.
/// The assertions are deliberately about what must be on screen rather than
/// exact copy, so that translating a string does not fail the test.
final class WalletWalkthroughUITests: XCTestCase {

    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        // Debug-only, and only because XCUITest cannot present a face. See
        // Biometrics.uiTestBypassArgument.
        app.launchArguments = ["-kvasir-ui-test-unlocked"]
        app.launch()
    }

    private func shoot(_ name: String) {
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }

    /// Wait for any element matching `predicate` to exist, and say what we were
    /// waiting for when it does not — an unlabelled timeout tells you nothing.
    @discardableResult
    private func awaitElement(
        _ query: XCUIElementQuery, _ label: String, timeout: TimeInterval = 30
    ) -> XCUIElement {
        let element = query.firstMatch
        XCTAssertTrue(
            element.waitForExistence(timeout: timeout),
            "\(label) never appeared within \(Int(timeout))s"
        )
        return element
    }

    /// The wallet is behind Face ID and XCUITest cannot satisfy biometrics, so
    /// a locked app is a reason to skip rather than a failure — reporting it as
    /// a broken balance would be a lie about what was observed.
    private func skipIfLocked() throws {
        let locked = app.staticTexts.containing(
            NSPredicate(format: "label CONTAINS[c] %@", "Kvasir Wallet")
        ).firstMatch
        if locked.waitForExistence(timeout: 3), !app.staticTexts["KVR"].exists {
            shoot("00-locked")
            throw XCTSkip("the wallet is locked; unlock it with Face ID on the device and run again")
        }
    }

    func testWalletHomeShowsABalanceOnOneLine() throws {
        try skipIfLocked()
        shoot("01-home")

        // The balance is the largest static text on the home screen. What
        // matters is not its value but that it is one line: the Android build
        // rendered "106043.08377" with a lone "9" underneath it, which is how a
        // six-figure balance looks when the label is allowed to wrap.
        let texts = app.staticTexts.allElementsBoundByIndex.filter { $0.exists && !$0.label.isEmpty }
        XCTAssertFalse(texts.isEmpty, "the home screen rendered no text at all")

        let numeric = texts.filter { element in
            let stripped = element.label.replacingOccurrences(of: ",", with: "")
                .replacingOccurrences(of: ".", with: "")
            return !stripped.isEmpty && stripped.allSatisfy(\.isNumber)
        }
        XCTAssertFalse(numeric.isEmpty, "no numeric balance was rendered on the home screen")

        // A wrapped number shows up as a label whose frame is taller than a
        // single line of its own type size. Two lines of a 46pt face clear 60pt;
        // one line does not.
        for element in numeric where element.frame.height > 60 {
            XCTFail("a numeric label is \(Int(element.frame.height))pt tall, so it wrapped: \(element.label)")
        }
    }

    func testNodeMonitorListsNoRetiredBridge() throws {
        // Staking → the node monitor, which is where the stored bridge list is
        // shown. hub.kvasir-ai.net was the control plane before it was retired;
        // it has answered 502 ever since and must not be presented as a bridge.
        tapFirstButton(containing: ["스테이킹", "Staking"], label: "the staking entry")
        shoot("02-staking")

        tapFirstButton(containing: ["노드 운영", "Node status", "Node operation"], label: "the node monitor link")
        shoot("03-monitor")

        for element in app.staticTexts.allElementsBoundByIndex where element.exists {
            XCTAssertFalse(
                element.label.contains("hub.kvasir-ai.net"),
                "the monitor still lists the retired control plane as a bridge"
            )
        }
    }

    func testDeviceConnectShowsAWorkingCommand() throws {
        try skipIfLocked()
        tapFirstButton(containing: ["스테이킹", "Staking"], label: "the staking entry")
        tapFirstButton(containing: ["노드 운영", "Node status", "Node operation"], label: "the node monitor link")
        tapFirstButton(containing: ["기기 연결", "Connect device"], label: "the device-connect link")
        shoot("04-device-connect")

        // The command is copied off this screen and pasted into a terminal, so
        // it has to be the one connect.js actually reads. It used to name
        // LINKCPP_SERVICE and a bare `connect.js`, and anyone who followed it
        // got a usage line and exit 1.
        let command = app.staticTexts.allElementsBoundByIndex
            .map(\.label)
            .first { $0.contains("connect.js") }
        XCTAssertNotNil(command, "the device-connect screen shows no connect.js command")
        guard let command else { return }
        XCTAssertTrue(command.contains("KVR_SERVICE"), "command does not set KVR_SERVICE: \(command)")
        XCTAssertTrue(command.contains("KVR_OWNER"), "command does not set KVR_OWNER: \(command)")
        XCTAssertTrue(
            command.contains("solana/node-client/connect.js"),
            "command does not give the path connect.js lives at: \(command)"
        )
        XCTAssertFalse(command.contains("LINKCPP"), "command still uses the retired variable names: \(command)")
    }

    /// The whole money path, from this phone: the catalogue comes from the
    /// gateway, the prompt crosses gateway → bridge → the ring, and the reply
    /// comes back with the tokens it actually cost. Nothing below this line is
    /// mocked, so a failure here means the production path is down rather than
    /// that the app is wrong — read it together with the gateway's own health.
    func testInferenceRoundTripAndBilling() throws {
        try skipIfLocked()
        tapFirstButton(containing: ["AI 추론", "AI inference"], label: "the AI inference entry")
        shoot("05-inference")

        // The model dropdown is populated from the gateway, so its presence is
        // already evidence the catalogue call succeeded.
        let model = app.staticTexts.matching(
            NSPredicate(format: "label CONTAINS[c] %@", "Step-3.7-Flash")
        ).firstMatch
        XCTAssertTrue(
            model.waitForExistence(timeout: 30),
            "the gateway catalogue never reached the composer"
        )

        // The composer is `TextField(..., axis: .vertical)`, which SwiftUI backs
        // with a text view rather than a text field, so asking only for
        // `textFields` types into nothing and leaves the send button disabled.
        let field = [app.textViews.firstMatch, app.textFields.firstMatch]
            .first { $0.waitForExistence(timeout: 10) }
        XCTAssertNotNil(field, "the composer has neither a text view nor a text field")
        guard let field else { return }
        field.tap()
        field.typeText("Say hello in one short sentence.")
        shoot("05b-typed")

        let send = app.buttons.matching(
            NSPredicate(format: "label CONTAINS[c] %@ OR label CONTAINS[c] %@", "전송", "Send")
        ).firstMatch
        XCTAssertTrue(send.waitForExistence(timeout: 5), "the composer has no send button")
        send.tap()

        // A 428B model on a two-stage ring answers a short prompt in seconds,
        // but the request also crosses a paid gateway and a settlement write,
        // so give it room before calling it a failure.
        let billed = app.staticTexts.matching(
            NSPredicate(format: "label CONTAINS[c] %@ OR label CONTAINS[c] %@",
                        "실제 사용 토큰", "Actual tokens used")
        ).firstMatch
        XCTAssertTrue(
            billed.waitForExistence(timeout: 180),
            "no reply was billed within 180s — the gateway, the bridge or the ring did not answer"
        )
        shoot("06-inference-reply")

        let tokens = billed.label
            .components(separatedBy: CharacterSet.decimalDigits.inverted)
            .compactMap(Int.init)
            .first
        XCTAssertNotNil(tokens, "the usage line carries no token count: \(billed.label)")
        XCTAssertGreaterThan(tokens ?? 0, 0, "the reply was billed zero tokens: \(billed.label)")
    }

    // MARK: - navigation

    /// Tap the first tappable element whose label contains any of `needles`.
    /// The app labels its rows in the active language, so each caller passes the
    /// Korean and English forms rather than the test pinning one locale.
    private func tapFirstButton(containing needles: [String], label: String) {
        // Query by element type rather than walking every descendant: asking
        // for `descendants(matching: .any)` makes XCTest snapshot the whole
        // tree, which on this screen fails outright with "No matches found for
        // Descendants matching type Alert".
        let predicate = NSPredicate(
            format: needles.map { _ in "label CONTAINS[c] %@" }.joined(separator: " OR "),
            argumentArray: needles
        )
        let queries = { [self] in [app.buttons, app.staticTexts, app.cells, app.otherElements] }

        // A row far down a NavigationStack's scroll view exists in the tree long
        // before it is hittable, so "exists" is the signal to scroll towards and
        // "isHittable" the signal to tap. Scroll the scroll view rather than the
        // application: swiping the app can land on the wrong container and
        // leave the row where it was.
        let scroller = app.scrollViews.firstMatch
        let deadline = Date().addingTimeInterval(30)
        while Date() < deadline {
            for query in queries() {
                let match = query.matching(predicate).firstMatch
                guard match.exists else { continue }
                if match.isHittable {
                    match.tap()
                    return
                }
                // A row resting on the bottom edge is drawn but not hittable.
                // Nudge it up rather than scrolling past it.
                if scroller.exists { scroller.swipeUp() } else { app.swipeUp() }
                if match.exists && match.isHittable {
                    match.tap()
                    return
                }
            }
            if scroller.exists { scroller.swipeUp() } else { app.swipeUp() }
        }

        // Last resort: the element is in the tree but XCTest will not call it
        // hittable — a composed SwiftUI NavigationLink label does this. Tapping
        // its centre works where tap() refuses.
        for query in queries() {
            let match = query.matching(predicate).firstMatch
            if match.exists {
                match.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
                return
            }
        }
        shoot("nav-failed-\(label.replacingOccurrences(of: " ", with: "-"))")
        XCTFail("\(label) was not on screen, and scrolling did not reveal it")
    }
}
