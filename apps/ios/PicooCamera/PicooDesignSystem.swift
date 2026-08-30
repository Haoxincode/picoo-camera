import SwiftUI

// ARCH-PICOO-UI-002: feature views consume semantic/component tokens only.
enum PicooColor {
    static let surfacePage = Color("PicooSurfacePage")
    static let surfaceGroup = Color("PicooSurfaceGroup")
    static let surfaceSubtle = Color("PicooSurfaceSubtle")
    static let contentPrimary = Color("PicooContentPrimary")
    static let contentMuted = Color("PicooContentMuted")
    static let borderDefault = Color("PicooBorderDefault")
    static let actionPrimary = Color("PicooActionPrimary")
    static let actionHighlight = Color("PicooActionHighlight")
    static let statusSuccess = Color("PicooStatusSuccess")
    static let statusWarning = Color("PicooStatusWarning")
    static let statusDanger = Color("PicooStatusDanger")
}

enum PicooCameraColor {
    static let surface = Color(red: 0.016, green: 0.020, blue: 0.027)
    static let overlay = Color.black.opacity(0.70)
    static let hudOverlay = Color.black.opacity(0.62)
    static let control = Color.white.opacity(0.16)
    static let controlBorder = Color.white.opacity(0.22)
    static let content = Color.white
    static let contentMuted = Color.white.opacity(0.64)
    static let contentSubtle = Color.white.opacity(0.58)
    static let overlayDetail = Color.white.opacity(0.66)
    static let safeFrame = Color.white.opacity(0.30)
    static let safeFrameLabel = Color.white.opacity(0.55)
    static let stopBorder = Color.white.opacity(0.88)
    static let selected = Color(red: 0.169, green: 0.498, blue: 1)
    static let success = Color(red: 0, green: 0.737, blue: 0.490)
    static let warning = Color(red: 0.941, green: 0.757, blue: 0.290)
    static let danger = Color(red: 1, green: 0.392, blue: 0.404)
}

enum PicooCameraLayout {
    static let safeHorizontalInset: CGFloat = 14
    static let safeFrameRadius: CGFloat = 8
    static let safeFrameStroke: CGFloat = 1
    static let safeFrameDash: [CGFloat] = [6, 5]
    static let videoAspectRatio: CGFloat = 16 / 9
    static let statusDot: CGFloat = 7
    static let hudItemSpacing: CGFloat = 7
    static let hudHorizontalPadding: CGFloat = 11
    static let hudVerticalPadding: CGFloat = 8
    static let controlIcon: CGFloat = 23
    static let controlPadding: CGFloat = 12
    static let overlayHorizontalInset: CGFloat = 36
}

enum PicooSpace {
    static let xxs: CGFloat = 2
    static let xs: CGFloat = 4
    static let sm: CGFloat = 8
    static let md: CGFloat = 12
    static let lg: CGFloat = 16
    static let xl: CGFloat = 24
    static let xxl: CGFloat = 32
}

enum PicooRadius {
    static let control: CGFloat = 7
    static let surface: CGFloat = 15
}

enum PicooIconSize {
    static let compact: CGFloat = 16
    static let standard: CGFloat = 20
    static let emphasis: CGFloat = 24
    static let hero: CGFloat = 32
    static let touchTarget: CGFloat = 44
}

enum PicooCameraStopControl {
    static let target: CGFloat = 64
    static let stroke: CGFloat = 3.5
    static let icon: CGFloat = PicooIconSize.emphasis
}

enum PicooMotion {
    static let fast = 0.12
    static let normal = 0.35
    static let deliberate = 0.50
}

enum PicooIcon {
    case settings
    case receiverDevice
    case discovering
    case network
    case secureConnection
    case switchCamera
    case exposure
    case mirror
    case interactionLock
    case interactionUnlock
    case refreshDiscovery
    case resetExposure
    case repairFrame
    case stopStream
    case disconnect
    case overheat
    case moreActions
    case navigateBack
    case rejected
    case expired

    var assetName: String {
        switch self {
        case .settings: "reicon_settings"
        case .receiverDevice: "reicon_desktop"
        case .discovering: "reicon_radio"
        case .network: "reicon_wifi"
        case .secureConnection: "reicon_shield_check"
        case .switchCamera: "reicon_camera_rotate"
        case .exposure: "reicon_sun"
        case .mirror: "reicon_flip_horizontal"
        case .interactionLock: "reicon_lock"
        case .interactionUnlock: "reicon_unlock"
        case .refreshDiscovery: "reicon_refresh"
        case .resetExposure: "reicon_refresh"
        case .repairFrame: "reicon_refresh"
        case .stopStream: "reicon_stop"
        case .disconnect: "reicon_phone_off"
        case .overheat: "reicon_flame"
        case .moreActions: "reicon_more_horizontal"
        case .navigateBack: "reicon_chevron_left"
        case .rejected: "reicon_xmark"
        case .expired: "reicon_clock"
        }
    }
}

struct ReiconIcon: View {
    let icon: PicooIcon

    var body: some View {
        Image(icon.assetName)
            .renderingMode(.template)
            .resizable()
            .scaledToFit()
            .accessibilityHidden(true)
    }
}

struct PicooPrimaryButtonStyle: ButtonStyle {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.isEnabled) private var isEnabled

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.body.weight(.semibold))
            .frame(maxWidth: .infinity, minHeight: PicooIconSize.touchTarget)
            .foregroundStyle(.white.opacity(isEnabled ? 1 : 0.58))
            .background(
                PicooColor.actionPrimary.opacity(
                    !isEnabled ? 0.42 : (configuration.isPressed ? 0.78 : 1)
                ),
                in: RoundedRectangle(cornerRadius: PicooRadius.control)
            )
            .scaleEffect(reduceMotion || !configuration.isPressed ? 1 : 0.985)
            .animation(
                reduceMotion ? nil : .easeOut(duration: PicooMotion.fast),
                value: configuration.isPressed
            )
    }
}

struct PicooIconButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .frame(width: PicooIconSize.touchTarget, height: PicooIconSize.touchTarget)
            .contentShape(Rectangle())
            .background(
                configuration.isPressed ? PicooColor.surfaceSubtle : .clear,
                in: RoundedRectangle(cornerRadius: PicooRadius.control)
            )
    }
}
