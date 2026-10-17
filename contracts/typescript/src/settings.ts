// Generated from contracts/protocol.json (sha256 4215ca4da68a5c1aac0c2c6e622a61375a04c6157e0a895566c3946275077d19). Do not edit.
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
