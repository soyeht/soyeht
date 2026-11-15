import Foundation
import Security
import Testing

@testable import SoyehtClient

// These tests never read or write the keychain: the vault under test is the in-memory one, and
// the system vault is only asked for the attributes it would use and for what it refuses
// before it reaches the keychain.

@Test func theKeychainItemIsPerProviderAndStaysOnThisDevice() {
  let query = KeychainProviderKeyVault.query(
    service: KeychainProviderKeyVault.service, provider: "cloud.example")
  #expect(query[kSecClass as String] as? String == kSecClassGenericPassword as String)
  #expect(query[kSecAttrAccount as String] as? String == "cloud.example")
  #expect((query[kSecAttrService as String] as? String)?.contains(".next.") == true)
}

@Test func theSystemVaultRefusesAKeyThatCannotBeOneBeforeTouchingTheKeychain() {
  let vault = KeychainProviderKeyVault(service: "com.soyeht.next.tests.never-written")
  for key in ["", "   \n", "two words", "line\nbreak"] {
    #expect(throws: ProviderKeyFailure.invalidKey) { try vault.save(key, for: "cloud.example") }
  }
}

@Test func aKeyIsKeptPerProviderAndForgotten() throws {
  let vault = MemoryProviderKeyVault()
  try vault.save("key-one", for: "a")
  try vault.save("key-two", for: "b")
  #expect(try vault.key(for: "a") == "key-one")
  try vault.forget(provider: "a")
  #expect(try vault.key(for: "a") == nil)
  #expect(try vault.key(for: "b") == "key-two")
}
