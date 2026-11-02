"""Renders `contracts/update.json`, the update check's contract (ADR 0122), for the clients.

The file holds where the signed manifest lives, the public keys that may sign it, the limits it
must keep, the systems and architectures it names and the failures a check can end in. The Rust
protocol crate and the Swift protocol package get them as constants, so the Mac and the desktop
client type no URL, key, limit or failure name by hand. The rules that read a manifest
(`soyeht_protocol::update`, `UpdateCheck` in `apps/shared`) and the script that writes one
(`scripts/release_manifest.py`) share the vectors in `contracts/vectors/update.json`.
"""

import base64
import binascii
import json
import re
import subprocess

import ed25519
import product_version
from generated_header import header_text

UPDATE_SOURCE = "contracts/update.json"
RUST_OUTPUT = "engine/soyeht-protocol/src/update_policy.rs"
SWIFT_OUTPUT = "contracts/swift/Sources/SoyehtProtocol/UpdatePolicy.swift"
FIELDS = ("version", "manifest", "signature", "channels", "defaultChannel", "systems",
          "architectures", "urlPrefixes", "release", "check", "texts", "failures")
MANIFEST_FIELDS = ("asset", "url", "format", "maxBytes", "maxPackages", "maxPackageBytes", "maxAddressBytes",
                   "sha256HexLength", "versionPartLimit", "maxSequence", "maxLifetimeSeconds")
CHECK_FIELDS = ("intervalSeconds", "timeoutSeconds", "connectSeconds", "maxRedirects", "userAgent")
WORD = re.compile(r"[a-z][a-z0-9_]*")
FAILURE_ID = re.compile(r"[a-z][A-Za-z0-9]*")
TEXT_KEY = re.compile(r"[a-z][A-Za-z0-9]*")
KEY_ID = re.compile(r"[a-z0-9][a-z0-9-]{0,15}")


def _words(values, what):
    if not values or len(set(values)) != len(values) or not all(
        isinstance(value, str) and WORD.fullmatch(value) for value in values
    ):
        raise ValueError(f"update {what} must be a non-empty list of distinct lowercase words (digits and underscores allowed)")


def _whole(value, what):
    if type(value) is not int or not 0 < value < 2**53:
        raise ValueError(f"update {what} must be a positive whole number")


def public_key_bytes(text):
    """The 32 bytes of a base64 public key, or a ValueError."""
    try:
        raw = base64.b64decode(text, validate=True)
    except (binascii.Error, TypeError, ValueError):
        raw = b""
    if len(raw) != 32 or base64.b64encode(raw).decode() != text:
        raise ValueError("an update public key is 32 bytes in padded base64")
    return raw


def validate_update(document):
    if set(document) != set(FIELDS) or document["version"] != 1:
        raise ValueError(f"update.json must have version 1 and exactly {', '.join(FIELDS[1:])}")
    manifest = document["manifest"]
    if set(manifest) != set(MANIFEST_FIELDS) or manifest["format"] != 1:
        raise ValueError(f"update manifest takes exactly {', '.join(MANIFEST_FIELDS)}, format 1")
    for field in MANIFEST_FIELDS[4:]:
        _whole(manifest[field], field)
    if manifest["versionPartLimit"] != product_version.PART_LIMIT:
        raise ValueError("versionPartLimit is the limit of a part of the product version (ADR 0115)")
    if manifest["sha256HexLength"] != 64:
        raise ValueError("sha256HexLength is the 64 digits of a SHA-256")
    signature = document["signature"]
    if set(signature) != {"algorithm", "context", "publicKeys"} or signature["algorithm"] != "ed25519":
        raise ValueError("update signature takes an ed25519 algorithm, a context and publicKeys")
    context = signature["context"]
    if not context.endswith("\n") or not context[:-1].isprintable() or context == "\n":
        raise ValueError("the update signing context is one printable line ending in a newline")
    ids = [key.get("id") for key in signature["publicKeys"]]
    if len(set(ids)) != len(ids):
        raise ValueError("update public keys must have distinct ids")
    for key in signature["publicKeys"]:
        if set(key) != {"id", "key"} or not KEY_ID.fullmatch(key["id"]):
            raise ValueError("an update public key is an id (lowercase, digits, dashes) and a key")
        if ed25519.is_weak(public_key_bytes(key["key"])):
            raise ValueError(f"update public key {key['id']} is not a point, or has an order of 8 or less")
    _words(document["channels"], "channels")
    _words(document["systems"], "systems")
    _words(document["architectures"], "architectures")
    if document["defaultChannel"] not in document["channels"]:
        raise ValueError("the default update channel must be one of the channels")
    prefixes = document["urlPrefixes"]
    if not prefixes or not all(isinstance(p, str) and p.startswith("https://") and p.endswith("/") for p in prefixes):
        raise ValueError("update urlPrefixes are https addresses ending in a slash")
    url = manifest["url"]
    if not any(url.startswith(prefix) for prefix in prefixes) or not url.endswith("/" + manifest["asset"]):
        raise ValueError("the manifest url is below an allowed prefix and ends in the asset name")
    release = document["release"]
    if set(release) != {"packageUrl", "notesUrl"} or not all(
        "{version}" in template and any(template.startswith(prefix) for prefix in prefixes)
        for template in release.values()
    ) or "{file}" not in release["packageUrl"]:
        raise ValueError("update release templates are addresses below a prefix with {version} (and {file} for a package)")
    check = document["check"]
    if set(check) != set(CHECK_FIELDS):
        raise ValueError(f"update check takes exactly {', '.join(CHECK_FIELDS)}")
    for field in CHECK_FIELDS[:4]:
        _whole(check[field], field)
    agent = check["userAgent"]
    if not agent.isascii() or not agent.isprintable() or not agent.strip() or '"' in agent or "\\" in agent:
        raise ValueError("the update user agent is printable ASCII without quotes or backslashes")
    texts = document["texts"]
    if not texts or not all(
        TEXT_KEY.fullmatch(key) and isinstance(text, str) and text for key, text in texts.items()
    ):
        raise ValueError("update texts are camelCase keys with a text each")
    failures = document["failures"]
    if not failures or len({f.get("id") for f in failures}) != len(failures) or not all(
        set(f) == {"id", "meaning"} and FAILURE_ID.fullmatch(f["id"]) and f["meaning"] for f in failures
    ):
        raise ValueError("update failures are distinct ids, each with a meaning")


def _title(name):
    return name[:1].upper() + name[1:]


def _rust_list(name, values, doc):
    items = ", ".join(f'"{value}"' for value in values)
    return f"/// {doc}\npub const {name}: [&str; {len(values)}] = [{items}];\n"


def _upper(name):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).upper()


def _quoted(text):
    return json.dumps(text, ensure_ascii=False)


def render_rust(document, header):
    manifest, signature, check = document["manifest"], document["signature"], document["check"]
    keys = "".join(
        f'    PublicKey {{ id: "{key["id"]}", key: "{key["key"]}" }},\n' for key in signature["publicKeys"]
    )
    context = signature["context"].replace("\n", "\\n")
    failures = "".join(f"    /// {f['meaning']}\n    {_title(f['id'])},\n" for f in document["failures"])
    names = "".join(f'            Self::{_title(f["id"])} => "{f["id"]}",\n' for f in document["failures"])
    texts = "".join(f"    pub const {_upper(key)}: &str = {_quoted(text)};\n" for key, text in document["texts"].items())
    source = f"""{header}
/// A key that may sign a manifest, by the id the manifest names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicKey {{
    pub id: &'static str,
    /// The 32 bytes of the Ed25519 public key in padded base64.
    pub key: &'static str,
}}

/// Where the signed manifest is published.
pub const MANIFEST_URL: &str = "{manifest["url"]}";
/// The envelope format this build reads.
pub const MANIFEST_FORMAT: u32 = {manifest["format"]};
pub const MANIFEST_MAX_BYTES: usize = {manifest["maxBytes"]};
pub const MAX_PACKAGES: usize = {manifest["maxPackages"]};
pub const MAX_PACKAGE_BYTES: u64 = {manifest["maxPackageBytes"]};
/// The longest address a manifest may carry.
pub const MAX_ADDRESS_BYTES: usize = {manifest["maxAddressBytes"]};
/// The digits of a hexadecimal SHA-256.
pub const SHA256_HEX_LENGTH: usize = {manifest["sha256HexLength"]};
/// Each part of a version stays below this (ADR 0115).
pub const VERSION_PART_LIMIT: u32 = {manifest["versionPartLimit"]};
/// The largest sequence number a manifest may carry.
pub const MAX_SEQUENCE: u64 = {manifest["maxSequence"]};
/// The longest a manifest may live after it is signed: its `expiresAt` is at most this many seconds
/// after its `sequence`, the time it was signed.
pub const MAX_LIFETIME_SECONDS: u64 = {manifest["maxLifetimeSeconds"]};
/// Put before the payload in the bytes that are signed.
pub const SIGNING_CONTEXT: &[u8] = b"{context}";
/// The keys that may sign a manifest. Empty until the release key exists: no check is trusted.
pub const PUBLIC_KEYS: &[PublicKey] = &[
{keys}];
{_rust_list("CHANNELS", document["channels"], "The channels a manifest can be for.")}
pub const DEFAULT_CHANNEL: &str = "{document["defaultChannel"]}";
{_rust_list("SYSTEMS", document["systems"], "The operating systems a package can be for, as `std::env::consts::OS` spells them.")}
{_rust_list("ARCHITECTURES", document["architectures"], "The architectures a package can be for, as `std::env::consts::ARCH` spells them.")}
{_rust_list("URL_PREFIXES", document["urlPrefixes"], "A package and a notes address must start with one of these.")}
/// How often a running client asks again.
pub const CHECK_INTERVAL_SECONDS: u64 = {check["intervalSeconds"]};
/// How long a fetch of the manifest may take.
pub const CHECK_TIMEOUT_SECONDS: u64 = {check["timeoutSeconds"]};
/// How long a connection may take to open.
pub const CONNECT_SECONDS: u64 = {check["connectSeconds"]};
/// Redirects a release asset address goes through before the file.
pub const MAX_REDIRECTS: u64 = {check["maxRedirects"]};
/// The one thing a check says about the program that asks: no version, no system.
pub const USER_AGENT: &str = "{check["userAgent"]}";

/// What both clients say about the check; `{{version}}` is the version the sentence is about.
pub mod text {{
{texts}}}

/// Why a check ended without a verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {{
{failures}}}

impl Failure {{
    /// The id the vectors and the logs use.
    pub fn as_str(self) -> &'static str {{
        match self {{
{names}        }}
    }}
}}
"""
    return subprocess.run(["rustfmt", "--edition", "2024"], input=source, text=True,
                          capture_output=True, check=True).stdout


def _swift_list(name, values, doc):
    items = ", ".join(f'"{value}"' for value in values)
    return f"  /// {doc}\n  public static let {name}: [String] = [{items}]\n"


def render_swift(document, header):
    manifest, signature, check = document["manifest"], document["signature"], document["check"]
    keys = ", ".join(f'UpdatePublicKey(id: "{k["id"]}", key: "{k["key"]}")' for k in signature["publicKeys"])
    small_order = ", ".join(f'"{base64.b64encode(raw).decode()}"' for raw in ed25519.small_order_encodings())
    context = signature["context"].replace("\n", "\\n")
    cases = "".join(f"  /// {f['meaning']}\n  case {f['id']}\n" for f in document["failures"])
    return (
        f"{header}"
        "\n/// A key that may sign a manifest, by the id the manifest names.\n"
        "public struct UpdatePublicKey: Equatable, Sendable {\n"
        "  public let id: String\n"
        "  /// The 32 bytes of the Ed25519 public key in padded base64.\n"
        "  public let key: String\n\n"
        "  public init(id: String, key: String) {\n    self.id = id\n    self.key = key\n  }\n}\n\n"
        "public enum UpdatePolicy {\n"
        f'  /// Where the signed manifest is published.\n  public static let manifestURL = "{manifest["url"]}"\n'
        f"  /// The envelope format this build reads.\n  public static let manifestFormat = {manifest['format']}\n"
        f"  public static let manifestMaxBytes = {manifest['maxBytes']}\n"
        f"  public static let maxPackages = {manifest['maxPackages']}\n"
        f"  public static let maxPackageBytes: UInt64 = {manifest['maxPackageBytes']}\n"
        f"  /// The longest address a manifest may carry.\n  public static let maxAddressBytes = {manifest['maxAddressBytes']}\n"
        f"  /// The digits of a hexadecimal SHA-256.\n  public static let sha256HexLength = {manifest['sha256HexLength']}\n"
        f"  /// Each part of a version stays below this (ADR 0115).\n  public static let versionPartLimit = {manifest['versionPartLimit']}\n"
        f"  /// The largest sequence number a manifest may carry.\n  public static let maxSequence: UInt64 = {manifest['maxSequence']}\n"
        f"  /// The longest a manifest may live after it is signed (its `sequence`), in seconds.\n  public static let maxLifetimeSeconds: UInt64 = {manifest['maxLifetimeSeconds']}\n"
        "  /// Every 32-byte form (base64) of a point of order 8 or less: no such key may be trusted.\n"
        "  /// CryptoKit does not refuse them as a strict verifier does.\n"
        f"  public static let smallOrderKeys: Set<String> = [{small_order}]\n"
        f'  /// Put before the payload in the bytes that are signed.\n  public static let signingContext = "{context}"\n'
        "  /// The keys that may sign a manifest. Empty until the release key exists: no check is trusted.\n"
        f"  public static let publicKeys: [UpdatePublicKey] = [{keys}]\n"
        + _swift_list("channels", document["channels"], "The channels a manifest can be for.")
        + f'  public static let defaultChannel = "{document["defaultChannel"]}"\n'
        + _swift_list("systems", document["systems"], "The operating systems a package can be for.")
        + _swift_list("architectures", document["architectures"], "The architectures a package can be for.")
        + _swift_list("urlPrefixes", document["urlPrefixes"], "A package and a notes address must start with one of these.")
        + f"  /// How often a running client asks again.\n  public static let checkIntervalSeconds = {check['intervalSeconds']}\n"
        f"  /// How long a fetch of the manifest may take.\n  public static let checkTimeoutSeconds = {check['timeoutSeconds']}\n"
        f"  /// How long a connection may take to open.\n  public static let connectSeconds = {check['connectSeconds']}\n"
        f"  /// Redirects a release asset address goes through before the file.\n  public static let maxRedirects = {check['maxRedirects']}\n"
        f"  /// The one thing a check says about the program that asks: no version, no system.\n"
        f'  public static let userAgent = "{check["userAgent"]}"\n'
        "}\n\n"
        "/// What both clients say about the check; `{version}` is the version the sentence is about.\n"
        "public enum UpdateText {\n"
        + "".join(f"  public static let {key} = {_quoted(text)}\n" for key, text in document["texts"].items())
        + "}\n\n"
        "/// Why a check ended without a verdict.\n"
        "public enum UpdateFailure: String, Error, Sendable, Equatable {\n"
        f"{cases}}}\n"
    )


def render_update(document):
    header = f"// {header_text(UPDATE_SOURCE)}\n"
    return {RUST_OUTPUT: render_rust(document, header), SWIFT_OUTPUT: render_swift(document, header)}


def render_update_source(root):
    """The outputs for `contracts/update.json` under the repository `root`, after validating it."""
    document = json.loads((root / UPDATE_SOURCE).read_bytes())
    validate_update(document)
    return render_update(document)
