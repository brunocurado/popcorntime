// pt-translate: on-device subtitle translation with Apple's Translation
// framework (macOS 26+). No network, no key.
//
// stdin : {"source":"en","target":"pt-PT","texts":["…", …]}
// stdout: {"texts":["…", …],"status":"installed"}   (same order and length)
//      or {"error":"…","status":"supported|unsupported|installed"}
//
// The system service translates about one line every 0.75 s; several
// sessions in parallel were measured to be no faster, so one is used.
import Foundation
import Translation

struct Input: Decodable {
  let source: String
  let target: String
  let texts: [String]
}

struct Output: Encodable {
  var texts: [String]? = nil
  var error: String? = nil
  var status: String? = nil
}

func emit(_ o: Output) {
  let data = try! JSONEncoder().encode(o)
  FileHandle.standardOutput.write(data)
  FileHandle.standardOutput.write("\n".data(using: .utf8)!)
}

@main
struct PtTranslate {
  static func main() async {
    let raw = FileHandle.standardInput.readDataToEndOfFile()
    guard let input = try? JSONDecoder().decode(Input.self, from: raw) else {
      emit(Output(error: "bad input json"))
      exit(2)
    }
    let src = Locale.Language(identifier: input.source)
    let dst = Locale.Language(identifier: input.target)
    let status = await LanguageAvailability().status(from: src, to: dst)
    let statusName: String
    switch status {
    case .installed: statusName = "installed"
    case .supported: statusName = "supported"
    case .unsupported: statusName = "unsupported"
    @unknown default: statusName = "unknown"
    }
    guard status == .installed else {
      emit(Output(error: "language pack not installed", status: statusName))
      exit(3)
    }
    if input.texts.isEmpty {
      emit(Output(texts: [], status: statusName))
      return
    }
    do {
      let session = TranslationSession(installedSource: src, target: dst)
      let reqs = input.texts.enumerated().map {
        TranslationSession.Request(sourceText: $0.element, clientIdentifier: String($0.offset))
      }
      var out = Array(repeating: "", count: input.texts.count)
      for r in try await session.translations(from: reqs) {
        if let id = r.clientIdentifier, let i = Int(id), i < out.count { out[i] = r.targetText }
      }
      emit(Output(texts: out, status: statusName))
    } catch {
      emit(Output(error: "\(error)", status: statusName))
      exit(4)
    }
  }
}
