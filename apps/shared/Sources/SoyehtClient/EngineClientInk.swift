import SoyehtProtocol

/// Handwriting on the wire (ADR 0158, `docs/contracts/14-handwriting-recognition.md`): the strokes
/// of a pen go to the computer the device is paired with and text comes back. The engine lists
/// the capability `ink-recognition` in its greeting when it can read.
extension EngineClient {
  /// The text the engine reads in `strokes`, likeliest candidate first. The strokes go as drawn,
  /// in the pen's own units, with the milliseconds since the first point; the engine scales them.
  /// `languages` are BCP 47 tags, most likely first. An engine that cannot read refuses with
  /// `ink_unsupported` (the reason is the error's detail) and one that reads none of the languages
  /// with `ink_language_unavailable`; neither is worth a retry. The engine ends a recognition that
  /// waits too long itself (`ProtocolLimits.inkDeadlineSec`), so the wait is its deadline plus the
  /// margin for the answer.
  public func recognizeInk(
    strokes: [InkStroke], languages: [String], maxCandidates: UInt32? = nil
  ) async throws -> InkRecognizeResult {
    try await request(
      .inkRecognize,
      .encoded(
        InkRecognizeParams(strokes: strokes, languages: languages, maxCandidates: maxCandidates)),
      timeoutSeconds: engineDeadlineTimeout(ProtocolLimits.inkDeadlineSec)
    ).decoded(InkRecognizeResult.self)
  }
}
