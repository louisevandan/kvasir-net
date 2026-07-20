import SwiftUI

/// The nine languages the app supports. Endonyms are shown in the picker regardless
/// of the active language so users of any tongue can find their own.
enum AppLanguage: String, CaseIterable, Identifiable {
    case ko
    case en
    case zh
    case es
    case ja
    case fr
    case de
    case nl
    case indonesian = "id"   // case renamed to avoid clashing with Identifiable's `id`

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .ko: return "한국어"
        case .en: return "English"
        case .zh: return "中文"
        case .es: return "Español"
        case .ja: return "日本語"
        case .fr: return "Français"
        case .de: return "Deutsch"
        case .nl: return "Nederlands"
        case .indonesian: return "Bahasa Indonesia"
        }
    }
}

/// Runtime, restart-free i18n. Views observe `Localizer.shared` and call `t(...)`;
/// flipping `lang` re-publishes and every observing view re-renders in the new language.
@MainActor
final class Localizer: ObservableObject {
    static let shared = Localizer()

    private static let storageKey = "app.language"

    @Published var lang: AppLanguage {
        didSet { UserDefaults.standard.set(lang.rawValue, forKey: Self.storageKey) }
    }

    init() {
        if let raw = UserDefaults.standard.string(forKey: Self.storageKey),
           let saved = AppLanguage(rawValue: raw) {
            lang = saved
        } else {
            // First launch: follow the device language if it matches one of the
            // supported codes, otherwise default to English.
            let preferred = (Locale.preferredLanguages.first ?? "en").lowercased()
            lang = AppLanguage.allCases.first { preferred.hasPrefix($0.rawValue) } ?? .en
        }
    }

    /// Localized string for `key`, falling back to English and then the key itself.
    func t(_ key: String) -> String {
        catalog[lang]?[key] ?? catalog[.en]?[key] ?? key
    }

    /// Localized, `String(format:)`-interpolated string. Templates use %@ (strings)
    /// and %d (integers) placeholders.
    func t(_ key: String, _ args: CVarArg...) -> String {
        String(format: t(key), arguments: args)
    }
}

// MARK: - Catalog

private let catalog: [AppLanguage: [String: String]] = [
    .ko: ko, .en: en, .zh: zh, .es: es, .ja: ja,
    .fr: fr, .de: de, .nl: nl, .indonesian: idn,
]

private let ko: [String: String] = [
    // common
    "common.close": "닫기",
    "common.save": "저장",
    "common.settings": "설정",
    "common.amount": "수량",
    "common.receive": "수신",
    "common.send": "송금",
    "common.copyAddress": "주소 복사",
    "common.copied": "복사됨!",

    // menu
    "menu.network": "네트워크",
    "menu.refresh": "새로고침",
    "menu.deleteWallet": "지갑 삭제",
    "menu.language": "언어 / Language",

    // home
    "home.title": "지갑",
    "home.balance": "잔액",
    "home.tokenNotIssued": "%@는 이 네트워크에 미발행",
    "home.stakingTitle": "스테이킹 & 노드 보상",
    "home.stakingSubtitle": "KVR 예치로 보상 받기 · 노드 운영 보상",
    "home.inferenceTitle": "AI 추론",
    "home.inferenceSubtitle": "KVR로 결제하고 Kvasir 추론 실행",
    "home.nodeSettingsTitle": "모바일폰 노드 설정",
    "home.nodeSettingsSubtitle": "이 기기를 추론 노드로 · MLX·모드·라이브 게이지",
    "home.nodeMonitorTitle": "노드 성능 · 기여도",
    "home.nodeMonitorSubtitle": "내 노드 성능 등급과 기여도·보상 확인",
    "common.cancel": "취소",
    "models.title": "모델 관리",
    "models.subtitle": "다운로드한 모델 보기 · 삭제",
    "models.storedTitle": "저장된 모델",
    "models.storedDesc": "허브가 이 기기에 내려받은 모델입니다. AI 추론에서 온디바이스로 사용할 수 있습니다.",
    "models.delete": "삭제",
    "models.deleteConfirm": "이 모델을 삭제할까요?",
    "models.emptyTitle": "다운로드한 모델 없음",
    "models.emptyHint": "노드로 참여해 분산 추론을 실행하면 허브가 이 기기로 모델을 내려받습니다.",
    "models.onDevice": "온디바이스",
    "models.loadFailed": "모델을 불러오지 못했습니다.",
    "home.history": "거래 내역",
    "home.noTransactions": "아직 거래가 없습니다",

    // onboarding
    "onboarding.subtitle": "Solana 기반 KVR 토큰 지갑 · devnet",
    "onboarding.create": "새 지갑 만들기",
    "onboarding.import": "기존 지갑 복구",

    // create wallet
    "create.title": "새 지갑",
    "create.phraseHeader": "복구 문구 (12단어)",
    "create.phraseWarning": "지갑을 복구하는 유일한 방법입니다. 안전한 곳에 적어 두세요. 절대 공유하지 마세요.",
    "create.savedToggle": "복구 문구를 안전하게 저장했습니다",
    "create.start": "지갑 시작",

    // import wallet
    "import.title": "지갑 복구",
    "import.phraseHeader": "복구 문구",
    "import.instruction": "12 또는 24단어를 공백으로 구분해 입력하세요.",
    "import.restore": "복구",
    "import.invalidPhrase": "복구 문구가 올바르지 않습니다.",

    // send
    "send.asset": "자산",
    "send.balance": "잔액 %@ %@",
    "send.recipient": "받는 주소",
    "send.solanaAddress": "Solana 주소",
    "send.sent": "전송 완료",
    "send.viewInExplorer": "explorer에서 보기",
    "send.invalidAmount": "수량이 올바르지 않습니다.",

    // staking
    "staking.title": "스테이킹",
    "staking.stake": "스테이킹",
    "staking.needOnline": "이 기기를 노드로 등록하고 온라인 상태여야 스테이킹할 수 있습니다.",
    "staking.guideLink": "스테이킹 방법 안내",
    "staking.recentTx": "최근 트랜잭션 explorer에서 보기",
    "staking.staked": "예치",
    "staking.reward": "보상",
    "staking.unstakeAll": "전체 언스테이크",
    "staking.nodeOperatorRewards": "노드 운영자 보상",
    "staking.claimableRewards": "청구 가능 보상",
    "staking.nodeIdPlaceholder": "노드 ID (예: node-slot-1)",
    "staking.register": "등록",
    "staking.claim": "보상 청구",
    "staking.nodeStatusLink": "노드 운영 현황 보기",
    "staking.serviceUrlTitle": "스테이킹 서비스 URL",
    "staking.serviceUrlDesc": "정산 백엔드 주소입니다. 같은 네트워크의 Mac IP로 설정하세요.",

    // staking guide
    "guide.navTitle": "스테이킹 안내",
    "guide.headerTitle": "KVR 스테이킹 가이드",
    "guide.headerSubtitle": "예치로 보상을 받고, 노드 운영으로 추가 보상을 얻는 방법",
    "guide.whatTitle": "스테이킹이란?",
    "guide.whatBody": "보유한 KVR를 예치하면 연 이율(APR)만큼 보상이 실시간으로 쌓입니다. 언제든 언스테이크로 원금과 보상을 함께 돌려받을 수 있습니다.",
    "guide.howTitle": "스테이킹 방법",
    "guide.howStep1": "홈에서 네트워크가 **Devnet**인지 확인하세요. 스테이킹은 devnet에서 동작합니다.",
    "guide.howStep2": "스테이킹 화면에서 예치할 **KVR 수량**을 입력합니다.",
    "guide.howStep3": "**스테이킹** 버튼을 누르면 지갑이 KVR를 스테이킹 볼트로 전송합니다. (전송 수수료용 SOL 소량 필요)",
    "guide.howStep4": "예치가 등록되고 **APR**에 따라 보상이 실시간 적립됩니다.",
    "guide.howStep5": "**전체 언스테이크**를 누르면 원금 + 적립 보상이 지갑으로 반환됩니다.",
    "guide.nodeStep1": "스테이킹 화면에서 **노드 ID**를 등록해 내 지갑에 연결합니다.",
    "guide.nodeStep2": "Kvasir 허브가 노드의 추론 기여도를 보고하면 보상이 적립됩니다.",
    "guide.nodeStep3": "**노드 운영 현황**에서 상태·기여·보상을 모니터링합니다.",
    "guide.nodeStep4": "**보상 청구**로 적립된 보상을 지갑으로 받습니다.",
    "guide.cautionTitle": "주의사항",
    "guide.caution1": "현재 **devnet 테스트** 단계입니다. 실제 자산이 아닙니다.",
    "guide.caution2": "오프체인 정산 방식이라 예치금은 정산 볼트가 보관합니다. (추후 온체인 프로그램으로 전환)",
    "guide.caution3": "전송에는 **가스용 SOL**이 소량 필요합니다.",
    "guide.caution4": "정산 서버 주소는 스테이킹 화면 우측 상단 ⚙️에서 설정합니다.",

    // node monitor
    "monitor.navTitle": "노드 운영 현황",
    "monitor.connectDevice": "기기 연결",
    "monitor.nodes": "노드",
    "monitor.online": "온라인",
    "monitor.hubTitle": "허브 연결",
    "monitor.hubServing": "서빙 중",
    "monitor.hubConnectedIdle": "연결됨 · 대기",
    "monitor.hubDisconnected": "연결 끊김",
    "monitor.hubNone": "연결된 허브 없음 — 노드 설정에서 연결하세요",
    "monitor.rawContribUnit": "원 기여(unit)",
    "monitor.effectiveWeighted": "유효 기여(성능가중)",
    "monitor.lifetimeRewards": "누적 보상(KVR)",
    "monitor.claimableKVR": "청구 가능(KVR)",
    "monitor.tierTitle": "성능 기반 기여도 재산정",
    "monitor.tierDesc": "측정 처리량(tok/s)으로 등급을 매겨 보상 배수를 적용합니다. 빠른 노드일수록 같은 작업에 더 많은 보상.",
    "monitor.perfTier": "성능 등급 %@ · ×%@",
    "monitor.rawContribInline": "원 기여 %@",
    "monitor.effectiveInline": "→ 유효 %@",
    "monitor.effectiveContrib": "유효 기여",
    "monitor.claimable": "청구 가능",
    "monitor.received": "받은 보상",
    "monitor.lastReport": "마지막 보고 ",
    "monitor.noReport": "아직 기여 보고 없음",
    "monitor.noPerfData": "성능 데이터 없음",
    "monitor.emptyTitle": "연결된 기기가 없습니다",
    "monitor.emptyDesc": "우측 상단 ⊕(기기 연결)에서 이 기기 또는 다른 기기를 연결하세요.",

    // node card actions
    "node.remove": "제거",
    "node.removeConfirm": "이 노드를 제거할까요?",
    "node.openSettings": "노드 설정 열기",

    // node mode labels
    "mode.localShard": "로컬 샤드",
    "mode.rpcWorker": "RPC 워커",

    // node status pills
    "status.online": "온라인",
    "status.idle": "유휴",
    "status.registered": "등록됨",
    "status.offline": "오프라인",

    // device connect
    "device.iosDevice": "iOS 기기",
    "device.accountAddressPlaceholder": "<계정주소>",
    "device.navTitle": "계정 · 기기 연결",
    "device.myAccount": "내 계정",
    "device.myAccountDesc": "계정은 곧 내 지갑 주소입니다. 이 주소로 여러 기기를 연결하세요.",
    "device.connectThis": "이 기기 연결",
    "device.connectThisDesc": "이 %@를 계정에 노드로 연결합니다. (OS: iOS · NPU)",
    "device.connectOther": "다른 기기 연결",
    "device.connectOtherDesc": "Node.js가 설치된 데스크탑/랩탑(macOS·Windows·Linux)에서 solana/node-client의 아래 명령을 실행하세요.",
    "device.copyCommand": "명령 복사",

    // node settings
    "nodeSettings.backendTitle": "컴퓨트 백엔드",
    "nodeSettings.backendDesc": "이 기기가 추론에 사용할 연산 유닛 · iOS GPU는 MLX(Metal)",
    "nodeSettings.modeTitle": "노드 모드",
    "nodeSettings.modeLocalTitle": "로컬 샤드 (권장)",
    "nodeSettings.modeLocalDesc": "레이어 샤드를 기기에서 로컬 실행, activation만 릴레이 — 가장 빠름",
    "nodeSettings.modeRpcDesc": "허브가 텐서 연산을 원격 실행 — 단순하나 네트워크 지연으로 느림",
    "nodeSettings.resourceTitle": "예상 리소스 영향",
    "nodeSettings.tokPerSecCaption": "tok/s (0.5B Q8, 추정)",
    "nodeSettings.memImpact": "메모리 점유",
    "nodeSettings.thermal": "발열",
    "nodeSettings.performance": "성능",
    "nodeSettings.pausedOffCharge": "충전 대기 중 — 케이블을 연결하면 노드가 다시 참여합니다",
    "nodeSettings.hubConnectTitle": "허브 연결",
    "nodeSettings.hubConnectDesc": "원격 허브에 지갑 서명으로 노드 토큰을 발급받아 폴링 등록 (OTP 불필요)",
    "nodeSettings.hubUrl": "허브 URL",
    "nodeSettings.hubConnectBtn": "지갑으로 연결",
    "nodeSettings.hubConnecting": "연결 중…",
    "nodeSettings.hubSigning": "지갑 서명 중…",
    "nodeSettings.hubConnected": "연결됨 · 노드 토큰 등록",
    "nodeSettings.hubConnectFailed": "실패",
    "nodeSettings.hubUrlMissing": "허브 URL을 입력하세요",
    "nodeSettings.walletLocked": "지갑 잠금 해제 필요",
    "nodeSettings.enableLiveFirst": "노드를 먼저 켜세요 (라이브)",
    "nodeSettings.screenOnHint": "충전 중이면 화면을 꺼도 iOS가 허용하는 범위에서 참여를 이어갑니다. 안정적 참여는 화면 켜짐 상태가 가장 좋습니다.",
    "nodeSettings.chargingOnly": "충전 중에만 참여",
    "nodeSettings.runLive": "노드 구동 (라이브)",
    "nodeSettings.liveGauges": "라이브 게이지",
    "nodeSettings.charging": "충전 중",
    "nodeSettings.battery": "배터리",
    "nodeSettings.cpuLoad": "CPU 부하",
    "nodeSettings.thermalState": "발열 상태",
    "nodeSettings.estimated": "추정",
    "nodeSettings.gaugeNote": "RAM·CPU·발열은 실측, GPU는 측정 기반 추정입니다.",

    // genesis gateway
    "genesis.title": "게네시스 게이트웨이",
    "genesis.desc": "네트워크 조정 노드입니다. 정산·추론·노드 보상이 이 게이트웨이를 거칩니다.",
    "genesis.reachable": "연결됨",
    "genesis.unreachable": "연결 불가",
    "genesis.active": "활성 게이트웨이",
    "genesis.advertised": "게이트웨이가 알리는 공개 URL",
    "genesis.cluster": "클러스터",
    "genesis.rewardPerUnit": "유닛당 보상",
    "genesis.gatewayBonus": "게이트웨이 호스트 보너스",
    "inference.clear": "대화 지우기",
    "inference.credits": "크레딧",
    "inference.currentBalance": "현재 잔액",
    "inference.topUpTitle": "크레딧 충전",
    "inference.topUpNote": "KVR을 게이트웨이에 예치하면 크레딧으로 적립되어 추론에 사용됩니다. 스트리밍이라 느린 모델도 끊기지 않습니다.",
    "inference.topUpAmount": "충전 금액 (KVR)",
    "inference.topUpConfirm": "KVR 예치 · 크레딧 충전",
    "inference.topUpOk": "충전 완료",
    "inference.topUpFailed": "충전 실패",
    "inference.topUpInvalid": "금액을 올바르게 입력하세요",
    "inference.topUpNoVault": "게이트웨이 볼트 주소를 불러오지 못했습니다",
    "home.more": "더보기",
    "history.title": "전체 거래내역",

    // thermal words
    "thermal.low": "낮음",
    "thermal.medium": "보통",
    "thermal.high": "높음",
    "thermal.critical": "위험",

    // node profile notes
    "nodeProfile.mlxNote": "MLX Metal 커널로 Apple GPU/ANE 활용. 로컬 샤드에서 최고 성능.",
    "nodeProfile.cpuNote": "소형모델 디코드 안정적. 지속 부하 시 발열 큼.",

    // inference
    "inference.paymentTx": "결제 트랜잭션 explorer에서 보기",
    "inference.model": "모델",
    "inference.loadingModels": "모델을 불러오는 중…",
    "inference.prompt": "프롬프트",
    "inference.estQuote": "예상 견적",
    "inference.estTokens": "예상 토큰 %d (프롬프트 %d + 생성 ~%d)",
    "inference.payAndRun": "결제하고 실행 (~%@ %@)",
    "inference.actualBillingNote": "실제 청구는 실행 후 사용된 토큰 기준으로 아래에 기록됩니다.",
    "inference.result": "결과",
    "inference.actualTokens": "실제 사용 토큰",
    "inference.totalTok": "총 %d tok",
    "inference.usageRow": "프롬프트 %d · 생성 %d · 합계 %d tok",
    "inference.thinking": "생각 중",
    "inference.messagePlaceholder": "메시지를 입력하세요…",
    "inference.send": "전송",
    "inference.emptyNote": "KVR로 결제하고 Kvasir 추론을 실행합니다. 응답은 실제 사용 토큰 기준으로 청구됩니다.",

    // export recovery phrase
    "export.title": "복구 문구 보기",
    "export.desc": "이 12/24단어로 다른 기기(iOS·Android·데스크톱)에서 같은 계정을 복구합니다.",
    "export.reveal": "복구 문구 보기",
    "export.hide": "숨기기",
    "export.copy": "문구 복사",
    "export.warn": "⚠️ 아무도 보지 않는 곳에서 확인하세요. 이 문구를 아는 사람은 자산을 모두 가져갈 수 있습니다. 절대 공유·촬영하지 마세요.",
    "export.denied": "생체 인증에 실패했습니다. 다시 시도하세요.",

    // errors / status
    "error.setStakingUrl": "스테이킹 서비스 URL을 설정하세요.",
    "error.setServiceUrl": "서비스 URL을 설정하세요.",
]

private let en: [String: String] = [
    // common
    "common.close": "Close",
    "common.save": "Save",
    "common.settings": "Settings",
    "common.amount": "Amount",
    "common.receive": "Receive",
    "common.send": "Send",
    "common.copyAddress": "Copy address",
    "common.copied": "Copied!",

    // menu
    "menu.network": "Network",
    "menu.refresh": "Refresh",
    "menu.deleteWallet": "Delete wallet",
    "menu.language": "언어 / Language",

    // home
    "home.title": "Wallet",
    "home.balance": "Balance",
    "home.tokenNotIssued": "%@ is not issued on this network",
    "home.stakingTitle": "Staking & node rewards",
    "home.stakingSubtitle": "Earn rewards by staking KVR · node operator rewards",
    "home.inferenceTitle": "AI inference",
    "home.inferenceSubtitle": "Pay with KVR to run Kvasir inference",
    "home.nodeSettingsTitle": "Mobile node settings",
    "home.nodeSettingsSubtitle": "Turn this device into an inference node · MLX · mode · live gauges",
    "home.nodeMonitorTitle": "Node performance · contribution",
    "home.nodeMonitorSubtitle": "Check your node's performance tier, contribution, and rewards",
    "common.cancel": "Cancel",
    "models.title": "Models",
    "models.subtitle": "View and delete downloaded models",
    "models.storedTitle": "Downloaded models",
    "models.storedDesc": "Models the hub has downloaded to this device. Usable on-device in AI inference.",
    "models.delete": "Delete",
    "models.deleteConfirm": "Delete this model?",
    "models.emptyTitle": "No downloaded models",
    "models.emptyHint": "When you join as a node and run distributed inference, the hub downloads models to this device.",
    "models.onDevice": "on-device",
    "models.loadFailed": "Could not load the model.",
    "home.history": "Transaction history",
    "home.noTransactions": "No transactions yet",

    // onboarding
    "onboarding.subtitle": "Solana-based KVR token wallet · devnet",
    "onboarding.create": "Create new wallet",
    "onboarding.import": "Restore existing wallet",

    // create wallet
    "create.title": "New wallet",
    "create.phraseHeader": "Recovery phrase (12 words)",
    "create.phraseWarning": "This is the only way to recover your wallet. Write it down somewhere safe. Never share it.",
    "create.savedToggle": "I have safely saved my recovery phrase",
    "create.start": "Start wallet",

    // import wallet
    "import.title": "Restore wallet",
    "import.phraseHeader": "Recovery phrase",
    "import.instruction": "Enter 12 or 24 words separated by spaces.",
    "import.restore": "Restore",
    "import.invalidPhrase": "The recovery phrase is invalid.",

    // send
    "send.asset": "Asset",
    "send.balance": "Balance %@ %@",
    "send.recipient": "Recipient address",
    "send.solanaAddress": "Solana address",
    "send.sent": "Sent",
    "send.viewInExplorer": "View in explorer",
    "send.invalidAmount": "The amount is invalid.",

    // staking
    "staking.title": "Staking",
    "staking.stake": "Stake",
    "staking.needOnline": "Staking requires this device registered as a node and online.",
    "staking.guideLink": "How to stake",
    "staking.recentTx": "View latest transaction in explorer",
    "staking.staked": "Staked",
    "staking.reward": "Reward",
    "staking.unstakeAll": "Unstake all",
    "staking.nodeOperatorRewards": "Node operator rewards",
    "staking.claimableRewards": "Claimable rewards",
    "staking.nodeIdPlaceholder": "Node ID (e.g. node-slot-1)",
    "staking.register": "Register",
    "staking.claim": "Claim rewards",
    "staking.nodeStatusLink": "View node operations",
    "staking.serviceUrlTitle": "Staking service URL",
    "staking.serviceUrlDesc": "The settlement backend address. Set it to your Mac's IP on the same network.",

    // staking guide
    "guide.navTitle": "Staking guide",
    "guide.headerTitle": "KVR staking guide",
    "guide.headerSubtitle": "How to earn rewards by staking and extra rewards by running a node",
    "guide.whatTitle": "What is staking?",
    "guide.whatBody": "When you stake your KVR, rewards accrue in real time at the annual rate (APR). You can unstake at any time to withdraw your principal and rewards together.",
    "guide.howTitle": "How to stake",
    "guide.howStep1": "On the home screen, make sure the network is **Devnet**. Staking runs on devnet.",
    "guide.howStep2": "On the staking screen, enter the **amount of KVR** to stake.",
    "guide.howStep3": "Tap **Stake** and your wallet sends the KVR to the staking vault. (A small amount of SOL is needed for the transfer fee.)",
    "guide.howStep4": "Your stake is registered and rewards accrue in real time based on the **APR**.",
    "guide.howStep5": "Tap **Unstake all** to return your principal plus accrued rewards to your wallet.",
    "guide.nodeStep1": "On the staking screen, register your **node ID** to link it to your wallet.",
    "guide.nodeStep2": "When the Kvasir hub reports your node's inference contribution, rewards accrue.",
    "guide.nodeStep3": "Monitor status, contribution, and rewards in **Node operations**.",
    "guide.nodeStep4": "Use **Claim rewards** to receive accrued rewards into your wallet.",
    "guide.cautionTitle": "Important notes",
    "guide.caution1": "This is currently in **devnet testing**. These are not real assets.",
    "guide.caution2": "Because settlement is off-chain, deposits are held by the settlement vault. (Will migrate to an on-chain program later.)",
    "guide.caution3": "Transfers require a small amount of **SOL for gas**.",
    "guide.caution4": "Set the settlement server address via the ⚙️ in the top-right of the staking screen.",

    // node monitor
    "monitor.navTitle": "Node operations",
    "monitor.connectDevice": "Connect device",
    "monitor.nodes": "Nodes",
    "monitor.online": "Online",
    "monitor.hubTitle": "Hub connection",
    "monitor.hubServing": "Serving",
    "monitor.hubConnectedIdle": "Connected · idle",
    "monitor.hubDisconnected": "Disconnected",
    "monitor.hubNone": "No hub connected — connect one in Node settings",
    "monitor.rawContribUnit": "Raw contribution (unit)",
    "monitor.effectiveWeighted": "Effective contribution (perf-weighted)",
    "monitor.lifetimeRewards": "Lifetime rewards (KVR)",
    "monitor.claimableKVR": "Claimable (KVR)",
    "monitor.tierTitle": "Performance-based contribution rescoring",
    "monitor.tierDesc": "Nodes are tiered by measured throughput (tok/s) and a reward multiplier is applied. Faster nodes earn more for the same work.",
    "monitor.perfTier": "Performance tier %@ · ×%@",
    "monitor.rawContribInline": "Raw %@",
    "monitor.effectiveInline": "→ Effective %@",
    "monitor.effectiveContrib": "Effective contribution",
    "monitor.claimable": "Claimable",
    "monitor.received": "Received",
    "monitor.lastReport": "Last report ",
    "monitor.noReport": "No contribution reports yet",
    "monitor.noPerfData": "No performance data",
    "monitor.emptyTitle": "No devices connected",
    "monitor.emptyDesc": "Use ⊕ (Connect device) in the top-right to connect this or another device.",

    // node card actions
    "node.remove": "Remove",
    "node.removeConfirm": "Remove this node?",
    "node.openSettings": "Open node settings",

    // node mode labels
    "mode.localShard": "Local shard",
    "mode.rpcWorker": "RPC worker",

    // node status pills
    "status.online": "Online",
    "status.idle": "Idle",
    "status.registered": "Registered",
    "status.offline": "Offline",

    // device connect
    "device.iosDevice": "iOS device",
    "device.accountAddressPlaceholder": "<account address>",
    "device.navTitle": "Account · Connect device",
    "device.myAccount": "My account",
    "device.myAccountDesc": "Your account is your wallet address. Connect multiple devices to this address.",
    "device.connectThis": "Connect this device",
    "device.connectThisDesc": "Connect this %@ to your account as a node. (OS: iOS · NPU)",
    "device.connectOther": "Connect another device",
    "device.connectOtherDesc": "On a desktop/laptop (macOS · Windows · Linux) with Node.js installed, run the command below from solana/node-client.",
    "device.copyCommand": "Copy command",

    // node settings
    "nodeSettings.backendTitle": "Compute backend",
    "nodeSettings.backendDesc": "The compute unit this device uses for inference · iOS GPU uses MLX (Metal)",
    "nodeSettings.modeTitle": "Node mode",
    "nodeSettings.modeLocalTitle": "Local shard (recommended)",
    "nodeSettings.modeLocalDesc": "Runs layer shards locally on the device and relays only activations — fastest",
    "nodeSettings.modeRpcDesc": "The hub runs tensor ops remotely — simple but slower due to network latency",
    "nodeSettings.resourceTitle": "Estimated resource impact",
    "nodeSettings.tokPerSecCaption": "tok/s (0.5B Q8, estimated)",
    "nodeSettings.memImpact": "Memory usage",
    "nodeSettings.thermal": "Thermal",
    "nodeSettings.performance": "Performance",
    "nodeSettings.pausedOffCharge": "Waiting for power — plug in and the node rejoins",
    "nodeSettings.hubConnectTitle": "Connect to a hub",
    "nodeSettings.hubConnectDesc": "Sign in to a remote hub with your wallet to mint a node token and register for polling (no OTP).",
    "nodeSettings.hubUrl": "Hub URL",
    "nodeSettings.hubConnectBtn": "Connect with wallet",
    "nodeSettings.hubConnecting": "Connecting…",
    "nodeSettings.hubSigning": "Signing with wallet…",
    "nodeSettings.hubConnected": "Connected · node token registered",
    "nodeSettings.hubConnectFailed": "Failed",
    "nodeSettings.hubUrlMissing": "Enter a hub URL",
    "nodeSettings.walletLocked": "Unlock the wallet first",
    "nodeSettings.enableLiveFirst": "Turn the node on first (live)",
    "nodeSettings.screenOnHint": "While charging, iOS grants background windows so participation can continue with the screen off. Screen-on is most reliable.",
    "nodeSettings.chargingOnly": "Participate only while charging",
    "nodeSettings.runLive": "Run node (live)",
    "nodeSettings.liveGauges": "Live gauges",
    "nodeSettings.charging": "Charging",
    "nodeSettings.battery": "Battery",
    "nodeSettings.cpuLoad": "CPU load",
    "nodeSettings.thermalState": "Thermal state",
    "nodeSettings.estimated": "Estimated",
    "nodeSettings.gaugeNote": "RAM, CPU, and thermal are measured; GPU is estimated from measurements.",

    // genesis gateway
    "genesis.title": "Genesis gateway",
    "genesis.desc": "The network coordination node. Settlement, inference, and node rewards all route through this gateway.",
    "genesis.reachable": "Reachable",
    "genesis.unreachable": "Unreachable",
    "genesis.active": "Active gateway",
    "genesis.advertised": "Public URL advertised by the gateway",
    "genesis.cluster": "Cluster",
    "genesis.rewardPerUnit": "Reward per unit",
    "genesis.gatewayBonus": "Gateway host bonus",
    "inference.clear": "Clear chat",
    "inference.credits": "Credits",
    "inference.currentBalance": "Current balance",
    "inference.topUpTitle": "Add credits",
    "inference.topUpNote": "Deposit KVR to the gateway to add credits used for inference. Replies stream, so slow models don't time out.",
    "inference.topUpAmount": "Amount (KVR)",
    "inference.topUpConfirm": "Deposit KVR · add credits",
    "inference.topUpOk": "Credits added",
    "inference.topUpFailed": "Top-up failed",
    "inference.topUpInvalid": "Enter a valid amount",
    "inference.topUpNoVault": "Couldn't load the gateway vault address",
    "home.more": "More",
    "history.title": "Transaction history",

    // thermal words
    "thermal.low": "Low",
    "thermal.medium": "Medium",
    "thermal.high": "High",
    "thermal.critical": "Critical",

    // node profile notes
    "nodeProfile.mlxNote": "Uses MLX Metal kernels to leverage the Apple GPU/ANE. Best performance in local shard mode.",
    "nodeProfile.cpuNote": "Stable decode for small models. Runs hot under sustained load.",

    // inference
    "inference.paymentTx": "View payment transaction in explorer",
    "inference.model": "Model",
    "inference.loadingModels": "Loading models…",
    "inference.prompt": "Prompt",
    "inference.estQuote": "Estimated quote",
    "inference.estTokens": "Est. tokens %d (prompt %d + generation ~%d)",
    "inference.payAndRun": "Pay & run (~%@ %@)",
    "inference.actualBillingNote": "Actual billing is recorded below based on the tokens used after the run.",
    "inference.result": "Result",
    "inference.actualTokens": "Actual tokens used",
    "inference.totalTok": "Total %d tok",
    "inference.usageRow": "Prompt %d · gen. %d · total %d tok",
    "inference.thinking": "Thinking",
    "inference.messagePlaceholder": "Type a message…",
    "inference.send": "Send",
    "inference.emptyNote": "Pay in KVR and run Kvasir inference. Each response is billed by the tokens actually used.",

    // export recovery phrase
    "export.title": "Export recovery phrase",
    "export.desc": "Restore the same account on another device (iOS · Android · desktop) with these 12/24 words.",
    "export.reveal": "Reveal recovery phrase",
    "export.hide": "Hide",
    "export.copy": "Copy phrase",
    "export.warn": "⚠️ View this where no one can see. Anyone with this phrase can take all your assets. Never share or photograph it.",
    "export.denied": "Biometric authentication failed. Please try again.",

    // errors / status
    "error.setStakingUrl": "Please set the staking service URL.",
    "error.setServiceUrl": "Please set the service URL.",
]

private let zh: [String: String] = [
    // common
    "common.close": "关闭",
    "common.save": "保存",
    "common.settings": "设置",
    "common.amount": "数量",
    "common.receive": "接收",
    "common.send": "转账",
    "common.copyAddress": "复制地址",
    "common.copied": "已复制！",

    // menu
    "menu.network": "网络",
    "menu.refresh": "刷新",
    "menu.deleteWallet": "删除钱包",
    "menu.language": "언어 / Language",

    // home
    "home.title": "钱包",
    "home.balance": "余额",
    "home.tokenNotIssued": "%@ 尚未在此网络发行",
    "home.stakingTitle": "质押与节点奖励",
    "home.stakingSubtitle": "质押 KVR 获取奖励 · 节点运营奖励",
    "home.inferenceTitle": "AI 推理",
    "home.inferenceSubtitle": "用 KVR 付费运行 Kvasir 推理",
    "home.nodeSettingsTitle": "手机节点设置",
    "home.nodeSettingsSubtitle": "将本设备变为推理节点 · MLX · 模式 · 实时仪表",
    "home.nodeMonitorTitle": "节点性能 · 贡献度",
    "home.nodeMonitorSubtitle": "查看节点的性能等级、贡献度与奖励",
    "home.history": "交易记录",
    "home.noTransactions": "暂无交易",

    // onboarding
    "onboarding.subtitle": "基于 Solana 的 KVR 代币钱包 · devnet",
    "onboarding.create": "创建新钱包",
    "onboarding.import": "恢复已有钱包",

    // create wallet
    "create.title": "新建钱包",
    "create.phraseHeader": "助记词（12 个词）",
    "create.phraseWarning": "这是恢复钱包的唯一方式。请抄写并妥善保管，切勿泄露给任何人。",
    "create.savedToggle": "我已安全保存助记词",
    "create.start": "启用钱包",

    // import wallet
    "import.title": "恢复钱包",
    "import.phraseHeader": "助记词",
    "import.instruction": "请输入以空格分隔的 12 或 24 个词。",
    "import.restore": "恢复",
    "import.invalidPhrase": "助记词无效。",

    // send
    "send.asset": "资产",
    "send.balance": "余额 %@ %@",
    "send.recipient": "收款地址",
    "send.solanaAddress": "Solana 地址",
    "send.sent": "转账完成",
    "send.viewInExplorer": "在 explorer 中查看",
    "send.invalidAmount": "数量无效。",

    // staking
    "staking.title": "质押",
    "staking.stake": "质押",
    "staking.needOnline": "需将此设备注册为节点并保持在线才能质押。",
    "staking.guideLink": "质押指南",
    "staking.recentTx": "在 explorer 中查看最新交易",
    "staking.staked": "已质押",
    "staking.reward": "奖励",
    "staking.unstakeAll": "全部解押",
    "staking.nodeOperatorRewards": "节点运营者奖励",
    "staking.claimableRewards": "可领取奖励",
    "staking.nodeIdPlaceholder": "节点 ID（例如 node-slot-1）",
    "staking.register": "注册",
    "staking.claim": "领取奖励",
    "staking.nodeStatusLink": "查看节点运营状态",
    "staking.serviceUrlTitle": "质押服务 URL",
    "staking.serviceUrlDesc": "结算后端地址。请设置为同一网络中 Mac 的 IP。",

    // staking guide
    "guide.navTitle": "质押说明",
    "guide.headerTitle": "KVR 质押指南",
    "guide.headerSubtitle": "通过质押获取奖励，通过运营节点获得额外奖励",
    "guide.whatTitle": "什么是质押？",
    "guide.whatBody": "质押你持有的 KVR 后，奖励会按年化收益率（APR）实时累积。你可以随时解押，一并取回本金和奖励。",
    "guide.howTitle": "如何质押",
    "guide.howStep1": "在主界面确认网络为 **Devnet**。质押在 devnet 上运行。",
    "guide.howStep2": "在质押界面输入要质押的 **KVR 数量**。",
    "guide.howStep3": "点击 **质押**，钱包会将 KVR 转入质押金库。（转账手续费需要少量 SOL）",
    "guide.howStep4": "质押完成后，奖励将按 **APR** 实时累积。",
    "guide.howStep5": "点击 **全部解押**，本金加上已累积的奖励会返还到钱包。",
    "guide.nodeStep1": "在质押界面注册 **节点 ID**，将其与你的钱包关联。",
    "guide.nodeStep2": "当 Kvasir 中枢上报节点的推理贡献后，奖励便会累积。",
    "guide.nodeStep3": "在 **节点运营状态** 中监控状态、贡献与奖励。",
    "guide.nodeStep4": "通过 **领取奖励** 将累积的奖励收入钱包。",
    "guide.cautionTitle": "注意事项",
    "guide.caution1": "当前处于 **devnet 测试** 阶段。这些并非真实资产。",
    "guide.caution2": "由于采用链下结算，质押金由结算金库保管。（未来将迁移至链上程序）",
    "guide.caution3": "转账需要少量 **用作 gas 的 SOL**。",
    "guide.caution4": "结算服务器地址可在质押界面右上角的 ⚙️ 中设置。",

    // node monitor
    "monitor.navTitle": "节点运营状态",
    "monitor.connectDevice": "连接设备",
    "monitor.nodes": "节点",
    "monitor.online": "在线",
    "monitor.hubTitle": "Hub 连接",
    "monitor.hubServing": "服务中",
    "monitor.hubConnectedIdle": "已连接 · 空闲",
    "monitor.hubDisconnected": "已断开",
    "monitor.hubNone": "未连接 Hub — 请在节点设置中连接",
    "monitor.rawContribUnit": "原始贡献（unit）",
    "monitor.effectiveWeighted": "有效贡献（性能加权）",
    "monitor.lifetimeRewards": "累计奖励（KVR）",
    "monitor.claimableKVR": "可领取（KVR）",
    "monitor.tierTitle": "基于性能的贡献度重算",
    "monitor.tierDesc": "按实测吞吐量（tok/s）划分等级并应用奖励倍数。节点越快，相同工作量获得的奖励越多。",
    "monitor.perfTier": "性能等级 %@ · ×%@",
    "monitor.rawContribInline": "原始贡献 %@",
    "monitor.effectiveInline": "→ 有效 %@",
    "monitor.effectiveContrib": "有效贡献",
    "monitor.claimable": "可领取",
    "monitor.received": "已领取",
    "monitor.lastReport": "最后上报 ",
    "monitor.noReport": "暂无贡献上报",
    "monitor.noPerfData": "暂无性能数据",
    "monitor.emptyTitle": "尚未连接设备",
    "monitor.emptyDesc": "点击右上角的 ⊕（连接设备）来连接本设备或其他设备。",

    // node mode labels
    "mode.localShard": "本地分片",
    "mode.rpcWorker": "RPC 工作节点",

    // node status pills
    "status.online": "在线",
    "status.idle": "空闲",
    "status.registered": "已注册",
    "status.offline": "离线",

    // device connect
    "device.iosDevice": "iOS 设备",
    "device.accountAddressPlaceholder": "<账户地址>",
    "device.navTitle": "账户 · 连接设备",
    "device.myAccount": "我的账户",
    "device.myAccountDesc": "账户即你的钱包地址。可用此地址连接多台设备。",
    "device.connectThis": "连接本设备",
    "device.connectThisDesc": "将此 %@ 作为节点连接到你的账户。（OS：iOS · NPU）",
    "device.connectOther": "连接其他设备",
    "device.connectOtherDesc": "在已安装 Node.js 的台式机／笔记本（macOS · Windows · Linux）上，从 solana/node-client 运行下方命令。",
    "device.copyCommand": "复制命令",

    // node settings
    "nodeSettings.backendTitle": "计算后端",
    "nodeSettings.backendDesc": "本设备用于推理的计算单元 · iOS GPU 使用 MLX（Metal）",
    "nodeSettings.modeTitle": "节点模式",
    "nodeSettings.modeLocalTitle": "本地分片（推荐）",
    "nodeSettings.modeLocalDesc": "在设备本地运行层分片，仅中继激活值 — 最快",
    "nodeSettings.modeRpcDesc": "由中枢远程执行张量运算 — 简单但受网络延迟影响较慢",
    "nodeSettings.resourceTitle": "预计资源占用",
    "nodeSettings.tokPerSecCaption": "tok/s（0.5B Q8，估算）",
    "nodeSettings.memImpact": "内存占用",
    "nodeSettings.thermal": "发热",
    "nodeSettings.performance": "性能",
    "nodeSettings.chargingOnly": "仅在充电时参与",
    "nodeSettings.runLive": "运行节点（实时）",
    "nodeSettings.liveGauges": "实时仪表",
    "nodeSettings.charging": "充电中",
    "nodeSettings.battery": "电池",
    "nodeSettings.cpuLoad": "CPU 负载",
    "nodeSettings.thermalState": "发热状态",
    "nodeSettings.estimated": "估算",
    "nodeSettings.gaugeNote": "RAM、CPU 与发热为实测，GPU 为基于测量的估算。",

    // genesis gateway
    "genesis.title": "创世网关",
    "genesis.desc": "网络协调节点。结算、推理与节点奖励均经由此网关。",
    "genesis.reachable": "可连接",
    "genesis.unreachable": "无法连接",
    "genesis.active": "当前网关",
    "genesis.advertised": "网关公布的公开 URL",
    "genesis.cluster": "集群",
    "genesis.rewardPerUnit": "每单位奖励",
    "genesis.gatewayBonus": "网关主机奖励",
    "inference.clear": "清除对话",
    "inference.credits": "额度",
    "inference.currentBalance": "当前余额",
    "inference.topUpTitle": "充值额度",
    "inference.topUpNote": "向网关存入 KVR 即可获得用于推理的额度。回复以流式返回，慢模型也不会超时。",
    "inference.topUpAmount": "金额 (KVR)",
    "inference.topUpConfirm": "存入 KVR · 充值额度",
    "inference.topUpOk": "充值成功",
    "inference.topUpFailed": "充值失败",
    "inference.topUpInvalid": "请输入有效金额",
    "inference.topUpNoVault": "无法加载网关金库地址",
    "home.more": "更多",
    "history.title": "全部交易记录",

    // thermal words
    "thermal.low": "低",
    "thermal.medium": "中",
    "thermal.high": "高",
    "thermal.critical": "危险",

    // node profile notes
    "nodeProfile.mlxNote": "通过 MLX Metal 内核充分利用 Apple GPU/ANE。本地分片模式下性能最佳。",
    "nodeProfile.cpuNote": "小模型解码稳定。持续高负载时发热明显。",

    // inference
    "inference.paymentTx": "在 explorer 中查看付款交易",
    "inference.model": "模型",
    "inference.loadingModels": "正在加载模型…",
    "inference.prompt": "提示词",
    "inference.estQuote": "预估报价",
    "inference.estTokens": "预计 token %d（提示词 %d + 生成 ~%d）",
    "inference.payAndRun": "付费并运行（~%@ %@）",
    "inference.actualBillingNote": "实际计费将在运行后按已用 token 记录于下方。",
    "inference.result": "结果",
    "inference.actualTokens": "实际使用的 token",
    "inference.totalTok": "共 %d tok",
    "inference.usageRow": "提示词 %d · 生成 %d · 合计 %d tok",

    // errors / status
    "error.setStakingUrl": "请设置质押服务 URL。",
    "error.setServiceUrl": "请设置服务 URL。",
]

private let es: [String: String] = [
    // common
    "common.close": "Cerrar",
    "common.save": "Guardar",
    "common.settings": "Ajustes",
    "common.amount": "Cantidad",
    "common.receive": "Recibir",
    "common.send": "Enviar",
    "common.copyAddress": "Copiar dirección",
    "common.copied": "¡Copiado!",

    // menu
    "menu.network": "Red",
    "menu.refresh": "Actualizar",
    "menu.deleteWallet": "Eliminar cartera",
    "menu.language": "언어 / Language",

    // home
    "home.title": "Cartera",
    "home.balance": "Saldo",
    "home.tokenNotIssued": "%@ no está emitido en esta red",
    "home.stakingTitle": "Staking y recompensas de nodo",
    "home.stakingSubtitle": "Gana recompensas haciendo staking de KVR · recompensas de operador de nodo",
    "home.inferenceTitle": "Inferencia de IA",
    "home.inferenceSubtitle": "Paga con KVR para ejecutar inferencia en Kvasir",
    "home.nodeSettingsTitle": "Ajustes de nodo móvil",
    "home.nodeSettingsSubtitle": "Convierte este dispositivo en un nodo de inferencia · MLX · modo · medidores en vivo",
    "home.nodeMonitorTitle": "Rendimiento del nodo · contribución",
    "home.nodeMonitorSubtitle": "Consulta el nivel de rendimiento, la contribución y las recompensas de tu nodo",
    "home.history": "Historial de transacciones",
    "home.noTransactions": "Aún no hay transacciones",

    // onboarding
    "onboarding.subtitle": "Cartera de tokens KVR basada en Solana · devnet",
    "onboarding.create": "Crear cartera nueva",
    "onboarding.import": "Restaurar cartera existente",

    // create wallet
    "create.title": "Cartera nueva",
    "create.phraseHeader": "Frase de recuperación (12 palabras)",
    "create.phraseWarning": "Es la única forma de recuperar tu cartera. Anótala en un lugar seguro. Nunca la compartas.",
    "create.savedToggle": "He guardado mi frase de recuperación de forma segura",
    "create.start": "Iniciar cartera",

    // import wallet
    "import.title": "Restaurar cartera",
    "import.phraseHeader": "Frase de recuperación",
    "import.instruction": "Introduce 12 o 24 palabras separadas por espacios.",
    "import.restore": "Restaurar",
    "import.invalidPhrase": "La frase de recuperación no es válida.",

    // send
    "send.asset": "Activo",
    "send.balance": "Saldo %@ %@",
    "send.recipient": "Dirección del destinatario",
    "send.solanaAddress": "Dirección de Solana",
    "send.sent": "Enviado",
    "send.viewInExplorer": "Ver en explorer",
    "send.invalidAmount": "La cantidad no es válida.",

    // staking
    "staking.title": "Staking",
    "staking.stake": "Hacer staking",
    "staking.needOnline": "El staking requiere este dispositivo registrado como nodo y en línea.",
    "staking.guideLink": "Cómo hacer staking",
    "staking.recentTx": "Ver la última transacción en explorer",
    "staking.staked": "En staking",
    "staking.reward": "Recompensa",
    "staking.unstakeAll": "Retirar todo",
    "staking.nodeOperatorRewards": "Recompensas de operador de nodo",
    "staking.claimableRewards": "Recompensas reclamables",
    "staking.nodeIdPlaceholder": "ID de nodo (p. ej. node-slot-1)",
    "staking.register": "Registrar",
    "staking.claim": "Reclamar recompensas",
    "staking.nodeStatusLink": "Ver operaciones del nodo",
    "staking.serviceUrlTitle": "URL del servicio de staking",
    "staking.serviceUrlDesc": "La dirección del backend de liquidación. Configúrala con la IP de tu Mac en la misma red.",

    // staking guide
    "guide.navTitle": "Guía de staking",
    "guide.headerTitle": "Guía de staking de KVR",
    "guide.headerSubtitle": "Cómo ganar recompensas con staking y recompensas adicionales operando un nodo",
    "guide.whatTitle": "¿Qué es el staking?",
    "guide.whatBody": "Cuando haces staking de tus KVR, las recompensas se acumulan en tiempo real según la tasa anual (APR). Puedes retirar el staking en cualquier momento para recuperar tu capital y tus recompensas juntos.",
    "guide.howTitle": "Cómo hacer staking",
    "guide.howStep1": "En la pantalla de inicio, asegúrate de que la red sea **Devnet**. El staking funciona en devnet.",
    "guide.howStep2": "En la pantalla de staking, introduce la **cantidad de KVR** que quieres poner en staking.",
    "guide.howStep3": "Toca **Hacer staking** y tu cartera enviará los KVR a la bóveda de staking. (Se necesita una pequeña cantidad de SOL para la comisión de transferencia).",
    "guide.howStep4": "Tu staking queda registrado y las recompensas se acumulan en tiempo real según la **APR**.",
    "guide.howStep5": "Toca **Retirar todo** para devolver tu capital más las recompensas acumuladas a tu cartera.",
    "guide.nodeStep1": "En la pantalla de staking, registra tu **ID de nodo** para vincularlo a tu cartera.",
    "guide.nodeStep2": "Cuando el hub de Kvasir reporte la contribución de inferencia de tu nodo, se acumularán recompensas.",
    "guide.nodeStep3": "Supervisa el estado, la contribución y las recompensas en **Operaciones del nodo**.",
    "guide.nodeStep4": "Usa **Reclamar recompensas** para recibir las recompensas acumuladas en tu cartera.",
    "guide.cautionTitle": "Notas importantes",
    "guide.caution1": "Actualmente está en **pruebas en devnet**. Estos no son activos reales.",
    "guide.caution2": "Como la liquidación es fuera de la cadena, los depósitos los custodia la bóveda de liquidación. (Se migrará a un programa on-chain más adelante).",
    "guide.caution3": "Las transferencias requieren una pequeña cantidad de **SOL para el gas**.",
    "guide.caution4": "Configura la dirección del servidor de liquidación desde el ⚙️ arriba a la derecha en la pantalla de staking.",

    // node monitor
    "monitor.navTitle": "Operaciones del nodo",
    "monitor.connectDevice": "Conectar dispositivo",
    "monitor.nodes": "Nodos",
    "monitor.online": "En línea",
    "monitor.hubTitle": "Conexión al hub",
    "monitor.hubServing": "Sirviendo",
    "monitor.hubConnectedIdle": "Conectado · inactivo",
    "monitor.hubDisconnected": "Desconectado",
    "monitor.hubNone": "Ningún hub conectado — conéctalo en Ajustes del nodo",
    "monitor.rawContribUnit": "Contribución bruta (unit)",
    "monitor.effectiveWeighted": "Contribución efectiva (ponderada por rendimiento)",
    "monitor.lifetimeRewards": "Recompensas acumuladas (KVR)",
    "monitor.claimableKVR": "Reclamable (KVR)",
    "monitor.tierTitle": "Recálculo de contribución según rendimiento",
    "monitor.tierDesc": "Los nodos se clasifican por el rendimiento medido (tok/s) y se aplica un multiplicador de recompensa. Los nodos más rápidos ganan más por el mismo trabajo.",
    "monitor.perfTier": "Nivel de rendimiento %@ · ×%@",
    "monitor.rawContribInline": "Bruta %@",
    "monitor.effectiveInline": "→ Efectiva %@",
    "monitor.effectiveContrib": "Contribución efectiva",
    "monitor.claimable": "Reclamable",
    "monitor.received": "Recibidas",
    "monitor.lastReport": "Último reporte ",
    "monitor.noReport": "Aún no hay reportes de contribución",
    "monitor.noPerfData": "Sin datos de rendimiento",
    "monitor.emptyTitle": "No hay dispositivos conectados",
    "monitor.emptyDesc": "Usa ⊕ (Conectar dispositivo) arriba a la derecha para conectar este u otro dispositivo.",

    // node mode labels
    "mode.localShard": "Fragmento local",
    "mode.rpcWorker": "Worker RPC",

    // node status pills
    "status.online": "En línea",
    "status.idle": "Inactivo",
    "status.registered": "Registrado",
    "status.offline": "Sin conexión",

    // device connect
    "device.iosDevice": "Dispositivo iOS",
    "device.accountAddressPlaceholder": "<dirección de la cuenta>",
    "device.navTitle": "Cuenta · Conectar dispositivo",
    "device.myAccount": "Mi cuenta",
    "device.myAccountDesc": "Tu cuenta es tu dirección de cartera. Conecta varios dispositivos a esta dirección.",
    "device.connectThis": "Conectar este dispositivo",
    "device.connectThisDesc": "Conecta este %@ a tu cuenta como nodo. (SO: iOS · NPU)",
    "device.connectOther": "Conectar otro dispositivo",
    "device.connectOtherDesc": "En un ordenador de escritorio o portátil (macOS · Windows · Linux) con Node.js instalado, ejecuta el siguiente comando desde solana/node-client.",
    "device.copyCommand": "Copiar comando",

    // node settings
    "nodeSettings.backendTitle": "Backend de cómputo",
    "nodeSettings.backendDesc": "La unidad de cómputo que este dispositivo usa para la inferencia · la GPU de iOS usa MLX (Metal)",
    "nodeSettings.modeTitle": "Modo de nodo",
    "nodeSettings.modeLocalTitle": "Fragmento local (recomendado)",
    "nodeSettings.modeLocalDesc": "Ejecuta fragmentos de capas localmente en el dispositivo y solo transmite las activaciones — lo más rápido",
    "nodeSettings.modeRpcDesc": "El hub ejecuta las operaciones de tensores de forma remota — sencillo pero más lento por la latencia de red",
    "nodeSettings.resourceTitle": "Impacto estimado en recursos",
    "nodeSettings.tokPerSecCaption": "tok/s (0.5B Q8, estimado)",
    "nodeSettings.memImpact": "Uso de memoria",
    "nodeSettings.thermal": "Temperatura",
    "nodeSettings.performance": "Rendimiento",
    "nodeSettings.chargingOnly": "Participar solo mientras se carga",
    "nodeSettings.runLive": "Ejecutar nodo (en vivo)",
    "nodeSettings.liveGauges": "Medidores en vivo",
    "nodeSettings.charging": "Cargando",
    "nodeSettings.battery": "Batería",
    "nodeSettings.cpuLoad": "Carga de CPU",
    "nodeSettings.thermalState": "Estado térmico",
    "nodeSettings.estimated": "Estimado",
    "nodeSettings.gaugeNote": "RAM, CPU y temperatura son medidas reales; la GPU es una estimación basada en mediciones.",

    // genesis gateway
    "genesis.title": "Pasarela génesis",
    "genesis.desc": "El nodo de coordinación de la red. La liquidación, la inferencia y las recompensas de nodo pasan por esta pasarela.",
    "genesis.reachable": "Accesible",
    "genesis.unreachable": "Inaccesible",
    "genesis.active": "Pasarela activa",
    "genesis.advertised": "URL pública anunciada por la pasarela",
    "genesis.cluster": "Clúster",
    "genesis.rewardPerUnit": "Recompensa por unidad",
    "genesis.gatewayBonus": "Bonificación por alojar la pasarela",
    "inference.clear": "Borrar chat",
    "inference.credits": "Créditos",
    "inference.currentBalance": "Saldo actual",
    "inference.topUpTitle": "Añadir créditos",
    "inference.topUpNote": "Deposita KVR en la pasarela para obtener créditos de inferencia. Las respuestas llegan en streaming, así los modelos lentos no expiran.",
    "inference.topUpAmount": "Importe (KVR)",
    "inference.topUpConfirm": "Depositar KVR · añadir créditos",
    "inference.topUpOk": "Créditos añadidos",
    "inference.topUpFailed": "Recarga fallida",
    "inference.topUpInvalid": "Introduce un importe válido",
    "inference.topUpNoVault": "No se pudo cargar la dirección del vault",
    "home.more": "Más",
    "history.title": "Historial de transacciones",

    // thermal words
    "thermal.low": "Baja",
    "thermal.medium": "Media",
    "thermal.high": "Alta",
    "thermal.critical": "Crítica",

    // node profile notes
    "nodeProfile.mlxNote": "Usa kernels de MLX Metal para aprovechar la GPU/ANE de Apple. Mejor rendimiento en modo de fragmento local.",
    "nodeProfile.cpuNote": "Decodificación estable para modelos pequeños. Se calienta bajo carga sostenida.",

    // inference
    "inference.paymentTx": "Ver la transacción de pago en explorer",
    "inference.model": "Modelo",
    "inference.loadingModels": "Cargando modelos…",
    "inference.prompt": "Prompt",
    "inference.estQuote": "Presupuesto estimado",
    "inference.estTokens": "Tokens estimados %d (prompt %d + generación ~%d)",
    "inference.payAndRun": "Pagar y ejecutar (~%@ %@)",
    "inference.actualBillingNote": "El cobro real se registrará abajo según los tokens usados tras la ejecución.",
    "inference.result": "Resultado",
    "inference.actualTokens": "Tokens realmente usados",
    "inference.totalTok": "Total %d tok",
    "inference.usageRow": "Prompt %d · gen. %d · total %d tok",

    // errors / status
    "error.setStakingUrl": "Configura la URL del servicio de staking.",
    "error.setServiceUrl": "Configura la URL del servicio.",
]

private let ja: [String: String] = [
    // common
    "common.close": "閉じる",
    "common.save": "保存",
    "common.settings": "設定",
    "common.amount": "数量",
    "common.receive": "受信",
    "common.send": "送金",
    "common.copyAddress": "アドレスをコピー",
    "common.copied": "コピーしました！",

    // menu
    "menu.network": "ネットワーク",
    "menu.refresh": "更新",
    "menu.deleteWallet": "ウォレットを削除",
    "menu.language": "언어 / Language",

    // home
    "home.title": "ウォレット",
    "home.balance": "残高",
    "home.tokenNotIssued": "%@ はこのネットワークでは未発行です",
    "home.stakingTitle": "ステーキング & ノード報酬",
    "home.stakingSubtitle": "KVR を預けて報酬を獲得 · ノード運営報酬",
    "home.inferenceTitle": "AI 推論",
    "home.inferenceSubtitle": "KVR で支払って Kvasir 推論を実行",
    "home.nodeSettingsTitle": "モバイルノード設定",
    "home.nodeSettingsSubtitle": "この端末を推論ノードに · MLX · モード · ライブゲージ",
    "home.nodeMonitorTitle": "ノード性能 · 貢献度",
    "home.nodeMonitorSubtitle": "自分のノードの性能ランクと貢献度・報酬を確認",
    "home.history": "取引履歴",
    "home.noTransactions": "取引はまだありません",

    // onboarding
    "onboarding.subtitle": "Solana ベースの KVR トークンウォレット · devnet",
    "onboarding.create": "新しいウォレットを作成",
    "onboarding.import": "既存のウォレットを復元",

    // create wallet
    "create.title": "新しいウォレット",
    "create.phraseHeader": "リカバリーフレーズ（12 単語）",
    "create.phraseWarning": "ウォレットを復元する唯一の方法です。安全な場所に書き留めてください。決して他人と共有しないでください。",
    "create.savedToggle": "リカバリーフレーズを安全に保存しました",
    "create.start": "ウォレットを開始",

    // import wallet
    "import.title": "ウォレットを復元",
    "import.phraseHeader": "リカバリーフレーズ",
    "import.instruction": "12 または 24 単語をスペースで区切って入力してください。",
    "import.restore": "復元",
    "import.invalidPhrase": "リカバリーフレーズが正しくありません。",

    // send
    "send.asset": "資産",
    "send.balance": "残高 %@ %@",
    "send.recipient": "送金先アドレス",
    "send.solanaAddress": "Solana アドレス",
    "send.sent": "送信完了",
    "send.viewInExplorer": "explorer で表示",
    "send.invalidAmount": "数量が正しくありません。",

    // staking
    "staking.title": "ステーキング",
    "staking.stake": "ステーキング",
    "staking.needOnline": "ステーキングにはこの端末をノードとして登録しオンラインである必要があります。",
    "staking.guideLink": "ステーキングの方法",
    "staking.recentTx": "最新のトランザクションを explorer で表示",
    "staking.staked": "預入額",
    "staking.reward": "報酬",
    "staking.unstakeAll": "すべてアンステーク",
    "staking.nodeOperatorRewards": "ノード運営者報酬",
    "staking.claimableRewards": "請求可能な報酬",
    "staking.nodeIdPlaceholder": "ノード ID（例：node-slot-1）",
    "staking.register": "登録",
    "staking.claim": "報酬を請求",
    "staking.nodeStatusLink": "ノード運営状況を表示",
    "staking.serviceUrlTitle": "ステーキングサービス URL",
    "staking.serviceUrlDesc": "精算バックエンドのアドレスです。同じネットワーク上の Mac の IP を設定してください。",

    // staking guide
    "guide.navTitle": "ステーキング案内",
    "guide.headerTitle": "KVR ステーキングガイド",
    "guide.headerSubtitle": "預け入れで報酬を得て、ノード運営で追加報酬を得る方法",
    "guide.whatTitle": "ステーキングとは？",
    "guide.whatBody": "保有する KVR を預けると、年利（APR）に応じて報酬がリアルタイムで積み上がります。いつでもアンステークして、元本と報酬をまとめて引き出せます。",
    "guide.howTitle": "ステーキングの方法",
    "guide.howStep1": "ホーム画面でネットワークが **Devnet** であることを確認してください。ステーキングは devnet で動作します。",
    "guide.howStep2": "ステーキング画面で預ける **KVR の数量** を入力します。",
    "guide.howStep3": "**ステーキング** をタップすると、ウォレットが KVR をステーキング金庫へ送金します。（送金手数料用に少量の SOL が必要です）",
    "guide.howStep4": "預け入れが登録され、**APR** に応じて報酬がリアルタイムで積み立てられます。",
    "guide.howStep5": "**すべてアンステーク** をタップすると、元本と積み立てた報酬がウォレットに返還されます。",
    "guide.nodeStep1": "ステーキング画面で **ノード ID** を登録し、ウォレットに紐付けます。",
    "guide.nodeStep2": "Kvasir ハブがノードの推論貢献を報告すると、報酬が積み立てられます。",
    "guide.nodeStep3": "**ノード運営状況** で状態・貢献・報酬をモニタリングします。",
    "guide.nodeStep4": "**報酬を請求** で積み立てた報酬をウォレットに受け取ります。",
    "guide.cautionTitle": "注意事項",
    "guide.caution1": "現在は **devnet テスト** 段階です。実際の資産ではありません。",
    "guide.caution2": "オフチェーン精算方式のため、預け入れ額は精算金庫が保管します。（今後オンチェーンプログラムへ移行予定）",
    "guide.caution3": "送金には少量の **ガス用 SOL** が必要です。",
    "guide.caution4": "精算サーバーのアドレスは、ステーキング画面右上の ⚙️ で設定します。",

    // node monitor
    "monitor.navTitle": "ノード運営状況",
    "monitor.connectDevice": "端末を接続",
    "monitor.nodes": "ノード",
    "monitor.online": "オンライン",
    "monitor.hubTitle": "ハブ接続",
    "monitor.hubServing": "処理中",
    "monitor.hubConnectedIdle": "接続済み · 待機",
    "monitor.hubDisconnected": "切断",
    "monitor.hubNone": "接続中のハブなし — ノード設定で接続してください",
    "monitor.rawContribUnit": "元の貢献（unit）",
    "monitor.effectiveWeighted": "有効貢献（性能加重）",
    "monitor.lifetimeRewards": "累計報酬（KVR）",
    "monitor.claimableKVR": "請求可能（KVR）",
    "monitor.tierTitle": "性能に基づく貢献度の再計算",
    "monitor.tierDesc": "実測スループット（tok/s）でランク付けし、報酬倍率を適用します。速いノードほど同じ作業でより多くの報酬を得られます。",
    "monitor.perfTier": "性能ランク %@ · ×%@",
    "monitor.rawContribInline": "元の貢献 %@",
    "monitor.effectiveInline": "→ 有効 %@",
    "monitor.effectiveContrib": "有効貢献",
    "monitor.claimable": "請求可能",
    "monitor.received": "受取済み",
    "monitor.lastReport": "最終報告 ",
    "monitor.noReport": "貢献の報告はまだありません",
    "monitor.noPerfData": "性能データなし",
    "monitor.emptyTitle": "接続された端末がありません",
    "monitor.emptyDesc": "右上の ⊕（端末を接続）から、この端末または別の端末を接続してください。",

    // node mode labels
    "mode.localShard": "ローカルシャード",
    "mode.rpcWorker": "RPC ワーカー",

    // node status pills
    "status.online": "オンライン",
    "status.idle": "アイドル",
    "status.registered": "登録済み",
    "status.offline": "オフライン",

    // device connect
    "device.iosDevice": "iOS 端末",
    "device.accountAddressPlaceholder": "<アカウントアドレス>",
    "device.navTitle": "アカウント · 端末接続",
    "device.myAccount": "マイアカウント",
    "device.myAccountDesc": "アカウントはあなたのウォレットアドレスです。このアドレスに複数の端末を接続できます。",
    "device.connectThis": "この端末を接続",
    "device.connectThisDesc": "この %@ をノードとしてアカウントに接続します。（OS：iOS · NPU）",
    "device.connectOther": "別の端末を接続",
    "device.connectOtherDesc": "Node.js をインストールしたデスクトップ／ラップトップ（macOS · Windows · Linux）で、solana/node-client から下記のコマンドを実行してください。",
    "device.copyCommand": "コマンドをコピー",

    // node settings
    "nodeSettings.backendTitle": "コンピュートバックエンド",
    "nodeSettings.backendDesc": "この端末が推論に使う演算ユニット · iOS GPU は MLX（Metal）",
    "nodeSettings.modeTitle": "ノードモード",
    "nodeSettings.modeLocalTitle": "ローカルシャード（推奨）",
    "nodeSettings.modeLocalDesc": "レイヤーシャードを端末上でローカル実行し、アクティベーションのみを中継 — 最速",
    "nodeSettings.modeRpcDesc": "ハブがテンソル演算をリモート実行 — シンプルだがネットワーク遅延で遅い",
    "nodeSettings.resourceTitle": "予想リソース影響",
    "nodeSettings.tokPerSecCaption": "tok/s（0.5B Q8、推定）",
    "nodeSettings.memImpact": "メモリ使用量",
    "nodeSettings.thermal": "発熱",
    "nodeSettings.performance": "性能",
    "nodeSettings.chargingOnly": "充電中のみ参加",
    "nodeSettings.runLive": "ノード稼働（ライブ）",
    "nodeSettings.liveGauges": "ライブゲージ",
    "nodeSettings.charging": "充電中",
    "nodeSettings.battery": "バッテリー",
    "nodeSettings.cpuLoad": "CPU 負荷",
    "nodeSettings.thermalState": "発熱状態",
    "nodeSettings.estimated": "推定",
    "nodeSettings.gaugeNote": "RAM・CPU・発熱は実測、GPU は測定に基づく推定です。",

    // genesis gateway
    "genesis.title": "ジェネシスゲートウェイ",
    "genesis.desc": "ネットワークの調整ノードです。精算・推論・ノード報酬はこのゲートウェイを経由します。",
    "genesis.reachable": "接続可能",
    "genesis.unreachable": "接続不可",
    "genesis.active": "有効なゲートウェイ",
    "genesis.advertised": "ゲートウェイが通知する公開 URL",
    "genesis.cluster": "クラスター",
    "genesis.rewardPerUnit": "ユニットあたり報酬",
    "genesis.gatewayBonus": "ゲートウェイホストボーナス",
    "inference.clear": "会話を消去",
    "inference.credits": "クレジット",
    "inference.currentBalance": "現在の残高",
    "inference.topUpTitle": "クレジット追加",
    "inference.topUpNote": "KVRをゲートウェイに預けると推論用クレジットになります。ストリーミングなので遅いモデルもタイムアウトしません。",
    "inference.topUpAmount": "金額 (KVR)",
    "inference.topUpConfirm": "KVRを預けてクレジット追加",
    "inference.topUpOk": "追加しました",
    "inference.topUpFailed": "追加に失敗",
    "inference.topUpInvalid": "有効な金額を入力してください",
    "inference.topUpNoVault": "ゲートウェイのボルトアドレスを取得できません",
    "home.more": "もっと見る",
    "history.title": "取引履歴",

    // thermal words
    "thermal.low": "低",
    "thermal.medium": "中",
    "thermal.high": "高",
    "thermal.critical": "危険",

    // node profile notes
    "nodeProfile.mlxNote": "MLX Metal カーネルで Apple GPU/ANE を活用。ローカルシャードモードで最高性能。",
    "nodeProfile.cpuNote": "小型モデルのデコードは安定。継続負荷時は発熱が大きい。",

    // inference
    "inference.paymentTx": "支払いトランザクションを explorer で表示",
    "inference.model": "モデル",
    "inference.loadingModels": "モデルを読み込み中…",
    "inference.prompt": "プロンプト",
    "inference.estQuote": "見積もり",
    "inference.estTokens": "予想トークン %d（プロンプト %d + 生成 ~%d）",
    "inference.payAndRun": "支払って実行（~%@ %@）",
    "inference.actualBillingNote": "実際の請求は、実行後に使用したトークンに基づいて下に記録されます。",
    "inference.result": "結果",
    "inference.actualTokens": "実際に使用したトークン",
    "inference.totalTok": "合計 %d tok",
    "inference.usageRow": "プロンプト %d · 生成 %d · 合計 %d tok",

    // errors / status
    "error.setStakingUrl": "ステーキングサービス URL を設定してください。",
    "error.setServiceUrl": "サービス URL を設定してください。",
]

private let fr: [String: String] = [
    // common
    "common.close": "Fermer",
    "common.save": "Enregistrer",
    "common.settings": "Réglages",
    "common.amount": "Montant",
    "common.receive": "Recevoir",
    "common.send": "Envoyer",
    "common.copyAddress": "Copier l'adresse",
    "common.copied": "Copié !",

    // menu
    "menu.network": "Réseau",
    "menu.refresh": "Actualiser",
    "menu.deleteWallet": "Supprimer le portefeuille",
    "menu.language": "언어 / Language",

    // home
    "home.title": "Portefeuille",
    "home.balance": "Solde",
    "home.tokenNotIssued": "%@ n'est pas émis sur ce réseau",
    "home.stakingTitle": "Staking et récompenses de nœud",
    "home.stakingSubtitle": "Gagnez des récompenses en stakant des KVR · récompenses d'opérateur de nœud",
    "home.inferenceTitle": "Inférence IA",
    "home.inferenceSubtitle": "Payez en KVR pour exécuter une inférence Kvasir",
    "home.nodeSettingsTitle": "Réglages du nœud mobile",
    "home.nodeSettingsSubtitle": "Transformez cet appareil en nœud d'inférence · MLX · mode · jauges en direct",
    "home.nodeMonitorTitle": "Performance du nœud · contribution",
    "home.nodeMonitorSubtitle": "Consultez le niveau de performance, la contribution et les récompenses de votre nœud",
    "home.history": "Historique des transactions",
    "home.noTransactions": "Aucune transaction pour le moment",

    // onboarding
    "onboarding.subtitle": "Portefeuille de jetons KVR basé sur Solana · devnet",
    "onboarding.create": "Créer un nouveau portefeuille",
    "onboarding.import": "Restaurer un portefeuille existant",

    // create wallet
    "create.title": "Nouveau portefeuille",
    "create.phraseHeader": "Phrase de récupération (12 mots)",
    "create.phraseWarning": "C'est le seul moyen de récupérer votre portefeuille. Notez-la dans un endroit sûr. Ne la partagez jamais.",
    "create.savedToggle": "J'ai enregistré ma phrase de récupération en lieu sûr",
    "create.start": "Démarrer le portefeuille",

    // import wallet
    "import.title": "Restaurer le portefeuille",
    "import.phraseHeader": "Phrase de récupération",
    "import.instruction": "Saisissez 12 ou 24 mots séparés par des espaces.",
    "import.restore": "Restaurer",
    "import.invalidPhrase": "La phrase de récupération est invalide.",

    // send
    "send.asset": "Actif",
    "send.balance": "Solde %@ %@",
    "send.recipient": "Adresse du destinataire",
    "send.solanaAddress": "Adresse Solana",
    "send.sent": "Envoyé",
    "send.viewInExplorer": "Voir dans explorer",
    "send.invalidAmount": "Le montant est invalide.",

    // staking
    "staking.title": "Staking",
    "staking.stake": "Staker",
    "staking.needOnline": "Le staking nécessite cet appareil enregistré comme nœud et en ligne.",
    "staking.guideLink": "Comment staker",
    "staking.recentTx": "Voir la dernière transaction dans explorer",
    "staking.staked": "Staké",
    "staking.reward": "Récompense",
    "staking.unstakeAll": "Tout unstaker",
    "staking.nodeOperatorRewards": "Récompenses d'opérateur de nœud",
    "staking.claimableRewards": "Récompenses réclamables",
    "staking.nodeIdPlaceholder": "ID de nœud (ex. node-slot-1)",
    "staking.register": "Enregistrer",
    "staking.claim": "Réclamer les récompenses",
    "staking.nodeStatusLink": "Voir les opérations du nœud",
    "staking.serviceUrlTitle": "URL du service de staking",
    "staking.serviceUrlDesc": "L'adresse du backend de règlement. Réglez-la sur l'IP de votre Mac sur le même réseau.",

    // staking guide
    "guide.navTitle": "Guide de staking",
    "guide.headerTitle": "Guide de staking KVR",
    "guide.headerSubtitle": "Comment gagner des récompenses par staking et des récompenses supplémentaires en exploitant un nœud",
    "guide.whatTitle": "Qu'est-ce que le staking ?",
    "guide.whatBody": "Lorsque vous stakez vos KVR, les récompenses s'accumulent en temps réel selon le taux annuel (APR). Vous pouvez unstaker à tout moment pour récupérer votre capital et vos récompenses ensemble.",
    "guide.howTitle": "Comment staker",
    "guide.howStep1": "Sur l'écran d'accueil, vérifiez que le réseau est **Devnet**. Le staking fonctionne sur devnet.",
    "guide.howStep2": "Sur l'écran de staking, saisissez le **montant de KVR** à staker.",
    "guide.howStep3": "Appuyez sur **Staker** et votre portefeuille envoie les KVR vers le coffre de staking. (Une petite quantité de SOL est nécessaire pour les frais de transfert.)",
    "guide.howStep4": "Votre staking est enregistré et les récompenses s'accumulent en temps réel selon l'**APR**.",
    "guide.howStep5": "Appuyez sur **Tout unstaker** pour renvoyer votre capital et les récompenses accumulées vers votre portefeuille.",
    "guide.nodeStep1": "Sur l'écran de staking, enregistrez votre **ID de nœud** pour le lier à votre portefeuille.",
    "guide.nodeStep2": "Lorsque le hub Kvasir signale la contribution d'inférence de votre nœud, des récompenses s'accumulent.",
    "guide.nodeStep3": "Suivez le statut, la contribution et les récompenses dans **Opérations du nœud**.",
    "guide.nodeStep4": "Utilisez **Réclamer les récompenses** pour recevoir les récompenses accumulées dans votre portefeuille.",
    "guide.cautionTitle": "Remarques importantes",
    "guide.caution1": "Il s'agit actuellement de **tests sur devnet**. Ce ne sont pas des actifs réels.",
    "guide.caution2": "Le règlement étant hors chaîne, les dépôts sont conservés par le coffre de règlement. (Migration ultérieure vers un programme on-chain.)",
    "guide.caution3": "Les transferts nécessitent une petite quantité de **SOL pour le gas**.",
    "guide.caution4": "Réglez l'adresse du serveur de règlement via le ⚙️ en haut à droite de l'écran de staking.",

    // node monitor
    "monitor.navTitle": "Opérations du nœud",
    "monitor.connectDevice": "Connecter un appareil",
    "monitor.nodes": "Nœuds",
    "monitor.online": "En ligne",
    "monitor.hubTitle": "Connexion au hub",
    "monitor.hubServing": "En service",
    "monitor.hubConnectedIdle": "Connecté · inactif",
    "monitor.hubDisconnected": "Déconnecté",
    "monitor.hubNone": "Aucun hub connecté — connectez-en un dans les réglages du nœud",
    "monitor.rawContribUnit": "Contribution brute (unit)",
    "monitor.effectiveWeighted": "Contribution effective (pondérée par la performance)",
    "monitor.lifetimeRewards": "Récompenses cumulées (KVR)",
    "monitor.claimableKVR": "Réclamable (KVR)",
    "monitor.tierTitle": "Recalcul de la contribution selon la performance",
    "monitor.tierDesc": "Les nœuds sont classés selon le débit mesuré (tok/s) et un multiplicateur de récompense est appliqué. Les nœuds plus rapides gagnent plus pour le même travail.",
    "monitor.perfTier": "Niveau de performance %@ · ×%@",
    "monitor.rawContribInline": "Brute %@",
    "monitor.effectiveInline": "→ Effective %@",
    "monitor.effectiveContrib": "Contribution effective",
    "monitor.claimable": "Réclamable",
    "monitor.received": "Reçues",
    "monitor.lastReport": "Dernier rapport ",
    "monitor.noReport": "Aucun rapport de contribution pour le moment",
    "monitor.noPerfData": "Aucune donnée de performance",
    "monitor.emptyTitle": "Aucun appareil connecté",
    "monitor.emptyDesc": "Utilisez ⊕ (Connecter un appareil) en haut à droite pour connecter cet appareil ou un autre.",

    // node mode labels
    "mode.localShard": "Fragment local",
    "mode.rpcWorker": "Worker RPC",

    // node status pills
    "status.online": "En ligne",
    "status.idle": "Inactif",
    "status.registered": "Enregistré",
    "status.offline": "Hors ligne",

    // device connect
    "device.iosDevice": "Appareil iOS",
    "device.accountAddressPlaceholder": "<adresse du compte>",
    "device.navTitle": "Compte · Connecter un appareil",
    "device.myAccount": "Mon compte",
    "device.myAccountDesc": "Votre compte est votre adresse de portefeuille. Connectez plusieurs appareils à cette adresse.",
    "device.connectThis": "Connecter cet appareil",
    "device.connectThisDesc": "Connectez ce %@ à votre compte en tant que nœud. (OS : iOS · NPU)",
    "device.connectOther": "Connecter un autre appareil",
    "device.connectOtherDesc": "Sur un ordinateur de bureau/portable (macOS · Windows · Linux) avec Node.js installé, exécutez la commande ci-dessous depuis solana/node-client.",
    "device.copyCommand": "Copier la commande",

    // node settings
    "nodeSettings.backendTitle": "Backend de calcul",
    "nodeSettings.backendDesc": "L'unité de calcul que cet appareil utilise pour l'inférence · le GPU iOS utilise MLX (Metal)",
    "nodeSettings.modeTitle": "Mode du nœud",
    "nodeSettings.modeLocalTitle": "Fragment local (recommandé)",
    "nodeSettings.modeLocalDesc": "Exécute les fragments de couches localement sur l'appareil et ne relaie que les activations — le plus rapide",
    "nodeSettings.modeRpcDesc": "Le hub exécute les opérations tensorielles à distance — simple mais plus lent en raison de la latence réseau",
    "nodeSettings.resourceTitle": "Impact estimé sur les ressources",
    "nodeSettings.tokPerSecCaption": "tok/s (0.5B Q8, estimé)",
    "nodeSettings.memImpact": "Utilisation mémoire",
    "nodeSettings.thermal": "Température",
    "nodeSettings.performance": "Performance",
    "nodeSettings.chargingOnly": "Participer uniquement en charge",
    "nodeSettings.runLive": "Lancer le nœud (en direct)",
    "nodeSettings.liveGauges": "Jauges en direct",
    "nodeSettings.charging": "En charge",
    "nodeSettings.battery": "Batterie",
    "nodeSettings.cpuLoad": "Charge CPU",
    "nodeSettings.thermalState": "État thermique",
    "nodeSettings.estimated": "Estimé",
    "nodeSettings.gaugeNote": "RAM, CPU et température sont mesurés ; le GPU est estimé à partir des mesures.",

    // genesis gateway
    "genesis.title": "Passerelle genesis",
    "genesis.desc": "Le nœud de coordination du réseau. Règlement, inférence et récompenses de nœud passent par cette passerelle.",
    "genesis.reachable": "Accessible",
    "genesis.unreachable": "Injoignable",
    "genesis.active": "Passerelle active",
    "genesis.advertised": "URL publique annoncée par la passerelle",
    "genesis.cluster": "Cluster",
    "genesis.rewardPerUnit": "Récompense par unité",
    "genesis.gatewayBonus": "Bonus pour hôte de la passerelle",
    "inference.clear": "Effacer la discussion",
    "inference.credits": "Crédits",
    "inference.currentBalance": "Solde actuel",
    "inference.topUpTitle": "Ajouter des crédits",
    "inference.topUpNote": "Déposez des KVR sur la passerelle pour obtenir des crédits d'inférence. Les réponses sont en streaming, les modèles lents n'expirent pas.",
    "inference.topUpAmount": "Montant (KVR)",
    "inference.topUpConfirm": "Déposer KVR · ajouter des crédits",
    "inference.topUpOk": "Crédits ajoutés",
    "inference.topUpFailed": "Échec de la recharge",
    "inference.topUpInvalid": "Saisissez un montant valide",
    "inference.topUpNoVault": "Impossible de charger l'adresse du coffre",
    "home.more": "Plus",
    "history.title": "Historique des transactions",

    // thermal words
    "thermal.low": "Faible",
    "thermal.medium": "Moyenne",
    "thermal.high": "Élevée",
    "thermal.critical": "Critique",

    // node profile notes
    "nodeProfile.mlxNote": "Utilise les noyaux MLX Metal pour exploiter le GPU/ANE Apple. Meilleures performances en mode fragment local.",
    "nodeProfile.cpuNote": "Décodage stable pour les petits modèles. Chauffe sous charge prolongée.",

    // inference
    "inference.paymentTx": "Voir la transaction de paiement dans explorer",
    "inference.model": "Modèle",
    "inference.loadingModels": "Chargement des modèles…",
    "inference.prompt": "Prompt",
    "inference.estQuote": "Devis estimé",
    "inference.estTokens": "Tokens estimés %d (prompt %d + génération ~%d)",
    "inference.payAndRun": "Payer et exécuter (~%@ %@)",
    "inference.actualBillingNote": "La facturation réelle est enregistrée ci-dessous selon les tokens utilisés après l'exécution.",
    "inference.result": "Résultat",
    "inference.actualTokens": "Tokens réellement utilisés",
    "inference.totalTok": "Total %d tok",
    "inference.usageRow": "Prompt %d · gén. %d · total %d tok",

    // errors / status
    "error.setStakingUrl": "Veuillez définir l'URL du service de staking.",
    "error.setServiceUrl": "Veuillez définir l'URL du service.",
]

private let de: [String: String] = [
    // common
    "common.close": "Schließen",
    "common.save": "Speichern",
    "common.settings": "Einstellungen",
    "common.amount": "Betrag",
    "common.receive": "Empfangen",
    "common.send": "Senden",
    "common.copyAddress": "Adresse kopieren",
    "common.copied": "Kopiert!",

    // menu
    "menu.network": "Netzwerk",
    "menu.refresh": "Aktualisieren",
    "menu.deleteWallet": "Wallet löschen",
    "menu.language": "언어 / Language",

    // home
    "home.title": "Wallet",
    "home.balance": "Guthaben",
    "home.tokenNotIssued": "%@ ist in diesem Netzwerk nicht ausgegeben",
    "home.stakingTitle": "Staking & Node-Belohnungen",
    "home.stakingSubtitle": "Belohnungen durch Staking von KVR verdienen · Node-Betreiber-Belohnungen",
    "home.inferenceTitle": "KI-Inferenz",
    "home.inferenceSubtitle": "Mit KVR bezahlen und Kvasir-Inferenz ausführen",
    "home.nodeSettingsTitle": "Mobile Node-Einstellungen",
    "home.nodeSettingsSubtitle": "Dieses Gerät zum Inferenz-Node machen · MLX · Modus · Live-Anzeigen",
    "home.nodeMonitorTitle": "Node-Leistung · Beitrag",
    "home.nodeMonitorSubtitle": "Leistungsstufe, Beitrag und Belohnungen deines Nodes prüfen",
    "home.history": "Transaktionsverlauf",
    "home.noTransactions": "Noch keine Transaktionen",

    // onboarding
    "onboarding.subtitle": "Solana-basiertes KVR-Token-Wallet · devnet",
    "onboarding.create": "Neues Wallet erstellen",
    "onboarding.import": "Vorhandenes Wallet wiederherstellen",

    // create wallet
    "create.title": "Neues Wallet",
    "create.phraseHeader": "Wiederherstellungsphrase (12 Wörter)",
    "create.phraseWarning": "Dies ist der einzige Weg, dein Wallet wiederherzustellen. Schreibe sie an einem sicheren Ort auf. Gib sie niemals weiter.",
    "create.savedToggle": "Ich habe meine Wiederherstellungsphrase sicher gespeichert",
    "create.start": "Wallet starten",

    // import wallet
    "import.title": "Wallet wiederherstellen",
    "import.phraseHeader": "Wiederherstellungsphrase",
    "import.instruction": "Gib 12 oder 24 Wörter durch Leerzeichen getrennt ein.",
    "import.restore": "Wiederherstellen",
    "import.invalidPhrase": "Die Wiederherstellungsphrase ist ungültig.",

    // send
    "send.asset": "Asset",
    "send.balance": "Guthaben %@ %@",
    "send.recipient": "Empfängeradresse",
    "send.solanaAddress": "Solana-Adresse",
    "send.sent": "Gesendet",
    "send.viewInExplorer": "In explorer ansehen",
    "send.invalidAmount": "Der Betrag ist ungültig.",

    // staking
    "staking.title": "Staking",
    "staking.stake": "Staken",
    "staking.needOnline": "Staking erfordert, dass dieses Gerät als Node registriert und online ist.",
    "staking.guideLink": "So funktioniert Staking",
    "staking.recentTx": "Neueste Transaktion in explorer ansehen",
    "staking.staked": "Gestakt",
    "staking.reward": "Belohnung",
    "staking.unstakeAll": "Alles entstaken",
    "staking.nodeOperatorRewards": "Node-Betreiber-Belohnungen",
    "staking.claimableRewards": "Beanspruchbare Belohnungen",
    "staking.nodeIdPlaceholder": "Node-ID (z. B. node-slot-1)",
    "staking.register": "Registrieren",
    "staking.claim": "Belohnungen beanspruchen",
    "staking.nodeStatusLink": "Node-Betrieb ansehen",
    "staking.serviceUrlTitle": "Staking-Service-URL",
    "staking.serviceUrlDesc": "Die Adresse des Abrechnungs-Backends. Stelle sie auf die IP deines Macs im selben Netzwerk ein.",

    // staking guide
    "guide.navTitle": "Staking-Anleitung",
    "guide.headerTitle": "KVR-Staking-Leitfaden",
    "guide.headerSubtitle": "Wie du durch Staking Belohnungen und durch den Betrieb eines Nodes zusätzliche Belohnungen verdienst",
    "guide.whatTitle": "Was ist Staking?",
    "guide.whatBody": "Wenn du deine KVR stakst, sammeln sich Belohnungen in Echtzeit gemäß dem Jahreszins (APR) an. Du kannst jederzeit entstaken, um dein Kapital und deine Belohnungen zusammen abzuheben.",
    "guide.howTitle": "So stakst du",
    "guide.howStep1": "Stelle im Startbildschirm sicher, dass das Netzwerk **Devnet** ist. Staking läuft auf devnet.",
    "guide.howStep2": "Gib im Staking-Bildschirm die zu stakende **KVR-Menge** ein.",
    "guide.howStep3": "Tippe auf **Staken**, und dein Wallet sendet die KVR an den Staking-Vault. (Für die Überweisungsgebühr wird etwas SOL benötigt.)",
    "guide.howStep4": "Dein Stake wird registriert und Belohnungen sammeln sich in Echtzeit gemäß der **APR** an.",
    "guide.howStep5": "Tippe auf **Alles entstaken**, um dein Kapital plus aufgelaufene Belohnungen an dein Wallet zurückzugeben.",
    "guide.nodeStep1": "Registriere im Staking-Bildschirm deine **Node-ID**, um sie mit deinem Wallet zu verknüpfen.",
    "guide.nodeStep2": "Wenn der Kvasir-Hub den Inferenzbeitrag deines Nodes meldet, sammeln sich Belohnungen an.",
    "guide.nodeStep3": "Überwache Status, Beitrag und Belohnungen unter **Node-Betrieb**.",
    "guide.nodeStep4": "Nutze **Belohnungen beanspruchen**, um aufgelaufene Belohnungen in dein Wallet zu erhalten.",
    "guide.cautionTitle": "Wichtige Hinweise",
    "guide.caution1": "Dies befindet sich derzeit im **devnet-Test**. Dies sind keine echten Vermögenswerte.",
    "guide.caution2": "Da die Abrechnung off-chain erfolgt, werden Einlagen vom Abrechnungs-Vault verwahrt. (Spätere Migration zu einem On-Chain-Programm.)",
    "guide.caution3": "Überweisungen erfordern eine kleine Menge **SOL für Gas**.",
    "guide.caution4": "Stelle die Adresse des Abrechnungsservers über das ⚙️ oben rechts im Staking-Bildschirm ein.",

    // node monitor
    "monitor.navTitle": "Node-Betrieb",
    "monitor.connectDevice": "Gerät verbinden",
    "monitor.nodes": "Nodes",
    "monitor.online": "Online",
    "monitor.hubTitle": "Hub-Verbindung",
    "monitor.hubServing": "Aktiv",
    "monitor.hubConnectedIdle": "Verbunden · inaktiv",
    "monitor.hubDisconnected": "Getrennt",
    "monitor.hubNone": "Kein Hub verbunden — in den Node-Einstellungen verbinden",
    "monitor.rawContribUnit": "Roher Beitrag (unit)",
    "monitor.effectiveWeighted": "Effektiver Beitrag (leistungsgewichtet)",
    "monitor.lifetimeRewards": "Gesamtbelohnungen (KVR)",
    "monitor.claimableKVR": "Beanspruchbar (KVR)",
    "monitor.tierTitle": "Leistungsbasierte Neuberechnung des Beitrags",
    "monitor.tierDesc": "Nodes werden nach gemessenem Durchsatz (tok/s) eingestuft und ein Belohnungsmultiplikator angewendet. Schnellere Nodes verdienen mehr für dieselbe Arbeit.",
    "monitor.perfTier": "Leistungsstufe %@ · ×%@",
    "monitor.rawContribInline": "Roh %@",
    "monitor.effectiveInline": "→ Effektiv %@",
    "monitor.effectiveContrib": "Effektiver Beitrag",
    "monitor.claimable": "Beanspruchbar",
    "monitor.received": "Erhalten",
    "monitor.lastReport": "Letzte Meldung ",
    "monitor.noReport": "Noch keine Beitragsmeldungen",
    "monitor.noPerfData": "Keine Leistungsdaten",
    "monitor.emptyTitle": "Keine Geräte verbunden",
    "monitor.emptyDesc": "Nutze ⊕ (Gerät verbinden) oben rechts, um dieses oder ein anderes Gerät zu verbinden.",

    // node mode labels
    "mode.localShard": "Lokaler Shard",
    "mode.rpcWorker": "RPC-Worker",

    // node status pills
    "status.online": "Online",
    "status.idle": "Inaktiv",
    "status.registered": "Registriert",
    "status.offline": "Offline",

    // device connect
    "device.iosDevice": "iOS-Gerät",
    "device.accountAddressPlaceholder": "<Kontoadresse>",
    "device.navTitle": "Konto · Gerät verbinden",
    "device.myAccount": "Mein Konto",
    "device.myAccountDesc": "Dein Konto ist deine Wallet-Adresse. Verbinde mehrere Geräte mit dieser Adresse.",
    "device.connectThis": "Dieses Gerät verbinden",
    "device.connectThisDesc": "Verbinde dieses %@ als Node mit deinem Konto. (OS: iOS · NPU)",
    "device.connectOther": "Anderes Gerät verbinden",
    "device.connectOtherDesc": "Führe auf einem Desktop/Laptop (macOS · Windows · Linux) mit installiertem Node.js den folgenden Befehl aus solana/node-client aus.",
    "device.copyCommand": "Befehl kopieren",

    // node settings
    "nodeSettings.backendTitle": "Compute-Backend",
    "nodeSettings.backendDesc": "Die Recheneinheit, die dieses Gerät für die Inferenz nutzt · iOS-GPU nutzt MLX (Metal)",
    "nodeSettings.modeTitle": "Node-Modus",
    "nodeSettings.modeLocalTitle": "Lokaler Shard (empfohlen)",
    "nodeSettings.modeLocalDesc": "Führt Layer-Shards lokal auf dem Gerät aus und leitet nur Aktivierungen weiter — am schnellsten",
    "nodeSettings.modeRpcDesc": "Der Hub führt Tensor-Operationen remote aus — einfach, aber wegen Netzwerklatenz langsamer",
    "nodeSettings.resourceTitle": "Geschätzte Ressourcenauswirkung",
    "nodeSettings.tokPerSecCaption": "tok/s (0.5B Q8, geschätzt)",
    "nodeSettings.memImpact": "Speichernutzung",
    "nodeSettings.thermal": "Wärme",
    "nodeSettings.performance": "Leistung",
    "nodeSettings.chargingOnly": "Nur beim Laden teilnehmen",
    "nodeSettings.runLive": "Node ausführen (live)",
    "nodeSettings.liveGauges": "Live-Anzeigen",
    "nodeSettings.charging": "Lädt",
    "nodeSettings.battery": "Batterie",
    "nodeSettings.cpuLoad": "CPU-Auslastung",
    "nodeSettings.thermalState": "Wärmezustand",
    "nodeSettings.estimated": "Geschätzt",
    "nodeSettings.gaugeNote": "RAM, CPU und Wärme werden gemessen; die GPU wird auf Basis von Messungen geschätzt.",

    // genesis gateway
    "genesis.title": "Genesis-Gateway",
    "genesis.desc": "Der Koordinationsknoten des Netzwerks. Abrechnung, Inferenz und Node-Belohnungen laufen über dieses Gateway.",
    "genesis.reachable": "Erreichbar",
    "genesis.unreachable": "Nicht erreichbar",
    "genesis.active": "Aktives Gateway",
    "genesis.advertised": "Vom Gateway angekündigte öffentliche URL",
    "genesis.cluster": "Cluster",
    "genesis.rewardPerUnit": "Belohnung pro Einheit",
    "genesis.gatewayBonus": "Gateway-Host-Bonus",
    "inference.clear": "Chat löschen",
    "inference.credits": "Guthaben",
    "inference.currentBalance": "Aktueller Kontostand",
    "inference.topUpTitle": "Guthaben aufladen",
    "inference.topUpNote": "KVR beim Gateway einzahlen, um Guthaben für Inferenz zu erhalten. Antworten streamen, langsame Modelle laufen nicht in einen Timeout.",
    "inference.topUpAmount": "Betrag (KVR)",
    "inference.topUpConfirm": "KVR einzahlen · Guthaben aufladen",
    "inference.topUpOk": "Guthaben aufgeladen",
    "inference.topUpFailed": "Aufladen fehlgeschlagen",
    "inference.topUpInvalid": "Gültigen Betrag eingeben",
    "inference.topUpNoVault": "Gateway-Vault-Adresse konnte nicht geladen werden",
    "home.more": "Mehr",
    "history.title": "Transaktionsverlauf",

    // thermal words
    "thermal.low": "Niedrig",
    "thermal.medium": "Mittel",
    "thermal.high": "Hoch",
    "thermal.critical": "Kritisch",

    // node profile notes
    "nodeProfile.mlxNote": "Nutzt MLX-Metal-Kernel, um die Apple-GPU/ANE auszuschöpfen. Beste Leistung im lokalen Shard-Modus.",
    "nodeProfile.cpuNote": "Stabiles Decoding für kleine Modelle. Wird bei Dauerlast heiß.",

    // inference
    "inference.paymentTx": "Zahlungstransaktion in explorer ansehen",
    "inference.model": "Modell",
    "inference.loadingModels": "Modelle werden geladen…",
    "inference.prompt": "Prompt",
    "inference.estQuote": "Geschätzter Kostenvoranschlag",
    "inference.estTokens": "Geschätzte Tokens %d (Prompt %d + Generierung ~%d)",
    "inference.payAndRun": "Bezahlen und ausführen (~%@ %@)",
    "inference.actualBillingNote": "Die tatsächliche Abrechnung wird nach dem Lauf anhand der verwendeten Tokens unten erfasst.",
    "inference.result": "Ergebnis",
    "inference.actualTokens": "Tatsächlich verwendete Tokens",
    "inference.totalTok": "Gesamt %d tok",
    "inference.usageRow": "Prompt %d · Gen. %d · gesamt %d tok",

    // errors / status
    "error.setStakingUrl": "Bitte lege die Staking-Service-URL fest.",
    "error.setServiceUrl": "Bitte lege die Service-URL fest.",
]

private let nl: [String: String] = [
    // common
    "common.close": "Sluiten",
    "common.save": "Opslaan",
    "common.settings": "Instellingen",
    "common.amount": "Bedrag",
    "common.receive": "Ontvangen",
    "common.send": "Verzenden",
    "common.copyAddress": "Adres kopiëren",
    "common.copied": "Gekopieerd!",

    // menu
    "menu.network": "Netwerk",
    "menu.refresh": "Vernieuwen",
    "menu.deleteWallet": "Wallet verwijderen",
    "menu.language": "언어 / Language",

    // home
    "home.title": "Wallet",
    "home.balance": "Saldo",
    "home.tokenNotIssued": "%@ is niet uitgegeven op dit netwerk",
    "home.stakingTitle": "Staking & node-beloningen",
    "home.stakingSubtitle": "Verdien beloningen door KVR te staken · node-operatorbeloningen",
    "home.inferenceTitle": "AI-inferentie",
    "home.inferenceSubtitle": "Betaal met KVR om Kvasir-inferentie uit te voeren",
    "home.nodeSettingsTitle": "Mobiele node-instellingen",
    "home.nodeSettingsSubtitle": "Maak van dit apparaat een inferentienode · MLX · modus · live-meters",
    "home.nodeMonitorTitle": "Node-prestaties · bijdrage",
    "home.nodeMonitorSubtitle": "Bekijk de prestatieklasse, bijdrage en beloningen van je node",
    "home.history": "Transactiegeschiedenis",
    "home.noTransactions": "Nog geen transacties",

    // onboarding
    "onboarding.subtitle": "Op Solana gebaseerde KVR-token-wallet · devnet",
    "onboarding.create": "Nieuwe wallet maken",
    "onboarding.import": "Bestaande wallet herstellen",

    // create wallet
    "create.title": "Nieuwe wallet",
    "create.phraseHeader": "Herstelzin (12 woorden)",
    "create.phraseWarning": "Dit is de enige manier om je wallet te herstellen. Schrijf hem op een veilige plek op. Deel hem nooit.",
    "create.savedToggle": "Ik heb mijn herstelzin veilig opgeslagen",
    "create.start": "Wallet starten",

    // import wallet
    "import.title": "Wallet herstellen",
    "import.phraseHeader": "Herstelzin",
    "import.instruction": "Voer 12 of 24 woorden in, gescheiden door spaties.",
    "import.restore": "Herstellen",
    "import.invalidPhrase": "De herstelzin is ongeldig.",

    // send
    "send.asset": "Asset",
    "send.balance": "Saldo %@ %@",
    "send.recipient": "Ontvangeradres",
    "send.solanaAddress": "Solana-adres",
    "send.sent": "Verzonden",
    "send.viewInExplorer": "Bekijken in explorer",
    "send.invalidAmount": "Het bedrag is ongeldig.",

    // staking
    "staking.title": "Staking",
    "staking.stake": "Staken",
    "staking.needOnline": "Staking vereist dat dit apparaat als node is geregistreerd en online is.",
    "staking.guideLink": "Zo stake je",
    "staking.recentTx": "Bekijk de laatste transactie in explorer",
    "staking.staked": "Gestaket",
    "staking.reward": "Beloning",
    "staking.unstakeAll": "Alles unstaken",
    "staking.nodeOperatorRewards": "Node-operatorbeloningen",
    "staking.claimableRewards": "Opeisbare beloningen",
    "staking.nodeIdPlaceholder": "Node-ID (bijv. node-slot-1)",
    "staking.register": "Registreren",
    "staking.claim": "Beloningen opeisen",
    "staking.nodeStatusLink": "Node-activiteit bekijken",
    "staking.serviceUrlTitle": "Staking-service-URL",
    "staking.serviceUrlDesc": "Het adres van de afwikkelingsbackend. Stel dit in op het IP-adres van je Mac in hetzelfde netwerk.",

    // staking guide
    "guide.navTitle": "Staking-uitleg",
    "guide.headerTitle": "KVR-stakinggids",
    "guide.headerSubtitle": "Hoe je beloningen verdient met staking en extra beloningen door een node te draaien",
    "guide.whatTitle": "Wat is staking?",
    "guide.whatBody": "Wanneer je je KVR staket, lopen beloningen in realtime op volgens het jaarlijkse rendement (APR). Je kunt op elk moment unstaken om je inleg en beloningen samen op te nemen.",
    "guide.howTitle": "Zo stake je",
    "guide.howStep1": "Controleer op het startscherm dat het netwerk **Devnet** is. Staking werkt op devnet.",
    "guide.howStep2": "Voer op het stakingscherm het **aantal KVR** in dat je wilt staken.",
    "guide.howStep3": "Tik op **Staken** en je wallet stuurt de KVR naar de staking-vault. (Er is een kleine hoeveelheid SOL nodig voor de overdrachtskosten.)",
    "guide.howStep4": "Je stake wordt geregistreerd en beloningen lopen in realtime op volgens de **APR**.",
    "guide.howStep5": "Tik op **Alles unstaken** om je inleg plus opgebouwde beloningen terug te sturen naar je wallet.",
    "guide.nodeStep1": "Registreer op het stakingscherm je **node-ID** om hem aan je wallet te koppelen.",
    "guide.nodeStep2": "Wanneer de Kvasir-hub de inferentiebijdrage van je node meldt, lopen beloningen op.",
    "guide.nodeStep3": "Volg status, bijdrage en beloningen in **Node-activiteit**.",
    "guide.nodeStep4": "Gebruik **Beloningen opeisen** om opgebouwde beloningen naar je wallet te ontvangen.",
    "guide.cautionTitle": "Belangrijke opmerkingen",
    "guide.caution1": "Dit is momenteel in **devnet-test**. Dit zijn geen echte activa.",
    "guide.caution2": "Omdat de afwikkeling off-chain is, worden stortingen bewaard door de afwikkelingsvault. (Later migratie naar een on-chain-programma.)",
    "guide.caution3": "Overdrachten vereisen een kleine hoeveelheid **SOL voor gas**.",
    "guide.caution4": "Stel het adres van de afwikkelingsserver in via het ⚙️ rechtsboven op het stakingscherm.",

    // node monitor
    "monitor.navTitle": "Node-activiteit",
    "monitor.connectDevice": "Apparaat verbinden",
    "monitor.nodes": "Nodes",
    "monitor.online": "Online",
    "monitor.hubTitle": "Hub-verbinding",
    "monitor.hubServing": "Actief",
    "monitor.hubConnectedIdle": "Verbonden · inactief",
    "monitor.hubDisconnected": "Verbroken",
    "monitor.hubNone": "Geen hub verbonden — verbind er een in Node-instellingen",
    "monitor.rawContribUnit": "Ruwe bijdrage (unit)",
    "monitor.effectiveWeighted": "Effectieve bijdrage (prestatiegewogen)",
    "monitor.lifetimeRewards": "Totale beloningen (KVR)",
    "monitor.claimableKVR": "Opeisbaar (KVR)",
    "monitor.tierTitle": "Prestatiegebaseerde herberekening van bijdrage",
    "monitor.tierDesc": "Nodes worden ingedeeld op basis van gemeten doorvoer (tok/s) en er wordt een beloningsvermenigvuldiger toegepast. Snellere nodes verdienen meer voor hetzelfde werk.",
    "monitor.perfTier": "Prestatieklasse %@ · ×%@",
    "monitor.rawContribInline": "Ruw %@",
    "monitor.effectiveInline": "→ Effectief %@",
    "monitor.effectiveContrib": "Effectieve bijdrage",
    "monitor.claimable": "Opeisbaar",
    "monitor.received": "Ontvangen",
    "monitor.lastReport": "Laatste rapport ",
    "monitor.noReport": "Nog geen bijdragerapporten",
    "monitor.noPerfData": "Geen prestatiegegevens",
    "monitor.emptyTitle": "Geen apparaten verbonden",
    "monitor.emptyDesc": "Gebruik ⊕ (Apparaat verbinden) rechtsboven om dit of een ander apparaat te verbinden.",

    // node mode labels
    "mode.localShard": "Lokale shard",
    "mode.rpcWorker": "RPC-worker",

    // node status pills
    "status.online": "Online",
    "status.idle": "Inactief",
    "status.registered": "Geregistreerd",
    "status.offline": "Offline",

    // device connect
    "device.iosDevice": "iOS-apparaat",
    "device.accountAddressPlaceholder": "<accountadres>",
    "device.navTitle": "Account · Apparaat verbinden",
    "device.myAccount": "Mijn account",
    "device.myAccountDesc": "Je account is je wallet-adres. Verbind meerdere apparaten met dit adres.",
    "device.connectThis": "Dit apparaat verbinden",
    "device.connectThisDesc": "Verbind dit %@ als node met je account. (OS: iOS · NPU)",
    "device.connectOther": "Ander apparaat verbinden",
    "device.connectOtherDesc": "Voer op een desktop/laptop (macOS · Windows · Linux) met Node.js geïnstalleerd de onderstaande opdracht uit vanuit solana/node-client.",
    "device.copyCommand": "Opdracht kopiëren",

    // node settings
    "nodeSettings.backendTitle": "Compute-backend",
    "nodeSettings.backendDesc": "De rekeneenheid die dit apparaat voor inferentie gebruikt · iOS-GPU gebruikt MLX (Metal)",
    "nodeSettings.modeTitle": "Node-modus",
    "nodeSettings.modeLocalTitle": "Lokale shard (aanbevolen)",
    "nodeSettings.modeLocalDesc": "Voert layer-shards lokaal op het apparaat uit en relayt alleen activaties — het snelst",
    "nodeSettings.modeRpcDesc": "De hub voert tensorbewerkingen op afstand uit — eenvoudig maar trager door netwerklatentie",
    "nodeSettings.resourceTitle": "Geschatte impact op resources",
    "nodeSettings.tokPerSecCaption": "tok/s (0.5B Q8, geschat)",
    "nodeSettings.memImpact": "Geheugengebruik",
    "nodeSettings.thermal": "Warmte",
    "nodeSettings.performance": "Prestaties",
    "nodeSettings.chargingOnly": "Alleen deelnemen tijdens opladen",
    "nodeSettings.runLive": "Node draaien (live)",
    "nodeSettings.liveGauges": "Live-meters",
    "nodeSettings.charging": "Opladen",
    "nodeSettings.battery": "Batterij",
    "nodeSettings.cpuLoad": "CPU-belasting",
    "nodeSettings.thermalState": "Warmtestatus",
    "nodeSettings.estimated": "Geschat",
    "nodeSettings.gaugeNote": "RAM, CPU en warmte zijn gemeten; de GPU is een schatting op basis van metingen.",

    // genesis gateway
    "genesis.title": "Genesis-gateway",
    "genesis.desc": "De coördinatienode van het netwerk. Afwikkeling, inferentie en node-beloningen lopen via deze gateway.",
    "genesis.reachable": "Bereikbaar",
    "genesis.unreachable": "Onbereikbaar",
    "genesis.active": "Actieve gateway",
    "genesis.advertised": "Openbare URL aangekondigd door de gateway",
    "genesis.cluster": "Cluster",
    "genesis.rewardPerUnit": "Beloning per eenheid",
    "genesis.gatewayBonus": "Gateway-hostbonus",
    "inference.clear": "Chat wissen",
    "inference.credits": "Tegoed",
    "inference.currentBalance": "Huidig saldo",
    "inference.topUpTitle": "Tegoed toevoegen",
    "inference.topUpNote": "Stort KVR bij de gateway voor inferentietegoed. Antwoorden streamen, dus trage modellen verlopen niet.",
    "inference.topUpAmount": "Bedrag (KVR)",
    "inference.topUpConfirm": "KVR storten · tegoed toevoegen",
    "inference.topUpOk": "Tegoed toegevoegd",
    "inference.topUpFailed": "Opwaarderen mislukt",
    "inference.topUpInvalid": "Voer een geldig bedrag in",
    "inference.topUpNoVault": "Kon het vault-adres van de gateway niet laden",
    "home.more": "Meer",
    "history.title": "Transactiegeschiedenis",

    // thermal words
    "thermal.low": "Laag",
    "thermal.medium": "Gemiddeld",
    "thermal.high": "Hoog",
    "thermal.critical": "Kritiek",

    // node profile notes
    "nodeProfile.mlxNote": "Gebruikt MLX Metal-kernels om de Apple-GPU/ANE te benutten. Beste prestaties in lokale-shardmodus.",
    "nodeProfile.cpuNote": "Stabiele decodering voor kleine modellen. Wordt warm bij aanhoudende belasting.",

    // inference
    "inference.paymentTx": "Betalingstransactie bekijken in explorer",
    "inference.model": "Model",
    "inference.loadingModels": "Modellen laden…",
    "inference.prompt": "Prompt",
    "inference.estQuote": "Geschatte prijsopgave",
    "inference.estTokens": "Geschatte tokens %d (prompt %d + generatie ~%d)",
    "inference.payAndRun": "Betalen en uitvoeren (~%@ %@)",
    "inference.actualBillingNote": "De werkelijke kosten worden hieronder geregistreerd op basis van de gebruikte tokens na de uitvoering.",
    "inference.result": "Resultaat",
    "inference.actualTokens": "Werkelijk gebruikte tokens",
    "inference.totalTok": "Totaal %d tok",
    "inference.usageRow": "Prompt %d · gen. %d · totaal %d tok",

    // errors / status
    "error.setStakingUrl": "Stel de staking-service-URL in.",
    "error.setServiceUrl": "Stel de service-URL in.",
]

private let idn: [String: String] = [
    // common
    "common.close": "Tutup",
    "common.save": "Simpan",
    "common.settings": "Pengaturan",
    "common.amount": "Jumlah",
    "common.receive": "Terima",
    "common.send": "Kirim",
    "common.copyAddress": "Salin alamat",
    "common.copied": "Tersalin!",

    // menu
    "menu.network": "Jaringan",
    "menu.refresh": "Segarkan",
    "menu.deleteWallet": "Hapus dompet",
    "menu.language": "언어 / Language",

    // home
    "home.title": "Dompet",
    "home.balance": "Saldo",
    "home.tokenNotIssued": "%@ belum diterbitkan di jaringan ini",
    "home.stakingTitle": "Staking & imbalan node",
    "home.stakingSubtitle": "Dapatkan imbalan dengan staking KVR · imbalan operator node",
    "home.inferenceTitle": "Inferensi AI",
    "home.inferenceSubtitle": "Bayar dengan KVR untuk menjalankan inferensi Kvasir",
    "home.nodeSettingsTitle": "Pengaturan node ponsel",
    "home.nodeSettingsSubtitle": "Jadikan perangkat ini node inferensi · MLX · mode · pengukur langsung",
    "home.nodeMonitorTitle": "Performa node · kontribusi",
    "home.nodeMonitorSubtitle": "Periksa tingkat performa, kontribusi, dan imbalan node Anda",
    "home.history": "Riwayat transaksi",
    "home.noTransactions": "Belum ada transaksi",

    // onboarding
    "onboarding.subtitle": "Dompet token KVR berbasis Solana · devnet",
    "onboarding.create": "Buat dompet baru",
    "onboarding.import": "Pulihkan dompet yang ada",

    // create wallet
    "create.title": "Dompet baru",
    "create.phraseHeader": "Frasa pemulihan (12 kata)",
    "create.phraseWarning": "Ini satu-satunya cara memulihkan dompet Anda. Catat di tempat yang aman. Jangan pernah membagikannya.",
    "create.savedToggle": "Saya telah menyimpan frasa pemulihan dengan aman",
    "create.start": "Mulai dompet",

    // import wallet
    "import.title": "Pulihkan dompet",
    "import.phraseHeader": "Frasa pemulihan",
    "import.instruction": "Masukkan 12 atau 24 kata yang dipisahkan spasi.",
    "import.restore": "Pulihkan",
    "import.invalidPhrase": "Frasa pemulihan tidak valid.",

    // send
    "send.asset": "Aset",
    "send.balance": "Saldo %@ %@",
    "send.recipient": "Alamat penerima",
    "send.solanaAddress": "Alamat Solana",
    "send.sent": "Terkirim",
    "send.viewInExplorer": "Lihat di explorer",
    "send.invalidAmount": "Jumlah tidak valid.",

    // staking
    "staking.title": "Staking",
    "staking.stake": "Staking",
    "staking.needOnline": "Staking memerlukan perangkat ini terdaftar sebagai node dan online.",
    "staking.guideLink": "Cara staking",
    "staking.recentTx": "Lihat transaksi terbaru di explorer",
    "staking.staked": "Di-stake",
    "staking.reward": "Imbalan",
    "staking.unstakeAll": "Unstake semua",
    "staking.nodeOperatorRewards": "Imbalan operator node",
    "staking.claimableRewards": "Imbalan yang dapat diklaim",
    "staking.nodeIdPlaceholder": "ID node (mis. node-slot-1)",
    "staking.register": "Daftar",
    "staking.claim": "Klaim imbalan",
    "staking.nodeStatusLink": "Lihat operasi node",
    "staking.serviceUrlTitle": "URL layanan staking",
    "staking.serviceUrlDesc": "Alamat backend penyelesaian. Setel ke IP Mac Anda di jaringan yang sama.",

    // staking guide
    "guide.navTitle": "Panduan staking",
    "guide.headerTitle": "Panduan staking KVR",
    "guide.headerSubtitle": "Cara mendapatkan imbalan lewat staking dan imbalan tambahan dengan menjalankan node",
    "guide.whatTitle": "Apa itu staking?",
    "guide.whatBody": "Saat Anda men-stake KVR, imbalan bertambah secara real-time sesuai suku bunga tahunan (APR). Anda dapat unstake kapan saja untuk menarik pokok dan imbalan sekaligus.",
    "guide.howTitle": "Cara staking",
    "guide.howStep1": "Di layar utama, pastikan jaringannya **Devnet**. Staking berjalan di devnet.",
    "guide.howStep2": "Di layar staking, masukkan **jumlah KVR** yang akan di-stake.",
    "guide.howStep3": "Ketuk **Staking** dan dompet Anda mengirim KVR ke vault staking. (Diperlukan sedikit SOL untuk biaya transfer.)",
    "guide.howStep4": "Stake Anda terdaftar dan imbalan bertambah secara real-time berdasarkan **APR**.",
    "guide.howStep5": "Ketuk **Unstake semua** untuk mengembalikan pokok plus imbalan yang terkumpul ke dompet Anda.",
    "guide.nodeStep1": "Di layar staking, daftarkan **ID node** untuk menautkannya ke dompet Anda.",
    "guide.nodeStep2": "Saat hub Kvasir melaporkan kontribusi inferensi node Anda, imbalan bertambah.",
    "guide.nodeStep3": "Pantau status, kontribusi, dan imbalan di **Operasi node**.",
    "guide.nodeStep4": "Gunakan **Klaim imbalan** untuk menerima imbalan yang terkumpul ke dompet Anda.",
    "guide.cautionTitle": "Catatan penting",
    "guide.caution1": "Saat ini dalam tahap **pengujian devnet**. Ini bukan aset nyata.",
    "guide.caution2": "Karena penyelesaian bersifat off-chain, setoran disimpan oleh vault penyelesaian. (Akan bermigrasi ke program on-chain nanti.)",
    "guide.caution3": "Transfer memerlukan sedikit **SOL untuk gas**.",
    "guide.caution4": "Setel alamat server penyelesaian melalui ⚙️ di kanan atas layar staking.",

    // node monitor
    "monitor.navTitle": "Operasi node",
    "monitor.connectDevice": "Hubungkan perangkat",
    "monitor.nodes": "Node",
    "monitor.online": "Daring",
    "monitor.hubTitle": "Koneksi hub",
    "monitor.hubServing": "Melayani",
    "monitor.hubConnectedIdle": "Terhubung · siaga",
    "monitor.hubDisconnected": "Terputus",
    "monitor.hubNone": "Tidak ada hub terhubung — hubungkan di Pengaturan node",
    "monitor.rawContribUnit": "Kontribusi mentah (unit)",
    "monitor.effectiveWeighted": "Kontribusi efektif (berbobot performa)",
    "monitor.lifetimeRewards": "Imbalan kumulatif (KVR)",
    "monitor.claimableKVR": "Dapat diklaim (KVR)",
    "monitor.tierTitle": "Penghitungan ulang kontribusi berbasis performa",
    "monitor.tierDesc": "Node diperingkat berdasarkan throughput terukur (tok/s) dan pengali imbalan diterapkan. Node yang lebih cepat mendapat lebih banyak untuk pekerjaan yang sama.",
    "monitor.perfTier": "Tingkat performa %@ · ×%@",
    "monitor.rawContribInline": "Mentah %@",
    "monitor.effectiveInline": "→ Efektif %@",
    "monitor.effectiveContrib": "Kontribusi efektif",
    "monitor.claimable": "Dapat diklaim",
    "monitor.received": "Diterima",
    "monitor.lastReport": "Laporan terakhir ",
    "monitor.noReport": "Belum ada laporan kontribusi",
    "monitor.noPerfData": "Tidak ada data performa",
    "monitor.emptyTitle": "Tidak ada perangkat terhubung",
    "monitor.emptyDesc": "Gunakan ⊕ (Hubungkan perangkat) di kanan atas untuk menghubungkan perangkat ini atau perangkat lain.",

    // node mode labels
    "mode.localShard": "Shard lokal",
    "mode.rpcWorker": "Worker RPC",

    // node status pills
    "status.online": "Daring",
    "status.idle": "Menganggur",
    "status.registered": "Terdaftar",
    "status.offline": "Luring",

    // device connect
    "device.iosDevice": "Perangkat iOS",
    "device.accountAddressPlaceholder": "<alamat akun>",
    "device.navTitle": "Akun · Hubungkan perangkat",
    "device.myAccount": "Akun saya",
    "device.myAccountDesc": "Akun Anda adalah alamat dompet Anda. Hubungkan beberapa perangkat ke alamat ini.",
    "device.connectThis": "Hubungkan perangkat ini",
    "device.connectThisDesc": "Hubungkan %@ ini ke akun Anda sebagai node. (OS: iOS · NPU)",
    "device.connectOther": "Hubungkan perangkat lain",
    "device.connectOtherDesc": "Di desktop/laptop (macOS · Windows · Linux) dengan Node.js terpasang, jalankan perintah di bawah dari solana/node-client.",
    "device.copyCommand": "Salin perintah",

    // node settings
    "nodeSettings.backendTitle": "Backend komputasi",
    "nodeSettings.backendDesc": "Unit komputasi yang digunakan perangkat ini untuk inferensi · GPU iOS memakai MLX (Metal)",
    "nodeSettings.modeTitle": "Mode node",
    "nodeSettings.modeLocalTitle": "Shard lokal (disarankan)",
    "nodeSettings.modeLocalDesc": "Menjalankan shard lapisan secara lokal di perangkat dan hanya merelai aktivasi — tercepat",
    "nodeSettings.modeRpcDesc": "Hub menjalankan operasi tensor dari jarak jauh — sederhana tetapi lebih lambat karena latensi jaringan",
    "nodeSettings.resourceTitle": "Perkiraan dampak sumber daya",
    "nodeSettings.tokPerSecCaption": "tok/s (0.5B Q8, perkiraan)",
    "nodeSettings.memImpact": "Penggunaan memori",
    "nodeSettings.thermal": "Panas",
    "nodeSettings.performance": "Performa",
    "nodeSettings.chargingOnly": "Ikut serta hanya saat mengisi daya",
    "nodeSettings.runLive": "Jalankan node (langsung)",
    "nodeSettings.liveGauges": "Pengukur langsung",
    "nodeSettings.charging": "Mengisi daya",
    "nodeSettings.battery": "Baterai",
    "nodeSettings.cpuLoad": "Beban CPU",
    "nodeSettings.thermalState": "Status panas",
    "nodeSettings.estimated": "Perkiraan",
    "nodeSettings.gaugeNote": "RAM, CPU, dan panas terukur; GPU adalah perkiraan berdasarkan pengukuran.",

    // genesis gateway
    "genesis.title": "Gerbang genesis",
    "genesis.desc": "Node koordinasi jaringan. Penyelesaian, inferensi, dan imbalan node melewati gerbang ini.",
    "genesis.reachable": "Dapat dijangkau",
    "genesis.unreachable": "Tidak dapat dijangkau",
    "genesis.active": "Gerbang aktif",
    "genesis.advertised": "URL publik yang diumumkan gerbang",
    "genesis.cluster": "Klaster",
    "genesis.rewardPerUnit": "Imbalan per unit",
    "genesis.gatewayBonus": "Bonus host gerbang",
    "inference.clear": "Hapus obrolan",
    "inference.credits": "Kredit",
    "inference.currentBalance": "Saldo saat ini",
    "inference.topUpTitle": "Tambah kredit",
    "inference.topUpNote": "Setor KVR ke gateway untuk kredit inferensi. Balasan mengalir (streaming), jadi model lambat tidak timeout.",
    "inference.topUpAmount": "Jumlah (KVR)",
    "inference.topUpConfirm": "Setor KVR · tambah kredit",
    "inference.topUpOk": "Kredit ditambahkan",
    "inference.topUpFailed": "Isi ulang gagal",
    "inference.topUpInvalid": "Masukkan jumlah yang valid",
    "inference.topUpNoVault": "Gagal memuat alamat vault gateway",
    "home.more": "Selengkapnya",
    "history.title": "Riwayat transaksi",

    // thermal words
    "thermal.low": "Rendah",
    "thermal.medium": "Sedang",
    "thermal.high": "Tinggi",
    "thermal.critical": "Kritis",

    // node profile notes
    "nodeProfile.mlxNote": "Menggunakan kernel MLX Metal untuk memaksimalkan GPU/ANE Apple. Performa terbaik dalam mode shard lokal.",
    "nodeProfile.cpuNote": "Dekode stabil untuk model kecil. Menjadi panas saat beban berkelanjutan.",

    // inference
    "inference.paymentTx": "Lihat transaksi pembayaran di explorer",
    "inference.model": "Model",
    "inference.loadingModels": "Memuat model…",
    "inference.prompt": "Prompt",
    "inference.estQuote": "Perkiraan biaya",
    "inference.estTokens": "Perkiraan token %d (prompt %d + generasi ~%d)",
    "inference.payAndRun": "Bayar dan jalankan (~%@ %@)",
    "inference.actualBillingNote": "Penagihan aktual dicatat di bawah berdasarkan token yang digunakan setelah dijalankan.",
    "inference.result": "Hasil",
    "inference.actualTokens": "Token yang benar-benar digunakan",
    "inference.totalTok": "Total %d tok",
    "inference.usageRow": "Prompt %d · gen. %d · total %d tok",

    // errors / status
    "error.setStakingUrl": "Harap setel URL layanan staking.",
    "error.setServiceUrl": "Harap setel URL layanan.",
]
