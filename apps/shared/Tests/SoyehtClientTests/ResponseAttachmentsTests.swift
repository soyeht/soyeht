import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func responseAttachmentsAreBoundedOrderedAndIdentityChecked() throws {
  let id = "00112233-4455-4677-8899-aabbccddeeff"
  let uuid = try #require(UUID(uuidString: id))
  let header = withUnsafeBytes(of: uuid.uuid) { Data($0) }
  let reference = AttachmentRef(
    attachmentId: id, mimeType: "image/png", name: "image.png", bytes: "6")
  var assembly = ResponseAttachmentAssembly()
  try assembly.accept(Frame(kind: 3, header: header, payload: Data([1, 2, 3])))
  try assembly.accept(Frame(kind: 3, header: header, payload: Data([4, 5, 6])))
  #expect(try assembly.commit(reference) == Data([1, 2, 3, 4, 5, 6]))
  #expect(!assembly.isPending)

  try assembly.accept(Frame(kind: 3, header: header, payload: Data([1])))
  let wrong = AttachmentRef(
    attachmentId: "11112233-4455-4677-8899-aabbccddeeff", mimeType: "image/png",
    name: "image.png", bytes: "1")
  #expect(throws: ClientFailure.self) { try assembly.commit(wrong) }
}
