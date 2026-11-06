import Testing

@testable import SoyehtClient

@Test func theOnboardingNamesTheDeviceTheSentenceIsOn() {
  let ipad = PairingCopy.onboarding(on: DictationPlatform.iPad.title)
  #expect(ipad.contains("with this iPad"))
  #expect(!ipad.contains("with this iPhone"))
  #expect(PairingCopy.onboarding(on: DictationPlatform.iPhone.title).contains("with this iPhone"))
}

@Test func theMacButtonTheOnboardingNamesIsTheOneThePairingSheetOffers() {
  #expect(PairingCopy.onboarding(on: "iPad").contains("“\(PairingCopy.pairButton)”"))
  #expect(PairingCopy.scanInstruction.contains(PairingCopy.devices))
}

@Test func theInstructionsNameTheBuildThePersonHolds() {
  #expect(AppName.display(info: ["CFBundleDisplayName": "Soyeht Dev"]) == "Soyeht Dev")
  #expect(AppName.display(info: [:]) == AppName.fallback)
  #expect(AppName.display(info: ["CFBundleDisplayName": ""]) == AppName.fallback)
  #expect(AppName.display(info: nil) == AppName.fallback)
}
