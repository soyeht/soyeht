import SoyehtProtocol

// The contract's request timeout, in seconds, under the name the engine client's signatures use.
let defaultRequestTimeout = ClientPolicy.requestTimeoutSeconds

/// The wait for a request the engine ends itself after `engineSeconds` (a `ProtocolLimits` value):
/// its deadline plus the margin for the answer to come back, so the engine's own failure reaches
/// the person before the client gives up on the request.
func engineDeadlineTimeout(_ engineSeconds: Int) -> Double {
  Double(engineSeconds) + ClientPolicy.engineDeadlineMarginSeconds
}

/// The size of the pieces raw terminal input goes out in.
let inputChunkBytes = ClientPolicy.terminalInputChunkBytes

/// How much raw terminal input may wait to be sent before a client refuses more.
let inputQueueBytes = ClientPolicy.terminalInputQueueBytes

/// How long the engine has to send its greeting after the socket opens.
let greetingTimeout: Duration = .seconds(ClientPolicy.greetingTimeoutSeconds)
