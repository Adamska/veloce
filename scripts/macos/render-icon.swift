import AppKit
import CoreGraphics

let side: CGFloat = 1024
let output = CommandLine.arguments.dropFirst().first ?? "AppIcon.png"
let space = CGColorSpace(name: CGColorSpace.sRGB)!
guard let context = CGContext(
    data: nil, width: Int(side), height: Int(side), bitsPerComponent: 8, bytesPerRow: 0,
    space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
) else { exit(1) }

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(
        srgbRed: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
        blue: CGFloat(hex & 0xFF) / 255, alpha: alpha)
}

let tile = CGRect(x: 100, y: 100, width: 824, height: 824)
let tilePath = CGPath(roundedRect: tile, cornerWidth: 185, cornerHeight: 185, transform: nil)

context.saveGState()
context.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: color(0x000000, 0.45))
context.addPath(tilePath)
context.setFillColor(color(0x0B0D11))
context.fillPath()
context.restoreGState()

context.saveGState()
context.addPath(tilePath)
context.clip()
let body = CGGradient(
    colorsSpace: space, colors: [color(0x1C1F27), color(0x060709)] as CFArray,
    locations: [0, 1])!
context.drawLinearGradient(
    body, start: CGPoint(x: 512, y: 924), end: CGPoint(x: 512, y: 100), options: [])
let glow = CGGradient(
    colorsSpace: space, colors: [color(0xFF8A3D, 0.28), color(0xFF8A3D, 0)] as CFArray,
    locations: [0, 1])!
context.drawRadialGradient(
    glow, startCenter: CGPoint(x: 560, y: 512), startRadius: 0,
    endCenter: CGPoint(x: 560, y: 512), endRadius: 420, options: [])
context.restoreGState()

context.addPath(
    CGPath(
        roundedRect: tile.insetBy(dx: 1.5, dy: 1.5), cornerWidth: 184, cornerHeight: 184,
        transform: nil))
context.setStrokeColor(color(0xFFFFFF, 0.14))
context.setLineWidth(3)
context.strokePath()

let triangle = CGMutablePath()
triangle.move(to: CGPoint(x: 445, y: 712))
triangle.addLine(to: CGPoint(x: 745, y: 512))
triangle.addLine(to: CGPoint(x: 445, y: 312))
triangle.closeSubpath()
let rounded = triangle.copy(strokingWithWidth: 56, lineCap: .round, lineJoin: .round, miterLimit: 1)
context.saveGState()
context.addPath(rounded)
context.addPath(triangle)
context.clip()
let accent = CGGradient(
    colorsSpace: space, colors: [color(0xFFB067), color(0xFF7A2A)] as CFArray, locations: [0, 1])!
context.drawLinearGradient(
    accent, start: CGPoint(x: 445, y: 740), end: CGPoint(x: 745, y: 284), options: [])
context.restoreGState()

for (y, start, alpha) in [(612.0, 268.0, 0.55), (512.0, 228.0, 0.85), (412.0, 268.0, 0.55)] {
    context.setStrokeColor(color(0xFF8A3D, alpha))
    context.setLineWidth(30)
    context.setLineCap(.round)
    context.move(to: CGPoint(x: start, y: y))
    context.addLine(to: CGPoint(x: 360, y: y))
    context.strokePath()
}

guard let image = context.makeImage(),
    let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
else { exit(1) }
try data.write(to: URL(fileURLWithPath: output))
