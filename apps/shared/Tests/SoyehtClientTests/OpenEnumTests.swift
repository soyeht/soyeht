import Foundation
import SoyehtProtocol
import Testing

/// The enums the engine may add values to (`x-openEnums`, ADR 0161) keep a value this build does
/// not know, and send it back as it came.
@Test func aValueALaterEngineAddsIsKeptNotRefused() throws {
  let decoder = JSONDecoder()
  let status = try decoder.decode(ToolStatus.self, from: Data(#""paused""#.utf8))
  #expect(status == .unknown("paused"))
  #expect(status.rawValue == "paused")
  #expect(String(data: try JSONEncoder().encode(status), encoding: .utf8) == #""paused""#)
  #expect(try decoder.decode(ToolStatus.self, from: Data(#""failed""#.utf8)) == .failed)
}

@Test func aKnownValueKeepsItsNameAndEveryOneIsListed() {
  #expect(ErrorKind(rawValue: "not_found") == .not_found)
  #expect(ErrorKind.protocol_version_unsupported.rawValue == "protocol_version_unsupported")
  #expect(ErrorKind(rawValue: "a_new_error") == .unknown("a_new_error"))
  #expect(!ErrorKind.allCases.contains(.unknown("a_new_error")))
  // Every kind the generator lists is a value this build knows: it reads back as itself, and the
  // list holds no catch-all, so adding an error kind to the contract needs no count here.
  for kind in ErrorKind.allCases {
    #expect(ErrorKind(rawValue: kind.rawValue) == kind)
  }
}

/// A closed enum still refuses a value it does not know: only a listed one is open.
@Test func anEnumThatIsNotOpenStaysClosed() {
  #expect(throws: DecodingError.self) {
    _ = try JSONDecoder().decode(Role.self, from: Data(#""root""#.utf8))
  }
}
