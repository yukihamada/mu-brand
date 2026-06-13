import SwiftUI
import AVFoundation
import UIKit

// 📷 Scan — 商品タグ/ムーンマーカーのQRをかざす。MUドメインのURLだけアプリ内で開く。
// 完全ネイティブ(カメラ)= Webにできない背骨の一つ。実物 → 真贋/音/つくる へ橋渡し。
struct ScanView: View {
    @State private var status: CameraStatus = .checking
    @State private var scannedURL: URL?
    @State private var showResult = false
    @State private var lastIgnored: String?

    enum CameraStatus { case checking, authorized, denied }

    // アプリ内で開いてよいホスト (フィッシング誘導を防ぐホワイトリスト)。
    private static let allowedHosts: Set<String> = [
        "wearmu.com", "www.wearmu.com", "mu.koe.live", "koe.live",
        "yukihamada.jp", "takibi.wtf",
    ]

    var body: some View {
        NavigationStack {
            ZStack {
                switch status {
                case .checking:
                    Color.black.ignoresSafeArea()
                    ProgressView().tint(.white)
                case .denied:
                    deniedView
                case .authorized:
                    scanner
                }
            }
            .navigationTitle(String(localized: "tab.scan"))
            .navigationBarTitleDisplayMode(.inline)
            .sheet(isPresented: $showResult, onDismiss: { scannedURL = nil }) {
                if let u = scannedURL { SafariView(url: u).ignoresSafeArea() }
            }
        }
        .task {
            await requestCamera()
            Analytics.track("view_scan")
        }
    }

    private var scanner: some View {
        ZStack {
            ScannerView(onScan: handle)
                .ignoresSafeArea()
            // フレーミングのレチクル
            RoundedRectangle(cornerRadius: 24)
                .stroke(Color.white.opacity(0.9), lineWidth: 3)
                .frame(width: 240, height: 240)
                .shadow(radius: 8)
            VStack {
                Spacer()
                VStack(spacing: 6) {
                    Image(systemName: "qrcode.viewfinder").font(.title2)
                    Text(String(localized: "scan.hint"))
                        .font(.subheadline)
                        .multilineTextAlignment(.center)
                    if lastIgnored != nil {
                        Text(String(localized: "scan.notMU"))
                            .font(.caption)
                            .foregroundStyle(.orange)
                    }
                }
                .foregroundStyle(.white)
                .padding(16)
                .frame(maxWidth: .infinity)
                .background(.black.opacity(0.45))
            }
        }
    }

    private var deniedView: some View {
        VStack(spacing: 14) {
            Image(systemName: "camera.metering.unknown").font(.largeTitle)
            Text(String(localized: "scan.denied"))
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
            Button(String(localized: "scan.openSettings")) {
                if let url = URL(string: UIApplication.openSettingsURLString) {
                    UIApplication.shared.open(url)
                }
            }
            .buttonStyle(.borderedProminent)
            .foregroundStyle(.black)
        }
        .padding(32)
    }

    private func requestCamera() async {
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized: status = .authorized
        case .notDetermined:
            let ok = await AVCaptureDevice.requestAccess(for: .video)
            status = ok ? .authorized : .denied
        default: status = .denied
        }
    }

    private func handle(_ code: String) {
        guard !showResult else { return }
        guard let u = URL(string: code), let host = u.host?.lowercased(),
              Self.allowedHosts.contains(host) else {
            lastIgnored = code
            UINotificationFeedbackGenerator().notificationOccurred(.warning)
            return
        }
        UINotificationFeedbackGenerator().notificationOccurred(.success)
        Analytics.track("scan_hit", ["host": host])
        scannedURL = u
        showResult = true
    }
}

// AVFoundation の QR リーダーを SwiftUI に橋渡し。最初の1件で結果を返し、シート表示中は止まる。
struct ScannerView: UIViewControllerRepresentable {
    let onScan: (String) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(onScan: onScan) }

    func makeUIViewController(context: Context) -> ScannerVC {
        let vc = ScannerVC()
        vc.coordinator = context.coordinator
        return vc
    }

    func updateUIViewController(_ vc: ScannerVC, context: Context) {}

    final class Coordinator: NSObject, AVCaptureMetadataOutputObjectsDelegate {
        let onScan: (String) -> Void
        private var lastFire = Date.distantPast

        init(onScan: @escaping (String) -> Void) { self.onScan = onScan }

        func metadataOutput(_ output: AVCaptureMetadataOutput,
                            didOutput objects: [AVMetadataObject],
                            from connection: AVCaptureConnection) {
            guard let obj = objects.first as? AVMetadataMachineReadableCodeObject,
                  let str = obj.stringValue else { return }
            // 連続発火を抑止 (同じコードを何度も拾わない)
            guard Date().timeIntervalSince(lastFire) > 1.5 else { return }
            lastFire = Date()
            DispatchQueue.main.async { self.onScan(str) }
        }
    }
}

// カメラセッションを持つ UIViewController。セッション開始/停止は専用キューで。
final class ScannerVC: UIViewController {
    weak var coordinator: ScannerView.Coordinator?
    private let session = AVCaptureSession()
    private let queue = DispatchQueue(label: "com.wearmu.mu.scan")
    private var preview: AVCaptureVideoPreviewLayer?

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black
        configure()
    }

    private func configure() {
        guard let device = AVCaptureDevice.default(for: .video),
              let input = try? AVCaptureDeviceInput(device: device),
              session.canAddInput(input) else { return }
        session.addInput(input)
        let output = AVCaptureMetadataOutput()
        guard session.canAddOutput(output) else { return }
        session.addOutput(output)
        output.setMetadataObjectsDelegate(coordinator, queue: DispatchQueue.main)
        output.metadataObjectTypes = [.qr]
        let pv = AVCaptureVideoPreviewLayer(session: session)
        pv.videoGravity = .resizeAspectFill
        pv.frame = view.bounds
        view.layer.addSublayer(pv)
        preview = pv
    }

    override func viewWillLayoutSubviews() {
        super.viewWillLayoutSubviews()
        preview?.frame = view.bounds
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        queue.async { [weak self] in
            guard let self, !self.session.isRunning else { return }
            self.session.startRunning()
        }
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        queue.async { [weak self] in
            guard let self, self.session.isRunning else { return }
            self.session.stopRunning()
        }
    }
}
