import Foundation

// 「自分が作ったもの」の端末ローカル台帳。/make は匿名でも作れる設計なので、
// この端末で作ったものは端末が一次記録 (edit_url = 編集権の鍵も含む)。
// 保存先は Application Support の JSON。秘匿性は edit_token のみ・PII なし。
@MainActor
final class MakeHistory: ObservableObject {
    @Published private(set) var creations: [LocalCreation] = []

    private let fileURL: URL = {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("creations.json")
    }()

    init() {
        load()
    }

    func add(_ creation: LocalCreation) {
        creations.removeAll { $0.sku == creation.sku }
        creations.insert(creation, at: 0)
        save()
    }

    func updateMockup(sku: String, mockupUrl: String) {
        guard let i = creations.firstIndex(where: { $0.sku == sku }) else { return }
        creations[i].mockupUrl = mockupUrl
        save()
    }

    func remove(sku: String) {
        creations.removeAll { $0.sku == sku }
        save()
    }

    private func load() {
        guard let data = try? Data(contentsOf: fileURL),
              let list = try? Self.decoder.decode([LocalCreation].self, from: data) else { return }
        creations = list
    }

    private func save() {
        guard let data = try? Self.encoder.encode(creations) else { return }
        try? data.write(to: fileURL, options: .atomic)
    }

    private static let encoder: JSONEncoder = {
        let e = JSONEncoder()
        e.dateEncodingStrategy = .iso8601
        return e
    }()

    private static let decoder: JSONDecoder = {
        let d = JSONDecoder()
        d.dateDecodingStrategy = .iso8601
        return d
    }()
}
