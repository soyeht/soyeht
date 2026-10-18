import SoyehtProtocol

extension EngineClient {
  #if DEBUG
    /// Answers requests in place of the wire; tests set `requestStub` to play the engine.
    typealias RequestStub = @MainActor (RequestMethod, JsonValue) async throws -> JsonValue

    func stubbedResponse(_ method: RequestMethod, _ params: JsonValue) async throws
      -> EngineResponse?
    {
      guard let requestStub else { return nil }
      return EngineResponse(value: try await requestStub(method, params), attachment: nil)
    }
  #else
    /// Production builds carry no test seam: every request goes to the wire.
    func stubbedResponse(_ method: RequestMethod, _ params: JsonValue) async throws
      -> EngineResponse?
    { nil }
  #endif
}
