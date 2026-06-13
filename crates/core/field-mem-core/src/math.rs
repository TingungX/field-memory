use crate::types::Vector;

/// Cosine similarity between two vectors
pub fn cosine_similarity(a: &Vector, b: &Vector) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na = l2_norm(a);
    let nb = l2_norm(b);
    if na < 1e-8 || nb < 1e-8 {
        0.0
    } else {
        (dot / (na * nb)).clamp(-1.0, 1.0)
    }
}

/// Cosine distance = 1 - cosine_similarity
pub fn cosine_distance(a: &Vector, b: &Vector) -> f32 {
    1.0 - cosine_similarity(a, b)
}

/// L2 norm (Euclidean length)
pub fn l2_norm(v: &Vector) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

/// Dot product
pub fn dot(a: &Vector, b: &Vector) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Normalize to unit vector
pub fn normalize(v: &Vector) -> Vector {
    let n = l2_norm(v);
    if n < 1e-8 {
        vec![0.0; v.len()]
    } else {
        v.iter().map(|x| x / n).collect()
    }
}

/// Element-wise addition of two vectors
pub fn add(a: &Vector, b: &Vector) -> Vector {
    a.iter().zip(b.iter()).map(|(x, y)| x + y).collect()
}

/// Scale a vector by scalar
pub fn scale(v: &Vector, s: f32) -> Vector {
    v.iter().map(|x| x * s).collect()
}

/// Moving average of two directions, rate in [0,1]
pub fn moving_avg_direction(current: &Vector, target: &Vector, rate: f32) -> Vector {
    let r = rate.clamp(0.0, 1.0);
    current.iter().zip(target.iter())
        .map(|(c, t)| c + r * (t - c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine_similarity(&a, &b) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_negative() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 0.001);
    }

    #[test]
    fn test_normalize_unit_length() {
        let v = vec![3.0, 4.0];
        let n = normalize(&v);
        assert!((l2_norm(&n) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_normalize_zero_vector() {
        let v = vec![0.0, 0.0];
        let n = normalize(&v);
        assert!(n.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn test_moving_avg_identity() {
        let c = vec![1.0, 2.0];
        let t = vec![3.0, 4.0];
        let result = moving_avg_direction(&c, &t, 0.0);
        assert_eq!(result, c);
    }

    #[test]
    fn test_moving_avg_full_replace() {
        let c = vec![1.0, 2.0];
        let t = vec![3.0, 4.0];
        let result = moving_avg_direction(&c, &t, 1.0);
        assert!((result[0] - 3.0).abs() < 0.001);
        assert!((result[1] - 4.0).abs() < 0.001);
    }
}

