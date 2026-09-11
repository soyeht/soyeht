import SwiftUI
import UIKit
import VisionKit

struct QRScanner: UIViewControllerRepresentable {
  let receive: (String) -> Void
  let failure: (String) -> Void
  func makeUIViewController(context: Context) -> ScannerController {
    ScannerController(receive: receive, failure: failure)
  }
  func updateUIViewController(_ controller: ScannerController, context: Context) {}
  static func dismantleUIViewController(_ controller: ScannerController, coordinator: ()) {
    controller.scanner.stopScanning()
  }
}

@MainActor final class ScannerController: UIViewController, DataScannerViewControllerDelegate {
  let scanner = DataScannerViewController(
    recognizedDataTypes: [.barcode(symbologies: [.qr])],
    isHighFrameRateTrackingEnabled: false, isHighlightingEnabled: true)
  private let receive: (String) -> Void
  private let failure: (String) -> Void
  private var delivered = false
  init(receive: @escaping (String) -> Void, failure: @escaping (String) -> Void) {
    self.receive = receive
    self.failure = failure
    super.init(nibName: nil, bundle: nil)
  }
  required init?(coder: NSCoder) { nil }
  override func viewDidLoad() {
    super.viewDidLoad()
    addChild(scanner)
    view.addSubview(scanner.view)
    scanner.view.frame = view.bounds
    scanner.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
    scanner.didMove(toParent: self)
    scanner.delegate = self
  }
  override func viewDidAppear(_ animated: Bool) {
    super.viewDidAppear(animated)
    do { try scanner.startScanning() } catch {
      failure("Could not start the camera. Use the pairing link.")
    }
  }
  func dataScanner(
    _ dataScanner: DataScannerViewController, didAdd addedItems: [RecognizedItem],
    allItems: [RecognizedItem]
  ) {
    guard !delivered else { return }
    for case .barcode(let code) in addedItems {
      if let value = code.payloadStringValue {
        delivered = true
        scanner.stopScanning()
        receive(value)
        return
      }
    }
  }
  func dataScanner(
    _ dataScanner: DataScannerViewController,
    becameUnavailableWithError error: DataScannerViewController.ScanningUnavailable
  ) {
    failure("Scanning was interrupted. Try again or use the pairing link.")
  }
}
