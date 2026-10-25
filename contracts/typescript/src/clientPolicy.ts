// Generated from contracts/protocol.json. Do not edit.
export const clientPolicy = {
  "timeouts": {
    "requestTimeoutMs": 12000,
    "greetingTimeoutMs": 5000,
    "reconnectDelayMs": 1000,
    "reconnectCeilingMs": 30000,
    "draftBeatMs": 8000,
    "agentPartialFlushMs": 16
  },
  "terminal": {
    "inputChunkBytes": 16384,
    "scrollbackLines": 10000
  },
  "caps": {
    "waitingMessages": 1000,
    "fileLinks": 128,
    "webLinks": 128,
    "modelPages": 32,
    "tickerHistory": 3
  },
  "pageSize": {
    "queue": 64,
    "harnesses": 64,
    "models": 64,
    "transcript": 100,
    "tasks": 100,
    "writingSection": 100
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
  "editor": {
    "prefetchLines": 160,
    "pageLeadDivisor": 4,
    "tabWidth": 4,
    "tabHistory": 64,
    "saveAttempts": 30,
    "saveRetryMs": 100,
    "indentUnit": "    ",
    "indentOpeners": "{[("
  },
  "texts": {
    "harnessSwitchTitle": "Switch agent",
    "harnessSwitchHandover": "This starts a new agent session. Recent messages and pending prompts are handed over as historical data.",
    "taskMoveReason": "Moved on the workspace board",
    "editorNoFile": "The engine has no file for this editor tab.",
    "editorFileNotText": "Binary file, can't be shown.",
    "editorFileTooLarge": "This file is too large to open in the editor.",
    "editorSaveMissing": "Couldn't save: this file was deleted from the disk. Save again to create it.",
    "editorSaveDenied": "Couldn't save: permission denied. Fix the permissions, then save again.",
    "editorSaveFull": "Couldn't save: the disk is full. Free some space, then save again.",
    "editorSaveOther": "Couldn't save this file. The engine keeps trying; save again to retry now.",
    "editorSaveAgain": "Save again"
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
  },
  "editorPairs": {
    "(": ")",
    "[": "]",
    "{": "}",
    "\"": "\"",
    "'": "'"
  },
  "highlightRoles": {
    "keyword": "accent",
    "string": "success",
    "comment": "muted",
    "number": "warning",
    "type": "syntaxType",
    "function": "accent",
    "property": "ink",
    "operator": "accent",
    "punctuation": "ink",
    "constant": "warning",
    "variable": "ink"
  }
} as const;
