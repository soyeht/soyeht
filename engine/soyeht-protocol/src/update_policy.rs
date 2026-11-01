// Generated from contracts/update.json. Do not edit.

/// A key that may sign a manifest, by the id the manifest names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicKey {
    pub id: &'static str,
    /// The 32 bytes of the Ed25519 public key in padded base64.
    pub key: &'static str,
}

/// Where the signed manifest is published.
pub const MANIFEST_URL: &str =
    "https://github.com/soyeht/soyeht/releases/latest/download/update-manifest.json";
/// The envelope format this build reads.
pub const MANIFEST_FORMAT: u32 = 1;
pub const MANIFEST_MAX_BYTES: usize = 65536;
pub const MAX_PACKAGES: usize = 32;
pub const MAX_PACKAGE_BYTES: u64 = 4294967296;
/// The longest address a manifest may carry.
pub const MAX_ADDRESS_BYTES: usize = 2048;
/// The digits of a hexadecimal SHA-256.
pub const SHA256_HEX_LENGTH: usize = 64;
/// Each part of a version stays below this (ADR 0115).
pub const VERSION_PART_LIMIT: u32 = 1000;
/// The largest sequence number a manifest may carry.
pub const MAX_SEQUENCE: u64 = 9007199254740991;
/// The longest a manifest may live after it is signed: its `expiresAt` is at most this many seconds
/// after its `sequence`, the time it was signed.
pub const MAX_LIFETIME_SECONDS: u64 = 7776000;
/// Put before the payload in the bytes that are signed.
pub const SIGNING_CONTEXT: &[u8] = b"soyeht-update-manifest-v1\n";
/// The keys that may sign a manifest. Empty until the release key exists: no check is trusted.
pub const PUBLIC_KEYS: &[PublicKey] = &[];
/// The channels a manifest can be for.
pub const CHANNELS: [&str; 1] = ["stable"];

pub const DEFAULT_CHANNEL: &str = "stable";
/// The operating systems a package can be for, as `std::env::consts::OS` spells them.
pub const SYSTEMS: [&str; 3] = ["macos", "linux", "windows"];

/// The architectures a package can be for, as `std::env::consts::ARCH` spells them.
pub const ARCHITECTURES: [&str; 2] = ["aarch64", "x86_64"];

/// A package and a notes address must start with one of these.
pub const URL_PREFIXES: [&str; 1] = ["https://github.com/soyeht/soyeht/releases/"];

/// How often a running client asks again.
pub const CHECK_INTERVAL_SECONDS: u64 = 86400;
/// How long a fetch of the manifest may take.
pub const CHECK_TIMEOUT_SECONDS: u64 = 20;
/// How long a connection may take to open.
pub const CONNECT_SECONDS: u64 = 10;
/// Redirects a release asset address goes through before the file.
pub const MAX_REDIRECTS: u64 = 5;
/// The one thing a check says about the program that asks: no version, no system.
pub const USER_AGENT: &str = "Soyeht-update-check";

/// What both clients say about the check; `{version}` is the version the sentence is about.
pub mod text {
    pub const TITLE: &str = "Updates";
    pub const SUMMARY: &str = "Soyeht asks GitHub once a day whether a newer version is published. Nothing is installed without you.";
    pub const TOGGLE: &str = "Check for updates automatically";
    pub const CHECK_NOW: &str = "Check now";
    pub const DOWNLOAD: &str = "Download Soyeht {version}";
    pub const NOTES: &str = "Release notes";
    pub const OFF: &str = "Automatic checks are off.";
    pub const NOT_CHECKED: &str = "Not checked yet.";
    pub const CHECKING: &str = "Checking for updates…";
    pub const UP_TO_DATE: &str = "Soyeht {version} is up to date.";
    pub const AVAILABLE: &str = "Soyeht {version} is available.";
    pub const REQUIRED: &str = "This version is no longer supported. Update to keep using Soyeht.";
    pub const NOT_CONFIGURED: &str = "Update checks are not set up in this build.";
    pub const EXPIRED: &str = "The update information is out of date. Check this computer's date and time, or get the newest Soyeht from the Soyeht website.";
    pub const FAILED: &str = "Could not check for updates.";
}

/// Why a check ended without a verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// This build carries no public key, so no manifest can be trusted.
    NotConfigured,
    /// The manifest could not be fetched: no network, a refusal or a timeout.
    Unreachable,
    /// The manifest is larger than the limit.
    TooLarge,
    /// The manifest is not an envelope of the supported format.
    BadEnvelope,
    /// The manifest names a key this build does not carry.
    UnknownKey,
    /// The signature does not match the payload and the key.
    BadSignature,
    /// The signed payload breaks a rule of the format.
    BadManifest,
    /// The manifest is for another channel.
    WrongChannel,
    /// A newer version exists but has no package for this system and architecture.
    NoPackage,
    /// The manifest is older than one this device has already seen.
    StaleManifest,
    /// The manifest's expiry has passed, so it may be an old one that someone keeps serving.
    Expired,
    /// This build's own version is not one the rules can compare.
    UnknownBuild,
}

impl Failure {
    /// The id the vectors and the logs use.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotConfigured => "notConfigured",
            Self::Unreachable => "unreachable",
            Self::TooLarge => "tooLarge",
            Self::BadEnvelope => "badEnvelope",
            Self::UnknownKey => "unknownKey",
            Self::BadSignature => "badSignature",
            Self::BadManifest => "badManifest",
            Self::WrongChannel => "wrongChannel",
            Self::NoPackage => "noPackage",
            Self::StaleManifest => "staleManifest",
            Self::Expired => "expired",
            Self::UnknownBuild => "unknownBuild",
        }
    }
}
