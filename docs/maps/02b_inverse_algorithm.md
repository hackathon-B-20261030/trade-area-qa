---
version: 1.1
last_modified: 2026-10-31
status: confirmed
---

# 온톨로지 기반 정방향 기록·목표 역산 알고리즘

이 문서는 [개념 Map C-100~C-105](01_concept_map.md#온톨로지-기반-정방향-기록과-목표-역산--c-100c-105)를 구현하기 위한 수학·계약 명세다. **설계 확정이며 실행 결과가 아니다.** 단위별 파일·인터페이스·테스트는 [구현체 Map](02_impl_map.md#온톨로지-기반-정방향역산-구현체-map), 작업 순서는 [구현 순서](03_build_order.md#온톨로지-기반-정방향역산-구현-순서), 선택 근거는 [DR-100~DR-108](decisions.md#dr-100--역산-엔진과-온톨로지의-연결-범위)에 있다.

## 1. 풀려는 문제

사용자가 확정한 계약은 다음과 같다.

1. 범용 엔진이 온톨로지와 동역학을 어댑터로 받는다. 명동 페르소나도 같은 계약을 쓴다.
2. 제어할 수 있는 외부 입력만 바꾼다. 날씨·인구 유입 시나리오·초기 상태·규칙 계수·난수는 주어진 조건이다.
3. 선택한 KPI가 목표 허용오차를 만족하는 후보 중 기준 제어에서 변경량이 작은 해를 찾는다.
4. 정방향 실행과 역산의 모든 평가를 재생 가능하게 기록하고, 최종 후보를 원래 시뮬레이터에서 다시 검증한다.

이는 관측된 결과에서 과거의 유일한 원인을 복원하는 문제와 다르다. 최소 변경량이라는 선택 기준으로 **허용되는 개입 조합**을 찾는다. 상태 변화에는 인과/실행 규칙이 있어야 한다. OWL의 개체·속성·관계 표현만으로 수치 전이식이 정해지지는 않는다. [W3C OWL 2 Primer](https://www.w3.org/TR/owl2-primer/), [Karimi et al., Algorithmic Recourse](https://arxiv.org/abs/2002.06278).

## 2. 그래프를 실행할 수 있게 만드는 계약

온톨로지 그래프 `G_semantic`은 개체·관계의 의미를 표현한다. 실행 그래프 `G_exec`는 등록된 규칙의 실제 읽기/쓰기 의존 관계를 표현한다. 둘을 같은 그래프로 간주하지 않는다.

| 역할 | 기호 | 예시 — 명동 확정 스키마가 아닌 설명용 | 역산 중 처리 |
|---|---|---|---|
| 상태 | x | 장소별 인원, 대기 인원, 활동 상태 | 규칙으로만 변화 |
| 제어 | u | 운영 인력 수, 허용된 유입 배분율 | 지정 범위에서 탐색 |
| 비제어 외생 조건 | w | 날씨, 주어진 유입량 | 시나리오별 고정 |
| 모델 계수 | θ | 이동·선호·서비스 규칙의 계수 | 모델 버전에 고정 |
| 난수 실현값 | ε | 개체별 선택에 쓰는 난수 | 시나리오별 키로 고정 |
| 관측 결과 | y | 특정 기간 최대 혼잡, 종료 대기 인원 | 목표와 비교 |

예를 들어 `Person —prefers→ Place` 관계만 있어서는 몇 명이 언제 이동하는지 계산할 수 없다. 어댑터가 `choice_rule_v3` 같은 명시적 함수를 연결해야 한다. 규칙에는 읽는 값, 쓰는 값, 시간 지연, 수치 단위, 출처와 근거 종류(`measured`, `calibrated`, `assumed`)를 붙인다. 이 라벨은 확률이나 정확도 보장이 아니다.

### 시간과 순환

- 실행 격자는 `t=0,…,H−1`, 상태는 `x_0,…,x_H`다. 각 단계의 제어·외생 입력은 `[t,t+1)` 구간에 적용한다. 실제 시각은 timezone이 있는 시작 시각과 양의 `step_seconds`로 정한다.
- 한 단계의 상태 전이는 전부 `x_t`를 읽고 `x_(t+1)`에 쓴다. 개체를 처리한 순서가 결과를 바꾸면 계약 위반이다.
- 같은 단계의 파생값은 명시된 DAG 순서로 계산한다. 단계 사이의 피드백은 허용한다. 예: 혼잡(t)→이동 선택(t)→혼잡(t+1).
- 같은 단계에 있는 미해결 순환식은 `MODEL_NOT_READY`로 거절한다. 어댑터가 해법·수렴 판정·다해 처리까지 명시한 원자적 규칙으로 제공하면 그 규칙 하나로 실행한다. 엔진이 임의로 한 단계 지연시키지 않는다.
- 장기 지연은 초기 이력을 포함한 상태로 확장해 어댑터가 제공한다. 숨겨진 이전 실행의 상태를 읽지 않는다.
- 그래프 구조·규칙 집합은 역산 한 번 동안 고정한다. 경로의 활성/비활성 등은 미리 선언된 상태나 제어로 표현한다. 새 구조가 필요하면 새 모델 버전으로 별도 역산한다.

## 3. 정방향 함수와 고정 목표

```text
x_(t+1) = f_θ(x_t, u_t, w_t, ε_t; G_exec)
y_j     = H_j(x_0:H, u_0:H−1, w_0:H−1)
F_s(u)  = (y_1,…,y_m)     # 시나리오 s의 원래 시뮬레이터
```

`H_j`는 집계 구간·개체 집합·집계식·단위가 버전으로 고정된 KPI 함수다. 평균인지 최대인지, 순간 상태인지 누적인지까지 입력 계약에 포함한다. KPI 계산에 쓰는 인구 가중치나 분모도 해당 함수와 설정에 포함한다.

목표는 `TargetSpec`에 수치로 고정한다. 과거 실행을 목표로 사용할 때는 `target_source_run_id`와 그 실행의 KPI 정의·결과 해시를 함께 저장한다. 목표 실행과 현재 시나리오의 `w`, `x_0`가 같아야 할 필요는 없다. 서로 다르면 차이를 기록한다. KPI 정의가 다르면 명시적 변환 규칙 없이 목표를 가져올 수 없다.

**자명한 해:** 현재 조건과 목표를 가져온 조건이 같고 기준 제어 `u⁰`가 통과하면 `u*=u⁰`, 변경량 0이다. 엔진은 이 경우 의미 없는 변경을 만들어내지 않는다. 외생 충격 뒤에도 이전 KPI를 유지하거나 새로운 KPI 목표를 지정할 때 보통 비자명한 역산이 된다.

## 4. 수학적 역문제

지표별 허용오차는 요청에 명시한 값으로 계산한다.

```text
δ_j = abs_tol_j + rel_tol_j · |y*_j|
목표 조건: |F_s(u)_j − y*_j| ≤ δ_j, 모든 s ∈ S_search, 모든 j
```

`abs_tol ≥ 0`, `rel_tol ≥ 0`이며 δ=0이면 수치적으로 정확히 같은 결과만 통과한다. δ가 작아 탐색이 어려워도 엔진이 몰래 늘리지 않는다. `S_search`는 요청에 열거된 유한한 외생 조건/난수 실현값의 집합이다. 평균이 맞는 것과 각 시나리오가 맞는 것을 혼동하지 않는다.

```text
u* ∈ argmin D(u,u⁰)
     u ∈ U
     g_k(u, x_s(u), w_s) ≤ 0                 모든 하드 제약과 s
     |F_s(u)_j − y*_j| ≤ δ_j                 모든 목표와 s
```

`U`에는 연속 범위, 정수/격자, 범주, 허용 시점이 들어간다. `g`에는 시간별 변화율, 변수 간 연동, 용량, 중간 상태 제한 등이 들어간다. 제약은 `residual=(value−limit−explicit_tolerance)/scale ≤ 0` 형태로 정규화한다. 원래 값·한계·단위·잔차도 보존한다.

### 변경량

KPI 허용오차와 제어 변경량 척도는 서로 다른 값이다. 요청에서 `l1` 또는 `squared_l2`를 선택하고 숫자 제어마다 양의 척도 `a_i`와 가중치 `ω_i`를 제공한다.

```text
D(u,u⁰) = Σ_t α_t Σ_i ω_i · d_i(u_it, u⁰_it)
α_t = 해당 구간 초 / 전체 기간 초
d_numeric = |(u−u⁰)/a_i|          # l1
          또는 ((u−u⁰)/a_i)^2     # squared_l2
d_category = 명시적 전환 비용 행렬[기준 범주, 후보 범주]
```

범주 비용 행렬의 대각선은 0, 다른 항목은 양수이며 비대칭도 허용한다. 숫자의 척도·가중치도 양수여야 한다. 이로써 `D≥0`, `D(u,u)=0`이고 변경한 값은 양의 비용을 가진다. 금전 비용을 뜻하지 않는다. 시간 블록을 둘로 쪼개도 동일한 제어 경로의 D는 같다. 정의가 없으면 `INVALID_REQUEST`이며 원시 단위의 차이를 임의로 더하지 않는다.

### 해를 못 찾았을 때의 가까움

각 목표는 탐색용 양의 `residual_scale_j`도 받는다. 허용오차 0일 때에도 나눗셈이 정의돼야 하기 때문이다.

```text
v_sj(u) = max(0, |F_s(u)_j − y*_j| − δ_j) / residual_scale_j
V(u)    = max_(s,j) v_sj(u)
```

하드 제약을 만족하는 후보 중 `(V, D, canonical_control_key)`를 사전식으로 비교해 가장 가까운 후보를 보관한다. `V=0`인 후보는 변경량 D로 비교한다. 전부 하드 제약에 걸리면 별도 `hard_violations` 목록을 반환하며 그런 후보를 실행 가능한 가까운 해로 부르지 않는다. NaN/규칙 실패를 큰 숫자의 정상 결과로 위장하지 않는다.

## 5. 상세 기록의 내용

각 정방향 평가에는 재현키 `experiment_key`와 시도별 `attempt_id`가 있다. 동일 실험을 검증 목적으로 다시 실행해도 attempt는 다르다. 기록 구조는 W3C PROV의 입력 개체·실행 활동·파생 결과 관점과 연결되지만, RDF 저장소 도입을 요구하지 않는다. [W3C PROV-DM](https://www.w3.org/TR/prov-dm/).

| 기록 | 반드시 남길 내용 |
|---|---|
| 모델 | 온톨로지·어댑터·규칙·계수·KPI 정의의 버전과 SHA-256, 원본 스냅샷 참조, 근거 종류 |
| 실험 | 시각 격자, 초기 상태, 기준/실제 제어 경로, 고정 외생 경로, 시나리오 ID, 난수 키 규약·seed |
| 계산 환경 | Python·라이브러리·플랫폼, 소스 내용 해시, 수치 dtype, 호출 설정 |
| 단계별 추적 | step, event ID, mechanism ID, 읽은 값과 참조, 쓴 값의 before/after, 조건 분기, 원문/규칙 출처 |
| 결과 | KPI별 정의·값·단위, 상태 제약, 종료 상태, 실패 지점, 상태/추적 내용 해시 |
| 역산 | 요청·목표 해시, 알고리즘 버전/설정/seed, 후보·부모/출처, 호출 순서, V·D·제약, 채택/기각 사유 |
| 검증 | 원본 재실행 참조, 추가 시나리오별 판정, 지표별 미달량, 최소성 보장 수준, 중단 사유 |

최종 결과만 저장하면 모델의 분기·지연·상호작용을 추적할 수 없다. 초기 상태 1회와 단계별 변경 이벤트를 정본으로 저장한다. 변화가 없었던 규칙도 실행/분기 이벤트를 남겨 실제로 실행한 것인지 식별한다. 읽은 값은 값 자체와 생성 이벤트 참조를 남긴다. 재구성 가능성을 확인하기 전에는 전체 상태를 덜 기록하는 모드를 추가하지 않는다.

### 저장 위치와 무결성

```text
runs/inverse/<request_hash>/
  request.json                 # 목표·제약·비용·예산의 정본
  model_manifest.json          # 스냅샷 파일 참조와 해시
  snapshots/<sha256>            # 원본/초기 상태/경로/규칙 설정, 내용 주소
  attempts/<attempt_id>/
    input.json
    trace-000001.jsonl          # 단계/규칙 이벤트, 16 MiB마다 회전
    result.json                # 완료 또는 오류; 부분 출력은 완료로 표시하지 않음
    manifest.json              # 파일별 해시·행 수·complete/failed/incomplete
  candidate_evaluations.jsonl   # 후보마다 참조한 실제 실행/캐시 hit/기각 원인
  search_state.json             # 완료된 탐색 단계와 재시작 정보
  report.json                   # 최종 판정과 변경안
```

JSON/JSONL은 UTF-8, 키 정렬, 공백 없는 표준 직렬화, NaN/Infinity 금지다. manifest는 자신을 해시 목록에 포함하지 않는다. `-0.0`은 `0.0`으로 정규화하고 키는 안정 ID로 정렬한다. 문자열 데이터는 원문 보존하며 다른 유니코드 표기를 임의로 합치지 않는다. 메타데이터 시각·attempt ID는 계산 추적의 재현 해시에서 제외한다. 파일 경로를 직접 모델 입력으로 믿지 않고 파일 내용을 확인한다.

입력은 실행 전 저장하고, 결과 manifest는 임시 파일을 쓴 뒤 같은 파일시스템에서 원자적으로 교체한다. 완료 manifest가 없는 시도는 incomplete이다. 하나의 request에는 단일 쓰기 잠금을 두고 다른 프로세스는 `REQUEST_BUSY`로 종료한다. 다른 요청은 다른 디렉터리를 사용한다. 최종 파일을 덮어쓰거나 로그를 자동 삭제하지 않는다.

### 캐시와 과거 실험 재사용

`experiment_key`는 모델/어댑터/규칙/계수/KPI/환경, 초기 상태, 시간 격자, 실제 제어 경로, 외생 경로, 난수 규약/seed의 내용 해시를 전부 포함한다. 목표값과 변경량 함수는 정방향 함수의 입력이 아니므로 캐시 키에서 제외하고, 캐시 결과를 새 목표로 다시 채점한다.

해시와 파일 재독이 통과한 complete 시도만 캐시에 넣는다. 과거 실험의 다른 날씨·초기 상태·seed에서 좋은 제어는 시작 후보로만 재사용하며 현재 조건으로 다시 실행한다. 같은 y를 낸 기록들이 서로 다른 u여도 지우지 않는다. 명시적 기록이 없는 모델의 내부 난수를 캐시 가능한 결정 함수로 간주하지 않는다.

## 6. 난수와 시나리오의 고정

한 seed에서 전역 RNG를 순서대로 소비하면 제어 변화로 분기가 달라질 때 다른 개체의 난수까지 바뀔 수 있다. 규칙은 다음 키에서만 난수를 얻는다.

```text
(scenario_seed, mechanism_id, stable_entity_id, step, event_slot, draw_index)
```

각 키는 canonical JSON의 SHA-256으로 해시한다. 앞 8바이트를 big-endian 정수로 읽고 상위 53비트를 `2^53`으로 나눈 `[0,1)` 값을 제공한다. draw_index는 호출 순서가 아니라 해당 규칙의 명시적 draw 번호이며 키에 제어값·candidate ID·attempt ID를 넣지 않는다. 범주 추첨 등의 변환은 어댑터의 버전이 있는 규칙이 담당한다. 한 계산에서 같은 키를 다시 요청하면 같은 값을 준다.

이 방식은 비교 실험의 난수를 맞추기 위한 규약이다. 어떤 ε가 현실의 동일한 개인에게 대응하는지는 모델 가정이다. 여러 seed를 쓰려면 각각을 시나리오로 미리 열거한다. 탐색기가 유리한 seed를 골라내지 않는다.

## 7. 역산 알고리즘

### 7.1 준비와 보수적 그래프 분석

1. 입력·모델·제어 역할·시간·단위·목표·예산을 검사한다. 모델이나 목표를 검증하는 동안 변경을 허용하지 않는다.
2. 선택한 KPI뿐 아니라 **모든 하드 제약**에 도달하는 실행 의존 관계를 역방향으로 조회한다. 읽기/쓰기 목록이 완전하다고 어댑터가 선언하지 못하면 가지치기하지 않는다.
3. 의존 경로가 없는 제어도 기준값이 해당 제어의 제약을 만족할 때만 기준값에 고정할 수 있다. 공유 자원·집계·다른 변수와의 제약에 영향을 주면 유지한다. 수치 민감도가 0이라는 이유로 제어를 삭제하지 않는다. `y=u1*u2`는 원점에서 두 미분이 0이어도 함께 바꾸면 영향이 있다.
4. 제어를 요청이 정한 시간 블록으로 펼친다. `mutable_blocks`에 있는 블록만 탐색하고 나머지 시간은 기준 경로를 그대로 복사한다. 엔진이 블록을 거칠게 줄이거나 일별 제어를 상수로 바꾸지 않는다.
5. 기준 제어가 현재 U에 들어가는지 먼저 확인한다. 모든 가변 블록의 값이 블록 안에서 일정하고 domain을 만족해야 한다. 이때만 정상 후보로 모든 탐색 시나리오에서 평가하고, 목표/하드 제약을 통과해 D=0이면 검증으로 간다. U 밖의 baseline은 거리 기준과 참고 실험으로 보존할 수 있지만 최적화의 feasible 후보가 아니다.

### 7.2 기존 기록과 초기 후보

같은 KPI 정의의 과거 기록에서 목표 잔차가 작은 제어부터 최대 `warm_start_count`개를 고른다. 역할/형식/현재 변수 범위가 맞는 후보만 사용하며 현재 시나리오에서 평가한다. 이미 complete한 같은 정방향 실험은 캐시를 쓴다. 목표 잔차 동점은 D, canonical 제어키 순으로 고른다. 기록이 없어도 초기 탐색을 진행한다.

### 7.3 유한 전수 탐색 분기

모든 자유 제어 좌표가 유한 집합이고, 후보 수와 시나리오 평가 수가 남은 탐색 예산 안에 들어가면 모든 조합을 canonical 순서로 평가한다. hard-feasible 중 V=0인 후보의 D가 가장 작은 것을 고른다. 평가 오류 없이 전부 평가한 경우에만 `finite_global` 또는 `proven_infeasible`을 표시한다. 정수 집합을 임의의 연속 변수로 풀었다가 반올림한 결과는 전수 증명이 아니다.

### 7.4 연속·큰 혼합 공간의 2단계 탐색

v1의 수치 탐색기는 현재 환경의 `scipy.optimize.differential_evolution` 1.17.1이다. 미분을 요구하지 않고 정수 제약을 제공한다. 이 선택은 불연속·정수 입력을 포함할 수 있는 어댑터의 탐색용이며 전역 최적 보장은 아니다. [SciPy differential_evolution 문서](https://docs.scipy.org/doc/scipy/reference/generated/scipy.optimize.differential_evolution.html).

**A. 목표 달성 후보 탐색**

- 하드 제약을 `NonlinearConstraint(residuals, -inf, 0)`로 주고 `V(u)`를 최소화한다.
- 모든 평가를 archive에 넣는다. 비용 D는 archive의 동률 비교에만 쓴다. 임의의 가중합 `V+λD`를 쓰지 않는다.
- baseline/기록 후보에 이미 V=0이 있으면 이 단계를 생략한다. 없으면 첫 feasible을 발견한 세대 끝에서 중단하고 B로 간다. 예산이 끝나면 가까운 후보와 미달량을 반환한다.

**B. 달성 조건을 지키면서 변경량 축소**

- 목적함수는 D다. 하드 제약에 각 시나리오·KPI의 양쪽 목표 경계 `(y−y*−δ)/scale≤0`, `(y*−y−δ)/scale≤0`를 추가한다.
- A에서 얻은 feasible을 초기 population에 넣고, 제약을 만족하는 새 후보만 최종 archive 순위에 반영한다.
- 탐색기 자체의 success 플래그 대신 직접 계산한 목표/제약으로 통과를 판정한다. B가 개선하지 못해도 A의 검증 가능한 후보를 잃지 않는다.
- 반환 표현은 **찾은 후보 중 최소 변경량**이다. 전역 최소라고 표시하려면 전수 확인이나 D=0 같은 별도 근거가 있어야 한다.

### 7.5 인코딩과 고정된 탐색 설정

- 연속 좌표: 유한한 `[lo,hi]`를 `[0,1]`로 선형 사상. lo=hi이면 고정 좌표로 제외한다.
- 정수/격자 좌표: `u=origin+step*k`, `k`의 허용 정수 범위를 사용한다. 범위 안에 점이 없으면 입력 오류다. 정확한 코드 표현을 위해 `|k|≤2^53−1`을 요구한다.
- 범주 좌표: 요청에 정한 선택지 순서의 정수 index를 사용하고 `integrality=True`를 지정한다. index 차이는 의미 거리로 쓰지 않으며 D는 전환 비용 표로 계산한다.
- 설정: `strategy='best1bin'`, `mutation=(0.5,1.0)`, `recombination=0.7`, `tol=1e-6`, `atol=0`, `polish=False`, `updating='deferred'`, `workers=1`, `vectorized=False`. 수치 수렴 설정은 KPI 허용오차를 바꾸지 않는다.
- population 크기 `P=max(5,10*d)`; 입력 후보를 앞에 넣고 나머지는 고정 seed의 `scipy.stats.qmc.LatinHypercube`로 채운다. 이산 좌표는 허용 index로 반올림하고 중복은 허용한다. d=0은 한 후보 전수 분기로 간다.
- A와 B 각각 최대 3회 restart. 각 restart는 시작 시 남은 단계 예산을 남은 restart 수로 정수 나눠 한도를 받고, 마지막이 나머지를 쓴다. A의 단계 예산은 탐색 시작 시 남은 전체 탐색 예산, B의 단계 예산은 A가 끝나고 남은 예산이다. A가 끝나지 않으면 B는 시작하지 않는다.
- restart seed는 `(search_seed, phase, restart_index)`의 SHA-256에서 파생해 `numpy.random.PCG64`에 넣는다. 시뮬레이션 noise seed와 별개다.
- `maxiter`는 restart의 시뮬레이션 호출 상한만큼 설정하고, 실제 한도는 공통 evaluator가 강제한다. 캐시 반복으로 호출 한도에 닿지 않더라도 maxiter와 wall-time이 종료를 보장한다.
- `warm_start_count=8`은 검색 시작점 개수의 내부 상수다. 알고리즘 설정과 함께 manifest에 기록한다. 범용 계약이나 사용자 목표를 바꾸지 않는 내부 탐색 선택이며 변경 시 알고리즘 버전을 올린다.

### 7.6 호출 예산과 종료

`max_simulations`, `max_wall_seconds`, `max_trace_bytes`는 필수 양수다. 실제 scenario×candidate 실행 1회를 1 simulation으로 센다. baseline·기록 후보 재평가·오류·검증도 포함하고 캐시 hit는 포함하지 않는다. 평가를 시작하기 전에 남은 예산을 확인한다.

검증용으로 `|S_search|+|S_validation|`회분을 사전에 남긴다. 나머지가 탐색 예산이다. 필요한 최소 평가 수보다 예산이 작으면 `INVALID_REQUEST`로 요청한다. 이미 캐시로 탐색 가능한 경우라도 실제 검증 예산은 확보한다. 상태 크기·실행 속도의 사전 성능 보장은 없으며 한 번의 호출에는 남은 wall-time을 deadline으로 전달한다. 시나리오 실행은 단일 자식 프로세스에서 수행하고 deadline에 종료해 멈추지 않는 어댑터를 통제한다. 쓰다 만 시도는 incomplete로 남는다. 최종 보고서를 기록할 64 KiB를 trace 예산 외의 제어 메타데이터 여유분으로 예약한다.

일부 시나리오만 평가한 후보는 `partial`로 기록한다. 모든 탐색 시나리오를 평가하기 전에는 V·D 순위의 feasible 후보가 될 수 없다. 치명적인 모델 변경·기록 손상은 즉시 중단한다. 후보 하나의 규칙 실패는 기록 후 다른 후보 탐색을 계속할 수 있다. 동일 오류로 모든 후보가 실패하면 `evaluation_error` 종료 사유와 오류 빈도를 반환한다.

재시작은 같은 요청/모델 해시와 기록 무결성을 먼저 검사한다. 이전 시도의 호출 수·저장 크기·실행에 사용한 wall-time을 합산하고 원래 총 예산에서 차감한다. 중단 기간은 wall-time 사용량이 아니며, 계측되지 않은 강제 종료 구간은 저장한 시작/종료 시각의 차이를 보수적으로 차감한다. 중단된 DE population 자체를 완전히 복원한다고 주장하지 않는다. 마지막 완료 단계·유효 archive에서 다음 restart를 시작하며 이전 시도는 보존한다. 요청이 다르면 새 디렉터리에서 새 탐색이다.

## 8. 원본 재실행과 최종 판정

1. archive의 최선 feasible 하나를 선택하고 cache read를 비활성화한 채 모든 탐색 시나리오에서 다시 실행한다. 후보·모델·입력·noise가 고정된 재실행이다.
2. KPI·하드 제약·정규화 계산 추적 해시를 비교한다. 이 단계가 다르면 `validation_failed`다. 메타데이터 시각이나 attempt ID의 차이는 계산 차이가 아니다.
3. `S_validation`이 있으면 동일 제어를 그 별도 조건에서도 평가한다. 여기에도 목표/하드 제약을 적용하고 결과는 별도로 보관한다. 추가 조건에서 실패하면 `validation_failed`; 그 실패를 보고 자동으로 같은 검증 집합에 반복 최적화하지 않는다. 조건을 탐색에 포함시키려면 새 요청 해시를 만든다.
4. 모든 선언 조건을 통과한 경우에만 `verified_feasible`을 반환한다. `S_validation=[]`이면 “지정 탐색 조건에서 재생 검증됨”으로 표시한다. 별도 조건 검증을 수행한 것처럼 표현하지 않는다.

유한한 시나리오 집합에서의 통과는 일반적인 성공확률 95% 등의 주장이 아니다. 확률 조건이 필요해지면 분포·독립 표본·유의수준·검증의 재사용 정책을 별도로 설계해야 한다. 모델이 실제 동역학을 충분히 설명하는지는 별도 검증 문제다. [Karimi et al., NeurIPS 2020](https://proceedings.neurips.cc/paper_files/paper/2020/hash/02a3c7fb3f489288ae6942498498db20-Abstract.html).

| 최종 status | 의미 |
|---|---|
| `verified_feasible` | 원본 재실행과 요청에 선언한 검증 조건을 통과 |
| `no_feasible_found` | 예산 내 달성 해를 찾지 못함; 불가능 증명 아님 |
| `proven_infeasible` | 선언된 유한 공간을 오류 없이 전수 확인했고 달성 해 없음 |
| `validation_failed` | 탐색에서는 달성했지만 재실행/별도 검증에서 실패 |
| `validation_incomplete` | 후보는 있으나 검증 시간·저장·호출 예산이 부족 |
| `invalid_request` | 목표·범위·단위·예산 등의 입력 계약 위반 |
| `model_not_ready` | 온톨로지 매핑·전이 규칙·시간 계약 등이 없음 |

별도 `optimality`는 `zero_global`(D=0), `finite_global`(열거한 유한 공간), `best_found`(휴리스틱), `none`이다. 실패한 후보에 global 라벨을 붙이지 않는다. `proven_infeasible`에는 proof의 domain·scenario·모델 해시·후보 수를 첨부한다. validation에서만 발견한 실패는 검색 공간 불가능 증명이 아니다.

## 9. 보고서와 설명

```text
request/model/target_source 참조
status + termination_reason + optimality
변수별: 제어 ID, 적용 시간, 기준값, 제안값, 차이, 단위, 척도, 가중치
목표별/시나리오별: 정의, 목표, 허용 범위, 실제값, 잔차, 통과 여부
하드 제약별: 실제값, 한계, 잔차, 통과 여부
변경량 D, 탐색/검증 호출 수, 시간/저장 사용량
상위 후보 참조(기본 최대 5개), 후보별 V·D와 실현 가능 여부
실제 추적의 규칙/상태 경로와 원본/가정 출처
추가 시나리오 검증 범위 및 실패 사유
누락된 실제 어댑터·현실 보정 등의 적용 한계
```

경로 설명은 `제어 변경→규칙 실행→중간 상태 변화→KPI`의 실제 이벤트를 가리킨다. 서로 다른 경로의 계수를 곱하거나 합해 자동으로 “실제 원인 기여율”을 만들지 않는다. 반사실적 기여를 별도로 계산하려면 제어를 되돌린 비교 실행과 그 정의가 필요하고, 상호작용에서는 개별 효과의 합이 전체 효과와 다를 수 있다. v1은 실행 경로와 차이를 보고한다.

## 10. 전체 의사코드

```text
inverse(model, request, journal):
    validate_model_and_request()                      # 실패면 명시적 status
    freeze_and_hash_all_inputs()
    reserve_validation_budget()
    compile_action_space_and_conservative_dependencies()
    archive = compatible_completed_evaluations()
    evaluate_baseline_and_warm_starts_under_current_scenarios()

    if archive has feasible with D == 0:
        optimality = zero_global
    elif whole_finite_space_fits_search_budget():
        enumerate_all_controls_without_skipping_errors()
        if enumeration completed without errors and no feasible:
            return proven_infeasible with certificate
        if enumeration completed without errors:
            optimality = finite_global
    else:
        if archive has no feasible:
            run_phase_A_minimize_V_subject_to_hard_constraints()
        if archive has feasible and search_budget_remains():
            run_phase_B_minimize_D_subject_to_hard_and_target_constraints()
        optimality = best_found

    if archive has no feasible:
        return no_feasible_found with nearest_hard_feasible_and_deficits
    candidate = archive.minimum_feasible_by_D_then_key()
    verification = rerun_without_cache(candidate, S_search)
    if verification differs or violates constraints:
        return validation_failed
    if verification_budget_exhausted():
        return validation_incomplete
    validation = run_without_cache(candidate, S_validation)
    return report_using_actual_validation_status_and_evidence()
```

## 11. 계산을 확인할 수 있는 작은 예시

아래는 명동 실측/추정 모델이 아닌 **알고리즘 확인용 가상 대기열**이다. 기준 상태 `q₀=20`, 외생 유입 `a`, 제어인 우회율 `r`과 창구 수 `k`, 창구 처리량 4를 둔다.

```text
q_next = max(0, 20 + a·(1−r) − 4k)
기준: a=100, r⁰=0.20, k⁰=10 → q_next=60
충격: a=120, r⁰=0.20, k⁰=10 → q_next=76
고정 목표: 60±1
허용 제어: r∈[0.20,0.40], k∈{10,11,12,13,14}
거리: D = |r−0.20|/0.10 + |k−10|       (l1, 동일 가중치)
```

각 정수 k에서 목표 상한 61을 맞추는 최소 r을 계산하면 다음과 같다.

| k | 최소 r | 결과 | D |
|---|---|---|---|
| 10 | 0.325 | 61 | 1.25 |
| 11 | 0.291666… | 61 | 1.916666… |
| 12 | 0.258333… | 61 | 2.583333… |
| 13 | 0.225 | 61 | 3.25 |
| 14 | 0.20 | 60 | 4.00 |

따라서 이 가상 모델/척도에서는 `r=0.325, k=10`이 최소 변경 해다. 결과가 중심 60이 아니라 허용 범위의 경계 61인 이유는, 목표를 만족한 뒤 변경량을 최소화하기 때문이다. 목표를 정확히 60으로 고정하면 같은 k에서 r=1/3이 된다. 최대 우회율을 0.30, 최대 k를 10으로 제한하면 최소 결과는 64이므로 이 예시의 목표는 도달 불가능하다. 일반 블랙박스 탐색기가 이를 증명할 수 있다는 뜻은 아니다.

## 12. 명동 어댑터의 연결 조건

확인된 프로젝트 자산은 페르소나 원본·명동 관련 부분집합·활동 엔진 설계·지면 고도다. 작업 도중 온톨로지 v0.2를 확인한 뒤 [v0.3 초안](01a_persona_ontology.md)(status=draft, Phase 1~3)의 §7.4와 변경 이력을 다시 확인했다. 아래는 [DR-108](decisions.md#dr-108--온톨로지-v03에-따른-연결-계약-갱신)의 v0.3 대응이다. 최종 스키마·수치·실행 코드는 미확정이므로, C-04~C-08 또는 온톨로지 초안을 이미 실행 가능한 f로 간주하지 않는다.

| 온톨로지 초안 | 역산 계약 | 연결 시 검사 |
|---|---|---|
| R-03 환경 속성의 role:u/w·형식·단위·유효 범위 | ControlSpec의 제어 후보·형식과 범위 검사 | role:u와 요청 whitelist의 교집합만 탐색; 허용 domain·거리 척도·가중치·시간 블록은 요청에 명시 |
| R-01 공통 차원·비용 곡선과 R-02 능력/민감도 | 모델 θ·초기 프로필의 스냅샷 | 역산 중 기준점·프로필·계수를 다시 맞추지 않음 |
| R-04 상황·대안 | 규칙의 읽기 값 및 derived | 노출·가능 대안·경성 한계의 입력과 제거 원인 기록 |
| R-05 react | rule 평가와 대안별 확률·선택 이벤트 | 확률 합·선택·근거·안정 난수 키와 임계점 처리 |
| R-06 상태/시간 + simulate | NativeRunner 또는 동일 규칙의 step 실행 | 시간별 state/derived 추적을 동일한 TraceEvent로 변환 |
| R-07 저장 버전·인물/상황 ID | model_hash·source_ref·trace event ID | 실제 실행 ID가 출처 인물/합성 인물 ID를 혼동하지 않음 |
| R-09~R-11 방문·혼잡·상점 반응 | 등록된 개인/집단/월별 전이 규칙 | 선택한 모델 버전에 있는 규칙만 실행; 미확정 공급 수치를 역산기가 생성하지 않음 |
| R-12의 Y-01~Y-19 | MetricSpec 후보 목록 | 복합 지표는 단위·집계별 scalar metric ID로 분리하고 요청이 고른 KPI만 목표화 |

초안의 `simulate(...)` 하나를 호출하는 형태는 `NativeRunner`로 감싼다. 반환하는 반응 기록 또는 observer에서 단계별 입력·출력·분기·상태를 얻을 수 있어야 한다. 추적 계약을 충족하지 못하면 `MODEL_NOT_READY`로 두며 그 내부 전이를 추측하지 않는다. 본 엔진의 rule 실행기는 합성 검증 모델과 규칙별 어댑터를 위한 기본 실행 경로다.

v0.2의 “확률의 기댓값 KPI는 seed와 무관”은 **주어진 개인 상태·상황의 한 번의 반응**에 한정되며, v0.3 S-R20도 다단계 지표를 공통 난수의 반복 평균과 표준오차로 정정했다. 선택된 행동이 다음 피로/위치/혼잡을 바꾸므로 조건부 확률을 매 시점 합하는 것만으로 전체 KPI의 기댓값을 정확히 계산할 수는 없다. 반복 평균을 목표로 고르면 metric/config 버전에 반복 수·전체 seed 규약·평균/표준오차 계산·개별 실행 참조를 명시해야 한다. 원본 목표가 단일 궤적 KPI였다면 새 metric 버전의 평균으로 조용히 바꾸지 않는다. 유한 반복 평균의 허용오차 통과는 모평균이나 성공확률의 보장이 아니다.

범용 엔진의 `max_simulations`는 시나리오×후보 단위의 **F_s 평가 횟수**다. NativeRunner가 한 평가 안에서 여러 궤적/일별/월별 계산을 수행하면 그 내부 실행 수·비용·참조를 추가 기록하며 C_F에 포함한다. 이 경우에도 전체 wall-time/trace budget은 그대로 적용된다. 반복 평균 어댑터는 이러한 기록과 안정 난수 계약이 확인된 뒤에만 연결할 수 있다.

개인 수준의 단조 반응은 집단·장기 KPI의 단조성 보장이 아니다. 따라서 한 변수씩 이분법으로 푸는 방식을 일반 역산기로 채택하지 않는다. 원래 시뮬레이션으로 조합을 재평가한다. ‘방문 수요’는 현재 요청에서 조절 장치가 정의되지 않았으므로 외생 시나리오로 남긴다. 벤치·경사 등의 표기 또한 실제 제어 가능 범위·시간·제약이 제공된 경우에만 제어 후보가 된다.

실제 연결 전에는 다음 계약 자료가 필요하다. 이는 새 요구사항 추측이 아닌 실행 모델의 필수 입력이다.

- 노드/속성/관계의 안정 ID와 타입, 상태 스냅샷을 읽고 쓰는 어댑터.
- 관계별 실행 규칙, 조절 가능한 입력 포트, 고정된 계수와 근거.
- 시간 간격, 시작 상태, 경계 밖 이동과 개체 생성/소멸의 처리.
- KPI 함수와 집계 범위, 제약, 실제 제어 범위·척도.
- 입력·출력·근거를 기록하는 step trace와 안정 난수 키 지원.

활동 엔진의 10분 격자·구 귀속 규약을 연결할 경우 해당 버전을 명시한다. 기존 약 30m 지면 고도에서 보행로·창구·건물·수용 인원을 자동 추정하지 않는다. 외부 충격을 매개변수로 받는 것과 외부 실측 자료를 새로 수집하는 것은 다른 작업이다. 이 설계는 기존 데이터 사용 제한을 변경하거나 수집을 시작하지 않는다.

## 13. 검증 범위와 계산량

모델 평가비를 `C_F`, 시나리오 수를 `S`, 실제 평가 후보 수를 `N`이라 하면 주비용은 대략 `N·S·C_F`다. trace는 실행한 규칙 이벤트 수에 비례하고 스트리밍한다. 모든 과거 상태를 RAM에 동시에 보관하지 않는다. 정수/범주 전수 공간은 각 좌표 cardinality의 곱이므로, 호출 예산 안에 들어갈 때만 전수 분기를 선택한다.

의미 있는 검증은 해석적 최소 변경 해, 금지 입력 불변성, 기록 재생, 임계값·상호작용, 유한 무해 증명, 연속 탐색 미발견 구분, 원본 재실행/추가 시나리오 실패다. 문서의 필드를 그대로 복제하는 테스트는 만들지 않는다. 실제 규모/정확도는 구현과 어댑터가 생긴 후 측정한다.

## 14. 구현에 쓰는 공통 타입과 직렬화 계약

다음 타입의 정본은 구현 시 `inverse_engine/contracts.py`다. 모든 필드는 명시적으로 전달한다. `| None`만 null 가능하며, 나머지 nullable은 허용하지 않는다. tuple은 JSON array, Mapping은 JSON object, enum은 아래 문자열을 쓴다. 기본값은 두지 않고 허용된 빈 tuple/dict도 호출자가 전달한다. 모든 숫자는 유한해야 하며 bool을 int/float로 받지 않는다. type 정의는 문서 명세이고 이번 작업에서 소스 파일을 생성하지 않는다.

```text
Scalar = bool | int | float | str
Values = Mapping[str, Scalar]
State = Values
Sha256 = str                             # 소문자 hex 64자리
SnapshotRef = {sha256: Sha256, path: str, schema_id: str}
SourceRef = {id: str, locator: str, evidence: measured|calibrated|assumed}

VariableSpec = {
  id: str, role: state|derived|control|exogenous|parameter,
  dtype: bool|int|float|str, unit: str, choices: tuple[Scalar,...],
  source_refs: tuple[SourceRef,...]
}
# id는 비어 있지 않고 유일. 무차원도 unit="1". choices는 제한 없으면 [].
# control/exogenous에는 별도 경로가, parameter에는 parameters 값이 있어야 한다.

MechanismSpec = {
  id: str, version: str, code_hash: Sha256,
  reads: tuple[str,...], writes: tuple[str,...],
  source_refs: tuple[SourceRef,...]
}
# 읽는 state는 x_t, 읽는 derived는 같은 단계에서 앞서 생성된 값이다.
# 쓰는 state는 x_(t+1), derived는 같은 단계의 값이다. 역할 혼합 쓰기는 거절.
# state/derived별 writer는 정확히 1개이며 바뀌지 않는 state도 명시적 hold 규칙을 둔다.
# control/exogenous/parameter는 입력이므로 writer가 없어야 한다.

ModelManifest = {
  schema_version: "inverse-model/1", model_id: str,
  ontology: SnapshotRef, adapter_code: SnapshotRef,
  variables: tuple[VariableSpec,...], mechanisms: tuple[MechanismSpec,...],
  parameters: Values, dependency_complete: bool, execution_mode: rules|native_trace,
  environment: Mapping[str,str], model_hash: Sha256
}
# model_hash 자신을 제외한 manifest와 참조 파일 내용 해시로 계산.
# environment에는 Python/numpy/scipy/platform/float_dtype(="float64") 포함.

TimeGrid = {start_iso: str, timezone: str, step_seconds: int, steps: int}
# timezone이 있는 start_iso와 timezone의 offset 일치, step_seconds>0, steps>0.
PathValues = tuple[Values,...]           # 길이 H, 각 시각에 선언 ID를 정확히 포함
Scenario = {
  id: str, initial_state: SnapshotRef, exogenous_path: SnapshotRef,
  noise_seed: int, source_refs: tuple[SourceRef,...]
}
# noise_seed는 0≤seed<2^64. initial_state의 키는 state ID와 정확히 일치.

ControlDomain = {
  kind: continuous|lattice|categorical,
  lo: float|None, hi: float|None, origin: float|None, step: float|None,
  choices: tuple[Scalar,...]
}
# continuous: lo≤hi, origin=step=None, choices=[].
# lattice: lo≤hi, step>0, origin 유한, choices=[]. int 변수면 세 값 모두 정수.
# categorical: lo=hi=origin=step=None, choices 비어 있지 않고 중복 없음.
ControlSpec = {
  variable_id: str, blocks: tuple[tuple[int,int],...], mutable_blocks: tuple[int,...],
  domain: ControlDomain,
  distance_scale: float|None, weight: float,
  transition_costs: tuple[tuple[float,...],...]
}
# blocks는 [start_step,end_step)로 [0,H)를 틈/겹침 없이 덮는다.
# mutable_blocks는 변경 가능한 block index의 정렬·유일한 부분집합, [] 허용.
# 그 블록만 domain 안의 상수값을 탐색한다. 나머지는 baseline 경로를 그대로 고정한다.
# 변경 불가 블록의 baseline은 블록 안에서 달라도 된다. 물리 제약은 고정 구간에도 적용한다.
# numeric: distance_scale>0, transition_costs=[]. categorical: scale=None,
# choices 순서의 정방 비용 행렬, 대각선0/다른항목>0. weight>0.
# baseline은 거리의 기준이므로 현재 허용 범위 밖일 수도 있으나 타입/시간은 유효해야 한다.

MetricSpec = {
  id: str, version: str, code_hash: Sha256, unit: str,
  state_steps: tuple[int,...], reads: tuple[str,...], config: SnapshotRef
}
# state_steps는 [0,H]의 비어 있지 않은 정렬·유일한 관측 시점.
# 코드가 제어/외생의 마지막 구간 밖을 읽으면 오류. 집계/모집단/가중치는 config에 명시.
TargetSpec = {
  metric: MetricSpec, value: float, abs_tol: float, rel_tol: float,
  residual_scale: float, source_run_id: str|None
}
# tol≥0, residual_scale>0. source_run_id가 있으면 KPI 계약·값·해시 일치 검사.

ConstraintSpec = {
  id: str, version: str, code_hash: Sha256, stage: input|trajectory,
  reads: tuple[str,...], config: SnapshotRef,
  limit: float, tolerance: float, scale: float, unit: str
}
# g=(adapter가 반환하는 원시 값-limit-tolerance)/scale≤0. scale>0,tolerance≥0.
# 등식은 양쪽 부등식 2개로 명시. input은 trace 없이 u,w에서 계산 가능해야 함.

BudgetSpec = {max_simulations: int, max_wall_seconds: float, max_trace_bytes: int}
InverseRequest = {
  schema_version: "inverse-request/1", model_hash: Sha256, grid: TimeGrid,
  baseline_controls: SnapshotRef, controls: tuple[ControlSpec,...],
  targets: tuple[TargetSpec,...], constraints: tuple[ConstraintSpec,...],
  search_scenarios: tuple[Scenario,...], validation_scenarios: tuple[Scenario,...],
  distance_norm: l1|squared_l2, search_seed: int, budget: BudgetSpec
}
# controls는 [] 허용(고정 문제). targets/search_scenarios는 빈 값 거절.
# validation_scenarios=[]는 추가 검증 없음이라는 명시적 선택.
# 동일 시나리오 내용 해시를 search와 validation 양쪽에 두면 입력 오류.
# 같은 u를 모든 시나리오에 적용하며 시나리오 ID별로 다른 제어를 만들지 않는다.

RuleContext = {step: int, mechanism_id: str, noise: NoiseSource}
RuleResult = {outputs: Values, branch: str, diagnostics: Mapping[str,Scalar]}
TraceEvent = {
  event_id: str, step: int, mechanism_id: str,
  inputs: Values, input_event_refs: Mapping[str,str],
  before: Values, after: Values, branch: str,
  source_refs: tuple[str,...], diagnostics: Mapping[str,Scalar]
}
# mechanism_id는 인물/상황 등에 바인딩된 실행 인스턴스 ID다. 같은 react 코드도
# synthetic_id가 다르면 별도 mechanism_id를 갖고 code_hash는 공유할 수 있다.
# event_id는 (step, mechanism_id)의 canonical 식별자. state 초기값/제어/외생/계수는
# 해당 스냅샷+step+변수ID로 input_event_refs를 만든다. before는 이전 값이 있을 때만 포함.
# trace 순서는 step, 컴파일한 위상 순서이며 동점은 mechanism_id 오름차순.

RunResult = {
  experiment_key: Sha256, attempt_id: str, status: complete|failed|incomplete,
  metrics: Mapping[str,float], trace_hash: Sha256|None,
  artifact_dir: str, error_code: str|None, error_message: str|None
}
# complete만 모든 metrics와 trace_hash를 갖는다. 오류 시 부분 metric은 결과로 쓰지 않는다.
Evaluation = {
  control_key: Sha256, controls: SnapshotRef,
  run_refs: tuple[str,...], complete: bool,
  hard_feasible: bool|None, target_feasible: bool|None,
  target_violations: tuple[float,...], hard_residuals: tuple[float,...],
  V: float|None, D: float,
  error_code: str|None, provenance: baseline|warm_start|enumeration|phase_a|phase_b
}
# 실패/partial은 feasible=None, V=None. 내부 SciPy callback의 +inf sentinel은 저장하지 않는다.
# hard_feasible=False인 정상 평가의 V는 계산해도 nearest-feasible 순위에는 넣지 않는다.

InverseReport = {
  schema_version: "inverse-report/1", request_hash: Sha256, model_hash: Sha256,
  status: [8절 enum], termination_reason: str,
  optimality: zero_global|finite_global|best_found|none,
  selected: Evaluation|None, alternatives: tuple[Evaluation,...],
  validation_run_refs: tuple[str,...], certificate: SnapshotRef|None,
  details: SnapshotRef, simulations_used: int, wall_seconds: float, trace_bytes: int
}
# details에는 9절의 변수/지표/제약/설명 테이블을 저장한다. top-level report는 64 KiB 이하.
# alternatives는 최대5개, 큰 평가 객체는 run_refs에 대한 내용 주소 참조로 직렬화한다.
```

`ModelAdapter`는 이미 등록된 신뢰 가능한 로컬 함수만 호출한다. 온톨로지/설정의 문자열을 `eval`하거나 가져온 임의 코드를 실행하지 않는다. runtime parameter나 source locator가 함수 이름과 일치한다는 이유만으로 코드를 로드하지 않는다.

```text
ModelAdapter.manifest() -> ModelManifest
ModelAdapter.native_runner() -> NativeRunner|None
ModelAdapter.evaluate_rule(rule_id: str, inputs: Values, context: RuleContext) -> RuleResult
ModelAdapter.metric(spec: MetricSpec, trace: TraceReader, controls: PathValues,
                    exogenous: PathValues) -> float
ModelAdapter.constraint(spec: ConstraintSpec, controls: PathValues,
                        exogenous: PathValues, trace: TraceReader|None) -> float
NoiseSource.uniform(entity_id: str, event_slot: str, draw_index: int) -> float
TraceReader.state_at(step: int) -> State
TraceReader.events(start_step: int, end_step: int) -> Iterator[TraceEvent]
NativeRunner(initial: State, controls: PathValues, exogenous: PathValues,
             scenario: Scenario, grid: TimeGrid, emit: Callable[[TraceEvent],None]) -> None
```

execution_mode=rules면 native_runner=None이고 core가 rule 순서를 실행한다. native_trace면 등록된 NativeRunner가 전체 simulate를 실행해 같은 trace를 내보내고, core는 ID/타입/시간/동시 갱신/완전한 기록과 KPI를 검증한다. 두 경로 모두 같은 난수 키/내용 해시/재생 계약을 지킨다. native의 읽기 목록이 완전하지 않으면 dependency_complete=False로 선언해 가지치기하지 않는다.

`state_at(k)`는 초기 x0에 **step<k**인 event의 state write만 반영한다. step=k의 write는 x_(k+1)에 속한다. derived는 step=k의 규칙 trace에서 읽고 다음 step으로 누적하지 않는다. native 반응의 대안별 확률·효용·제거 원인은 `diagnostics`에 대안의 안정 ID를 포함한 scalar key로 펼쳐 기록할 수 있다.

규칙은 `reads`에 선언된 값만 입력으로 받는다. engine이 규칙 순서를 정하고 반환된 `outputs`가 `writes`와 정확히 맞는지 검사한다. 규칙이 기록되지 않은 전역 상태·외부 IO를 사용하는 어댑터는 지원 계약에 맞지 않는다. 최종 replay가 그 위반을 전부 증명할 수 있다는 뜻은 아니며 어댑터 코드 검토도 필요하다.

## 15. 이번 설계의 확인 기록

2026-10-31, 구현 코드를 만들지 않고 다음 항목을 확인했다.

- 가상 대기열 예시를 Python 표준 `fractions.Fraction`의 정확한 유리수 연산으로 검산했다. 기준60/충격76, k=10~14의 각 최소 변경 후보, 최소 D=5/4, r=13/40, 결과61, exact-target r=1/3과 제한된 범위의 최소 결과64가 모두 일치했다.
- 동일 제어 경로를 시간 블록으로 다시 나누어도 시간 가중 변경량이 같음을 확인했다.
- 개념 완료 기준 E100-1~E105-5의 31개 항목 모두에 구현 테스트 명세가 대응하며, 신규 구현 todo 8개는 모두 미착수로 남아 있음을 확인했다.
- 새 설계 절의 문서 링크/앵커, 코드 fence, 결정 ID 중복을 점검했다. 명동 온톨로지 원문은 수정하지 않고 확인한 버전과 해시를 DR-107~108에 기록했다.
- 실제 SciPy 1.17.1의 DE/NonlinearConstraint 시그니처를 조회해 사용 예정 인자가 존재함을 확인했다. 엔진·DE 성능·명동 어댑터·실제 시뮬레이션 결과를 테스트했다고 주장하지 않는다.

## Changelog

- 2026-10-31: 설계 검토에서 변경 허용 시간 블록, U 밖 baseline의 무변경 해 배제, state_at의 시간 경계, 재시작 예산 누적을 명확히 했다. 온톨로지 v0.3 계약에 맞춰 대응을 갱신했고 수학 예시와 문서 추적을 검증했다.

- 2026-10-31: 병행 작성된 온톨로지 초안 v0.2의 §7.4를 확인해 native trace 어댑터와 대응 표를 추가했다. 다단계 기대값·집단 단조성·제어 가능성의 적용 범위를 명시했다. 초안의 미확정 의사결정은 대신 확정하지 않았다.

- 2026-10-31: DR-100~104에 따른 알고리즘·수식·입력/기록/탐색/검증 계약을 작성했다. 명동 실데이터 실행이나 엔진 구현은 수행하지 않았다.
