// Generated from contracts/protocol.json. Do not edit.
export const clientPolicy = {
  "timeouts": {
    "requestTimeoutMs": 12000,
    "engineDeadlineMarginMs": 5000,
    "greetingTimeoutMs": 5000,
    "reconnectDelayMs": 1000,
    "reconnectCeilingMs": 30000,
    "draftBeatMs": 8000,
    "agentPartialFlushMs": 16,
    "diagnosticsMs": 30000,
    "copiedFeedbackMs": 1500,
    "serviceCommandMs": 60000
  },
  "webRead": {
    "settlePollMs": 80,
    "deadlineShare": 0.8
  },
  "terminal": {
    "inputChunkBytes": 16384,
    "inputQueueBytes": 1048576,
    "scrollbackLines": 10000,
    "outputFlushMs": 16,
    "outputFloodMs": 50,
    "outputFloodBytes": 8192,
    "interactiveWindowMs": 150,
    "outputFlushBytes": 1048576
  },
  "caps": {
    "waitingMessages": 1000,
    "fileLinks": 128,
    "webLinks": 128,
    "modelPages": 32,
    "tickerHistory": 3,
    "diagnosticsReportBytes": 4194304,
    "serviceReportBytes": 65536
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
  "crashReports": {
    "folder": "crashes",
    "incomingPrefix": "incoming-"
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
  "clientName": {
    "mac": "mac",
    "macDev": "mac-dev",
    "iphone": "iphone",
    "iphoneDev": "iphone-dev",
    "desktop": "desktop",
    "desktopDev": "desktop-dev",
    "android": "android",
    "androidDev": "android-dev"
  },
  "texts": {
    "harnessSwitchTitle": "Switch agent",
    "harnessSwitchHandover": "This starts a new agent session. Recent messages and pending prompts are handed over as historical data.",
    "harnessesLoading": "Loading agents\u2026",
    "harnessesNone": "No other agents",
    "harnessesRescan": "Check agents again",
    "cancel": "Cancel",
    "taskMoveReason": "Moved on the workspace board",
    "editorNoFile": "The engine has no file for this editor tab.",
    "editorFileNotText": "Binary file, can't be shown.",
    "editorFileTooLarge": "This file is too large to open in the editor.",
    "workspaceFolderGone": "This workspace's folder is gone:",
    "editorSaveMissing": "Couldn't save: this file was deleted from the disk. Save again to create it.",
    "editorSaveDenied": "Couldn't save: permission denied. Fix the permissions, then save again.",
    "editorSaveFull": "Couldn't save: the disk is full. Free some space, then save again.",
    "editorSaveOther": "Couldn't save this file. The engine keeps trying; save again to retry now.",
    "editorSaveAgain": "Save again",
    "diagnosticsTitle": "Diagnostics",
    "diagnosticsSummary": "Copies a report of this Soyeht and its recent logs, with paths, names, addresses and tokens removed, to paste into a support message.",
    "diagnosticsAction": "Copy diagnostics",
    "diagnosticsWorking": "Collecting diagnostics",
    "diagnosticsCopied": "Diagnostics copied. Paste them into your message.",
    "diagnosticsFailed": "Soyeht could not collect the diagnostics.",
    "diagnosticsUnavailable": "This copy of Soyeht has no diagnostics program.",
    "copyCode": "Copy code",
    "copiedCode": "Copied",
    "crashNoticeTitle": "Soyeht closed unexpectedly",
    "crashNoticeSummary": "A crash report was saved on this computer. Nothing was sent.",
    "crashNoticeCopy": "Copy report",
    "crashNoticeDismiss": "Dismiss",
    "loginStartQuestion": "Keep Soyeht running in the background?",
    "loginStartSummary": "Soyeht's engine keeps your terminals and agents running when this window is closed, and starts when you log in.",
    "loginStartLater": "You can turn it on later from Preferences.",
    "loginStartTurnOn": "Turn on",
    "loginStartNotNow": "Not now",
    "tryAgain": "Try again",
    "ok": "OK",
    "appMoveQuestion": "Move Soyeht to Applications?",
    "appMoveSummary": "Soyeht's engine needs the app in your Applications folder so macOS remembers the permissions you give it. Soyeht copies itself there and opens again. If it runs from a disk image, you can eject the image afterwards.",
    "appMoveAction": "Move to Applications",
    "appMoveMenu": "Move to Applications\u2026",
    "appMoveExplanation": "The engine is off. It needs Soyeht in your Applications folder so macOS remembers its permissions.",
    "appMoveReplaceQuestion": "Replace the app in Applications?",
    "appMoveReplaceSummary": "Soyeht is already in Applications. The old copy goes to the Trash.",
    "appMoveReplace": "Replace",
    "appMoveFailedTitle": "Soyeht could not move itself",
    "appMoveFailedLater": "The engine stays off until Soyeht is in Applications. You can drag it there yourself, or choose Move to Applications again.",
    "appMoveRelaunchFailedTitle": "Soyeht is now in Applications",
    "appMoveRelaunchFailed": "Soyeht was moved to Applications but could not open there. Open it from Applications.",
    "appMoveNotThisApp": "This copy is not the app Soyeht expected, so nothing was changed.",
    "appMoveAnotherApp": "A different app with the same name is in Applications. Soyeht did not touch it.",
    "appMoveNoWritableFolder": "Neither the Applications folder nor your own Applications folder can be written to.",
    "appMoveChanged": "Applications changed while Soyeht was moving. Nothing was replaced.",
    "appMoveNotFinished": "The copy did not finish. The app that was there is unchanged.",
    "appMoveNotSigned": "The copy is not signed like this app, so it was not installed. The app that was there is unchanged.",
    "appMoveAlreadyThere": "This is the copy that is in Applications. Open it from there.",
    "appMoveRunning": "Soyeht is running from Applications. Quit it there, then move this copy.",
    "appMoveCopyLocked": "A copy of Soyeht is already in Applications and cannot be replaced from this account. Remove it, or open that copy.",
    "appMoveOldInTrash": "The old copy of Soyeht is in the Trash and could not be put back. Drag it from the Trash to Applications to restore it.",
    "pushNeedsYouTitle": "An agent needs you",
    "pushNeedsYouBody": "Open Soyeht to see what it asks.",
    "projectTrustTitle": "Trust this folder?",
    "projectTrustSummary": "Trusting a folder lets Soyeht run what the folder contains. Language servers read its build files, which can run programs. Keep it restricted if you do not know where it came from.",
    "projectTrustTrust": "Trust folder",
    "projectTrustRestrict": "Keep restricted",
    "projectTrustRestrictedNote": "This folder is restricted: language servers run with code execution off, or do not start.",
    "projectTrustFailed": "Soyeht could not save this choice.",
    "updateSoyeht": "This app and the Mac speak different versions of Soyeht. Update Soyeht on the older one."
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
    "cursor": "Cursor",
    "opencode": "OpenCode"
  },
  "usageHarnesses": {
    "claude": "claude",
    "codex": "codex",
    "cursor": "cursor",
    "opencode": "opencode"
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
