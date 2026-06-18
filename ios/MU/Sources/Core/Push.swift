import UIKit
import UserNotifications

// APNs 登録。トークンが取れたらサーバ (/api/app/push/register) に渡す。
// 実際の通知送信は APNs 認証キー (Apple Developer・人間ゲート) が要るが、
// 宛先の収集とパーミッション導線はここで完結する。
final class AppDelegate: NSObject, UIApplicationDelegate, UNUserNotificationCenterDelegate {
    func application(_ application: UIApplication,
                     didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        UNUserNotificationCenter.current().delegate = self
        // 既に許可済みなら毎起動で最新トークンを取り直す (トークンは更新されうる)。
        UNUserNotificationCenter.current().getNotificationSettings { settings in
            if settings.authorizationStatus == .authorized {
                DispatchQueue.main.async { application.registerForRemoteNotifications() }
            }
        }
        return true
    }

    func application(_ application: UIApplication,
                     didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data) {
        let token = deviceToken.map { String(format: "%02x", $0) }.joined()
        Task { await MUAPI.registerPush(token: token, apiKey: Session.currentAPIKey()) }
    }

    func application(_ application: UIApplication,
                     didFailToRegisterForRemoteNotificationsWithError error: Error) {
        // 端末がプッシュ非対応 (シミュレータ等)。黙って無視。
    }

    // フォアグラウンドでも通知を出す。
    func userNotificationCenter(_ center: UNUserNotificationCenter,
                                willPresent notification: UNNotification) async
        -> UNNotificationPresentationOptions { [.banner, .sound, .badge] }

    // 通知タップ → ペイロードに応じてアプリ内へ遷移。AppDelegate は SwiftUI の
    // AppState を直接持てないので NotificationCenter 経由で AppState に橋渡しする。
    // ペイロード例: {"aps":{...}, "tab":"shop", "id":"drop-2026..."}。
    func userNotificationCenter(_ center: UNUserNotificationCenter,
                                didReceive response: UNNotificationResponse) async {
        let info = response.notification.request.content.userInfo
        Analytics.track("push_open", ["id": (info["id"] as? String) ?? ""])
        NotificationCenter.default.post(name: .muPushOpen, object: nil, userInfo: info)
    }
}

extension Notification.Name {
    // 通知タップを SwiftUI 層へ渡すためのアプリ内イベント。
    static let muPushOpen = Notification.Name("mu.push.open")
}

@MainActor
enum PushManager {
    // 現在の許可状態を返す。
    static func status() async -> UNAuthorizationStatus {
        await UNUserNotificationCenter.current().notificationSettings().authorizationStatus
    }

    // 許可をリクエストして APNs 登録まで進める。許可済みなら即登録のみ。
    static func enable() async -> Bool {
        let center = UNUserNotificationCenter.current()
        let current = await center.notificationSettings().authorizationStatus
        switch current {
        case .denied:
            return false // 設定アプリでしか変えられない
        case .authorized, .provisional, .ephemeral:
            UIApplication.shared.registerForRemoteNotifications()
            return true
        default:
            let granted = (try? await center.requestAuthorization(options: [.alert, .badge, .sound])) ?? false
            if granted { UIApplication.shared.registerForRemoteNotifications() }
            return granted
        }
    }
}

// 軽量 funnel 計測。fire-and-forget。失敗はユーザー体験に出さない。
enum Analytics {
    static func track(_ event: String, _ props: [String: Any] = [:]) {
        Task { await MUAPI.track(event, props: props, apiKey: Session.currentAPIKey()) }
    }
}
