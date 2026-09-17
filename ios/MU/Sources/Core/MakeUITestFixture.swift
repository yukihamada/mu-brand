#if DEBUG
import Foundation
import SwiftUI

/// Opt-in, offline UI regression fixture. Never compiled into distribution builds.
///
/// `canInit` intentionally intercepts every request: the point of this fixture is
/// full offline isolation, so nothing may reach the production network.
final class MakeUITestFixture: URLProtocol {
    static var enabled: Bool { ProcessInfo.processInfo.environment["MU_UI_FIXTURE"] == "make" }

    /// When set, the app starts signed out so a test can walk the real AuthGate
    /// flow (nil -> email) instead of beginning already logged in.
    static var startsSignedOut: Bool {
        ProcessInfo.processInfo.environment["MU_UI_FIXTURE_SIGNED_OUT"] == "1"
    }

    /// Seconds before a POST /api/make response is delivered. Long enough that a
    /// test can leave the tab while the creation is still in flight.
    static let makeDelay: TimeInterval = 2

    /// Seconds before a POST /api/app/agent/chat response is delivered.
    static let chatDelay: TimeInterval = 2

    private static let lock = NSLock()
    private static var counters: [String: Int] = [:]

    static func count(_ key: String) -> Int {
        lock.lock(); defer { lock.unlock() }
        return counters[key] ?? 0
    }

    @discardableResult
    private static func bump(_ key: String) -> Int {
        lock.lock(); defer { lock.unlock() }
        let next = (counters[key] ?? 0) + 1
        counters[key] = next
        return next
    }

    /// Read by `FixtureCounterProbe` so UI tests can assert real request counts.
    /// A creation that is never displayed still increments `make`, which is how a
    /// hidden automatic second POST is detected.
    static var counterLabel: String {
        "make=\(count("make")) delivered=\(count("make.delivered")) peek=\(count("peek")) chat=\(count("chat"))"
    }

    /// Offline account switching for tests. Memory only: nothing is written to the
    /// Keychain and no network login happens.
    @MainActor
    static func signInAs(_ email: String, session: Session) { session.logInForUITest(email: email) }

    @MainActor
    static func signOut(_ session: Session) { session.logOutForUITest() }

    override class func canInit(with request: URLRequest) -> Bool { enabled }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let path = request.url?.path ?? ""
        var body = "{}"
        var delay = 0.0
        switch path {
        case "/api/make/kinds":
            body = #"{"ok":true,"items":[{"kind":"tee","label_ja":"Tシャツ","label_en":"T-shirt","category":"wear","retail_jpy":3800,"can_make":true}]}"#
        case "/api/make":
            let n = Self.bump("make")
            let sku = "FIXTURE-\(n)"
            body = """
            {"ok":true,"sku":"\(sku)","kind":"tee","display":"\(sku)","hook":"\(sku)",
             "retail_jpy":3800,"design_url":"https://fixture.invalid/design.png",
             "pdp_url":"https://wearmu.com/shop/\(sku)","status":"live","auto_approved":true,
             "note":"Ready","edit_token":"fixture","maker_pct":10,"maker_earn_jpy":380}
            """
            delay = Self.makeDelay
        case "/api/app/agent/chat":
            _ = Self.bump("chat")
            // Deliberately slow, and it asks for a creation, so a test can switch
            // accounts mid-flight and assert the old action never runs.
            body = #"{"ok":true,"reply":"Making it","action":"make","args":{"prompt":"late design","kind":"tee","royalty":10}}"#
            delay = Self.chatDelay
        case "/api/make/peek":
            _ = Self.bump("peek")
            // applyPeek marks a preview complete only when ready == true, the kind is
            // a product preview (.mockup/.printful/.lifestyle) and a mockup URL is
            // present. preview_revision is omitted: it is only compared after polish.
            body = #"{"ok":true,"status":"live","ready":true,"mockup":"https://fixture.invalid/mockup.png","preview_kind":"mockup"}"#
        case "/api/shop/feed.json":
            body = #"{"page":1,"page_size":60,"products":[]}"#
        default: break
        }
        let data = Data(body.utf8)
        let isMake = path == "/api/make"
        let work = DispatchWorkItem { [weak self] in
            guard let self, let url = self.request.url else { return }
            let response = HTTPURLResponse(url: url, statusCode: 200, httpVersion: nil,
                                           headerFields: ["Content-Type": "application/json"])!
            self.client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            self.client?.urlProtocol(self, didLoad: data)
            self.client?.urlProtocolDidFinishLoading(self)
            if isMake { _ = Self.bump("make.delivered") }
        }
        pending = work
        DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: work)
    }

    override func stopLoading() { pending?.cancel() }

    private var pending: DispatchWorkItem?
}

/// DEBUG-only, fixture-gated probe that publishes fixture counters to UI tests.
/// Renders nothing unless `MU_UI_FIXTURE=make`, and never compiles into Release.
struct FixtureCounterProbe: View {
    var body: some View {
        TimelineView(.periodic(from: .now, by: 0.2)) { _ in
            let label = MakeUITestFixture.counterLabel
            Text(verbatim: label)
                .font(.caption2.monospacedDigit())
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("fixture.counters")
                .accessibilityValue(label)
        }
        .allowsHitTesting(false)
    }
}

/// DEBUG-only, fixture-gated account switcher. Buttons are used instead of a shared
/// file because the UI test runner and the app run in separate sandboxes.
/// Memory only: no Keychain write and no network login.
struct FixtureAccountSwitcher: View {
    @EnvironmentObject private var session: Session

    var body: some View {
        HStack(spacing: 6) {
            Text(verbatim: session.identity.map { "user=\($0)" } ?? "user=<none>")
                .font(.caption2.monospacedDigit())
                .accessibilityIdentifier("fixture.account")
                .accessibilityValue(session.identity.map { "user=\($0)" } ?? "user=<none>")
            Button("out") { MakeUITestFixture.signOut(session) }
                .accessibilityIdentifier("fixture.signOut")
            Button("switch") { MakeUITestFixture.signInAs("second@example.invalid", session: session) }
                .accessibilityIdentifier("fixture.switchAccount")
            // First login (nil -> email) from any tab, so a test can authenticate
            // without going through Make's AuthGate sheet.
            Button("first") { MakeUITestFixture.signInAs("first@example.invalid", session: session) }
                .accessibilityIdentifier("fixture.loginFirst")
        }
        .font(.caption2)
        .allowsHitTesting(true)
    }
}
#endif
