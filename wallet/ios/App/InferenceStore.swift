import Foundation
import SwiftUI
import Core

/// One message in the inference conversation. Assistant replies carry the actual
/// token usage reported by the gateway; a placeholder reply is marked `thinking`
/// until the run completes.
struct ChatMessage: Identifiable, Codable {
    enum Role: String, Codable { case user, assistant }
    var id = UUID()
    let role: Role
    var text: String
    var thinking: Bool = false
    var isError: Bool = false
    var usage: TokenUsage?
    /// Display name of the model that produced this reply (distributed across the
    /// network) — shown so the requester knows which model answered.
    var model: String?
}

/// Drives the chat-style inference screen: each send runs quote → pay KVR
/// on-chain → inference, replacing an animated placeholder with the reply.
/// A model the picker can offer: a networked gateway model (pay-per-message) or a
/// GGUF downloaded to this phone, run on-device for free.
struct InferenceOption: Identifiable, Hashable {
    let id: String        // gateway model id, or "local:<filename>"
    let name: String
    let isLocal: Bool
}

@MainActor
final class InferenceStore: ObservableObject {
    @Published var models: [PayModel] = []
    @Published var localModels: [LocalModel] = []
    @Published var selectedModel: String = ""
    @Published var messages: [ChatMessage] = []
    @Published var busy = false
    @Published var loadError: String?

    // Prepaid-credit state: networked inference streams over /v1/chat/completions
    // (credit-debited) instead of on-chain pay-per-message, so slow models stream
    // past Cloudflare's 100s timeout. The wallet self-registers + mints an API key
    // lazily on first use; credits are topped up by an on-chain KVR deposit.
    @Published var creditBalance: Double?
    @Published var depositRecipient: String = ""
    @Published var toppingUp = false

    /// Everything selectable: on-device models first (free), then gateway models.
    var options: [InferenceOption] {
        localModels.map { InferenceOption(id: "local:\($0.name)", name: $0.displayName, isLocal: true) }
            + models.map { InferenceOption(id: $0.id, name: $0.name, isLocal: false) }
    }
    var selectedIsLocal: Bool { selectedModel.hasPrefix("local:") }

    private unowned let wallet: WalletStore
    private let service: GatewayService?
    private let credit: CreditService

    init(wallet: WalletStore) {
        self.wallet = wallet
        self.service = GatewayService(baseURL: wallet.stakingServiceURL)
        self.credit = CreditService(baseUrl: wallet.stakingServiceURL)
        self.messages = Self.loadHistory()
    }

    // MARK: - Credit account (prepaid)

    /// The stored API key for this wallet, if any (re-mintable).
    private var apiKey: String? {
        guard let w = wallet.address else { return nil }
        return KeyStore.loadApiKey(wallet: w)
    }

    /// Ensure the wallet has a credit API key, self-registering and minting one on
    /// first use (both steps sign a gateway nonce with the wallet key). Returns the
    /// key. Requires the wallet to be unlockable (mnemonic reveal).
    private func ensureApiKey() async throws -> String {
        if let k = apiKey { return k }
        return try await mintApiKey()
    }

    /// Register (idempotent) and mint a key, replacing whatever is stored.
    private func mintApiKey() async throws -> String {
        guard let w = wallet.address else { throw CreditError.message(Localizer.shared.t("nodeSettings.walletLocked")) }
        guard let phrase = wallet.revealMnemonic() else { throw CreditError.message(Localizer.shared.t("nodeSettings.walletLocked")) }
        try await credit.register(mnemonic: phrase)                       // self-whitelist (idempotent)
        let key = try await credit.mintApiKey(mnemonic: phrase, label: "Kvasir iOS")
        try? KeyStore.saveApiKey(key, wallet: w)
        return key
    }

    /// Discard the stored key and mint a new one.
    ///
    /// Only the key's hash is kept by the gateway, so a key it no longer
    /// recognises cannot be repaired by asking for it back — it has to be
    /// reminted. Without this, "invalid API key" has no way out from inside the
    /// app. Credit balance is held against the wallet, not the key, so nothing
    /// is lost by reissuing.
    @discardableResult
    public func reissueApiKey() async throws -> String {
        if let w = wallet.address { KeyStore.deleteApiKey(wallet: w) }
        let key = try await mintApiKey()
        await refreshBalance()
        return key
    }

    /// Refresh the credit balance shown in the UI (best-effort).
    func refreshBalance() async {
        guard let key = apiKey else { creditBalance = nil; return }
        creditBalance = try? await credit.balance(apiKey: key).balance
    }

    /// Top up credits by transferring KVR on-chain to the gateway vault, then
    /// proving the transfer. Returns a user-facing status message.
    func topUp(amount: Double) async -> String {
        guard amount > 0 else { return Localizer.shared.t("inference.topUpInvalid") }
        guard let w = wallet.address else { return Localizer.shared.t("nodeSettings.walletLocked") }
        let recipient = depositRecipient
        guard !recipient.isEmpty else { return Localizer.shared.t("inference.topUpNoVault") }
        toppingUp = true; defer { toppingUp = false }
        do {
            _ = try await ensureApiKey()   // must be whitelisted before deposit is accepted
            let sig = try await wallet.sendToken(to: recipient, amount: amount)
            let bal = try await credit.deposit(wallet: w, amount: amount, signature: sig)
            creditBalance = bal
            await wallet.refresh()
            return Localizer.shared.t("inference.topUpOk")
        } catch {
            return "\(Localizer.shared.t("inference.topUpFailed")): \(error)"
        }
    }

    // MARK: - Persistence
    //
    // The conversation is saved locally so it survives app relaunches. Transient
    // "thinking" placeholders and error bubbles are dropped; the tail is capped.
    private static let historyKey = "inference.history"
    private static let historyMax = 100

    private static func loadHistory() -> [ChatMessage] {
        guard let data = UserDefaults.standard.data(forKey: historyKey),
              let msgs = try? JSONDecoder().decode([ChatMessage].self, from: data) else { return [] }
        return msgs.filter { !$0.thinking && !$0.isError }
    }

    private func saveHistory() {
        let persistable = Array(messages.filter { !$0.thinking && !$0.isError }.suffix(Self.historyMax))
        if let data = try? JSONEncoder().encode(persistable) {
            UserDefaults.standard.set(data, forKey: Self.historyKey)
        }
    }

    /// Clear the saved conversation.
    func clearHistory() {
        messages = []
        UserDefaults.standard.removeObject(forKey: Self.historyKey)
    }

    func loadModels() async {
        // On-device models first: these work with no network and no payment.
        ModelStore.shared.refresh()
        localModels = ModelStore.shared.models
        guard let service else {
            if selectedModel.isEmpty { selectedModel = options.first?.id ?? "" }
            loadError = localModels.isEmpty ? Localizer.shared.t("error.setServiceUrl") : nil
            return
        }
        do {
            let m = try await service.models()
            models = m.models
            depositRecipient = m.recipient   // gateway vault for credit top-ups
            loadError = nil
        } catch {
            loadError = localModels.isEmpty ? String(describing: error) : nil
        }
        if selectedModel.isEmpty { selectedModel = options.first?.id ?? "" }
        await refreshBalance()
    }

    /// Append the user's message and an animated placeholder, then stream the reply
    /// over the credit-billed `/v1/chat/completions` endpoint, appending tokens as
    /// they arrive. Streaming is what lets slow models (M3) run past Cloudflare's
    /// 100s origin timeout — the connection never idles while chunks flow.
    func send(_ raw: String) async {
        let prompt = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.isEmpty, !selectedModel.isEmpty, !busy else { return }
        if selectedIsLocal { await sendLocal(prompt); return }
        busy = true; defer { busy = false }

        // Resolve the selected model's display name up front so the reply can show it.
        let modelName = models.first(where: { $0.id == selectedModel })?.name ?? selectedModel

        messages.append(ChatMessage(role: .user, text: prompt))
        let placeholder = ChatMessage(role: .assistant, text: "", thinking: true, model: modelName)
        messages.append(placeholder)
        let pid = placeholder.id
        func update(_ change: (inout ChatMessage) -> Void) {
            guard let idx = messages.firstIndex(where: { $0.id == pid }) else { return }
            change(&messages[idx])
        }

        do {
            let key = try await ensureApiKey()
            let convo = conversationForAPI()   // prior turns + this prompt, OpenAI shape
            for try await ev in credit.streamChat(apiKey: key, model: selectedModel, messages: convo) {
                switch ev {
                case .token(let t): update { m in m.thinking = false; m.text += t }
                case .reasoning: break   // thinking is suppressed server-side; ignore any leak
                case .usage(let u): update { m in m.usage = u }
                }
            }
            update { m in m.thinking = false }
            await refreshBalance()
        } catch {
            update { m in
                m.thinking = false
                m.isError = true
                m.text = String(describing: error)
            }
        }
        saveHistory()
    }

    /// The visible conversation as OpenAI messages (drops placeholders/errors, caps
    /// the tail so context stays bounded). Includes the just-appended user prompt.
    private func conversationForAPI() -> [[String: String]] {
        messages
            .filter { !$0.thinking && !$0.isError && !$0.text.isEmpty }
            .suffix(20)
            .map { ["role": $0.role.rawValue, "content": $0.text] }
    }

    /// On-device inference over a downloaded GGUF: no payment, tokens streamed
    /// straight from the phone's GPU via the embedded llama.cpp.
    private func sendLocal(_ prompt: String) async {
        let fileName = String(selectedModel.dropFirst("local:".count))
        guard let model = ModelStore.shared.models.first(where: { $0.name == fileName }) else { return }
        busy = true; defer { busy = false }

        messages.append(ChatMessage(role: .user, text: prompt))
        let placeholder = ChatMessage(role: .assistant, text: "", thinking: true,
                                      model: model.displayName + " · on-device")
        messages.append(placeholder)
        let pid = placeholder.id

        let ok = await ModelStore.shared.generate(model: model, prompt: prompt) { [weak self] delta in
            guard let self, let idx = self.messages.firstIndex(where: { $0.id == pid }) else { return }
            self.messages[idx].thinking = false
            self.messages[idx].text += delta
        }
        if let idx = messages.firstIndex(where: { $0.id == pid }) {
            messages[idx].thinking = false
            if !ok && messages[idx].text.isEmpty {
                messages[idx].isError = true
                messages[idx].text = Localizer.shared.t("models.loadFailed")
            }
        }
        saveHistory()
    }
}
