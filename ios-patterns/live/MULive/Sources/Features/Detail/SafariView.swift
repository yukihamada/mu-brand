import SafariServices
import SwiftUI

// Stripe Checkout / 商品ページをアプリ内 Safari で開く (Apple Pay 対応・既存 MU と同じ)。
struct SafariView: UIViewControllerRepresentable {
    let url: URL

    func makeUIViewController(context: Context) -> SFSafariViewController {
        let vc = SFSafariViewController(url: url)
        vc.dismissButtonStyle = .close
        vc.preferredControlTintColor = UIColor(Color.muGold)
        return vc
    }

    func updateUIViewController(_ vc: SFSafariViewController, context: Context) {}
}
