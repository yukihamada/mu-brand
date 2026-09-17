import XCTest
@testable import MU

@MainActor
final class ProductFeedTests: XCTestCase {
    private func product(_ sku: String) -> FeedProduct {
        FeedProduct(sku: sku, brand: "test", description: sku, priceJpy: 3000,
                    mockupUrl: nil, sold: 0, createdAt: "2026-09-17 00:00:00",
                    pdpUrl: "https://wearmu.com/shop/\(sku)",
                    checkoutUrl: "https://wearmu.com/api/shop/checkout?sku=\(sku)")
    }

    func testLateSearchCannotReplaceNewSearch() async {
        var oldResponse: CheckedContinuation<[FeedProduct], Error>?
        let started = expectation(description: "Old search in flight")
        let newest = product("new")
        let feed = ProductFeed { _, _, query in
            if query == "old" {
                return try await withCheckedThrowingContinuation {
                    oldResponse = $0
                    started.fulfill()
                }
            }
            return [newest]
        }
        let old = Task { await feed.reload(kind: .all, query: "old") }
        await fulfillment(of: [started], timeout: 2)
        await feed.reload(kind: .all, query: "new")
        oldResponse?.resume(returning: [product("old")])
        await old.value
        XCTAssertEqual(feed.products.map(\.sku), ["new"])
        XCTAssertFalse(feed.loading)
        XCTAssertNil(feed.error)
    }

    func testOldFailureCannotClearLoadingForNewRequest() async {
        var responses: [String: CheckedContinuation<[FeedProduct], Error>] = [:]
        let oldStarted = expectation(description: "Old request")
        let newStarted = expectation(description: "New request")
        let feed = ProductFeed { _, _, query in
            try await withCheckedThrowingContinuation {
                responses[query] = $0
                (query == "old" ? oldStarted : newStarted).fulfill()
            }
        }
        let old = Task { await feed.reload(kind: .all, query: "old") }
        await fulfillment(of: [oldStarted], timeout: 2)
        let new = Task { await feed.reload(kind: .all, query: "new") }
        await fulfillment(of: [newStarted], timeout: 2)
        responses["old"]?.resume(throwing: URLError(.notConnectedToInternet))
        await old.value
        XCTAssertTrue(feed.loading)
        XCTAssertNil(feed.error)
        responses["new"]?.resume(returning: [product("new")])
        await new.value
        XCTAssertFalse(feed.loading)
        XCTAssertEqual(feed.products.map(\.sku), ["new"])
    }

    func testFailedPageRetriesSamePageAndDeduplicatesSKU() async {
        let first = product("one"), second = product("two")
        var calls: [Int] = []
        var shouldFail = true
        let feed = ProductFeed { page, _, _ in
            calls.append(page)
            if page == 1 { return [first, first] }
            if shouldFail { shouldFail = false; throw URLError(.timedOut) }
            return page == 2 ? [first, second] : []
        }
        await feed.reload(kind: .all)
        await feed.loadMore()
        XCTAssertNotNil(feed.error)
        XCTAssertEqual(feed.products.map(\.sku), ["one"])
        await feed.loadMore()
        XCTAssertEqual(calls, [1, 2, 2])
        XCTAssertEqual(feed.products.map(\.sku), ["one", "two"])
        XCTAssertNil(feed.error)
        await feed.loadMore()
        XCTAssertTrue(feed.reachedEnd)
        await feed.loadMore()
        XCTAssertEqual(calls, [1, 2, 2, 3])
    }

    func testPaginationUsesSubmittedQueryNotTextBeingEdited() async {
        var calls: [String] = []
        let row = product("one")
        let feed = ProductFeed { page, kind, query in
            calls.append("\(page):\(kind.rawValue):\(query)")
            return page == 1 ? [row] : []
        }
        await feed.reload(kind: .all, query: "  dojo  ")
        await feed.loadMore()
        XCTAssertEqual(calls, ["1::dojo", "2::dojo"])
    }
}
