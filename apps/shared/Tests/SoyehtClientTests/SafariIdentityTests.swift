import Testing

@testable import SoyehtClient

@Test func theWebPaneNamesTheSafariInstalledOnThisMac() {
  #expect(
    SafariIdentity.applicationName(installedVersion: "18.3.1", osMajorVersion: 15)
      == "Version/18.3.1 Safari/605.1.15")
  #expect(
    SafariIdentity.applicationName(installedVersion: "26.0", osMajorVersion: 26)
      == "Version/26.0 Safari/605.1.15")
}

@Test func aMissingOrUnreadableSafariVersionFallsBackToTheOneTheSystemShips() {
  for (os, expected) in [(13, "16.0"), (14, "17.0"), (15, "18.0"), (26, "26.0")] {
    #expect(
      SafariIdentity.applicationName(installedVersion: nil, osMajorVersion: os)
        == "Version/\(expected) Safari/605.1.15")
  }
  for unreadable in ["", "Safari", "18.3 beta", "..", ".1", "18.", "18..3", "18.3; x"] {
    #expect(
      SafariIdentity.applicationName(installedVersion: unreadable, osMajorVersion: 15)
        == "Version/18.0 Safari/605.1.15", "\(unreadable)")
  }
}
