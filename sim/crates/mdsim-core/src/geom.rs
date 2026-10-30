//! 평면 기하 (지역 미터, x 동쪽, z 남쪽).

use serde::{Deserialize, Serialize};
use std::ops::{Add, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct V2 {
    pub x: f64,
    pub z: f64,
}

impl V2 {
    pub const ZERO: V2 = V2 { x: 0.0, z: 0.0 };

    pub fn new(x: f64, z: f64) -> V2 {
        V2 { x, z }
    }

    pub fn dot(self, o: V2) -> f64 {
        self.x * o.x + self.z * o.z
    }

    /// 2차원 외적(z 성분). 양수면 o가 self의 왼쪽(x→z 회전 방향 기준).
    pub fn cross(self, o: V2) -> f64 {
        self.x * o.z - self.z * o.x
    }

    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn dist(self, o: V2) -> f64 {
        (self - o).len()
    }

    pub fn norm(self) -> V2 {
        let l = self.len();
        if l > 1e-12 {
            V2::new(self.x / l, self.z / l)
        } else {
            V2::ZERO
        }
    }

    /// 진행 방향의 왼쪽 수직 단위 벡터.
    pub fn perp(self) -> V2 {
        V2::new(-self.z, self.x)
    }

    pub fn angle(self) -> f64 {
        self.z.atan2(self.x)
    }

    pub fn from_angle(a: f64) -> V2 {
        V2::new(a.cos(), a.sin())
    }

    pub fn lerp(self, o: V2, t: f64) -> V2 {
        self + (o - self) * t
    }
}

impl Add for V2 {
    type Output = V2;
    fn add(self, o: V2) -> V2 {
        V2::new(self.x + o.x, self.z + o.z)
    }
}

impl Sub for V2 {
    type Output = V2;
    fn sub(self, o: V2) -> V2 {
        V2::new(self.x - o.x, self.z - o.z)
    }
}

impl Mul<f64> for V2 {
    type Output = V2;
    fn mul(self, k: f64) -> V2 {
        V2::new(self.x * k, self.z * k)
    }
}

impl Neg for V2 {
    type Output = V2;
    fn neg(self) -> V2 {
        V2::new(-self.x, -self.z)
    }
}

/// 각도를 (−π, π]로 맞춘다.
pub fn wrap_angle(a: f64) -> f64 {
    let two_pi = std::f64::consts::TAU;
    let mut x = (a + std::f64::consts::PI) % two_pi;
    if x < 0.0 {
        x += two_pi;
    }
    let r = x - std::f64::consts::PI;
    if r <= -std::f64::consts::PI {
        r + two_pi
    } else {
        r
    }
}

/// 두 방향 사이 각의 절댓값(0~π).
pub fn angle_between(a: f64, b: f64) -> f64 {
    wrap_angle(a - b).abs()
}

/// 점과 선분 사이 거리와 선분 위 매개변수 t∈[0,1].
pub fn dist_point_segment(p: V2, a: V2, b: V2) -> (f64, f64) {
    let ab = b - a;
    let l2 = ab.dot(ab);
    if l2 <= 1e-18 {
        return (p.dist(a), 0.0);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (p.dist(a + ab * t), t)
}

pub fn point_in_polygon(p: V2, poly: &[V2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.z > p.z) != (b.z > p.z) {
            let x = (b.x - a.x) * (p.z - a.z) / (b.z - a.z) + a.x;
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// 폴리라인의 누적 길이(첫 값 0).
pub fn cumulative(poly: &[V2]) -> Vec<f64> {
    let mut out = Vec::with_capacity(poly.len());
    let mut s = 0.0;
    out.push(0.0);
    for w in poly.windows(2) {
        s += w[0].dist(w[1]);
        out.push(s);
    }
    out
}

/// 누적 길이 s의 점과 그 구간의 단위 방향.
pub fn point_at(poly: &[V2], cum: &[f64], s: f64) -> (V2, V2) {
    let n = poly.len();
    if n == 1 {
        return (poly[0], V2::new(1.0, 0.0));
    }
    let total = *cum.last().unwrap();
    let s = s.clamp(0.0, total);
    let mut i = match cum.binary_search_by(|v| v.partial_cmp(&s).unwrap()) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };
    if i >= n - 1 {
        i = n - 2;
    }
    let seg = cum[i + 1] - cum[i];
    let t = if seg > 1e-12 { (s - cum[i]) / seg } else { 0.0 };
    let dir = (poly[i + 1] - poly[i]).norm();
    (poly[i].lerp(poly[i + 1], t), dir)
}

/// 점의 폴리라인 위 투영(누적 길이, 거리).
pub fn project_on_polyline(poly: &[V2], cum: &[f64], p: V2) -> (f64, f64) {
    let mut best = (0.0, f64::INFINITY);
    for i in 0..poly.len().saturating_sub(1) {
        let (d, t) = dist_point_segment(p, poly[i], poly[i + 1]);
        if d < best.1 {
            best = (cum[i] + t * (cum[i + 1] - cum[i]), d);
        }
    }
    if poly.len() == 1 {
        best = (0.0, p.dist(poly[0]));
    }
    best
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    pub min: V2,
    pub max: V2,
}

impl BBox {
    pub fn empty() -> BBox {
        BBox { min: V2::new(f64::INFINITY, f64::INFINITY), max: V2::new(f64::NEG_INFINITY, f64::NEG_INFINITY) }
    }

    pub fn add(&mut self, p: V2) {
        self.min.x = self.min.x.min(p.x);
        self.min.z = self.min.z.min(p.z);
        self.max.x = self.max.x.max(p.x);
        self.max.z = self.max.z.max(p.z);
    }

    pub fn grow(&self, m: f64) -> BBox {
        BBox { min: V2::new(self.min.x - m, self.min.z - m), max: V2::new(self.max.x + m, self.max.z + m) }
    }

    pub fn is_valid(&self) -> bool {
        self.min.x <= self.max.x && self.min.z <= self.max.z
    }

    pub fn contains(&self, p: V2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.z >= self.min.z && p.z <= self.max.z
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_distance_and_polygon() {
        let (d, t) = dist_point_segment(V2::new(5.0, 3.0), V2::new(0.0, 0.0), V2::new(10.0, 0.0));
        assert!((d - 3.0).abs() < 1e-12 && (t - 0.5).abs() < 1e-12);
        let sq = [V2::new(0.0, 0.0), V2::new(4.0, 0.0), V2::new(4.0, 4.0), V2::new(0.0, 4.0)];
        assert!(point_in_polygon(V2::new(2.0, 2.0), &sq));
        assert!(!point_in_polygon(V2::new(5.0, 2.0), &sq));
    }

    #[test]
    fn polyline_walk() {
        let poly = vec![V2::new(0.0, 0.0), V2::new(10.0, 0.0), V2::new(10.0, 10.0)];
        let cum = cumulative(&poly);
        assert_eq!(cum, vec![0.0, 10.0, 20.0]);
        let (p, d) = point_at(&poly, &cum, 15.0);
        assert!((p.x - 10.0).abs() < 1e-12 && (p.z - 5.0).abs() < 1e-12);
        assert!((d.z - 1.0).abs() < 1e-12);
        let (s, dist) = project_on_polyline(&poly, &cum, V2::new(12.0, 4.0));
        assert!((s - 14.0).abs() < 1e-12 && (dist - 2.0).abs() < 1e-12);
    }

    #[test]
    fn angles_wrap() {
        assert!((wrap_angle(3.0 * std::f64::consts::PI) - std::f64::consts::PI).abs() < 1e-12);
        assert!((angle_between(0.1, -0.1) - 0.2).abs() < 1e-12);
        assert!((angle_between(3.1, -3.1) - (std::f64::consts::TAU - 6.2)).abs() < 1e-9);
    }
}
