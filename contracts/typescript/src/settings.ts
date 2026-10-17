// Generated from contracts/protocol.json (sha256 0fae169dadd6d5c6bf76481a29509c3f62e9018799bfb9a7817b97bca607da72). Do not edit.
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
