import Foundation

public enum FrameError: Error { case invalidType, invalidLength, truncated }

public struct Frame: Equatable, Sendable {
    public var kind: UInt8
    public var header: Data
    public var payload: Data

    public init(kind: UInt8, header: Data = Data(), payload: Data) {
        self.kind = kind
        self.header = header
        self.payload = payload
    }

    private static func bounds(_ kind: UInt8) throws -> ClosedRange<Int> {
        switch kind {
        case 1: return 1...ProtocolLimits.jsonBytes
        case 2: return ProtocolLimits.terminalHeaderBytes...(ProtocolLimits.terminalHeaderBytes + ProtocolLimits.terminalPayloadBytes)
        case 3: return ProtocolLimits.attachmentHeaderBytes...(ProtocolLimits.attachmentHeaderBytes + ProtocolLimits.attachmentPayloadBytes)
        default: throw FrameError.invalidType
        }
    }

    /// Decode one whole frame. Transport code assembles the bounded frame first.
    public static func decode(_ data: Data) throws -> Frame {
        let bytes = Array(data)
        guard bytes.count >= 5 else { throw FrameError.truncated }
        let kind = bytes[0]
        let length = bytes[1..<5].reduce(0) { ($0 << 8) | Int($1) }
        guard try bounds(kind).contains(length) else { throw FrameError.invalidLength }
        guard bytes.count == 5 + length else { throw FrameError.truncated }
        let headerSize = kind == 1 ? 0 : 16
        return Frame(kind: kind, header: Data(bytes[5..<(5 + headerSize)]), payload: Data(bytes[(5 + headerSize)...]))
    }

    public func encode() throws -> Data {
        let length = header.count + payload.count
        guard try Self.bounds(kind).contains(length), header.count == (kind == 1 ? 0 : 16) else {
            throw FrameError.invalidLength
        }
        var encoded = Data([kind])
        for shift in stride(from: 24, through: 0, by: -8) {
            encoded.append(UInt8(truncatingIfNeeded: length >> shift))
        }
        encoded.append(header)
        encoded.append(payload)
        return encoded
    }
}
