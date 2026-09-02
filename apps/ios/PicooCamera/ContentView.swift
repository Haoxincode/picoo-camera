import Observation
import SwiftUI
import UIKit

private let picooAppVersion =
    Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "—"

struct ContentView: View {
    @Bindable var model: SenderAppModel
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        ZStack {
            PicooColor.surfacePage.ignoresSafeArea()

            switch model.screen {
            case .devices:
                DevicesView(model: model)
            case .pairing:
                PairingView(model: model)
            case .waiting:
                PairingWaitingView(model: model)
            case .live:
                LiveCameraView(model: model)
            }
        }
        .foregroundStyle(PicooColor.contentPrimary)
        .task { model.start() }
        .onChange(of: scenePhase) { _, phase in
            Task { await model.handleScenePhase(phase) }
        }
        .sheet(isPresented: $model.isManualConnectPresented) {
            ManualConnectSheet(model: model)
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
                .presentationBackground(PicooColor.surfaceGroup)
        }
        .sheet(isPresented: $model.isSettingsPresented) {
            SettingsSheet(model: model)
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
                .presentationBackground(PicooColor.surfaceGroup)
        }
    }
}

private struct DevicesView: View {
    @Bindable var model: SenderAppModel

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: PicooSpace.xl) {
                    VStack(alignment: .leading, spacing: PicooSpace.xs) {
                        Text("把手机变成无线摄像头")
                            .font(.largeTitle.weight(.bold))
                        Text("选择同一 Wi-Fi 下的电脑，已配对设备点按直连。")
                            .font(.subheadline)
                            .foregroundStyle(PicooColor.contentMuted)
                    }

                    discoveryStatus

                    if model.receivers.isEmpty {
                        emptyState
                    } else {
                        receiverList
                    }

                    if let error = model.errorMessage {
                        InlineNotice(text: error, tone: .danger)
                    }

                    manualConnectButton
                }
                .frame(maxWidth: 680, alignment: .leading)
                .padding(.horizontal, PicooSpace.lg)
                .padding(.top, PicooSpace.lg)
                .padding(.bottom, PicooSpace.xl)
            }
            .background(PicooColor.surfacePage)
            .navigationTitle("Picoo Camera")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        model.isSettingsPresented = true
                    } label: {
                        ReiconIcon(icon: .settings)
                            .frame(width: PicooIconSize.standard, height: PicooIconSize.standard)
                    }
                    .buttonStyle(PicooIconButtonStyle())
                    .accessibilityLabel("设置")
                }
            }
        }
    }

    private var discoveryStatus: some View {
        HStack(spacing: PicooSpace.md) {
            ReiconIcon(icon: .discovering)
                .frame(width: PicooIconSize.standard, height: PicooIconSize.standard)
                .foregroundStyle(model.isDiscovering ? PicooColor.actionHighlight : PicooColor.statusSuccess)

            VStack(alignment: .leading, spacing: PicooSpace.xxs) {
                Text(model.isDiscovering ? "正在搜索同一 Wi‑Fi 下的电脑…" : "可以连接同一 Wi‑Fi 下的电脑")
                    .font(.subheadline.weight(.semibold))
                Text("保持电脑端 Picoo Camera 已打开")
                    .font(.caption)
                    .foregroundStyle(PicooColor.contentMuted)
            }

            Spacer()

            Button {
                model.refreshDiscovery()
            } label: {
                ReiconIcon(icon: .refreshDiscovery)
                    .frame(width: PicooIconSize.standard, height: PicooIconSize.standard)
                    .foregroundStyle(PicooColor.actionHighlight)
            }
            .buttonStyle(PicooIconButtonStyle())
            .accessibilityLabel("刷新局域网设备")
        }
    }

    private var receiverList: some View {
        VStack(alignment: .leading, spacing: PicooSpace.sm) {
            HStack {
                Text("可连接的电脑")
                Spacer()
                Text("\(model.receivers.filter(\.isTrusted).count) 台已信任")
                    .font(.caption.monospacedDigit())
            }
            .font(.subheadline.weight(.semibold))
            .foregroundStyle(PicooColor.contentMuted)

            VStack(spacing: 0) {
                ForEach(Array(model.receivers.enumerated()), id: \.offset) { index, receiver in
                    Button {
                        model.selectReceiver(receiver)
                    } label: {
                        ReceiverRow(receiver: receiver, isConnecting: model.isConnecting)
                    }
                    .buttonStyle(.plain)
                    if index != model.receivers.indices.last {
                        Divider().padding(.leading, 56)
                    }
                }
            }
            .background(PicooColor.surfaceGroup, in: RoundedRectangle(cornerRadius: PicooRadius.surface))
        }
    }

    private var emptyState: some View {
        VStack(spacing: PicooSpace.md) {
            ReiconIcon(icon: .receiverDevice)
                .frame(width: PicooIconSize.hero, height: PicooIconSize.hero)
                .foregroundStyle(PicooColor.contentMuted)
            Text("还没有发现电脑")
                .font(.headline)
            Text("确认电脑端 Picoo Camera 已打开，并与手机连接同一 Wi‑Fi。仍未找到时，可输入电脑端显示的 IP 地址直连。")
                .font(.subheadline)
                .foregroundStyle(PicooColor.contentMuted)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, PicooSpace.xxl)
        .padding(.horizontal, PicooSpace.xl)
    }

    private var manualConnectButton: some View {
        Button {
            model.isManualConnectPresented = true
        } label: {
            HStack(spacing: PicooSpace.sm) {
                ReiconIcon(icon: .secureConnection)
                    .frame(width: PicooIconSize.standard, height: PicooIconSize.standard)
                    .foregroundStyle(PicooColor.actionHighlight)
                Text("输入 IP 地址直连")
                    .font(.body.weight(.semibold))
            }
            .frame(maxWidth: .infinity, minHeight: PicooIconSize.touchTarget)
            .background(PicooColor.surfaceGroup, in: RoundedRectangle(cornerRadius: PicooRadius.control))
            .overlay {
                RoundedRectangle(cornerRadius: PicooRadius.control)
                    .stroke(PicooColor.borderDefault, lineWidth: 1)
            }
        }
        .buttonStyle(.plain)
    }
}

private struct ReceiverRow: View {
    let receiver: ReceiverSummary
    let isConnecting: Bool

    var body: some View {
        HStack(spacing: PicooSpace.md) {
            ReiconIcon(icon: .receiverDevice)
                .frame(width: PicooIconSize.emphasis, height: PicooIconSize.emphasis)
                .foregroundStyle(PicooColor.actionHighlight)

            VStack(alignment: .leading, spacing: PicooSpace.xs) {
                HStack(spacing: PicooSpace.sm) {
                    Text(receiver.displayName)
                        .font(.body.weight(.semibold))
                    if receiver.isTrusted {
                        Text("已配对")
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(PicooColor.actionHighlight)
                            .padding(.horizontal, PicooSpace.sm)
                            .padding(.vertical, PicooSpace.xxs)
                            .background(PicooColor.actionHighlight.opacity(0.10), in: Capsule())
                    }
                }
                Text(receiver.endpoint.displayText)
                    .font(.caption2.monospaced())
                    .foregroundStyle(PicooColor.contentMuted)
            }

            Spacer()

            Text(isConnecting ? "连接中" : "连接")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(PicooColor.actionHighlight)
        }
        .frame(minHeight: PicooIconSize.touchTarget)
        .padding(.horizontal, PicooSpace.md)
        .padding(.vertical, PicooSpace.sm)
    }
}

private struct PairingView: View {
    let model: SenderAppModel

    var body: some View {
        ScrollView {
            VStack(spacing: PicooSpace.lg) {
                ReiconIcon(icon: .secureConnection)
                .frame(width: PicooIconSize.hero, height: PicooIconSize.hero)
                .foregroundStyle(PicooColor.actionHighlight)

                VStack(spacing: PicooSpace.xs) {
                    Text(model.receiverName)
                        .font(.subheadline)
                        .foregroundStyle(PicooColor.contentMuted)
                    Text("核对 6 位配对短码")
                        .font(.title2.weight(.bold))
                }

                Text(model.formattedPairingCode)
                    .font(.largeTitle.monospaced().weight(.heavy))
                    .tracking(6)
                    .foregroundStyle(PicooColor.actionHighlight)
                    .accessibilityLabel("配对短码 \(model.pairingCode)")

                Text(model.pairingSecondsRemaining > 0
                     ? "剩余 \(model.pairingSecondsRemaining) 秒"
                     : "短码已过期")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(model.pairingSecondsRemaining > 0
                                     ? PicooColor.contentMuted
                                     : PicooColor.statusDanger)

                Text("确认手机与电脑屏幕上显示相同数字；不一致时请立即取消。")
                    .font(.subheadline)
                    .foregroundStyle(PicooColor.contentMuted)
                    .multilineTextAlignment(.center)

                if let error = model.errorMessage {
                    InlineNotice(text: error, tone: .danger)
                }

                if model.pairingSecondsRemaining == 0 {
                    Button("重新发起连接") { model.retryPairing() }
                        .buttonStyle(PicooPrimaryButtonStyle())
                } else {
                    Button("两端数字一致，确认配对") { model.confirmPairing() }
                        .buttonStyle(PicooPrimaryButtonStyle())
                        .disabled(model.pairingCode.isEmpty)
                }

                Button("取消") { model.cancelConnection() }
                    .frame(minHeight: PicooIconSize.touchTarget)
                    .foregroundStyle(PicooColor.contentMuted)
            }
            .frame(maxWidth: 560)
            .padding(PicooSpace.xl)
            .frame(maxWidth: .infinity, minHeight: 600)
        }
    }
}

private struct PairingWaitingView: View {
    let model: SenderAppModel

    var body: some View {
        VStack(spacing: PicooSpace.lg) {
            Spacer()
            waitSymbol
            Text(waitTitle)
                .font(.title2.weight(.bold))
            Text(waitDetail)
                .font(.subheadline)
                .foregroundStyle(PicooColor.contentMuted)
                .multilineTextAlignment(.center)
            if model.pairingWaitOutcome == .pending {
                Button("取消连接") { model.cancelConnection() }
                    .frame(minHeight: PicooIconSize.touchTarget)
                    .foregroundStyle(PicooColor.statusDanger)
            } else {
                Button("重新发起连接") { model.retryPairing() }
                    .buttonStyle(PicooPrimaryButtonStyle())
                Button("返回设备列表") { model.cancelConnection() }
                    .frame(minHeight: PicooIconSize.touchTarget)
                    .foregroundStyle(PicooColor.contentMuted)
            }
            Spacer()
        }
        .frame(maxWidth: 560)
        .padding(PicooSpace.xl)
        .frame(maxWidth: .infinity)
    }

    @ViewBuilder
    private var waitSymbol: some View {
        switch model.pairingWaitOutcome {
        case .pending:
            ProgressView()
                .controlSize(.large)
                .tint(PicooColor.actionHighlight)
        case .rejected:
            ReiconIcon(icon: .rejected)
                .frame(width: PicooIconSize.hero, height: PicooIconSize.hero)
                .foregroundStyle(PicooColor.statusDanger)
        case .expired:
            ReiconIcon(icon: .expired)
                .frame(width: PicooIconSize.hero, height: PicooIconSize.hero)
                .foregroundStyle(PicooColor.statusWarning)
        }
    }

    private var waitTitle: String {
        switch model.pairingWaitOutcome {
        case .pending: "等待电脑端确认"
        case .rejected: "电脑端拒绝了连接"
        case .expired: "配对确认已超时"
        }
    }

    private var waitDetail: String {
        switch model.pairingWaitOutcome {
        case .pending:
            "请在 \(model.receiverName) 的配对提示中确认相同短码。双方确认后会自动开始推流。"
        case .rejected:
            "请确认选择了正确的电脑，然后重新核对两端短码。"
        case .expired:
            "为保护连接安全，本次短码已失效。请重新发起连接获取新短码。"
        }
    }
}

private struct LiveCameraView: View {
    let model: SenderAppModel
    @State private var cameraRotation = 0.0
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        GeometryReader { proxy in
            ZStack {
                PicooCameraColor.surface.ignoresSafeArea()
                cameraSurface
                safeFrame(width: proxy.size.width - PicooCameraLayout.safeHorizontalInset * 2)
                hud
                cameraStatus
                controls
            }
        }
        .preferredColorScheme(.dark)
    }

    @ViewBuilder
    private var cameraSurface: some View {
        if let previewLayer = model.camera.previewLayer {
            CameraPreview(previewLayer: previewLayer)
                .ignoresSafeArea()
        }
    }

    private func safeFrame(width: CGFloat) -> some View {
        RoundedRectangle(cornerRadius: PicooCameraLayout.safeFrameRadius)
            .stroke(
                PicooCameraColor.safeFrame,
                style: StrokeStyle(
                    lineWidth: PicooCameraLayout.safeFrameStroke,
                    dash: PicooCameraLayout.safeFrameDash
                )
            )
            .frame(width: width, height: width / PicooCameraLayout.videoAspectRatio)
            .overlay(alignment: .topLeading) {
                Text("16:9 PC 裁切框")
                    .font(.caption2.monospaced())
                    .foregroundStyle(PicooCameraColor.safeFrameLabel)
                    .padding(PicooSpace.sm)
            }
    }

    private var hud: some View {
        VStack {
            HStack {
                HStack(spacing: PicooCameraLayout.hudItemSpacing) {
                    Circle()
                        .fill(PicooCameraColor.success)
                        .frame(
                            width: PicooCameraLayout.statusDot,
                            height: PicooCameraLayout.statusDot
                        )
                    Text(model.receiverName)
                        .font(.caption.weight(.bold))
                    Text(model.senderStatus == .streaming ? "已连接" : "准备视频")
                        .font(.caption2.monospaced())
                        .foregroundStyle(PicooCameraColor.success)
                }
                .padding(.horizontal, PicooCameraLayout.hudHorizontalPadding)
                .padding(.vertical, PicooCameraLayout.hudVerticalPadding)
                .background(PicooCameraColor.hudOverlay, in: Capsule())

                Spacer()

                Button {
                    Task { await model.toggleResolution() }
                } label: {
                    Text(model.resolutionLabel)
                        .font(.caption.weight(.bold).monospaced())
                        .padding(.horizontal, PicooCameraLayout.hudHorizontalPadding)
                        .padding(.vertical, PicooCameraLayout.hudVerticalPadding)
                        .background(PicooCameraColor.hudOverlay, in: Capsule())
                }
                .buttonStyle(.plain)
                .disabled(model.camera.state != .running)
                .accessibilityLabel("切换视频分辨率")
            }
            .foregroundStyle(PicooCameraColor.content)
            .padding(.horizontal, PicooCameraLayout.safeHorizontalInset)
            .padding(.top, PicooSpace.sm)
            Spacer()
        }
    }

    @ViewBuilder
    private var cameraStatus: some View {
        if model.senderStatus == .reconnecting {
            VStack(spacing: PicooSpace.md) {
                CameraOverlay(
                    title: "网络中断，正在重连…",
                    detail: reconnectDetail
                )
                Button("停止重连并退出") { model.cancelConnection() }
                    .font(.subheadline.weight(.bold))
                    .foregroundStyle(PicooCameraColor.danger)
                    .frame(minHeight: PicooIconSize.touchTarget)
            }
        } else if model.senderStatus == .networkUnstable {
            CameraOverlay(
                title: "网络不稳定，正在优化…",
                detail: "视频会优先保持低延迟，画质可能暂时降低。"
            )
        } else {
            switch model.camera.state {
        case .requestingPermission:
            CameraOverlay(title: "正在请求相机权限", detail: "只有开始推流时才会访问摄像头。")
        case .starting:
            CameraOverlay(title: "正在启动摄像头", detail: "准备本机低延迟预览。")
        case .stopping:
            CameraOverlay(title: "正在停止摄像头", detail: "释放本机采集资源。")
        case .denied:
            VStack(spacing: PicooCameraLayout.safeHorizontalInset) {
                CameraOverlay(title: "需要相机权限", detail: "请在系统设置中允许 Picoo Camera 使用摄像头。")
                Button("打开系统设置") {
                    guard let url = URL(string: UIApplication.openSettingsURLString) else { return }
                    UIApplication.shared.open(url)
                }
                .font(.subheadline.weight(.bold))
                .foregroundStyle(PicooCameraColor.selected)
            }
        case .unavailable:
            CameraOverlay(title: "当前设备没有可用摄像头", detail: "Simulator 只验证界面与状态；真实预览需要 iPhone。")
        case let .failed(message):
            CameraOverlay(title: "摄像头启动失败", detail: message)
        case .idle, .running:
            EmptyView()
            }
        }
    }

    private var reconnectDetail: String {
        let delaySeconds = Double(model.reconnectDelayMs) / 1_000
        return "第 \(max(1, Int(model.reconnectAttempt))) 次尝试，约 \(delaySeconds.formatted(.number.precision(.fractionLength(1)))) 秒后继续。"
    }

    private var controls: some View {
        VStack {
            Spacer()
            HStack {
                VStack(alignment: .leading, spacing: PicooSpace.xxs) {
                    Text("预览就绪")
                        .font(.caption.weight(.bold))
                    Text("H.264 · \(model.activeBitrateBps / 1_000_000) Mbps")
                        .font(.caption2)
                        .foregroundStyle(PicooCameraColor.contentSubtle)
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                Button {
                    model.handleStopTap()
                } label: {
                    ZStack {
                        Circle()
                            .stroke(
                                model.stopArmed ? PicooCameraColor.danger : PicooCameraColor.stopBorder,
                                lineWidth: PicooCameraStopControl.stroke
                            )
                            .frame(
                                width: PicooCameraStopControl.target,
                                height: PicooCameraStopControl.target
                            )
                        ReiconIcon(icon: .stopStream)
                            .frame(
                                width: PicooCameraStopControl.icon,
                                height: PicooCameraStopControl.icon
                            )
                            .foregroundStyle(PicooCameraColor.danger)
                    }
                }
                .buttonStyle(.plain)
                .accessibilityLabel(model.stopArmed ? "再次点击确认断开" : "断开连接")

                Button {
                    cameraRotation += 180
                    Task { await model.switchCamera() }
                } label: {
                    ReiconIcon(icon: .switchCamera)
                        .frame(
                            width: PicooCameraLayout.controlIcon,
                            height: PicooCameraLayout.controlIcon
                        )
                        .rotationEffect(.degrees(cameraRotation))
                        .animation(
                            reduceMotion ? nil : .smooth(duration: PicooMotion.normal),
                            value: cameraRotation
                        )
                        .padding(PicooCameraLayout.controlPadding)
                        .background(PicooCameraColor.control, in: Circle())
                        .overlay { Circle().stroke(PicooCameraColor.controlBorder) }
                }
                .buttonStyle(.plain)
                .disabled(model.camera.state != .running)
                .accessibilityLabel("切换前后摄像头")
                .frame(maxWidth: .infinity, alignment: .trailing)
            }
            .foregroundStyle(PicooCameraColor.content)
            .padding(.horizontal, PicooSpace.lg)
            .padding(.bottom, PicooSpace.xl)

            if model.stopArmed {
                Text("再次点击确认断开")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(PicooCameraColor.danger)
                    .padding(.bottom, PicooSpace.sm)
            }
        }
    }
}

private struct CameraOverlay: View {
    let title: String
    let detail: String

    var body: some View {
        VStack(spacing: PicooSpace.sm) {
            Text(title)
                .font(.headline)
            Text(detail)
                .font(.caption)
                .foregroundStyle(PicooCameraColor.overlayDetail)
                .multilineTextAlignment(.center)
        }
        .foregroundStyle(PicooCameraColor.content)
        .padding(PicooSpace.lg)
        .background(PicooCameraColor.overlay, in: RoundedRectangle(cornerRadius: PicooRadius.surface))
        .padding(.horizontal, PicooCameraLayout.overlayHorizontalInset)
    }
}

private struct ManualConnectSheet: View {
    @Bindable var model: SenderAppModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: PicooSpace.lg) {
            HStack {
                VStack(alignment: .leading, spacing: PicooSpace.xs) {
                    Text("输入电脑局域网地址")
                        .font(.headline)
                    Text("仅在自动发现不可用时使用")
                        .font(.caption)
                        .foregroundStyle(PicooColor.contentMuted)
                }
                Spacer()
                ReiconIcon(icon: .secureConnection)
                    .frame(width: PicooIconSize.emphasis, height: PicooIconSize.emphasis)
                    .foregroundStyle(PicooColor.actionHighlight)
            }

            IPv4EndpointInput(endpoint: $model.manualEndpointText)

            if let error = model.errorMessage {
                InlineNotice(text: error, tone: .danger)
            }

            Button("建立加密连接") {
                model.connectManualEndpoint()
            }
            .buttonStyle(PicooPrimaryButtonStyle())

            Text("这里只定位 Receiver Endpoint，不会跳过双端短码核对。")
                .font(.caption)
                .foregroundStyle(PicooColor.contentMuted)
            }
            .padding(PicooSpace.xl)
        }
        .scrollDismissesKeyboard(.interactively)
    }
}

private struct SettingsSheet: View {
    @Bindable var model: SenderAppModel
    @State private var pendingRemoval: TrustedReceiverSummary?

    var body: some View {
        NavigationStack {
            Form {
                Section("连接") {
                    Toggle("打开 App 自动直连", isOn: $model.autoConnectEnabled)
                    Picker("默认初始画质", selection: $model.preferredResolution) {
                        ForEach(VideoResolution.allCases, id: \.self) { resolution in
                            Text("\(resolution.rawValue)P · 30 FPS").tag(resolution)
                        }
                    }
                }

                Section("已配对信任电脑") {
                    if model.trustedReceivers.isEmpty {
                        Text("还没有已配对电脑")
                            .foregroundStyle(PicooColor.contentMuted)
                    } else {
                        ForEach(model.trustedReceivers) { receiver in
                            HStack(spacing: PicooSpace.md) {
                                ReiconIcon(icon: .receiverDevice)
                                    .frame(width: PicooIconSize.standard, height: PicooIconSize.standard)
                                    .foregroundStyle(PicooColor.actionHighlight)
                                VStack(alignment: .leading, spacing: PicooSpace.xxs) {
                                    Text(receiver.name)
                                    Text(shortFingerprint(receiver.certificateFingerprint))
                                        .font(.caption.monospaced())
                                        .foregroundStyle(PicooColor.contentMuted)
                                }
                                Spacer()
                                Button("撤销", role: .destructive) {
                                    pendingRemoval = receiver
                                }
                            }
                        }
                    }
                }

                Section {
                    LabeledContent("相机权限", value: cameraPermissionLabel)
                    Button("打开系统设置") {
                        guard let url = URL(string: UIApplication.openSettingsURLString) else { return }
                        UIApplication.shared.open(url)
                    }
                } header: {
                    Text("权限")
                } footer: {
                    Text("相机权限只会在进入直播取景时请求。")
                }

                Section("关于") {
                    LabeledContent("Picoo Camera", value: "v\(picooAppVersion)")
                }
            }
            .navigationTitle("手机端设置")
            .navigationBarTitleDisplayMode(.inline)
            .confirmationDialog(
                "撤销信任？",
                isPresented: removalConfirmationPresented,
                titleVisibility: .visible,
                presenting: pendingRemoval
            ) { receiver in
                Button("撤销对 \(receiver.name) 的信任", role: .destructive) {
                    model.removeTrustedReceiver(receiver)
                    pendingRemoval = nil
                }
                Button("取消", role: .cancel) {
                    pendingRemoval = nil
                }
            } message: { receiver in
                Text("下次连接 \(receiver.name) 时需要重新核对配对短码。")
            }
        }
    }

    private var removalConfirmationPresented: Binding<Bool> {
        Binding(
            get: { pendingRemoval != nil },
            set: { if !$0 { pendingRemoval = nil } }
        )
    }

    private var cameraPermissionLabel: String {
        switch model.camera.state {
        case .denied: "未授权"
        case .running: "使用中"
        case .stopping: "正在停止"
        default: "按需请求"
        }
    }

    private func shortFingerprint(_ value: String) -> String {
        let compact = value.replacingOccurrences(of: ":", with: "")
        return String(compact.prefix(12))
    }
}

private struct InlineNotice: View {
    enum Tone { case danger, neutral }

    let text: String
    let tone: Tone

    var body: some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(tone == .danger ? PicooColor.statusDanger : PicooColor.contentMuted)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
            .background(
                (tone == .danger ? PicooColor.statusDanger : PicooColor.contentMuted).opacity(0.1),
                in: RoundedRectangle(cornerRadius: 11)
            )
    }
}

#Preview("Devices · Light") {
    ContentView(model: SenderAppModel(session: nil))
        .preferredColorScheme(.light)
}

#Preview("Devices · Dark · Accessibility") {
    ContentView(model: SenderAppModel(session: nil))
        .preferredColorScheme(.dark)
        .environment(\.dynamicTypeSize, .accessibility2)
}
