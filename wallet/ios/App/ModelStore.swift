import Foundation
import SwiftUI

/// A GGUF the hub has staged onto this device (Documents/models), usable for
/// on-device inference without the network.
struct LocalModel: Identifiable, Hashable {
    var id: String { name }
    let name: String
    let sizeBytes: Int64
    var sizeLabel: String {
        ByteCountFormatter.string(fromByteCount: sizeBytes, countStyle: .file)
    }
    /// A friendlier label: strip the .gguf suffix and quant tail noise.
    var displayName: String {
        (name as NSString).deletingPathExtension
    }
}

/// Lists, deletes, and runs on-device inference over the phone's downloaded
/// models. Shared with AgentControlServer's models directory.
@MainActor
final class ModelStore: ObservableObject {
    static let shared = ModelStore()

    @Published private(set) var models: [LocalModel] = []
    @Published var generating = false

    var dir: URL { AgentControlServer.modelsDir }

    func refresh() {
        migrateShardArtifacts()
        let fm = FileManager.default
        let items = (try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: [.fileSizeKey]))
            ?? []
        models = items
            .filter { $0.pathExtension == "gguf" }
            .map { url in
                let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
                return LocalModel(name: url.lastPathComponent, sizeBytes: Int64(size))
            }
            .sorted { $0.name < $1.name }
    }

    /// Relocate node-serving expert slices left in the top-level models dir (from
    /// before they were routed to shards/) so they stop appearing as pickable
    /// on-device models. A slice carries a `.L<layer>_e<a>-<b>.gguf` suffix — never
    /// a real model — so this is safe. Idempotent; runs once artifacts remain.
    private func migrateShardArtifacts() {
        let fm = FileManager.default
        guard let items = try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil) else { return }
        let slice = try? NSRegularExpression(pattern: #"\.L\d+_e\d+-\d+\.gguf$"#)
        let shards = AgentControlServer.shardsDir
        for url in items where url.pathExtension == "gguf" {
            let name = url.lastPathComponent
            let range = NSRange(name.startIndex..., in: name)
            if slice?.firstMatch(in: name, range: range) != nil {
                try? fm.moveItem(at: url, to: shards.appendingPathComponent(name))
            }
        }
    }

    func delete(_ model: LocalModel) {
        try? FileManager.default.removeItem(at: dir.appendingPathComponent(model.name))
        refresh()
    }

    /// Run on-device inference, streaming tokens to `onDelta`. Wraps the prompt in
    /// a minimal ChatML turn (matches the ring/gateway convention) so instruct
    /// models answer rather than continue.
    func generate(model: LocalModel, prompt: String,
                  onDelta: @escaping (String) -> Void) async -> Bool {
        generating = true
        defer { generating = false }
        let path = dir.appendingPathComponent(model.name).path
        let wrapped = "<|im_start|>user\n\(prompt)<|im_end|>\n<|im_start|>assistant\n"
        return await withCheckedContinuation { cont in
            DispatchQueue.global(qos: .userInitiated).async {
                let box = TokenSink(onDelta: onDelta)
                let ok = kvasir_local_generate(path, wrapped, 2048, 512, { piece, ctx in
                    guard let ctx, let piece else { return true }
                    let sink = Unmanaged<TokenSink>.fromOpaque(ctx).takeUnretainedValue()
                    let s = String(cString: piece)
                    DispatchQueue.main.async { sink.onDelta(s) }
                    return true
                }, Unmanaged.passUnretained(box).toOpaque())
                _ = box  // keep alive across the C call
                cont.resume(returning: ok)
            }
        }
    }
}

private final class TokenSink {
    let onDelta: (String) -> Void
    init(onDelta: @escaping (String) -> Void) { self.onDelta = onDelta }
}
