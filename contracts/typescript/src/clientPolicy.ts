// Generated from contracts/protocol.json (sha256 5c9246216c7089634574b1e4ee21720d759b8abed6a21bfac81bffec76c08a3e). Do not edit.
export const clientPolicy = {
  "timeouts": {
    "requestTimeoutMs": 12000,
    "greetingTimeoutMs": 5000,
    "reconnectDelayMs": 1000,
    "reconnectCeilingMs": 30000,
    "draftBeatMs": 8000
  },
  "terminal": {
    "inputChunkBytes": 16384
  },
  "pageSize": {
    "queue": 64,
    "harnesses": 64,
    "models": 64,
    "transcript": 100,
    "tasks": 100
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
  "texts": {
    "harnessSwitchTitle": "Switch agent",
    "harnessSwitchHandover": "This starts a new agent session. Recent messages and pending prompts are handed over as historical data.",
    "taskMoveReason": "Moved on the workspace board"
  },
  "inlineImageTypes": [
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp"
  ],
  "paneStates": {
    "starting": {
      "turnRunning": false,
      "dot": "secondary"
    },
    "working": {
      "turnRunning": true,
      "dot": "success"
    },
    "idle": {
      "turnRunning": false,
      "dot": "success"
    },
    "waiting_for_you": {
      "turnRunning": true,
      "dot": "accent"
    },
    "cancelling": {
      "turnRunning": true,
      "dot": "secondary"
    },
    "no_signal": {
      "turnRunning": true,
      "dot": "warning"
    },
    "recovering": {
      "turnRunning": false,
      "dot": "secondary"
    },
    "dead": {
      "turnRunning": false,
      "dot": "muted"
    },
    "unknown": {
      "turnRunning": false,
      "dot": "muted"
    }
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
