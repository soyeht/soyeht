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
