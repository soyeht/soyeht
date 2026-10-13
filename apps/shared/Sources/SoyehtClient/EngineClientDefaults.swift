import SoyehtProtocol

// The contract's request timeout, in seconds, under the name the engine client's signatures use.
let defaultRequestTimeout = ClientPolicy.requestTimeoutSeconds

/// The size of the pieces raw terminal input goes out in.
let inputChunkBytes = ClientPolicy.terminalInputChunkBytes

/// How long the engine has to send its greeting after the socket opens.
let greetingTimeout: Duration = .seconds(ClientPolicy.greetingTimeoutSeconds)
