// ba-transcribe: on-device transcription with Apple's SpeechAnalyzer.
//
// Better Audible runs this as a helper process and reads one JSON object per
// line from stdout.
//
//   ba-transcribe status <locale>
//       {"status":"installed"|"supported"|"downloading"|"unsupported","locale":"en-US"}
//   ba-transcribe install <locale>
//       {"progress":0.42} ... then {"done":true}
//   ba-transcribe transcribe <audio> <start-seconds> <end-seconds> <locale>
//       {"start":12.3,"end":15.8,"text":"…","words":[["It",12.3,12.5],…]} per sentence
//
// Errors go to stdout as {"error":"…"} with a non-zero exit status.

import AVFoundation
import CoreMedia
import Foundation
import Speech

func emit(_ object: [String: Any]) {
    if let data = try? JSONSerialization.data(withJSONObject: object),
        let line = String(data: data, encoding: .utf8)
    {
        print(line)
        fflush(stdout)
    }
}

func fail(_ message: String) -> Never {
    emit(["error": message])
    exit(1)
}

func transcriber(for locale: Locale) -> SpeechTranscriber {
    SpeechTranscriber(
        locale: locale,
        transcriptionOptions: [],
        reportingOptions: [],
        attributeOptions: [.audioTimeRange])
}

func resolve(_ identifier: String) async -> Locale? {
    await SpeechTranscriber.supportedLocale(equivalentTo: Locale(identifier: identifier))
}

func status(_ identifier: String) async {
    guard let locale = await resolve(identifier) else {
        emit(["status": "unsupported", "locale": identifier])
        return
    }
    let state = await AssetInventory.status(forModules: [transcriber(for: locale)])
    let name: String
    switch state {
    case .installed: name = "installed"
    case .downloading: name = "downloading"
    case .supported: name = "supported"
    default: name = "unsupported"
    }
    emit(["status": name, "locale": locale.identifier(.bcp47)])
}

func install(_ identifier: String) async {
    guard let locale = await resolve(identifier) else { fail("This language is not supported.") }
    let module = transcriber(for: locale)
    do {
        if let request = try await AssetInventory.assetInstallationRequest(supporting: [module]) {
            let progress = request.progress
            let reporter = Task {
                while !Task.isCancelled {
                    emit(["progress": progress.fractionCompleted])
                    try? await Task.sleep(for: .milliseconds(400))
                }
            }
            try await request.downloadAndInstall()
            reporter.cancel()
        }
        emit(["done": true])
    } catch {
        fail("macOS could not add speech for this language: \(error.localizedDescription)")
    }
}

/// Splits a result into words with absolute times.
func words(in text: AttributedString, offset: Double) -> [[Any]] {
    var out: [[Any]] = []
    for run in text.runs {
        guard let range = run.audioTimeRange else { continue }
        let word = String(text[run.range].characters).trimmingCharacters(in: .whitespaces)
        if word.isEmpty { continue }
        out.append([word, range.start.seconds + offset, range.end.seconds + offset])
    }
    return out
}

func transcribe(path: String, start: Double, end: Double, identifier: String) async {
    guard let locale = await resolve(identifier) else { fail("This language is not supported.") }
    let module = transcriber(for: locale)
    guard await AssetInventory.status(forModules: [module]) == .installed else {
        fail("macOS has not added speech for this language yet.")
    }
    let file: AVAudioFile
    do {
        file = try AVAudioFile(forReading: URL(fileURLWithPath: path))
    } catch {
        fail("Could not open the audio: \(error.localizedDescription)")
    }
    let source = file.processingFormat
    guard
        let target = await SpeechAnalyzer.bestAvailableAudioFormat(
            compatibleWith: [module], considering: source),
        let converter = AVAudioConverter(from: source, to: target)
    else { fail("Could not prepare the audio for transcription.") }

    let sampleRate = source.sampleRate
    let first = AVAudioFramePosition(max(0, start) * sampleRate)
    let last = min(AVAudioFramePosition(end * sampleRate), file.length)
    guard last > first else { fail("Nothing to transcribe in that range.") }
    file.framePosition = first

    let (stream, input) = AsyncStream<AnalyzerInput>.makeStream()
    let analyzer = SpeechAnalyzer(modules: [module])
    do {
        try await analyzer.start(inputSequence: stream)
    } catch {
        fail("Could not start transcription: \(error.localizedDescription)")
    }

    let collector = Task {
        do {
            for try await result in module.results where result.isFinal {
                let text = String(result.text.characters)
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                if text.isEmpty { continue }
                emit([
                    "start": result.range.start.seconds + start,
                    "end": result.range.end.seconds + start,
                    "text": text,
                    "words": words(in: result.text, offset: start),
                ])
            }
        } catch {
            emit(["error": "Transcription stopped: \(error.localizedDescription)"])
        }
    }

    // Feed the range in ~4 s chunks, converted to the analyzer's format.
    let chunk = AVAudioFrameCount(sampleRate * 4)
    var position = first
    while position < last {
        let count = AVAudioFrameCount(min(AVAudioFramePosition(chunk), last - position))
        guard let buffer = AVAudioPCMBuffer(pcmFormat: source, frameCapacity: count) else { break }
        do {
            try file.read(into: buffer, frameCount: count)
        } catch {
            break
        }
        if buffer.frameLength == 0 { break }
        let ratio = target.sampleRate / source.sampleRate
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * ratio) + 1024
        guard let converted = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else {
            break
        }
        var supplied = false
        var conversionError: NSError?
        converter.convert(to: converted, error: &conversionError) { _, status in
            if supplied {
                status.pointee = .noDataNow
                return nil
            }
            supplied = true
            status.pointee = .haveData
            return buffer
        }
        if conversionError != nil { break }
        input.yield(AnalyzerInput(buffer: converted))
        position += AVAudioFramePosition(buffer.frameLength)
    }
    input.finish()
    do {
        try await analyzer.finalizeAndFinishThroughEndOfInput()
    } catch {
        fail("Could not finish transcription: \(error.localizedDescription)")
    }
    _ = await collector.value
}

let args = CommandLine.arguments
guard args.count >= 3 else { fail("usage: ba-transcribe status|install|transcribe …") }
switch args[1] {
case "status":
    await status(args[2])
case "install":
    await install(args[2])
case "transcribe":
    guard args.count == 6, let start = Double(args[3]), let end = Double(args[4]) else {
        fail("usage: ba-transcribe transcribe <audio> <start> <end> <locale>")
    }
    await transcribe(path: args[2], start: start, end: end, identifier: args[5])
default:
    fail("unknown command \(args[1])")
}
