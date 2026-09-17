#if DEBUG
import XCTest
@testable import MU

/// Session identity must be compared by generation, not by email: returning to the
/// same account (A -> B -> A) is a different session and in-flight work from the
/// first visit must not be accepted.
@MainActor
final class SessionIdentityTests: XCTestCase {

    /// The fixture helpers are gated on `MU_UI_FIXTURE=make`, so this runs the
    /// rotation through a Session that is opted in by construction.
    private func fixtureSession() -> Session {
        let session = Session()
        session.enableFixtureForUnitTest()
        return session
    }

    func testGenerationRotatesOnEveryIdentityChange() {
        let session = fixtureSession()
        let initial = session.identityGeneration

        session.logInForUITest(email: "a@example.invalid")
        let afterFirstA = session.identityGeneration
        XCTAssertNotEqual(afterFirstA, initial, "First login must rotate the generation")
        XCTAssertEqual(session.identity, "a@example.invalid")

        session.logInForUITest(email: "b@example.invalid")
        let afterB = session.identityGeneration
        XCTAssertNotEqual(afterB, afterFirstA, "Switching accounts must rotate the generation")

        // Returning to A is a new session, so the generation must differ from the
        // first visit even though the email is identical.
        session.logInForUITest(email: "a@example.invalid")
        XCTAssertEqual(session.identity, "a@example.invalid")
        XCTAssertNotEqual(session.identityGeneration, afterFirstA,
                          "A -> B -> A must not reuse the first visit's generation")

        session.logOutForUITest()
        XCTAssertNil(session.identity)
        XCTAssertNotEqual(session.identityGeneration, afterB, "Sign-out must rotate the generation")
    }

    func testFixtureAccountHelpersAreNoOpsWithoutTheFixture() {
        // Guards the memory-only helpers: without MU_UI_FIXTURE=make they must not
        // change the session, so a normal device login cannot be faked.
        XCTAssertFalse(MakeUITestFixture.enabled)
        let session = Session()
        let before = session.identityGeneration
        let wasLoggedIn = session.isLoggedIn

        session.logInForUITest(email: "attacker@example.invalid")
        session.logOutForUITest()

        XCTAssertEqual(session.identityGeneration, before, "Fixture helpers must be inert outside the fixture")
        XCTAssertEqual(session.isLoggedIn, wasLoggedIn)
    }
}

#endif
