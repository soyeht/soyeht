import Foundation
import SoyehtProtocol

struct ResponseAttachmentAssembly {
  private var header: Data?
  private var bytes = Data()

  var isPending: Bool { header != nil }

  mutating func accept(_ frame: Frame) throws {
    guard frame.kind == 3, frame.header.count == ProtocolLimits.attachmentHeaderBytes,
      !frame.payload.isEmpty
    else { throw ClientFailure.invalidFrame }
    guard header == nil || header == frame.header else { throw ClientFailure.invalidFrame }
    guard bytes.count <= ProtocolLimits.writingImageBytes - frame.payload.count else {
      throw ClientFailure.invalidFrame
    }
    header = frame.header
    bytes.append(frame.payload)
  }

  mutating func commit(_ reference: AttachmentRef) throws -> Data {
    guard let uuid = UUID(uuidString: reference.attachmentId),
      uuid.uuidString.lowercased() == reference.attachmentId,
      let count = Int(reference.bytes), String(count) == reference.bytes,
      count > 0, count <= ProtocolLimits.writingImageBytes
    else { throw ClientFailure.invalidFrame }
    let expected = withUnsafeBytes(of: uuid.uuid) { Data($0) }
    guard header == expected, bytes.count == count else { throw ClientFailure.invalidFrame }
    let result = bytes
    reset()
    return result
  }

  mutating func reset() {
    header = nil
    bytes.removeAll(keepingCapacity: false)
  }
}
