//! 키드 난수 (02b §6, 02d §1.4, S-E3).
//!
//! 키 `[scenario_seed, mechanism_id, stable_entity_id, step, event_slot, draw_index]`를
//! 공백 없는 JSON 배열로 직렬화해 SHA-256을 내고, 앞 8바이트(big-endian)의 상위 53비트를
//! 2^53으로 나눈다. 같은 접두를 여러 번 쓰는 곳은 해시 상태를 복제해 같은 값을 더 싸게 낸다.

use sha2::{Digest, Sha256};

const TWO_53: f64 = 9_007_199_254_740_992.0;

/// JSON 문자열 표기(필수 이스케이프만, 비ASCII는 그대로).
pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn to_unit(digest: &[u8]) -> f64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&digest[..8]);
    (u64::from_be_bytes(b) >> 11) as f64 / TWO_53
}

/// 키의 정규 JSON 바이트(적합성 벡터용).
pub fn canonical_key(seed: u64, mech: &str, entity: &str, step: u64, slot: u64, draw: u32) -> String {
    format!("[{seed},{},{},{step},{slot},{draw}]", json_str(mech), json_str(entity))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyRng {
    pub seed: u64,
}

impl KeyRng {
    pub fn new(seed: u64) -> KeyRng {
        KeyRng { seed }
    }

    /// [0, 1) 균등값.
    pub fn u(&self, mech: &str, entity: &str, step: u64, slot: u64, draw: u32) -> f64 {
        let key = canonical_key(self.seed, mech, entity, step, slot, draw);
        to_unit(&Sha256::digest(key.as_bytes()))
    }

    pub fn prefix(&self, mech: &str, entity: &str) -> KeyPrefix {
        let mut h = Sha256::new();
        h.update(format!("[{},{},{},", self.seed, json_str(mech), json_str(entity)).as_bytes());
        KeyPrefix { state: h }
    }
}

/// 같은 (시드, 메커니즘, 개체)의 해시 상태. 결과는 `KeyRng::u`와 같다.
#[derive(Clone)]
pub struct KeyPrefix {
    state: Sha256,
}

impl KeyPrefix {
    pub fn u(&self, step: u64, slot: u64, draw: u32) -> f64 {
        let mut h = self.state.clone();
        h.update(format!("{step},{slot},{draw}]").as_bytes());
        to_unit(&h.finalize())
    }
}

/// 균등값 u에서 Poisson(λ) 표본(역누적 분포). λ ≤ 0이면 0.
pub fn poisson(lambda: f64, u: f64) -> u32 {
    if !(lambda > 0.0) {
        return 0;
    }
    if lambda > 60.0 {
        // 정규 근사(Box–Muller 대신 역함수 근사): 여기서는 쓰이지 않을 만큼 큰 λ에 대한 보호.
        let z = inv_norm(u.clamp(1e-12, 1.0 - 1e-12));
        return (lambda + z * lambda.sqrt()).round().max(0.0) as u32;
    }
    let mut p = (-lambda).exp();
    let mut f = p;
    let mut k = 0u32;
    while u >= f && k < 10_000 {
        k += 1;
        p *= lambda / k as f64;
        f += p;
    }
    k
}

/// 표준정규 역누적(Acklam 근사). 계수는 원문 자릿수 그대로 둔다.
#[allow(clippy::excessive_precision)]
fn inv_norm(p: f64) -> f64 {
    const A: [f64; 6] = [-3.969683028665376e+01, 2.209460984245205e+02, -2.759285104469687e+02, 1.383577518672690e+02, -3.066479806614716e+01, 2.506628277459239e+00];
    const B: [f64; 5] = [-5.447609879822406e+01, 1.615858368580409e+02, -1.556989798598866e+02, 6.680131188771972e+01, -1.328068155288572e+01];
    const C: [f64; 6] = [-7.784894002430293e-03, -3.223964580411365e-01, -2.400758277161838e+00, -2.549732539343734e+00, 4.374664141464968e+00, 2.938163982698783e+00];
    const D: [f64; 4] = [7.784695709041462e-03, 3.224671290700398e-01, 2.445134137142996e+00, 3.754408661907416e+00];
    let pl = 0.02425;
    if p < pl {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5]) / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= 1.0 - pl {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5]) / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
}

/// 몸의 개체 ID.
pub fn body_entity(persona_id: &str, replicate: u16) -> String {
    format!("{persona_id}#{replicate}")
}

/// 반복 시드(02c §2.7 (2)): SHA-256("{base_seed}:{r}")의 앞 8바이트 big-endian.
pub fn rep_seed(base_seed: u64, r: u32) -> u64 {
    let d = Sha256::digest(format!("{base_seed}:{r}").as_bytes());
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[..8]);
    u64::from_be_bytes(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 02c §2.8 적합성 벡터(Python `reaction.rng`이 낸 값)와 비트 단위로 같아야 한다.
    #[test]
    fn conformance_vectors_match_python() {
        assert_eq!(KeyRng::new(0).u("VISIT", "P:d60eabf5-4b9e-5f4a-9e6b-297274538bea", 0, 0, 0), 0.8840986844213997);
        assert_eq!(KeyRng::new(20261031).u("NOTICE", "P:abc#1", 1234, 7, 0), 0.2690967652350067);
        assert_eq!(KeyRng::new(u64::MAX).u("PY.TOURIST", "T:000123", 0, 3, 2), 0.8020344030445702);
        assert_eq!(rep_seed(20261031, 0), 9599481731617930288);
        assert_eq!(rep_seed(20261031, 1), 8923839971560613668);
    }

    #[test]
    fn canonical_form_is_compact_json() {
        assert_eq!(canonical_key(7, "NOTICE", "p-1#0", 12, 3, 0), r#"[7,"NOTICE","p-1#0",12,3,0]"#);
        assert_eq!(canonical_key(1, "VISIT", "김\"x", 0, 0, 1), "[1,\"VISIT\",\"김\\\"x\",0,0,1]");
    }

    #[test]
    fn known_vector_matches_sha256_definition() {
        // 정의: SHA-256("[1,\"VISIT\",\"a\",0,0,0]")의 앞 8바이트 BE >> 11 / 2^53.
        let key = canonical_key(1, "VISIT", "a", 0, 0, 0);
        let d = Sha256::digest(key.as_bytes());
        let mut b = [0u8; 8];
        b.copy_from_slice(&d[..8]);
        let expect = (u64::from_be_bytes(b) >> 11) as f64 / TWO_53;
        assert_eq!(KeyRng::new(1).u("VISIT", "a", 0, 0, 0), expect);
    }

    #[test]
    fn prefix_matches_direct() {
        let r = KeyRng::new(20261031);
        let p = r.prefix("NOTICE", "persona-7#2");
        for step in [0u64, 1, 99, 123_456] {
            for slot in [0u64, 5, 1_000_000_007] {
                for draw in [0u32, 1, 3] {
                    assert_eq!(p.u(step, slot, draw), r.u("NOTICE", "persona-7#2", step, slot, draw));
                }
            }
        }
    }

    #[test]
    fn unit_interval_and_key_sensitivity() {
        let r = KeyRng::new(3);
        let a = r.u("LOOK", "x#0", 1, 0, 0);
        let b = r.u("LOOK", "x#0", 2, 0, 0);
        let c = r.u("LOOK", "x#1", 1, 0, 0);
        assert!((0.0..1.0).contains(&a));
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, r.u("LOOK", "x#0", 1, 0, 0));
    }

    #[test]
    fn poisson_mean_close_to_lambda() {
        let r = KeyRng::new(11);
        for &lam in &[0.05, 0.7, 3.0, 12.0] {
            let n = 20_000;
            let s: u64 = (0..n).map(|i| poisson(lam, r.u("SPAWN", "t", i, 0, 0)) as u64).sum();
            let mean = s as f64 / n as f64;
            assert!((mean - lam).abs() < 0.05 * lam.max(1.0), "λ={lam} 평균={mean}");
        }
        assert_eq!(poisson(0.0, 0.5), 0);
        assert_eq!(poisson(-1.0, 0.5), 0);
    }
}
