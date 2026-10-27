import AppKit
import AVFoundation
import CoreVideo

let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
let image = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 320, pixelsHigh: 180, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
for y in 0..<180 { for x in 0..<320 {
    image.setColor(NSColor(calibratedRed: CGFloat(x)/320, green: CGFloat(y)/180, blue: 0.8, alpha: 1), atX: x, y: y)
} }
try image.representation(using: .png, properties: [:])!.write(to: directory.appendingPathComponent("background.png"))
let movie = directory.appendingPathComponent("background.mp4")
let writer = try AVAssetWriter(outputURL: movie, fileType: .mp4)
let input = AVAssetWriterInput(mediaType: .video, outputSettings: [AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: 320, AVVideoHeightKey: 180])
let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32ARGB, kCVPixelBufferWidthKey as String: 320, kCVPixelBufferHeightKey as String: 180])
writer.add(input)
guard writer.startWriting() else { fatalError("Unable to start video writer: \(String(describing: writer.error))") }
writer.startSession(atSourceTime: .zero)
for frame in 0..<36 {
    let deadline = Date().addingTimeInterval(5)
    while !input.isReadyForMoreMediaData && Date() < deadline { Thread.sleep(forTimeInterval: 0.01) }
    guard input.isReadyForMoreMediaData else { fatalError("Video writer stalled") }
    var buffer: CVPixelBuffer?
    guard CVPixelBufferCreate(kCFAllocatorDefault, 320, 180, kCVPixelFormatType_32ARGB, nil, &buffer) == kCVReturnSuccess, let buffer else { fatalError("No pixel buffer") }
    CVPixelBufferLockBaseAddress(buffer, [])
    let pixels = CVPixelBufferGetBaseAddress(buffer)!.assumingMemoryBound(to: UInt8.self), row = CVPixelBufferGetBytesPerRow(buffer)
    for y in 0..<180 { for x in 0..<320 {
        let at = y * row + x * 4
        pixels[at] = 255; pixels[at+1] = UInt8((frame * 7 + x) % 256); pixels[at+2] = UInt8(y); pixels[at+3] = 180
    } }
    CVPixelBufferUnlockBaseAddress(buffer, [])
    guard adaptor.append(buffer, withPresentationTime: CMTime(value: Int64(frame), timescale: 12)) else { fatalError("Video append failed: \(String(describing: writer.error))") }
}
input.markAsFinished()
let completion = DispatchSemaphore(value: 0)
writer.finishWriting { completion.signal() }
guard completion.wait(timeout: .now()+15) == .success, writer.status == .completed else { fatalError("Video finalization failed") }
print("Generated local PNG and silent H.264 media fixtures")
