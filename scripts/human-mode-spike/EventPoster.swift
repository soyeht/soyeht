import CoreGraphics
import Foundation

let args = CommandLine.arguments
guard args.count >= 5, let target = Int32(args[1]) else { exit(2) }
let globally = args[2] == "global"
func send(_ event: CGEvent) { globally ? event.post(tap: .cghidEventTap) : event.postToPid(target) }
if args[3] == "key", let code = UInt16(args[4]) {
  let down = CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: true)!
  if args.count > 5 {
    var units = Array(args[5].utf16)
    down.keyboardSetUnicodeString(stringLength: units.count, unicodeString: &units)
  }
  send(down)
  usleep(12_000)
  send(CGEvent(keyboardEventSource: nil, virtualKey: code, keyDown: false)!)
} else if args[3] == "mouse", args.count >= 8, let raw = UInt32(args[4]), let x = Double(args[5]),
  let y = Double(args[6]), let clicks = Int64(args[7]), let type = CGEventType(rawValue: raw)
{
  let event = CGEvent(
    mouseEventSource: nil, mouseType: type, mouseCursorPosition: CGPoint(x: x, y: y),
    mouseButton: .left)
  event?.setIntegerValueField(.mouseEventClickState, value: clicks)
  if let event { send(event) }
} else {
  exit(2)
}
