//! REQ-PICOO-BITSTREAM-002: dependency parsing must reject unrepresentable integers.
use scuffle_bytes_util::BitReader;
use scuffle_expgolomb::BitReaderExpGolombExt;

fn encoded(zeros: usize, suffix: u64) -> Vec<u8> {
    let mut bits = vec![false; zeros];
    bits.push(true);
    for shift in (0..zeros).rev() {
        bits.push(shift < 64 && suffix & (1_u64 << shift) != 0);
    }
    let mut data = vec![0; bits.len().div_ceil(8)];
    for (index, bit) in bits.into_iter().enumerate() {
        data[index / 8] |= u8::from(bit) << (7 - index % 8);
    }
    data
}

#[test]
fn unsigned_limits_and_oversized_prefixes_are_explicit_results() {
    for zeros in 0..64 {
        let data = encoded(zeros, 0);
        assert_eq!(
            BitReader::new(data.as_slice()).read_exp_golomb().unwrap(),
            (1_u64 << zeros) - 1
        );
    }
    let maximum = encoded(64, 0);
    assert_eq!(
        BitReader::new(maximum.as_slice())
            .read_exp_golomb()
            .unwrap(),
        u64::MAX
    );
    for (zeros, suffix) in [(64, 1), (65, 0), (128, 0)] {
        let data = encoded(zeros, suffix);
        assert!(BitReader::new(data.as_slice()).read_exp_golomb().is_err());
    }
}

#[test]
fn signed_overflow_is_rejected_and_truncation_never_becomes_a_value() {
    let overflow = encoded(64, 0);
    assert!(BitReader::new(overflow.as_slice())
        .read_signed_exp_golomb()
        .is_err());
    let maximum = encoded(63, u64::MAX - 2 - ((1_u64 << 63) - 1));
    assert_eq!(
        BitReader::new(maximum.as_slice())
            .read_signed_exp_golomb()
            .unwrap(),
        i64::MAX
    );
    for length in 0..overflow.len() {
        assert!(BitReader::new(&overflow[..length])
            .read_exp_golomb()
            .is_err());
    }
}
