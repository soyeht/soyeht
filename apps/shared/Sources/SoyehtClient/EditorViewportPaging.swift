import SoyehtProtocol

public enum EditorViewportPaging {
  public static let pageLines = UInt32(ProtocolLimits.editorViewportLines)
  public static let prefetchLines: UInt32 = 160

  public static func requestedStart(
    visibleFrom: UInt32, visibleLines: UInt32, loadedFrom: UInt32, loadedTo: UInt32,
    totalLines: UInt32
  ) -> UInt32? {
    guard totalLines > loadedTo - loadedFrom else { return nil }
    let visibleTo = Swift.min(visibleFrom.saturatingAdd(max(visibleLines, 1)), totalLines)
    let loadedCount = loadedTo.saturatingSubtract(loadedFrom)
    let margin = min(prefetchLines, loadedCount / 4)
    let protectedFrom = loadedFrom.saturatingAdd(margin)
    let protectedTo = loadedTo.saturatingSubtract(margin)
    if loadedCount >= pageLines, visibleFrom >= protectedFrom, visibleTo <= protectedTo {
      return nil
    }
    let lead = pageLines / 4
    let lastStart = totalLines.saturatingSubtract(pageLines)
    let start = Swift.min(visibleFrom.saturatingSubtract(lead), lastStart)
    if start == loadedFrom, loadedCount >= pageLines { return nil }
    return start
  }
}

extension UInt32 {
  fileprivate func saturatingAdd(_ other: UInt32) -> UInt32 {
    let result = addingReportingOverflow(other)
    return result.overflow ? .max : result.partialValue
  }

  fileprivate func saturatingSubtract(_ other: UInt32) -> UInt32 {
    self >= other ? self - other : 0
  }
}
