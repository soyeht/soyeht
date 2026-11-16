import CoreML
import CryptoKit
import Foundation
import SoyehtClient

/// One file of a model at a pinned revision: its size and the SHA-256 of its bytes.
struct PinnedFile: Sendable {
  let path: String
  let size: Int
  let sha256: String
}

/// A model that is downloaded from a repository at one commit, never from the moving branch, and
/// whose every file is checked against the size and checksum written here. A new version of a model
/// is a change to these numbers, reviewed with its license (`scripts/third_party/models.json`).
struct PinnedModel: Sendable {
  let repository: String
  let revision: String
  let files: [PinnedFile]
}

extension PinnedModel {
  /// Silero VAD v6.2.1, the CoreML conversion (MIT).
  static let sileroVAD = PinnedModel(
    repository: "aufklarer/Silero-VAD-v6.2.1-CoreML",
    revision: "523876545a57961474fee9df913e833e130560b8",
    files: [
      PinnedFile(
        path: "config.json", size: 888,
        sha256: "459e764d58cdc13f3db6878adfdf8a29b5fd467ad1f4ef2161137cc115339c81"),
      PinnedFile(
        path: "silero_vad.mlmodelc/analytics/coremldata.bin", size: 243,
        sha256: "b777c3751d72b7430eac7f8544769a3d918faf77c15db184fec30e44c56007a3"),
      PinnedFile(
        path: "silero_vad.mlmodelc/coremldata.bin", size: 399,
        sha256: "f6fcd92c3132c9c718e5f54e0e770a8c8075beaa50a5b212a6287273b4ddae67"),
      PinnedFile(
        path: "silero_vad.mlmodelc/metadata.json", size: 3005,
        sha256: "1b953eb3818e7092deedd96e976c05354f77beb2ddc2976fe416af17e47f62d2"),
      PinnedFile(
        path: "silero_vad.mlmodelc/model.mil", size: 18203,
        sha256: "b0a1384c4a664697989d9eb9cfb166b4b85f151206aeefd1bfa391ef9e5ad08f"),
      PinnedFile(
        path: "silero_vad.mlmodelc/weights/weight.bin", size: 619136,
        sha256: "83210545de90c65195e8d6db1b349b7e5c31f989f48d0a908a8dc0e2f586e5f9"),
    ])

  /// Smart Turn v3.2, the CoreML conversion (BSD-2-Clause).
  static let smartTurn = PinnedModel(
    repository: "aufklarer/Smart-Turn-v3.2-CoreML",
    revision: "75f069717f500947195fa1945c46ddb472559cff",
    files: [
      PinnedFile(
        path: "config.json", size: 1029,
        sha256: "bc09d1b40285465b389dc42cef54290fa1db7b4928348018bc61d819b4914402"),
      PinnedFile(
        path: "smart_turn.mlmodelc/analytics/coremldata.bin", size: 243,
        sha256: "1646746d0029e78d26be065e62602d4f89e51013ec238b653adf7d7d21057f89"),
      PinnedFile(
        path: "smart_turn.mlmodelc/coremldata.bin", size: 643,
        sha256: "9245ac99889c9d738cba6c89da9e851fd16488bf8effbc15a109d06b401c9383"),
      PinnedFile(
        path: "smart_turn.mlmodelc/model.mil", size: 69968,
        sha256: "41a6d9ac0f7d01d738e56ecdf4435e55f139591541b7cf1a4dba6b9ce2f1c9ef"),
      PinnedFile(
        path: "smart_turn.mlmodelc/weights/weight.bin", size: 16_714_240,
        sha256: "b6295316741a0149e1fcab7cdf030e1b9bfb0eef778207accee91a2a9703a39c"),
    ])
}

enum PinnedDownload {
  enum Failure: LocalizedError {
    case missing(String)
    case status(String, Int)
    case mismatch(String)

    var errorDescription: String? {
      switch self {
      case .missing(let path): "The model file \(path) is not on this device."
      case .status(let path, let code):
        "The model file \(path) could not be fetched (HTTP \(code))."
      case .mismatch(let path):
        "The model file \(path) is not the one expected, so it was not kept."
      }
    }
  }

  /// Makes `directory` hold every file of `model`. With `offline` it only checks that they are
  /// there (by size: the checksum was verified when they were fetched); otherwise it fetches what
  /// is missing, one file at a time, and keeps a file only if its size and checksum are the pinned
  /// ones. `report` gets the share of the bytes done, before and after each file.
  static func fetch(
    _ model: PinnedModel, to directory: URL, offline: Bool,
    report: @escaping @Sendable (Double) -> Void
  ) async throws {
    let total = Double(model.files.reduce(0) { $0 + $1.size })
    var done = 0.0
    for file in model.files {
      report(done / total)
      let target = directory.appendingPathComponent(file.path)
      if present(target, size: file.size) {
        done += Double(file.size)
        continue
      }
      guard !offline else { throw Failure.missing(file.path) }
      try await download(file, of: model, to: target)
      done += Double(file.size)
    }
    report(1)
  }

  private static func present(_ url: URL, size: Int) -> Bool {
    do {
      return try FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int == size
    } catch {
      return false
    }
  }

  private static func download(_ file: PinnedFile, of model: PinnedModel, to target: URL)
    async throws
  {
    guard
      let address = URL(
        string: "https://huggingface.co/\(model.repository)/resolve/\(model.revision)/\(file.path)")
    else { throw Failure.missing(file.path) }
    let (temporary, response) = try await URLSession.shared.download(from: address)
    defer {
      // The download's own temporary file: the system sweeps it if it stays.
      _ = ClientDiagnostics.attempt("model_temporary_file_remove_failed", key: "model") {
        try FileManager.default.removeItem(at: temporary)
      }
    }
    if let status = (response as? HTTPURLResponse)?.statusCode, status != 200 {
      throw Failure.status(file.path, status)
    }
    let bytes = try Data(contentsOf: temporary)
    let digest = SHA256.hash(data: bytes).map { String(format: "%02x", $0) }.joined()
    guard bytes.count == file.size, digest == file.sha256 else { throw Failure.mismatch(file.path) }
    try FileManager.default.createDirectory(
      at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
    if FileManager.default.fileExists(atPath: target.path) {
      try FileManager.default.removeItem(at: target)
    }
    try FileManager.default.moveItem(at: temporary, to: target)
  }
}

extension PinnedDownload {
  /// Fetches a pinned model (or, when it is `installed`, checks it) and loads the compiled CoreML
  /// program called `compiled` that it holds, for the Neural Engine and the CPU. Only a fetch reports
  /// a download; the load reports that it is preparing.
  static func loadCompiled(
    _ model: PinnedModel, compiled: String, in directory: URL, installed: Bool,
    report: @escaping @Sendable (DictationModelStage) -> Void
  ) async throws -> MLModel {
    try await fetch(model, to: directory, offline: installed) {
      if !installed { report(.downloading($0)) }
    }
    report(.preparing)
    let configuration = MLModelConfiguration()
    configuration.computeUnits = .cpuAndNeuralEngine
    return try MLModel(
      contentsOf: directory.appendingPathComponent(compiled, isDirectory: true),
      configuration: configuration)
  }
}
