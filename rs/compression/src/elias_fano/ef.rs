use anyhow::{anyhow, Result};
use bitvec::prelude::*;

pub struct EliasFano {
    size: usize,
    lower_bits: BitVec,
    upper_bits: BitVec,
    lower_bit_mask: u64,
    lower_bit_length: usize,
}

// TODO(tyb): consider moving this to utils
fn msb(n: u64) -> u64 {
    if n == 0 {
        0
    } else {
        let highest_index = 63u64;
        highest_index - n.leading_zeros() as u64
    }
}

impl EliasFano {
    /// Creates a new EliasFano structure
    pub fn new(universe: usize, size: usize) -> Self {
        // lower_bit_length = floor(log(universe / size))
        // More efficient way to do it is with bit manipulation
        let lower_bit_length = if universe > size {
            msb((universe / size) as u64)
        } else {
            0
        } as usize;
        let lower_bit_mask = (1 << lower_bit_length) - 1;
        let mut lower_bits = BitVec::with_capacity(size * lower_bit_length);
        // Ensure lower_bits is filled with false initially
        lower_bits.resize(size * lower_bit_length, false);

        // The upper bits are encoded using unary coding for the gaps between consecutive values.
        // This part uses at most 2n bits:
        // - There are exactly n '1' bits, one for each of the n elements in the sequence.
        // - The number of '0' bits is at most n, representing the gaps between the high bits of
        // consecutive elements (the total number of possible distinct values that can be
        // represented by the high parts is limited by the number of elements in the sequence)
        Self {
            size,
            lower_bits,
            upper_bits: BitVec::with_capacity(2 * size),
            lower_bit_mask,
            lower_bit_length,
        }
    }

    /// Encodes a sorted slice of integers
    // Algorithm described in https://vigna.di.unimi.it/ftp/papers/QuasiSuccinctIndices.pdf
    pub fn encode(&mut self, values: &[u64]) {
        let mut prev_high = 0;
        for (i, &val) in values.iter().enumerate() {
            // Encode lower bits efficiently
            if self.lower_bit_length > 0 {
                let low = val & self.lower_bit_mask;
                let start = i * self.lower_bit_length;
                self.lower_bits[start..start + self.lower_bit_length].store(low as u64);
            }

            // Encode upper bits using unary coding
            let high = val >> self.lower_bit_length;
            let gap = high - prev_high;
            self.upper_bits
                .extend_from_bitslice(&BitVec::<u8>::repeat(false, gap as usize));
            self.upper_bits.push(true);

            prev_high = high;
        }
    }

    /// Returns the value at the given index
    #[allow(dead_code)]
    fn get(&self, index: usize) -> Result<u64> {
        if index >= self.size {
            return Err(anyhow!("Index {} out of bound", index));
        }

        // Calculate the position in upper bits
        let mut high = 0;
        let mut pos = 0;

        // Calculate the high part of the value
        for _ in 0..index + 1 {
            while pos < self.upper_bits.len() && !self.upper_bits[pos] {
                // Add the gap to high
                high += 1;
                pos += 1;
            }
            // Skip the '1' that terminates the unary code
            pos += 1;
        }

        // Calculate the low part of the value
        let mut low = 0;
        if self.lower_bit_length > 0 {
            let low_start = index * self.lower_bit_length;
            low = (self.lower_bits[low_start..low_start + self.lower_bit_length].load::<u64>()
                & self.lower_bit_mask) as usize;
        }

        Ok((high << self.lower_bit_length | low) as u64)
    }

    /// Returns the number of elements in the encoded sequence
    pub fn len(&self) -> usize {
        self.size as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elias_fano_encoding() {
        let values = vec![5, 8, 8, 15, 32];
        let upper_bound = 36;
        let mut ef = EliasFano::new(upper_bound, values.len());
        ef.encode(&values);

        // Calculate expected lower bits
        // L = floor(log2(36/5)) = 2
        // Lower 2 bits of each value: 01, 00, 00, 11, 00
        let expected_lower_bits = bitvec![u8, Lsb0; 1, 0, 0, 0, 0, 0, 1, 1, 0, 0];

        // Calculate expected upper bits
        // Upper bits: 1, 2, 2, 3, 8
        // Gaps: 1, 1, 0, 1, 5
        // Unary encoding: 01|01|1|01|000001
        let expected_upper_bits = bitvec![u8, Lsb0; 0, 1, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 1];

        assert_eq!(ef.lower_bit_length, 2);
        assert_eq!(ef.lower_bits, expected_lower_bits,);
        assert_eq!(ef.upper_bits, expected_upper_bits,);
    }

    #[test]
    fn test_elias_fano_decoding() {
        let test_cases = vec![
            (vec![5, 8, 8, 15, 32], 36),                // Basic case
            (vec![0, 1, 2, 3, 4], 5),                   // Start with 0
            (vec![10], 20),                             // Single element
            (vec![1000, 2000, 3000, 4000, 5000], 6000), // Large numbers
            (vec![2, 4, 6, 8, 10], 10),                 // Non-consecutive integers
        ];

        for (values, upper_bound) in test_cases {
            let mut ef = EliasFano::new(upper_bound, values.len());
            ef.encode(&values);

            for i in 0..values.len() {
                let decoded_value = ef.get(i).expect("Failed to decode value");
                assert_eq!(values[i], decoded_value);
            }
        }

        // Test random access on a larger set
        let values: Vec<u64> = (1..=100).collect(); // Sorted list from 1 to 100
        let upper_bound = 9999;

        let mut ef = EliasFano::new(upper_bound, values.len());
        ef.encode(&values);

        // Check random accesses
        assert_eq!(ef.get(0).expect("Failed to decode value"), 1);
        assert_eq!(ef.get(50).expect("Failed to decode value"), 51);
        assert_eq!(ef.get(99).expect("Failed to decode value"), 100);

        // Test out of bounds
        assert!(ef.get(100).is_err());
    }
}
