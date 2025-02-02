use std::ops::AddAssign;
use std::simd::num::SimdFloat;
use std::simd::{LaneCount, Simd, SupportedLaneCount};

use crate::{CalculateSquared, DistanceCalculator};

pub struct DotProductDistanceCalculator {}

impl DotProductDistanceCalculator {
    pub fn calculate_scalar(a: &[f32], b: &[f32]) -> f32 {
        let mut ret = 0.0;
        for i in 0..a.len() {
            ret += a[i] * b[i];
        }
        Self::neg_score(ret)
    }

    /*
     * In our code, the lower distance value, the greater similarity between two vectors.
     * However, in dot product, two vector having the same direction
     * will yield the largest distance.
     * Thus, we need to take negative value of the original dot product value.
     */
    #[inline(always)]
    pub fn neg_score(x: f32) -> f32 {
        -x
    }
}

impl CalculateSquared for DotProductDistanceCalculator {
    fn calculate_squared(a: &[f32], b: &[f32]) -> f32 {
        DotProductDistanceCalculator::calculate(a, b)
    }
}

impl DistanceCalculator for DotProductDistanceCalculator {
    #[inline(always)]
    fn calculate(a: &[f32], b: &[f32]) -> f32 {
        let mut res = 0.0;
        let mut a_vec = a;
        let mut b_vec = b;

        if a_vec.len() > 16 {
            let mut accumulator = Simd::<f32, 16>::splat(0.0);
            Self::accumulate_lanes::<16>(a_vec, b_vec, &mut accumulator);
            res += accumulator.reduce_sum();
            a_vec = a_vec.chunks_exact(16).remainder();
            b_vec = b_vec.chunks_exact(16).remainder();
        }

        if a_vec.len() > 8 {
            let mut accumulator = Simd::<f32, 8>::splat(0.0);
            Self::accumulate_lanes::<8>(a_vec, b_vec, &mut accumulator);
            res += accumulator.reduce_sum();
            a_vec = a_vec.chunks_exact(8).remainder();
            b_vec = b_vec.chunks_exact(8).remainder();
        }

        if a_vec.len() > 4 {
            let mut accumulator = Simd::<f32, 4>::splat(0.0);
            Self::accumulate_lanes::<4>(a_vec, b_vec, &mut accumulator);
            res += accumulator.reduce_sum();
            a_vec = a_vec.chunks_exact(4).remainder();
            b_vec = b_vec.chunks_exact(4).remainder();
        }

        for i in 0..a_vec.len() {
            res += a_vec[i] * b_vec[i];
        }
        Self::neg_score(res)
    }

    #[inline(always)]
    fn accumulate_lanes<const LANES: usize>(
        a: &[f32],
        b: &[f32],
        accumulator: &mut Simd<f32, LANES>,
    ) where
        LaneCount<LANES>: SupportedLaneCount,
    {
        a.chunks_exact(LANES)
            .zip(b.chunks_exact(LANES))
            .for_each(|(a_chunk, b_chunk)| {
                let a_simd = Simd::<f32, LANES>::from_slice(a_chunk);
                let b_simd = Simd::<f32, LANES>::from_slice(b_chunk);
                accumulator.add_assign(a_simd * b_simd);
            });
    }

    #[inline(always)]
    fn accumulate_scalar(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b.iter()).map(|(&x, &y)| x * y).sum()
    }

    #[inline(always)]
    fn outermost_op(x: f32) -> f32 {
        Self::neg_score(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::generate_random_vector;
    #[test]
    fn test_dot_product_distance_calculator() {
        let a = generate_random_vector(128);
        let b = generate_random_vector(128);
        let eps = 2.0 * 1e-5;
        let result = DotProductDistanceCalculator::calculate(&a, &b);
        let expected = DotProductDistanceCalculator::calculate_scalar(&a, &b);
        assert!((result - expected).abs() < eps);
    }

    #[test]
    fn test_accumulate_scalar() {
        let a = generate_random_vector(30);
        let b = generate_random_vector(30);

        let epsilon = 1e-5;
        let distance_scalar = DotProductDistanceCalculator::calculate_scalar(&a, &b);
        let accumulate_scalar = DotProductDistanceCalculator::accumulate_scalar(&a, &b);
        assert!((distance_scalar - accumulate_scalar.sqrt()) < epsilon)
    }
}
