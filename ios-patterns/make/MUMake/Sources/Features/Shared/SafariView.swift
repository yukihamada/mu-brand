import SafariServices
import SwiftUI

// Stripe Checkout / 商品ページ / 編集リンクをアプリ内 Safari で開く (Apple Pay 対応)。
struct SafariView: UIViewControllerRepresentable {
    let url: URL

    func makeUIViewController(context: Context) -> SFSafariViewController {
        let vc = SFSafariViewController(url: url)
        vc.dismissButtonStyle = .close
        vc.preferredBarTintColor = .black
        vc.preferredControlTintColor = UIColor(MUTheme.gold)
        return vc
    }

    func updateUIViewController(_ vc: SFSafariViewController, context: Context) {}
}
