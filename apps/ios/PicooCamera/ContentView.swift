import SwiftUI

struct ContentView: View {
    let protocolVersion: String
    let coreReady: Bool

    var body: some View {
        VStack(spacing: 20) {
            Spacer()

            Text("Picoo Camera")
                .font(.largeTitle.bold())

            Text("将 iPhone 摄像头通过局域网连接到电脑")
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)

            HStack(spacing: 8) {
                Circle()
                    .fill(coreReady ? Color.green : Color.red)
                    .frame(width: 8, height: 8)
                Text(coreReady ? "Rust Core 已就绪" : "Rust Core 不可用")
                    .font(.subheadline.weight(.medium))
            }

            Text(protocolVersion)
                .font(.caption.monospaced())
                .foregroundStyle(.secondary)

            Spacer()

            Text("相机与配对流程将在下一步接入")
                .font(.footnote)
                .foregroundStyle(.tertiary)
        }
        .padding(32)
    }
}

#Preview {
    ContentView(protocolVersion: "PCP/1", coreReady: true)
}
