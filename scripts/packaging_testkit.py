"""Stand-ins the packaging tests share: the result of a command, a built Mac bundle, and the
bundle check with `codesign` replaced by recorded answers (`test_package_mac.py`,
`test_package_ios.py`, `test_release_identity.py`)."""

import plistlib
from pathlib import Path
from unittest.mock import patch

import package_mac


def result(stdout="", stderr="", returncode=0):
    """What `package_common.run` returns for a command."""
    return type("Result", (), {"stdout": stdout, "stderr": stderr, "returncode": returncode})()


FULL = {
    package_mac.IDENTITY_VARIABLE: "Developer ID Application: Example (ABCDE12345)",
    package_mac.TEAM_VARIABLE: "ABCDE12345",
    package_mac.NOTARY_VARIABLE: "example-profile",
}
MACHO = b"\xcf\xfa\xed\xfe" + b"\0" * 28


def fake_app(root):
    """A bundle with the shape of the built app: one main executable, four programs, one
    framework with its own program, a resource that is not code and a link."""
    app = root / "Soyeht Next.app"
    macos = app / "Contents/MacOS"
    macos.mkdir(parents=True)
    (app / "Contents/Info.plist").write_bytes(
        plistlib.dumps({"CFBundleExecutable": "Soyeht Next", "CFBundleIdentifier": package_mac.APP_ID})
    )
    for name in ("Soyeht Next", "soyeht-engine", "soyeht-ptyd", "soyeht", "soyeht-mcp"):
        (macos / name).write_bytes(MACHO)
    framework = app / "Contents/Frameworks/Inner.framework/Versions/A"
    framework.mkdir(parents=True)
    (framework / "Inner").write_bytes(MACHO)
    (app / "Contents/Resources").mkdir()
    (app / "Contents/Resources/default.metallib").write_bytes(b"MTLB")
    (app / "Contents/Resources/link").symlink_to(macos / "soyeht")
    return app


def verify_recorded(app, signing, details, requirement=lambda path: ""):
    """`verify_bundle` with `codesign` replaced by recorded answers: `details(path)` for `-dvv` and
    `requirement(path)` for `-d -r-`; every other command answers "arm64"."""
    answer = result("arm64")
    with (
        patch.object(package_mac, "run", return_value=answer),
        patch.object(package_mac, "check_info", return_value=[]),
        patch.object(package_mac, "signature_details", side_effect=details),
        patch.object(package_mac, "requirement_text", side_effect=requirement),
        patch.object(
            package_mac, "entitlements_of",
            side_effect=lambda path: package_mac.EXPECTED_APP_ENTITLEMENTS if Path(path) == app else set(),
        ),
    ):
        return package_mac.verify_bundle(app, signing, package_mac.release.current())
