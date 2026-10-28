// Generated from contracts/profiles.json. Do not edit.
public struct ProfileValues: Sendable {
    public let name: String
    public let applicationSupportSubpath: String
    public let tcpPort: UInt16
    public let appBundleId: String
    public let engineLaunchdLabel: String
    public let ptydLaunchdLabel: String

    public static let dev = ProfileValues(
        name: "dev",
        applicationSupportSubpath: "Soyeht.next/dev",
        tcpPort: 8291,
        appBundleId: "com.soyeht.next.dev.mac",
        engineLaunchdLabel: "com.soyeht.next.dev.engine",
        ptydLaunchdLabel: "com.soyeht.next.dev.ptyd"
    )
    public static let prod = ProfileValues(
        name: "prod",
        applicationSupportSubpath: "Soyeht.next",
        tcpPort: 8290,
        appBundleId: "com.soyeht.next.mac",
        engineLaunchdLabel: "com.soyeht.next.engine",
        ptydLaunchdLabel: "com.soyeht.next.ptyd"
    )

    public static let all = [dev, prod]
    public static let selectionEnvironment = "SOYEHT_PROFILE"
    public static let engineSocketName = "engine.sock"
    public static let ptydSocketName = "ptyd.sock"
}
