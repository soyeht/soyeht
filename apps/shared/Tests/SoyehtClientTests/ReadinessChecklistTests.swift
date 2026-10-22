import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/readiness.json`, which the desktop client passes as well.
@Test func everySharedCaseGivesTheSameChecklist() throws {
  let cases = ContractVector.cases(try ContractVector.load("readiness"), "cases")
  #expect(cases.count >= 11)
  for item in cases {
    let name = item["name"].string ?? ""
    let result =
      item["result"] == .null ? nil : try item["result"].decoded(EngineReadinessResult.self)
    let checklist = ReadinessChecklist.make(
      connected: item["connected"].bool ?? false, result: result)
    let expected = item["expected"]
    #expect(
      checklist.rows == (try expected["rows"].decoded([ReadinessChecklist.Row].self)), "\(name)")
    #expect(checklist.complete == expected["complete"].bool, "\(name)")
    #expect(checklist.blockers == expected["blockers"].int, "\(name)")
    #expect(checklist.headline == expected["headline"].string, "\(name)")
  }
}
