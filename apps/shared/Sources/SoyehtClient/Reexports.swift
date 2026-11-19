// The avatar and the voice rules live in targets of their own so that a watch app can link them
// without the terminal, the speech framework or the views (ADR 0166). A caller of this library
// keeps importing `SoyehtClient` only.
@_exported import SoyehtAvatar
@_exported import SoyehtVoice
