//! 공통 차원과 관심 차원 (C-201, S-E18). 순서는 02d §2.1과 같다.

use serde::{Deserialize, Serialize};

pub const ND: usize = 10;
pub const NI: usize = 7;

pub type DimVec = [f64; ND];
pub type IntVec = [f64; NI];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Dim {
    Climb,
    Fall,
    Step,
    Walk,
    Time,
    Money,
    Way,
    Sense,
    Crowd,
    Safe,
}

impl Dim {
    pub const ALL: [Dim; ND] = [
        Dim::Climb,
        Dim::Fall,
        Dim::Step,
        Dim::Walk,
        Dim::Time,
        Dim::Money,
        Dim::Way,
        Dim::Sense,
        Dim::Crowd,
        Dim::Safe,
    ];

    pub fn ix(self) -> usize {
        self as usize
    }

    pub fn id(self) -> &'static str {
        match self {
            Dim::Climb => "D-CLIMB",
            Dim::Fall => "D-FALL",
            Dim::Step => "D-STEP",
            Dim::Walk => "D-WALK",
            Dim::Time => "D-TIME",
            Dim::Money => "D-MONEY",
            Dim::Way => "D-WAY",
            Dim::Sense => "D-SENSE",
            Dim::Crowd => "D-CROWD",
            Dim::Safe => "D-SAFE",
        }
    }

    /// 열 이름에 쓰는 소문자 꼬리(cap_climb_comfort 등).
    pub fn key(self) -> &'static str {
        match self {
            Dim::Climb => "climb",
            Dim::Fall => "fall",
            Dim::Step => "step",
            Dim::Walk => "walk",
            Dim::Time => "time",
            Dim::Money => "money",
            Dim::Way => "way",
            Dim::Sense => "sense",
            Dim::Crowd => "crowd",
            Dim::Safe => "safe",
        }
    }

    pub fn from_id(s: &str) -> Option<Dim> {
        Dim::ALL.iter().copied().find(|d| d.id() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Interest {
    Food,
    Beauty,
    Fashion,
    Souvenir,
    Culture,
    Faith,
    Nature,
}

impl Interest {
    pub const ALL: [Interest; NI] = [
        Interest::Food,
        Interest::Beauty,
        Interest::Fashion,
        Interest::Souvenir,
        Interest::Culture,
        Interest::Faith,
        Interest::Nature,
    ];

    pub fn ix(self) -> usize {
        self as usize
    }

    pub fn id(self) -> &'static str {
        match self {
            Interest::Food => "A-FOOD",
            Interest::Beauty => "A-BEAUTY",
            Interest::Fashion => "A-FASHION",
            Interest::Souvenir => "A-SOUVENIR",
            Interest::Culture => "A-CULTURE",
            Interest::Faith => "A-FAITH",
            Interest::Nature => "A-NATURE",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Interest::Food => "food",
            Interest::Beauty => "beauty",
            Interest::Fashion => "fashion",
            Interest::Souvenir => "souvenir",
            Interest::Culture => "culture",
            Interest::Faith => "faith",
            Interest::Nature => "nature",
        }
    }

    pub fn from_id(s: &str) -> Option<Interest> {
        Interest::ALL.iter().copied().find(|i| i.id() == s)
    }

    /// 짧은 이름이나 ID 모두 받는다(목적 목록의 interest 열).
    pub fn parse(s: &str) -> Option<Interest> {
        Interest::from_id(s).or_else(|| Interest::ALL.iter().copied().find(|i| i.key() == s))
    }
}

pub fn zero_dims() -> DimVec {
    [0.0; ND]
}

pub fn zero_ints() -> IntVec {
    [0.0; NI]
}
