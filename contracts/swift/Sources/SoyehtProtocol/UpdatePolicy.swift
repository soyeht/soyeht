// Generated from contracts/update.json. Do not edit.

/// A key that may sign a manifest, by the id the manifest names.
public struct UpdatePublicKey: Equatable, Sendable {
  public let id: String
  /// The 32 bytes of the Ed25519 public key in padded base64.
  public let key: String

  public init(id: String, key: String) {
    self.id = id
    self.key = key
  }
}

public enum UpdatePolicy {
  /// Where the signed manifest is published.
  public static let manifestURL = "https://github.com/soyeht/soyeht/releases/latest/download/update-manifest.json"
  /// The envelope format this build reads.
  public static let manifestFormat = 1
  public static let manifestMaxBytes = 65536
  public static let maxPackages = 32
  public static let maxPackageBytes: UInt64 = 4294967296
  /// Put before the payload in the bytes that are signed.
  public static let signingContext = "soyeht-update-manifest-v1\n"
  /// The keys that may sign a manifest. Empty until the release key exists: no check is trusted.
  public static let publicKeys: [UpdatePublicKey] = []
  /// The channels a manifest can be for.
  public static let channels: [String] = ["stable"]
  public static let defaultChannel = "stable"
  /// The operating systems a package can be for.
  public static let systems: [String] = ["macos", "linux", "windows"]
  /// The architectures a package can be for.
  public static let architectures: [String] = ["aarch64", "x86_64"]
  /// A package and a notes address must start with one of these.
  public static let urlPrefixes: [String] = ["https://github.com/soyeht/soyeht/releases/"]
  /// How often a running client asks again.
  public static let checkIntervalSeconds = 86400
  /// How long a fetch of the manifest may take.
  public static let checkTimeoutSeconds = 20
}

/// What both clients say about the check; `{version}` is the version the sentence is about.
public enum UpdateText {
  public static let title = "Updates"
  public static let summary = "Soyeht asks GitHub once a day whether a newer version is published. Nothing is installed without you."
  public static let toggle = "Check for updates automatically"
  public static let checkNow = "Check now"
  public static let download = "Download Soyeht {version}"
  public static let notes = "Release notes"
  public static let off = "Automatic checks are off."
  public static let notChecked = "Not checked yet."
  public static let checking = "Checking for updates…"
  public static let upToDate = "Soyeht {version} is up to date."
  public static let available = "Soyeht {version} is available."
  public static let required = "This version is no longer supported. Update to keep using Soyeht."
  public static let notConfigured = "Update checks are not set up in this build."
  public static let failed = "Could not check for updates."
}

/// Why a check ended without a verdict.
public enum UpdateFailure: String, Error, Sendable, Equatable {
  /// This build carries no public key, so no manifest can be trusted.
  case notConfigured
  /// The manifest could not be fetched: no network, a refusal or a timeout.
  case unreachable
  /// The manifest is larger than the limit.
  case tooLarge
  /// The manifest is not an envelope of the supported format.
  case badEnvelope
  /// The manifest names a key this build does not carry.
  case unknownKey
  /// The signature does not match the payload and the key.
  case badSignature
  /// The signed payload breaks a rule of the format.
  case badManifest
  /// The manifest is for another channel.
  case wrongChannel
  /// A newer version exists but has no package for this system and architecture.
  case noPackage
}
