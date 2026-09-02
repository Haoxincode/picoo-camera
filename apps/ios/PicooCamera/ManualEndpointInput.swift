import SwiftUI

// REQ-PICOO-UI-003 / AC-M-MANUAL-01
nonisolated struct ManualEndpointDraft: Equatable, Sendable {
    static let defaultPort = "4433"

    var octets: [String]
    var port: String

    init(text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let separator = trimmed.lastIndex(of: ":")
        let hostText = separator.map { String(trimmed[..<$0]) } ?? trimmed
        let portText = separator.map { String(trimmed[trimmed.index(after: $0)...]) } ?? Self.defaultPort
        let parsedOctets = hostText.split(separator: ".", omittingEmptySubsequences: false)
        octets = (0..<4).map { index in
            guard parsedOctets.indices.contains(index) else { return "" }
            return String(parsedOctets[index]).decimalDigits(maxLength: 3)
        }
        port = separator == nil ? Self.defaultPort : portText.decimalDigits(maxLength: 5)
    }

    init?(pastedText: String) {
        let colonCount = pastedText.filter { $0 == ":" }.count
        let hostText = pastedText.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false).first ?? ""
        guard colonCount <= 1,
              hostText.split(separator: ".", omittingEmptySubsequences: false).count == 4
        else { return nil }
        self.init(text: pastedText)
        guard octets.allSatisfy({ !$0.isEmpty }) else { return nil }
    }

    var text: String { "\(octets.joined(separator: ".")):\(port)" }

    mutating func updateOctet(_ index: Int, value: String) {
        octets[index] = value.decimalDigits(maxLength: 3)
    }

    mutating func updatePort(_ value: String) {
        port = value.decimalDigits(maxLength: 5)
    }

    static func shouldAdvance(octet: String) -> Bool {
        let digits = octet.decimalDigits(maxLength: 3)
        return digits.count == 3 || (digits.count == 2 && (Int(digits) ?? 0) > 25)
    }
}

nonisolated private extension String {
    func decimalDigits(maxLength: Int) -> String {
        String(filter { "0123456789".contains($0) }.prefix(maxLength))
    }
}

struct IPv4EndpointInput: View {
    @Binding var endpoint: String
    @FocusState private var focusedField: Field?

    private enum Field: Hashable {
        case octet(Int)
        case port
    }

    var body: some View {
        let draft = ManualEndpointDraft(text: endpoint)
        VStack(alignment: .leading, spacing: PicooSpace.xs) {
            Text("IP 地址")
                .font(.caption)
                .foregroundStyle(PicooColor.contentMuted)

            HStack(spacing: PicooSpace.xs) {
                ForEach(draft.octets.indices, id: \.self) { index in
                    endpointField(
                        value: draft.octets[index],
                        placeholder: index == 0 ? "192" : index == 1 ? "168" : "1",
                        label: "IP 地址第 \(index + 1) 段",
                        field: .octet(index)
                    ) { value in
                        updateOctet(index, value: value)
                    }
                    if index != draft.octets.indices.last {
                        Text(".").font(.headline)
                    }
                }
                Text(":").font(.headline)
                endpointField(
                    value: draft.port,
                    placeholder: ManualEndpointDraft.defaultPort,
                    label: "端口",
                    field: .port
                ) { value in
                    var updated = ManualEndpointDraft(text: endpoint)
                    updated.updatePort(value)
                    endpoint = updated.text
                }
                .frame(maxWidth: 76)
            }
        }
        .toolbar {
            ToolbarItemGroup(placement: .keyboard) {
                Spacer()
                Button(focusedField == .port ? "完成" : "下一项") {
                    advanceFocus()
                }
            }
        }
        .onAppear { focusedField = .octet(0) }
    }

    private func endpointField(
        value: String,
        placeholder: String,
        label: String,
        field: Field,
        onChange: @escaping (String) -> Void
    ) -> some View {
        TextField(
            "",
            text: Binding(get: { value }, set: { newValue in onChange(newValue) }),
            prompt: Text(placeholder)
        )
            .font(.body.monospacedDigit())
            .multilineTextAlignment(.center)
            .keyboardType(.numberPad)
            .focused($focusedField, equals: field)
            .textFieldStyle(.roundedBorder)
            .frame(minWidth: 0, maxWidth: .infinity, minHeight: PicooIconSize.touchTarget)
            .accessibilityLabel(label)
    }

    private func updateOctet(_ index: Int, value: String) {
        if let pasted = ManualEndpointDraft(pastedText: value) {
            endpoint = pasted.text
            focusedField = .port
            return
        }

        var updated = ManualEndpointDraft(text: endpoint)
        updated.updateOctet(index, value: value)
        endpoint = updated.text
        if ManualEndpointDraft.shouldAdvance(octet: value) {
            focusedField = index == 3 ? .port : .octet(index + 1)
        }
    }

    private func advanceFocus() {
        switch focusedField {
        case let .octet(index):
            focusedField = index == 3 ? .port : .octet(index + 1)
        case .port:
            focusedField = nil
        case nil:
            focusedField = .octet(0)
        }
    }
}
