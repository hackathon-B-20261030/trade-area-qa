//! 시험용 작은 XML 정형성 검사기(표준 라이브러리만 쓴다).
//!
//! 단위 시험과 `tests/plots.rs`가 함께 쓴다(`#[path]`로 포함). 글자 내용은 보지 않고
//! 태그 짝, 빈 요소(`/>`), 선언(`<?`·`<!`), 주석, 따옴표 안의 `<`, 문자 참조(`&...;`)만 검사한다.

#![allow(dead_code)]

/// 문서가 정형인지 검사한다. 정형이 아니면 위치와 이유를 담은 오류를 돌려준다.
pub fn check_well_formed(doc: &str) -> Result<(), String> {
    check_entities(doc)?;
    let b = doc.as_bytes();
    let mut i = 0usize;
    let mut stack: Vec<String> = Vec::new();
    let mut roots = 0usize;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &doc[i..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").ok_or_else(|| format!("{i}: 닫히지 않은 주석"))?;
            i += end + 3;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").ok_or_else(|| format!("{i}: 닫히지 않은 CDATA"))?;
            i += end + 3;
            continue;
        }
        if rest.starts_with("<?") {
            let end = rest.find("?>").ok_or_else(|| format!("{i}: 닫히지 않은 처리 명령"))?;
            i += end + 2;
            continue;
        }
        if rest.starts_with("<!") {
            let end = rest.find('>').ok_or_else(|| format!("{i}: 닫히지 않은 선언"))?;
            i += end + 1;
            continue;
        }
        // 태그 끝 찾기(따옴표 안의 '>'는 건너뛴다)
        let mut j = i + 1;
        let mut quote: Option<u8> = None;
        while j < b.len() {
            let c = b[j];
            match quote {
                Some(q) => {
                    if c == q {
                        quote = None;
                    } else if c == b'<' {
                        return Err(format!("{j}: 속성 값 안의 '<'"));
                    }
                }
                None => {
                    if c == b'"' || c == b'\'' {
                        quote = Some(c);
                    } else if c == b'>' {
                        break;
                    } else if c == b'<' {
                        return Err(format!("{j}: 태그 안의 '<'"));
                    }
                }
            }
            j += 1;
        }
        if j >= b.len() {
            return Err(format!("{i}: 닫히지 않은 태그"));
        }
        let inner = &doc[i + 1..j];
        if let Some(name) = inner.strip_prefix('/') {
            let name = name.trim();
            match stack.pop() {
                Some(open) if open == name => {}
                other => return Err(format!("{i}: </{name}>가 여는 태그 {other:?}와 맞지 않음")),
            }
        } else {
            let self_closing = inner.ends_with('/');
            let body = inner.trim_end_matches('/');
            let name: String = body.split(|c: char| c.is_whitespace()).next().unwrap_or("").to_string();
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.')) {
                return Err(format!("{i}: 잘못된 태그 이름 {name:?}"));
            }
            check_attrs(&body[name.len()..]).map_err(|e| format!("{i}: <{name}> {e}"))?;
            if stack.is_empty() {
                roots += 1;
            }
            if !self_closing {
                stack.push(name);
            }
        }
        i = j + 1;
    }
    if let Some(open) = stack.last() {
        return Err(format!("닫히지 않은 요소 <{open}> (남은 {}개)", stack.len()));
    }
    if roots == 0 {
        return Err("루트 요소 없음".to_string());
    }
    Ok(())
}

/// 속성 목록: `이름="값"` 또는 `이름='값'`이 공백으로 나뉘고 이름이 겹치지 않아야 한다.
fn check_attrs(s: &str) -> Result<(), String> {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut names: Vec<&str> = Vec::new();
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            return Ok(());
        }
        let start = i;
        while i < b.len() && b[i] != b'=' && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        let name = &s[start..i];
        if name.is_empty() {
            return Err("빈 속성 이름".to_string());
        }
        if names.contains(&name) {
            return Err(format!("겹친 속성 {name}"));
        }
        names.push(name);
        if i >= b.len() || b[i] != b'=' {
            return Err(format!("속성 {name}에 값이 없음"));
        }
        i += 1;
        if i >= b.len() || (b[i] != b'"' && b[i] != b'\'') {
            return Err(format!("속성 {name}의 값에 따옴표가 없음"));
        }
        let q = b[i];
        i += 1;
        while i < b.len() && b[i] != q {
            i += 1;
        }
        if i >= b.len() {
            return Err(format!("속성 {name}의 따옴표가 닫히지 않음"));
        }
        i += 1;
        if i < b.len() && !b[i].is_ascii_whitespace() {
            return Err(format!("속성 {name} 뒤에 공백이 없음"));
        }
    }
}

/// 모든 `&`가 알려진 문자 참조로 시작하는지 검사한다.
fn check_entities(doc: &str) -> Result<(), String> {
    for (i, _) in doc.match_indices('&') {
        let rest = &doc[i + 1..];
        let end = rest.find(';').ok_or_else(|| format!("{i}: ';' 없는 '&'"))?;
        let ent = &rest[..end];
        let ok = matches!(ent, "amp" | "lt" | "gt" | "quot" | "apos")
            || ent.strip_prefix("#x").is_some_and(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit()))
            || ent.strip_prefix('#').is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
        if !ok {
            return Err(format!("{i}: 알 수 없는 문자 참조 &{ent};"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check_well_formed;

    #[test]
    fn checker_accepts_and_rejects() {
        assert!(check_well_formed("<a><b x=\"1\"/><c>t &amp; u</c></a>").is_ok());
        assert!(check_well_formed("<!doctype html><html><meta charset=\"utf-8\"/></html>").is_ok());
        assert!(check_well_formed("<?xml version=\"1.0\"?><a/>").is_ok());
        assert!(check_well_formed("<a><b></a>").is_err());
        assert!(check_well_formed("<a>").is_err());
        assert!(check_well_formed("<a x=1/>").is_err());
        assert!(check_well_formed("<a x=\"1\" x=\"2\"/>").is_err());
        assert!(check_well_formed("<a>&bad;</a>").is_err());
        assert!(check_well_formed("<a>a & b</a>").is_err());
        assert!(check_well_formed("<a x=\"<\"/>").is_err());
        assert!(check_well_formed("text only").is_err());
    }
}
