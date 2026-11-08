// Generate a short H.264/MOV acceptance fixture from the supplied public-domain portrait.
// Developer utility only; never shipped or invoked by the application.
import AppKit
import AVFoundation
import CoreVideo
import Foundation
let arguments=CommandLine.arguments
guard arguments.count>=3 else {fatalError("usage: face_fixture_video input-image new-output.mov [rotation-degrees]")}
let source=URL(fileURLWithPath:arguments[1]),output=URL(fileURLWithPath:arguments[2])
guard !FileManager.default.fileExists(atPath:output.path),let image=NSImage(contentsOf:source),let cg=image.cgImage(forProposedRect:nil,context:nil,hints:nil) else {fatalError("fixture input unavailable or output exists")}
let width=512,height=384
let writer=try AVAssetWriter(outputURL:output,fileType:.mov)
let input=AVAssetWriterInput(mediaType:.video,outputSettings:[AVVideoCodecKey:AVVideoCodecType.h264,AVVideoWidthKey:width,AVVideoHeightKey:height])
let rotation=arguments.count>3 ? Double(arguments[3]) ?? 0 : 0
input.transform=CGAffineTransform(rotationAngle:rotation * .pi/180)
let attributes:[String:Any]=[kCVPixelBufferPixelFormatTypeKey as String:kCVPixelFormatType_32ARGB,kCVPixelBufferWidthKey as String:width,kCVPixelBufferHeightKey as String:height,kCVPixelBufferCGImageCompatibilityKey as String:true,kCVPixelBufferCGBitmapContextCompatibilityKey as String:true]
let adaptor=AVAssetWriterInputPixelBufferAdaptor(assetWriterInput:input,sourcePixelBufferAttributes:attributes)
writer.add(input)
guard writer.startWriting() else {fatalError("writer could not start: \(String(describing:writer.error))")}
writer.startSession(atSourceTime:.zero)
for frame in 0..<6 {
 let deadline=Date().addingTimeInterval(15)
 while !input.isReadyForMoreMediaData {if Date()>deadline {fatalError("encoder readiness timeout: \(String(describing:writer.error))")};RunLoop.current.run(until:Date().addingTimeInterval(0.01))}
 var buffer:CVPixelBuffer?
 guard CVPixelBufferCreate(kCFAllocatorDefault,width,height,kCVPixelFormatType_32ARGB,attributes as CFDictionary,&buffer)==kCVReturnSuccess,let pixels=buffer else {fatalError("pixel buffer")}
 CVPixelBufferLockBaseAddress(pixels,[])
 let context=CGContext(data:CVPixelBufferGetBaseAddress(pixels),width:width,height:height,bitsPerComponent:8,bytesPerRow:CVPixelBufferGetBytesPerRow(pixels),space:CGColorSpaceCreateDeviceRGB(),bitmapInfo:CGImageAlphaInfo.noneSkipFirst.rawValue)!
 context.draw(cg,in:CGRect(x:0,y:0,width:width,height:height))
 CVPixelBufferUnlockBaseAddress(pixels,[])
 guard adaptor.append(pixels,withPresentationTime:CMTime(value:Int64(frame),timescale:1)) else {fatalError("append failed")}
}
input.markAsFinished()
let finished=DispatchSemaphore(value:0)
writer.finishWriting {finished.signal()}
if finished.wait(timeout:.now()+15) == .timedOut {fatalError("encoder finish timed out")}
guard writer.status == .completed else {fatalError("writer failed: \(String(describing:writer.error))")}
print(output.path)
