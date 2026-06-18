import SafariServices
import SwiftUI

// Stripe Checkout / Web ページをアプリ内 Safari で開く (Checkout 内で Apple Pay が出る)。
struct SafariView: UIViewControllerRepresentable {
    let url: URL

    func makeUIViewController(context: Context) -> SFSafariViewController {
        let vc = SFSafariViewController(url: url)
        vc.dismissButtonStyle = .close
        return vc
    }

    func updateUIViewController(_ vc: SFSafariViewController, context: Context) {}
}
