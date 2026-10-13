// Generated from contracts/protocol.json (sha256 96e9a6c2600dc2ec7c40a94f545618ee858e5ce15ee105719d8c3c8114ec5404). Do not edit.
export const clientPolicy = {
  "timeouts": {
    "requestTimeoutMs": 12000,
    "greetingTimeoutMs": 5000,
    "reconnectDelayMs": 1000
  },
  "terminal": {
    "inputChunkBytes": 16384
  },
  "usageBand": {
    "warningFrom": 0.5,
    "dangerAbove": 0.8
  },
  "contextBand": {
    "warningAbove": 0.75,
    "dangerAbove": 0.9
  },
  "usageRing": {
    "lastKnownArcOpacity": 0.45,
    "unavailableOpacity": 0.46
  },
  "usageWindow": {
    "weeklyKeyword": "week"
  },
  "usageProviderNames": {
    "claude": "Claude",
    "codex": "Codex",
    "cursor": "Cursor"
  },
  "configRoles": {
    "model": {
      "category": "model"
    },
    "effort": {
      "category": "thought_level",
      "legacyId": "effort"
    },
    "permission": {
      "category": "mode",
      "legacyId": "mode"
    }
  }
} as const;
