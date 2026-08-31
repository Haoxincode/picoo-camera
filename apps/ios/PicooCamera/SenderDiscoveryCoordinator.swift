import Foundation

// REQ-PICOO-DISCOVERY-008: poll mDNS results and auto-connect the latest trusted receiver.
@MainActor
final class SenderDiscoveryCoordinator {
    private var lastAutoConnectReceiverID = ""

    func resetAutoConnect() {
        lastAutoConnectReceiverID = ""
    }

    func poll(
        session: PicooSenderSession,
        browser: PicooDiscoveryBrowser?,
        host: SenderAppModel
    ) {
        let trustedReceivers = session.trustedReceivers()
        guard let discoveryBrowser = browser else {
            host.applyDiscoveryPoll(
                receivers: host.receivers,
                trustedReceivers: trustedReceivers,
                isDiscovering: false
            )
            return
        }
        do {
            let receivers = try discoveryBrowser.poll(
                trustedReceiverIDs: session.trustedReceiverIDs()
            )
            host.applyDiscoveryPoll(
                receivers: receivers,
                trustedReceivers: trustedReceivers,
                isDiscovering: receivers.isEmpty
            )
            let trustedByID = Dictionary(
                uniqueKeysWithValues: trustedReceivers.map { ($0.id, $0.lastConnectedAtMs) }
            )
            let preferredReceiver = receivers
                .filter(\.isTrusted)
                .max { left, right in
                    let leftTime = trustedByID[left.id] ?? 0
                    let rightTime = trustedByID[right.id] ?? 0
                    if leftTime == rightTime {
                        return left.displayName.localizedStandardCompare(right.displayName)
                            == .orderedDescending
                    }
                    return leftTime < rightTime
                }
            if host.autoConnectEnabled,
               host.senderStatus == .disconnected,
               !host.isConnecting,
               let receiver = preferredReceiver,
               receiver.id != lastAutoConnectReceiverID
            {
                lastAutoConnectReceiverID = receiver.id
                host.selectReceiver(receiver)
            }
        } catch {
            host.markDiscoveryUnavailable()
        }
    }
}
