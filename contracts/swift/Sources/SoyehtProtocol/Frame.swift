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
        case FrameKind.json: return 1...ProtocolLimits.jsonBytes
        case FrameKind.terminal:
            return ProtocolLimits.terminalHeaderBytes...(ProtocolLimits.terminalHeaderBytes + ProtocolLimits.terminalPayloadBytes)
        case FrameKind.attachment:
            return ProtocolLimits.attachmentHeaderBytes...(ProtocolLimits.attachmentHeaderBytes + ProtocolLimits.attachmentPayloadBytes)
        default: throw FrameError.invalidType
        }
    }

    /// Whether a frame of this kind may advertise this length. A reader checks it before it
    /// allocates or reads the body.
    public static func accepts(kind: UInt8, length: Int) -> Bool {
        guard let range = try? bounds(kind) else { return false }
        return range.contains(length)
    }

    /// Decode one whole frame. Transport code assembles the bounded frame first.
    public static func decode(_ data: Data) throws -> Frame {
        let bytes = Array(data)
        guard bytes.count >= 5 else { throw FrameError.truncated }
        let kind = bytes[0]
        let length = bytes[1..<5].reduce(0) { ($0 << 8) | Int($1) }
        guard try bounds(kind).contains(length) else { throw FrameError.invalidLength }
        guard bytes.count == 5 + length else { throw FrameError.truncated }
        let headerSize = kind == FrameKind.json ? 0 : 16
        return Frame(kind: kind, header: Data(bytes[5..<(5 + headerSize)]), payload: Data(bytes[(5 + headerSize)...]))
    }

    public func encode() throws -> Data {
        let length = header.count + payload.count
        guard try Self.bounds(kind).contains(length), header.count == (kind == FrameKind.json ? 0 : 16) else {
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
