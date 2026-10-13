import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/usage.json`, which the desktop client passes as well.
private func vector() throws -> JsonValue { try ContractVector.load("usage") }

private func name(_ band: UsageBand?) -> String? {
  band.map {
    switch $0 {
    case .success: "success"
    case .warning: "warning"
    case .danger: "danger"
    }
  }
}

@Test func theBandsSplitWhereTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "bands") {
    let used = try #require(item["used"].double)
    #expect(name(UsageBand(usedFraction: used)) == item["band"].string, "\(used)")
  }
}

@Test func theOpacitiesAreTheVectors() throws {
  for item in ContractVector.cases(try vector(), "emphasis") {
    let emphasis: UsageEmphasis =
      switch item["emphasis"].string {
      case "current": .current
      case "last_known": .lastKnown
      default: .none
      }
    #expect(emphasis.arcOpacity == item["arcOpacity"].double)
    #expect(emphasis.ringOpacity == item["ringOpacity"].double)
  }
}

@Test func aSelectorHasTheRolesTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "configRoles") {
    let option = try item["option"].decoded(ConfigOption.self)
    var roles: [String] = []
    if option.isModel { roles.append("model") }
    if option.isEffort { roles.append("effort") }
    if option.isPermissionMode { roles.append("permission") }
    #expect(
      roles == (item["roles"].array ?? []).compactMap(\.string), "\(item["name"].string ?? "")")
  }
}
