use std::borrow::Cow;

/// Corpus files may use `hex:` so security-relevant binary seeds remain
/// reviewable as ordinary text. Mutated fuzzer inputs continue down the raw path.
pub fn decode_seed(data: &[u8]) -> Cow<'_, [u8]> {
    let Some(mut hex) = data.strip_prefix(b"hex:") else {
        return Cow::Borrowed(data);
    };
    while hex.last().is_some_and(u8::is_ascii_whitespace) {
        hex = &hex[..hex.len() - 1];
    }
    let mut output = Vec::with_capacity(hex.len() / 2);
    let mut pairs = hex.chunks_exact(2);
    for pair in &mut pairs {
        let Some(high) = digit(pair[0]) else {
            return Cow::Borrowed(data);
        };
        let Some(low) = digit(pair[1]) else {
            return Cow::Borrowed(data);
        };
        output.push((high << 4) | low);
    }
    if !pairs.remainder().is_empty() {
        return Cow::Borrowed(data);
    }
    Cow::Owned(output)
}

fn digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
