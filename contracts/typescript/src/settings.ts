// Generated from contracts/protocol.json (sha256 9bb117402f4ec1fb09eae31802dbe473e93d71604bc115885ff580588304f488). Do not edit.
export const settings = {
  "terminal.defaultCols": {
    "type": "integer",
    "minimum": 1,
    "maximum": 1000,
    "default": 80
  },
  "terminal.defaultRows": {
    "type": "integer",
    "minimum": 1,
    "maximum": 1000,
    "default": 24
  },
  "record.default": {
    "type": "boolean",
    "default": true
  },
  "harness.preferredId": {
    "type": "string",
    "maxLength": 128,
    "default": ""
  },
  "harness.defaultMode.<harnessId>": {
    "type": "string",
    "maxLength": 128,
    "default": "",
    "parameter": {
      "minLength": 1,
      "maxLength": 128
    }
  }
} as const;
