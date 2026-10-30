//! 오류 종류와 종료 코드 (02d §1.7).

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum SimError {
    /// 정의 해시 불일치, 실행에 필요한 키 누락, 식 오류. 종료 코드 2.
    Defs(String),
    /// 번들 계약 위반(열·타입·범위·참조). 종료 코드 3.
    Contract { file: String, msg: String },
    /// 런타임 불변식 위반. 종료 코드 4.
    Invariant(String),
    /// 파일 입출력. 종료 코드 1.
    Io(String),
    /// 명령 사용·실행 설정 오류(이미 있는 산출 폴더 등). 종료 코드 5.
    Run(String),
}

impl SimError {
    pub fn exit_code(&self) -> i32 {
        match self {
            SimError::Defs(_) => 2,
            SimError::Contract { .. } => 3,
            SimError::Invariant(_) => 4,
            SimError::Io(_) => 1,
            SimError::Run(_) => 5,
        }
    }

    pub fn contract(file: impl Into<String>, msg: impl Into<String>) -> SimError {
        SimError::Contract { file: file.into(), msg: msg.into() }
    }

    /// 단계 이름을 앞에 붙인다(C-228 ①).
    pub fn at_stage(self, stage: &str) -> SimError {
        match self {
            SimError::Defs(m) => SimError::Defs(format!("[{stage}] {m}")),
            SimError::Contract { file, msg } => SimError::Contract { file, msg: format!("[{stage}] {msg}") },
            SimError::Invariant(m) => SimError::Invariant(format!("[{stage}] {m}")),
            SimError::Io(m) => SimError::Io(format!("[{stage}] {m}")),
            SimError::Run(m) => SimError::Run(format!("[{stage}] {m}")),
        }
    }
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimError::Defs(m) => write!(f, "정의 오류: {m}"),
            SimError::Contract { file, msg } => write!(f, "계약 위반({file}): {msg}"),
            SimError::Invariant(m) => write!(f, "불변식 위반: {m}"),
            SimError::Io(m) => write!(f, "입출력 오류: {m}"),
            SimError::Run(m) => write!(f, "실행 설정 오류: {m}"),
        }
    }
}

impl std::error::Error for SimError {}

impl From<std::io::Error> for SimError {
    fn from(e: std::io::Error) -> Self {
        SimError::Io(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, SimError>;
