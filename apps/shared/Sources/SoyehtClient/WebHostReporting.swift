import SoyehtProtocol

extension EngineClient {
  public func reportWebNavigation(
    paneId: String, navigationGeneration: String, url: String, title: String,
    idempotencyKey: String
  ) async throws {
    _ = try await mutation(
      "web.host.navigated",
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
      "web.host.console_counts",
      WebHostConsoleCountsParams(
        paneId: paneId, warnings: warnings, errors: errors, idempotencyKey: idempotencyKey)
    )
    .decoded(WebHostConsoleCountsResult.self)
  }
}
