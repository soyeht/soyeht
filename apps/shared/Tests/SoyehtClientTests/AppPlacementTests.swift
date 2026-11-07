#if os(macOS)
  import Foundation
  import SoyehtProtocol
  import Testing

  @testable import SoyehtClient

  private let home = URL(fileURLWithPath: "/Users/person")

  @Test(arguments: [
    ("/Applications/Soyeht Next.app", AppPlacement.Place.stable),
    ("/Applications/Soyeht Next.app/Contents/MacOS/soyeht", .stable),
    ("/Applications/Utilities/Soyeht Next.app", .stable),
    ("/Users/person/Applications/Soyeht Next.app", .stable),
    ("/Users/person/Applications/Tools/Soyeht Next.app", .stable),
    ("/Applications/../Users/person/Downloads/Soyeht Next.app", .elsewhere),
    ("/Applications", .elsewhere),
    ("/Applications-old/Soyeht Next.app", .elsewhere),
    ("/Users/person/Downloads/Soyeht Next.app", .elsewhere),
    ("/Users/person/Desktop/Applications.app", .elsewhere),
    ("/Users/other/Applications/Soyeht Next.app", .elsewhere),
    ("/Users/person/Downloads/Applications/Soyeht Next.app", .elsewhere),
    ("/Volumes/Soyeht Next/Soyeht Next.app", .diskImage),
    ("/Volumes/Data/Applications/Soyeht Next.app", .diskImage),
    ("/private/var/folders/ab/xyz/AppTranslocation/ID/d/Soyeht Next.app", .translocated),
    ("/Applications/AppTranslocation/Soyeht Next.app", .translocated),
  ])
  func theRuleNamesTheStablePlacesAndWhyEveryOtherIsNot(path: String, place: AppPlacement.Place) {
    #expect(AppPlacement.place(of: URL(fileURLWithPath: path), home: home) == place)
    #expect(AppPlacement.isStable(URL(fileURLWithPath: path), home: home) == (place == .stable))
  }

  @Test func theAppInstallsToTheSystemFolderFirstThenTheUsers() {
    #expect(
      AppPlacement.installFolders(home: home).map(\.path)
        == ["/Applications", "/Users/person/Applications"])
  }

  @Test func onlyAReleasedProfileIsHeldToTheRule() {
    #expect(AppPlacement.appliesTo(.prod))
    #expect(!AppPlacement.appliesTo(.dev))
  }

  @Test func aDiskImageAndATranslocatedCopyAreLeftAloneAfterAMove() {
    #expect(AppPlacement.Place.diskImage.isReadOnlyOrigin)
    #expect(AppPlacement.Place.translocated.isReadOnlyOrigin)
    #expect(!AppPlacement.Place.elsewhere.isReadOnlyOrigin)
    #expect(!AppPlacement.Place.stable.isReadOnlyOrigin)
  }
#endif
