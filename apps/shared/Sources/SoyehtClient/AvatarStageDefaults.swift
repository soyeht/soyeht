import Foundation

extension AvatarStageModel {
  /// A stage for the package in `folder`, at the size and the pace of a phone and a Mac, logging
  /// to the client's log.
  public convenience init(folder: URL) {
    self.init(folder: folder, settings: AvatarStageSettings(logger: clientLogger))
  }
}
