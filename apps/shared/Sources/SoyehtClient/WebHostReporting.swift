import SoyehtProtocol

extension EngineClient {
  public func reportWebNavigation(
    paneId: String, navigationGeneration: String, url: String, title: String,
    idempotencyKey: String
  ) async throws {
    _ = try await mutation(
      .webHostNavigated,
      WebHostNavigatedParams(
        paneId: paneId, navigationGeneration: navigationGeneration, url: url, title: title,
        idempotencyKey: idempotencyKey)
    )
    .decoded(WebHostNavigatedResult.self)
  }

  public func reportWebConsoleCounts(
    paneId: String, warnings: UInt32, errors: UInt32, idempotencyKey: String
  ) async throws {
    _ = try await mutation(
      .webHostConsoleCounts,
      WebHostConsoleCountsParams(
        paneId: paneId, warnings: warnings, errors: errors, idempotencyKey: idempotencyKey)
    )
    .decoded(WebHostConsoleCountsResult.self)
  }
}
