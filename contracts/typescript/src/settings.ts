// Generated from contracts/protocol.json (sha256 a83e5a091c82349a4173728fab5308dc11fe83521977008e26f2fb81c3b50038). Do not edit.
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
