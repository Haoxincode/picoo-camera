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
            LiveCameraView(model: model)
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
        .sheet(isPresented: $model.isConnectionPresented) {
            DevicesView(model: model)
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
                .presentationBackground(PicooColor.surfaceGroup)
        }
        .sheet(
            isPresented: Binding(
                get: { model.screen == .pairing || model.screen == .waiting },
                set: { presented in
                    guard !presented else { return }
                    if model.screen == .pairing || model.screen == .waiting {
                        model.cancelConnection()
                    }
                }
            )
        ) {
            if model.screen == .pairing {
                PairingView(model: model)
            } else {
                PairingWaitingView(model: model)
            }
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
                        Text("连接电脑")
                            .font(.largeTitle.weight(.bold))
                        Text("搜索同一 Wi‑Fi 下的电脑，或输入局域网 IP 直连。")
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
            .navigationTitle("连接电脑")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        model.isConnectionPresented = false
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
        ZStack {
            PicooCameraColor.surface.ignoresSafeArea()
            cameraSurface
            hud
            cameraStatus
            controls
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

    private var hud: some View {
        VStack {
            HStack {
                Button {
                    model.isConnectionPresented = true
                } label: {
                    HStack(spacing: PicooCameraLayout.hudItemSpacing) {
                        Circle()
                            .fill(isConnected ? PicooCameraColor.success : PicooCameraColor.contentMuted)
                            .frame(
                                width: PicooCameraLayout.statusDot,
                                height: PicooCameraLayout.statusDot
                            )
                        Text(connectionTitle)
                            .font(.caption.weight(.bold))
                        if !isConnected {
                            Text(connectionDetail)
                                .font(.caption2.monospaced())
                                .foregroundStyle(PicooCameraColor.contentMuted)
                        }
                    }
                    .padding(.horizontal, PicooCameraLayout.hudHorizontalPadding)
                    .padding(.vertical, PicooCameraLayout.hudVerticalPadding)
                    .background(PicooCameraColor.hudOverlay, in: Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(connectionTitle)
                .accessibilityHint("打开连接面板")

                Spacer()

                HStack(spacing: PicooSpace.sm) {
                    if isConnected {
                        Menu {
                            ForEach(model.availableSourceFormats ?? [], id: \.self) { source in
                                Button(source.label) { Task { await model.applySourceFormat(source) } }
                            }
                        } label: {
                            Text(model.sourceFormatLabel)
                                .font(.caption.weight(.bold).monospaced())
                                .padding(.horizontal, PicooCameraLayout.hudHorizontalPadding)
                                .padding(.vertical, PicooCameraLayout.hudVerticalPadding)
                                .background(PicooCameraColor.hudOverlay, in: Capsule())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("选择视频格式")
                    }

                    Button {
                        model.isSettingsPresented = true
                    } label: {
                        ReiconIcon(icon: .settings)
                            .frame(width: PicooIconSize.standard, height: PicooIconSize.standard)
                            .padding(PicooSpace.sm)
                            .background(PicooCameraColor.hudOverlay, in: Circle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("设置")
                }
            }
            .foregroundStyle(PicooCameraColor.content)
            .padding(.horizontal, PicooCameraLayout.safeHorizontalInset)
            .padding(.top, PicooSpace.sm)
            Spacer()
        }
    }

    private var isConnected: Bool {
        model.senderStatus == .streaming ||
            model.senderStatus == .reconnecting ||
            model.senderStatus == .networkUnstable
    }

    private var connectionTitle: String {
        switch model.senderStatus {
        case .streaming, .networkUnstable:
            return "\(model.receiverName) 已连接"
        case .permissionRequired:
            return "需要相机权限"
        case .reconnecting:
            return "正在重连电脑"
        case .pairing:
            return "等待配对确认"
        case .connecting, .negotiating:
            return "正在连接电脑"
        case .discovering:
            return "正在寻找电脑…"
        case .disconnected:
            return "点击连接电脑"
        }
    }

    private var connectionDetail: String {
        switch model.senderStatus {
        case .streaming, .networkUnstable, .reconnecting:
            return "已建立安全连接"
        case .permissionRequired:
            return "允许相机权限后开始预览"
        case .discovering:
            return "保持手机与电脑连接同一 Wi‑Fi"
        default:
            return model.errorMessage ?? "自动发现或输入局域网 IP"
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
        } else if model.senderStatus == .connecting || model.senderStatus == .negotiating {
            CameraOverlay(
                title: "正在连接电脑…",
                detail: "连接建立后会自动开始本机预览。"
            )
        } else if !isConnected && model.senderStatus != .permissionRequired {
            VStack(spacing: PicooSpace.md) {
                CameraOverlay(
                    title: connectionTitle,
                    detail: connectionDetail
                )
                Button("打开连接面板") {
                    model.isConnectionPresented = true
                }
                .font(.subheadline.weight(.bold))
                .foregroundStyle(PicooCameraColor.selected)
                .frame(minHeight: PicooIconSize.touchTarget)
            }
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
                    Text(isConnected ? "预览就绪" : connectionTitle)
                        .font(.caption.weight(.bold))
                    Text(isConnected
                         ? "H.264 · \(model.activeBitrateBps / 1_000_000) Mbps"
                         : connectionDetail)
                        .font(.caption2)
                        .foregroundStyle(PicooCameraColor.contentSubtle)
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                if isConnected {
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
                } else {
                    Button {
                        model.isConnectionPresented = true
                    } label: {
                        ReiconIcon(icon: .secureConnection)
                            .frame(
                                width: PicooCameraLayout.controlIcon,
                                height: PicooCameraLayout.controlIcon
                            )
                            .padding(PicooCameraLayout.controlPadding)
                            .background(PicooCameraColor.control, in: Circle())
                            .overlay { Circle().stroke(PicooCameraColor.controlBorder) }
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("连接电脑")
                }

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
            ScrollView {
                VStack(alignment: .leading, spacing: PicooSpace.xl) {
                    IOSSettingsSection(title: "连接") {
                        Toggle(isOn: $model.autoConnectEnabled) {
                            IOSSettingsText(
                                title: "打开 App 自动直连",
                                detail: "上次连接的电脑在线时自动连接"
                            )
                        }
                        .tint(PicooColor.actionHighlight)
                        .frame(minHeight: PicooIconSize.touchTarget)

                        IOSSettingsDivider()

                        Menu {
                            ForEach(model.camera.localSourceFormats ?? [], id: \.self) { source in
                                Button {
                                    model.preferredSourceFormat = source
                                } label: {
                                    HStack {
                                        Text(source.label)
                                        if source == model.preferredSourceFormat {
                                            Image(systemName: "checkmark")
                                        }
                                    }
                                }
                            }
                        } label: {
                            IOSSettingsRow(
                                title: "默认初始画质",
                                detail: "新连接的编码格式、分辨率与帧率",
                                value: model.preferredSourceFormat.label,
                                icon: .exposure
                            )
                        }
                        .buttonStyle(.plain)
                    }

                    IOSSettingsSection(title: "设备与权限") {
                        if model.trustedReceivers.isEmpty {
                            IOSSettingsRow(
                                title: "已配对信任电脑",
                                detail: "首次配对成功后会显示在这里",
                                value: "0 台",
                                icon: .secureConnection,
                                showsChevron: false
                            )
                        } else {
                            ForEach(model.trustedReceivers) { receiver in
                                Button {
                                    pendingRemoval = receiver
                                } label: {
                                    IOSSettingsRow(
                                        title: receiver.name,
                                        detail: "公钥指纹 \(shortFingerprint(receiver.certificateFingerprint))",
                                        value: "撤销",
                                        valueColor: PicooColor.statusDanger,
                                        icon: .receiverDevice
                                    )
                                }
                                .buttonStyle(.plain)
                                if receiver.id != model.trustedReceivers.last?.id {
                                    IOSSettingsDivider()
                                }
                            }
                        }

                        IOSSettingsDivider()

                        Button {
                            guard let url = URL(string: UIApplication.openSettingsURLString) else { return }
                            UIApplication.shared.open(url)
                        } label: {
                            IOSSettingsRow(
                                title: "相机权限",
                                detail: "进入直播取景时按需请求",
                                value: cameraPermissionLabel,
                                valueColor: cameraPermissionColor,
                                icon: .secureConnection
                            )
                        }
                        .buttonStyle(.plain)
                    }

                    IOSSettingsSection(title: "关于") {
                        IOSSettingsRow(
                            title: "Picoo Camera",
                            detail: "无线低延迟摄像头",
                            value: "v\(picooAppVersion)",
                            valueColor: PicooColor.contentMuted,
                            icon: .receiverDevice,
                            showsChevron: false
                        )
                    }
                }
                .padding(.horizontal, PicooSpace.xl)
                .padding(.vertical, PicooSpace.lg)
            }
            .background(PicooColor.surfacePage)
            .navigationTitle("设置")
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
        case .idle: "按需请求"
        case .requestingPermission: "正在请求"
        case .starting: "正在启动"
        case .stopping: "正在停止"
        case .running: "使用中"
        case .denied: "未授权"
        case .unavailable: "不可用"
        case .failed: "检查失败"
        }
    }

    private var cameraPermissionColor: Color {
        switch model.camera.state {
        case .running:
            PicooColor.statusSuccess
        case .denied, .unavailable, .failed:
            PicooColor.statusWarning
        case .idle, .requestingPermission, .starting, .stopping:
            PicooColor.contentMuted
        }
    }

    private func shortFingerprint(_ value: String) -> String {
        let compact = value.replacingOccurrences(of: ":", with: "")
        return String(compact.prefix(12))
    }
}

private struct IOSSettingsSection<Content: View>: View {
    let title: String
    let content: Content

    init(title: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: PicooSpace.sm) {
            Text(title)
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(PicooColor.contentMuted)
                .padding(.horizontal, PicooSpace.sm)
            VStack(alignment: .leading, spacing: 0) {
                content
            }
        }
    }
}

private struct IOSSettingsRow: View {
    let title: String
    let detail: String
    let value: String
    let valueColor: Color
    let icon: PicooIcon
    let showsChevron: Bool

    init(
        title: String,
        detail: String,
        value: String,
        valueColor: Color = PicooColor.contentMuted,
        icon: PicooIcon,
        showsChevron: Bool = true
    ) {
        self.title = title
        self.detail = detail
        self.value = value
        self.valueColor = valueColor
        self.icon = icon
        self.showsChevron = showsChevron
    }

    var body: some View {
        HStack(spacing: PicooSpace.sm) {
            ReiconIcon(icon: icon)
                .frame(width: PicooIconSize.emphasis, height: PicooIconSize.emphasis)
                .foregroundStyle(PicooColor.actionHighlight)

            VStack(alignment: .leading, spacing: PicooSpace.xxs) {
                Text(title)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(PicooColor.contentPrimary)
                Text(detail)
                    .font(.subheadline)
                    .foregroundStyle(PicooColor.contentMuted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .layoutPriority(1)

            Spacer(minLength: PicooSpace.sm)

            HStack(spacing: PicooSpace.xs) {
                Text(value)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(valueColor)
                    .multilineTextAlignment(.trailing)
                if showsChevron {
                    ReiconIcon(icon: .navigateBack)
                        .frame(width: PicooIconSize.compact, height: PicooIconSize.compact)
                        .rotationEffect(.degrees(180))
                        .foregroundStyle(PicooColor.contentMuted)
                }
            }
        }
        .frame(maxWidth: .infinity, minHeight: PicooIconSize.touchTarget, alignment: .center)
        .contentShape(Rectangle())
    }
}

private struct IOSSettingsText: View {
    let title: String
    let detail: String

    var body: some View {
        VStack(alignment: .leading, spacing: PicooSpace.xxs) {
            Text(title)
                .font(.body.weight(.semibold))
                .foregroundStyle(PicooColor.contentPrimary)
            Text(detail)
                .font(.subheadline)
                .foregroundStyle(PicooColor.contentMuted)
        }
    }
}

private struct IOSSettingsDivider: View {
    var body: some View {
        Divider()
            .overlay(PicooColor.borderDefault)
            .padding(.leading, PicooIconSize.emphasis + PicooSpace.sm)
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
