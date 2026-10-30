"""The signing identifier of each program a profile installs.

The Dev installer and the Mac packaging script sign the same four programs, each under an
identifier that carries the profile's launchd prefix. The engine checks its own identifier at
run time (`ui_attestation`), so it must be the profile's engine label, never a name a signing
tool derives from the file.
"""


def binary_identifiers(profile):
    """`{component: identifier}` for the engine, the supervisor, the command line and the MCP
    program of a profile of `generated_profile.PROFILES`."""
    prefix = profile["launchdPrefix"]
    return {
        **profile["launchdLabels"],
        "mcp": f"{prefix}.mcp",
        "cli": f"{prefix}.cli",
    }
