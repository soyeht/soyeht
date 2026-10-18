// Generated from contracts/protocol.json (sha256 230818b5f2ad0fae9840e1fa388036a3a72b6df9d0b201f9f054dc7169c935cd). Do not edit.
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
