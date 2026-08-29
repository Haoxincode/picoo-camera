import Observation
import SwiftUI
import UIKit

struct ContentView: View {
    @Bindable var model: SenderAppModel
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        ZStack {
            PicooPalette.background.ignoresSafeArea()

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
        .foregroundStyle(PicooPalette.foreground)
        .preferredColorScheme(.dark)
        .task { model.start() }
        .onChange(of: scenePhase) { _, phase in
            Task { await model.handleScenePhase(phase) }
        }
        .sheet(isPresented: $model.isManualConnectPresented) {
            ManualConnectSheet(model: model)
                .presentationDetents([.medium])
                .presentationDragIndicator(.visible)
                .presentationBackground(PicooPalette.panel)
        }
        .sheet(isPresented: $model.isSettingsPresented) {
            SettingsSheet(model: model)
                .presentationDetents([.medium])
                .presentationDragIndicator(.visible)
                .presentationBackground(PicooPalette.panel)
        }
    }
}

private struct DevicesView: View {
    @Bindable var model: SenderAppModel

    var body: some View {
        ScrollView {
            VStack(spacing: 18) {
                header
                discoveryBanner

                if model.receivers.isEmpty {
                    emptyState
                } else {
                    receiverList
                }

                if let error = model.errorMessage {
                    InlineNotice(text: error, tone: .danger)
                }

                manualConnectButton

                Text("\(model.protocolVersion) · QUIC 4433")
                    .font(.caption2.monospaced())
                    .foregroundStyle(PicooPalette.muted)
                    .padding(.top, 2)
            }
            .padding(.horizontal, 18)
            .padding(.top, 14)
            .padding(.bottom, 24)
        }
    }

    private var header: some View {
        HStack(spacing: 12) {
            ReiconIcon(name: "reicon_camera")
                .frame(width: 22, height: 22)
                .padding(9)
                .background(PicooPalette.primary, in: RoundedRectangle(cornerRadius: 11))

            VStack(alignment: .leading, spacing: 2) {
                Text("Picoo Camera")
                    .font(.headline.weight(.bold))
                Text("无线推流端 · iPhone")
                    .font(.caption)
                    .foregroundStyle(PicooPalette.muted)
            }

            Spacer()

            Button {
                model.isSettingsPresented = true
            } label: {
                ReiconIcon(name: "reicon_settings")
                    .frame(width: 20, height: 20)
                    .padding(10)
                    .background(PicooPalette.panel, in: RoundedRectangle(cornerRadius: 11))
            }
            .buttonStyle(.plain)
            .accessibilityLabel("设置")
        }
    }

    private var discoveryBanner: some View {
        HStack(spacing: 12) {
            ZStack {
                Circle()
                    .fill(PicooPalette.primary.opacity(0.16))
                    .frame(width: 42, height: 42)
                ReiconIcon(name: "reicon_wifi")
                    .frame(width: 21, height: 21)
                    .foregroundStyle(PicooPalette.highlight)
            }

            VStack(alignment: .leading, spacing: 3) {
                Text(model.isDiscovering ? "局域网自动发现中" : "局域网设备")
                    .font(.subheadline.weight(.semibold))
                Text("Wi-Fi · Bonjour _picoocam._udp")
                    .font(.caption2.monospaced())
                    .foregroundStyle(PicooPalette.muted)
            }

            Spacer()

            Button {
                model.refreshDiscovery()
            } label: {
                ReiconIcon(name: "reicon_refresh")
                    .frame(width: 18, height: 18)
                    .foregroundStyle(PicooPalette.highlight)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("刷新局域网设备")
        }
        .padding(14)
        .background(PicooPalette.panel, in: RoundedRectangle(cornerRadius: 16))
        .overlay {
            RoundedRectangle(cornerRadius: 16)
                .stroke(PicooPalette.border, lineWidth: 1)
        }
    }

    private var receiverList: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("附近可连接的电脑")
                Spacer()
                Text("\(model.receivers.filter(\.isTrusted).count) 台已信任")
                    .font(.caption2.monospaced())
                    .foregroundStyle(PicooPalette.highlight)
            }
            .font(.caption.weight(.semibold))
            .foregroundStyle(PicooPalette.muted)

            ForEach(model.receivers) { receiver in
                Button {
                    model.selectReceiver(receiver)
                } label: {
                    ReceiverRow(receiver: receiver, isConnecting: model.isConnecting)
                }
                .buttonStyle(.plain)
            }
        }
    }

    private var emptyState: some View {
        VStack(spacing: 14) {
            ReiconIcon(name: "reicon_desktop")
                .frame(width: 34, height: 34)
                .foregroundStyle(PicooPalette.muted)
            Text("还没有发现电脑")
                .font(.headline)
            Text("请确认电脑端已启动，并且手机与电脑连接到同一个 Wi-Fi。")
                .font(.subheadline)
                .foregroundStyle(PicooPalette.muted)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 42)
        .padding(.horizontal, 24)
        .background(PicooPalette.panel.opacity(0.72), in: RoundedRectangle(cornerRadius: 18))
        .overlay {
            RoundedRectangle(cornerRadius: 18)
                .stroke(PicooPalette.border, style: StrokeStyle(lineWidth: 1, dash: [5, 5]))
        }
    }

    private var manualConnectButton: some View {
        Button {
            model.isManualConnectPresented = true
        } label: {
            HStack(spacing: 9) {
                ReiconIcon(name: "reicon_lock")
                    .frame(width: 18, height: 18)
                    .foregroundStyle(PicooPalette.highlight)
                Text("输入 IP 地址直连")
                    .font(.subheadline.weight(.semibold))
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 14)
            .background(PicooPalette.panel, in: RoundedRectangle(cornerRadius: 14))
            .overlay {
                RoundedRectangle(cornerRadius: 14)
                    .stroke(PicooPalette.border, lineWidth: 1)
            }
        }
        .buttonStyle(.plain)
    }
}

private struct ReceiverRow: View {
    let receiver: ReceiverSummary
    let isConnecting: Bool

    var body: some View {
        HStack(spacing: 12) {
            ReiconIcon(name: "reicon_desktop")
                .frame(width: 21, height: 21)
                .foregroundStyle(PicooPalette.highlight)
                .padding(10)
                .background(PicooPalette.background, in: RoundedRectangle(cornerRadius: 11))

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(receiver.displayName)
                        .font(.subheadline.weight(.bold))
                    if receiver.isTrusted {
                        Text("已信任")
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(PicooPalette.ready)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(PicooPalette.ready.opacity(0.12), in: Capsule())
                    }
                }
                Text(receiver.endpoint.displayText)
                    .font(.caption2.monospaced())
                    .foregroundStyle(PicooPalette.muted)
            }

            Spacer()

            Text(isConnecting ? "连接中" : (receiver.isTrusted ? "推流" : "连接"))
                .font(.caption.weight(.bold))
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .foregroundStyle(.white)
                .background(PicooPalette.primary, in: RoundedRectangle(cornerRadius: 10))
        }
        .padding(13)
        .background(PicooPalette.panel, in: RoundedRectangle(cornerRadius: 16))
        .overlay {
            RoundedRectangle(cornerRadius: 16)
                .stroke(receiver.isTrusted ? PicooPalette.highlight.opacity(0.45) : PicooPalette.border)
        }
    }
}

private struct PairingView: View {
    let model: SenderAppModel

    var body: some View {
        VStack(spacing: 22) {
            Spacer()

            ReiconIcon(name: "reicon_lock")
                .frame(width: 34, height: 34)
                .foregroundStyle(PicooPalette.highlight)
                .padding(18)
                .background(PicooPalette.highlight.opacity(0.12), in: Circle())

            VStack(spacing: 8) {
                Text("核对 6 位配对短码")
                    .font(.title2.weight(.bold))
                Text("确认手机与 \(model.receiverName) 显示相同数字")
                    .font(.subheadline)
                    .foregroundStyle(PicooPalette.muted)
                    .multilineTextAlignment(.center)
            }

            Text(model.formattedPairingCode)
                .font(.system(size: 42, weight: .heavy, design: .monospaced))
                .tracking(8)
                .foregroundStyle(PicooPalette.highlight)
                .accessibilityLabel("配对短码 \(model.pairingCode)")

            Text(model.pairingSecondsRemaining > 0
                 ? "\(model.pairingSecondsRemaining) 秒内有效"
                 : "短码已过期")
                .font(.caption.monospaced())
                .foregroundStyle(model.pairingSecondsRemaining > 0
                                 ? PicooPalette.muted
                                 : PicooPalette.danger)

            if let error = model.errorMessage {
                InlineNotice(text: error, tone: .danger)
            }

            Button("两端数字一致，确认配对") {
                model.confirmPairing()
            }
            .buttonStyle(PicooPrimaryButtonStyle())
            .disabled(model.pairingSecondsRemaining == 0 || model.pairingCode.isEmpty)

            if model.pairingSecondsRemaining == 0 {
                Button("重新发起连接") { model.retryPairing() }
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(PicooPalette.highlight)
            }

            Button("取消") { model.cancelConnection() }
                .font(.subheadline)
                .foregroundStyle(PicooPalette.muted)

            Spacer()
        }
        .padding(24)
    }
}

private struct PairingWaitingView: View {
    let model: SenderAppModel

    var body: some View {
        VStack(spacing: 20) {
            Spacer()
            ProgressView()
                .controlSize(.large)
                .tint(PicooPalette.highlight)
            Text("等待电脑确认")
                .font(.title2.weight(.bold))
            Text("已确认短码一致，正在等待 \(model.receiverName) 完成配对。")
                .font(.subheadline)
                .foregroundStyle(PicooPalette.muted)
                .multilineTextAlignment(.center)
            Button("取消连接") { model.cancelConnection() }
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(PicooPalette.danger)
                .padding(.top, 12)
            Spacer()
        }
        .padding(28)
    }
}

private struct LiveCameraView: View {
    let model: SenderAppModel
    @State private var cameraRotation = 0.0

    var body: some View {
        GeometryReader { proxy in
            ZStack {
                Color.black.ignoresSafeArea()
                cameraSurface
                safeFrame(width: proxy.size.width - 28)
                hud
                cameraStatus
                controls
            }
        }
    }

    @ViewBuilder
    private var cameraSurface: some View {
        if let previewLayer = model.camera.previewLayer {
            CameraPreview(previewLayer: previewLayer)
                .ignoresSafeArea()
        }
    }

    private func safeFrame(width: CGFloat) -> some View {
        RoundedRectangle(cornerRadius: 8)
            .stroke(.white.opacity(0.3), style: StrokeStyle(lineWidth: 1, dash: [6, 5]))
            .frame(width: width, height: width * 9 / 16)
            .overlay(alignment: .topLeading) {
                Text("16:9 PC 裁切框")
                    .font(.caption2.monospaced())
                    .foregroundStyle(.white.opacity(0.55))
                    .padding(8)
            }
    }

    private var hud: some View {
        VStack {
            HStack {
                HStack(spacing: 7) {
                    Circle()
                        .fill(PicooPalette.ready)
                        .frame(width: 7, height: 7)
                    Text(model.receiverName)
                        .font(.caption.weight(.bold))
                    Text(model.senderStatus == .streaming ? "已连接" : "准备视频")
                        .font(.caption2.monospaced())
                        .foregroundStyle(PicooPalette.ready)
                }
                .padding(.horizontal, 11)
                .padding(.vertical, 8)
                .background(.black.opacity(0.62), in: Capsule())

                Spacer()

                Text("1080P · 30")
                    .font(.caption.weight(.bold).monospaced())
                    .padding(.horizontal, 11)
                    .padding(.vertical, 8)
                    .background(.black.opacity(0.62), in: Capsule())
            }
            .foregroundStyle(.white)
            .padding(.horizontal, 14)
            .padding(.top, 8)
            Spacer()
        }
    }

    @ViewBuilder
    private var cameraStatus: some View {
        switch model.camera.state {
        case .requestingPermission:
            CameraOverlay(title: "正在请求相机权限", detail: "只有开始推流时才会访问摄像头。")
        case .starting:
            CameraOverlay(title: "正在启动摄像头", detail: "准备本机低延迟预览。")
        case .denied:
            VStack(spacing: 14) {
                CameraOverlay(title: "需要相机权限", detail: "请在系统设置中允许 Picoo Camera 使用摄像头。")
                Button("打开系统设置") {
                    guard let url = URL(string: UIApplication.openSettingsURLString) else { return }
                    UIApplication.shared.open(url)
                }
                .font(.subheadline.weight(.bold))
                .foregroundStyle(PicooPalette.highlight)
            }
        case .unavailable:
            CameraOverlay(title: "当前设备没有可用摄像头", detail: "Simulator 只验证界面与状态；真实预览需要 iPhone。")
        case let .failed(message):
            CameraOverlay(title: "摄像头启动失败", detail: message)
        case .idle, .running:
            EmptyView()
        }
    }

    private var controls: some View {
        VStack {
            Spacer()
            HStack {
                VStack(alignment: .leading, spacing: 3) {
                    Text("预览就绪")
                        .font(.caption.weight(.bold))
                    Text("H.264 编码将在下一批接入")
                        .font(.caption2)
                        .foregroundStyle(.white.opacity(0.58))
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                Button {
                    model.handleStopTap()
                } label: {
                    ZStack {
                        Circle()
                            .stroke(model.stopArmed ? PicooPalette.danger : .white.opacity(0.88), lineWidth: 3.5)
                            .frame(width: 64, height: 64)
                        RoundedRectangle(cornerRadius: 5)
                            .fill(PicooPalette.danger)
                            .frame(width: 22, height: 22)
                    }
                }
                .buttonStyle(.plain)
                .accessibilityLabel(model.stopArmed ? "再次点击确认断开" : "断开连接")

                Button {
                    cameraRotation += 180
                    Task { await model.switchCamera() }
                } label: {
                    ReiconIcon(name: "reicon_camera_rotate")
                        .frame(width: 23, height: 23)
                        .rotationEffect(.degrees(cameraRotation))
                        .animation(.smooth(duration: 0.35), value: cameraRotation)
                        .padding(12)
                        .background(.white.opacity(0.14), in: Circle())
                        .overlay { Circle().stroke(.white.opacity(0.22)) }
                }
                .buttonStyle(.plain)
                .disabled(model.camera.state != .running)
                .accessibilityLabel("切换前后摄像头")
                .frame(maxWidth: .infinity, alignment: .trailing)
            }
            .foregroundStyle(.white)
            .padding(.horizontal, 18)
            .padding(.bottom, 24)

            if model.stopArmed {
                Text("再次点击确认断开")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(PicooPalette.danger)
                    .padding(.bottom, 8)
            }
        }
    }
}

private struct CameraOverlay: View {
    let title: String
    let detail: String

    var body: some View {
        VStack(spacing: 8) {
            Text(title)
                .font(.headline)
            Text(detail)
                .font(.caption)
                .foregroundStyle(.white.opacity(0.66))
                .multilineTextAlignment(.center)
        }
        .foregroundStyle(.white)
        .padding(18)
        .background(.black.opacity(0.68), in: RoundedRectangle(cornerRadius: 16))
        .padding(.horizontal, 36)
    }
}

private struct ManualConnectSheet: View {
    @Bindable var model: SenderAppModel
    @FocusState private var endpointFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("输入电脑局域网地址")
                        .font(.headline)
                    Text("仅在自动发现不可用时使用")
                        .font(.caption)
                        .foregroundStyle(PicooPalette.muted)
                }
                Spacer()
                ReiconIcon(name: "reicon_lock")
                    .frame(width: 21, height: 21)
                    .foregroundStyle(PicooPalette.highlight)
            }

            TextField("192.168.1.108:4433", text: $model.manualEndpointText)
                .font(.body.monospaced())
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .keyboardType(.numbersAndPunctuation)
                .focused($endpointFocused)
                .padding(14)
                .background(PicooPalette.background, in: RoundedRectangle(cornerRadius: 12))
                .overlay { RoundedRectangle(cornerRadius: 12).stroke(PicooPalette.border) }

            if let error = model.errorMessage {
                InlineNotice(text: error, tone: .danger)
            }

            Button("建立加密连接") {
                model.connectManualEndpoint()
            }
            .buttonStyle(PicooPrimaryButtonStyle())

            Text("这里只定位 Receiver Endpoint，不会跳过双端短码核对。")
                .font(.caption)
                .foregroundStyle(PicooPalette.muted)
        }
        .padding(22)
        .onAppear { endpointFocused = true }
    }
}

private struct SettingsSheet: View {
    let model: SenderAppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("手机端设置")
                .font(.title3.weight(.bold))

            SettingsRow(label: "默认画质", value: "1080P · 30 FPS")
            SettingsRow(label: "相机权限", value: cameraPermissionLabel)
            SettingsRow(label: "协议", value: model.protocolVersion)
            SettingsRow(label: "语言模式", value: "Swift 6")

            Text("相机权限只会在连接进入直播取景时请求；设备发现和手动连接不会访问摄像头。")
                .font(.caption)
                .foregroundStyle(PicooPalette.muted)
                .padding(.top, 4)
        }
        .padding(22)
    }

    private var cameraPermissionLabel: String {
        switch model.camera.state {
        case .denied: "未授权"
        case .running: "使用中"
        default: "按需请求"
        }
    }
}

private struct SettingsRow: View {
    let label: String
    let value: String

    var body: some View {
        HStack {
            Text(label)
            Spacer()
            Text(value)
                .font(.subheadline.monospaced())
                .foregroundStyle(PicooPalette.highlight)
        }
        .font(.subheadline)
        .padding(14)
        .background(PicooPalette.background, in: RoundedRectangle(cornerRadius: 12))
        .overlay { RoundedRectangle(cornerRadius: 12).stroke(PicooPalette.border) }
    }
}

private struct InlineNotice: View {
    enum Tone { case danger, neutral }

    let text: String
    let tone: Tone

    var body: some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(tone == .danger ? PicooPalette.danger : PicooPalette.muted)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
            .background(
                (tone == .danger ? PicooPalette.danger : PicooPalette.muted).opacity(0.1),
                in: RoundedRectangle(cornerRadius: 11)
            )
    }
}

private struct PicooPrimaryButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.subheadline.weight(.bold))
            .frame(maxWidth: .infinity)
            .padding(.vertical, 14)
            .foregroundStyle(.white)
            .background(PicooPalette.primary.opacity(configuration.isPressed ? 0.75 : 1), in: RoundedRectangle(cornerRadius: 13))
            .scaleEffect(configuration.isPressed ? 0.98 : 1)
    }
}

private struct ReiconIcon: View {
    let name: String

    var body: some View {
        Image(name)
            .renderingMode(.template)
            .resizable()
            .scaledToFit()
    }
}

private enum PicooPalette {
    static let background = Color(red: 0.043, green: 0.051, blue: 0.067)
    static let panel = Color(red: 0.078, green: 0.094, blue: 0.129)
    static let foreground = Color(red: 0.937, green: 0.961, blue: 1)
    static let muted = Color(red: 0.56, green: 0.60, blue: 0.68)
    static let border = Color.white.opacity(0.10)
    static let primary = Color(red: 0.098, green: 0.235, blue: 0.722)
    static let highlight = Color(red: 0.169, green: 0.498, blue: 1)
    static let ready = Color(red: 0.243, green: 0.812, blue: 0.557)
    static let danger = Color(red: 1, green: 0.29, blue: 0.341)
}

#Preview("Devices") {
    ContentView(model: SenderAppModel(session: nil))
}
