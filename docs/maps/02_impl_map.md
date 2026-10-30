---
version: 1.10
last_modified: 2026-10-31
status: draft
---

# 구현체 Map

범위: C-01~C-03(서울 원본 확보·검증, 구현·검증 완료)과 C-04~C-08(시간별 활동 엔진, 아래 '활동 엔진 공통 규약'부터, **구현 전**)이다. 활동 엔진의 규칙 데이터(정규식·키워드·템플릿 수치·지명 사전 시드)는 [02a_rule_data.md](02a_rule_data.md)가 정본이다.

## I-01-a — 원본 정보와 조각 획득 (C-01)

- 위치: `scripts/download_seoul_personas.py`의 `fetch_manifest(session)` 및 `download_shard(session, item, destination)`.
- 기술: Python 3.13.1, requests 2.34.2, PyArrow 24.0.0. 이미 설치된 라이브러리이며 원본 Parquet와 스트리밍 HTTP 처리를 지원한다.
- 시그니처: `fetch_manifest(session: requests.Session) -> list[dict]`; `download_shard(session: requests.Session, item: dict, destination: pathlib.Path) -> None`.
- 입력 구조: Hugging Face 데이터셋 `nvidia/Nemotron-Personas-Korea`, 리비전 `ada0f5b53a38bb5a30cce09358adde883c1ab63a`; `data/train-00000-of-00009.parquet`부터 `...00008...`까지. API `siblings`의 `rfilename`, `size`, `lfs.sha256`은 필수다.
- 로직: 리비전 API의 `blobs=true`로 목록을 받고 이름이 예상 9개와 정확히 일치하는지 검사한다. 이름순으로 다운로드하며 요청 스트림을 4 MiB씩 임시 파일에 쓰고 SHA-256과 실제 바이트 수를 공식 값과 비교한다. 성공 시 다음 처리 단계에 넘긴다.
- 오류: HTTP, 타임아웃, 크기/해시 오류는 조각별 최대 3회 재시도 후 실패한다. 메타데이터 누락, 조각 수 불일치 또는 파일 읽기 오류는 즉시 실패한다. 실패 시 예외를 표시하고 완성 파일은 만들지 않는다.
- 복구: 전송이 반복 실패하는 경우 작업 중인 임시 결과에 `/private/tmp` 하드링크를 만들어 완료된 조각의 데이터를 보존한다. 중단 또는 실패 후 복구 방법과 그 검증은 구현 전에 이 Map에 추가하며, 미검증 임시본은 최종 결과로 인계하지 않는다.
- 엣지: 빈 조각이라도 메타데이터에 없으면 실패; 조각 다운로드 중 연결이 끊기면 임시 파일을 처음부터 다시 쓴다; 이름 중복/누락은 실패.
- 자원: 원본은 한 번에 1개만 임시 보관한다. 네트워크는 전체 약 2 GB; 전송 단위 4 MiB; 연결 15초, 읽기 180초 타임아웃.
- DoD: 고정 원본 9개 각각의 크기/해시가 일치하고 원본 행 수가 1,000,000건이다.

## I-02-a — 원본 스키마 보존 및 서울 필터 (C-02)

- 위치: `scripts/download_seoul_personas.py`의 `extract(session, manifest, output) -> dict` 및 `table_digest(table: pyarrow.Table) -> str`.
- 기술: Python 3.13.1, PyArrow 24.0.0; 원본 순서와 Arrow 타입을 보존한다.
- 스키마: 열 순서는 `uuid`, `professional_persona`, `sports_persona`, `arts_persona`, `travel_persona`, `culinary_persona`, `family_persona`, `persona`, `cultural_background`, `skills_and_expertise`, `skills_and_expertise_list`, `hobbies_and_interests`, `hobbies_and_interests_list`, `career_goals_and_ambitions`, `sex`, `age`, `marital_status`, `military_status`, `family_type`, `housing_type`, `education_level`, `bachelors_field`, `occupation`, `district`, `province`, `country`. `age`는 int64, 나머지는 string. nullable 및 스키마 메타데이터는 첫 원본을 그대로 보존하며 조각 간 타입·열 순서 불일치는 실패한다.
- 로직: 각 조각을 Parquet 배치 최대 8,192행으로 순차 읽는다. `province`가 정확히 `서울`인 행만 선택하고 행 순서 그대로 Snappy Parquet 임시 파일에 쓴다. 비어 있는 선택 배치는 쓰지 않는다. 각 선택 배치를 단일 청크, 스키마 메타데이터 제거 Arrow IPC로 직렬화해 SHA-256을 보관한다. 입력 총 행 수, 선택 수와 조각별 건수를 기록한다. 조각 처리 후 임시 원본을 삭제한다.
- 오류: 파일 손상, 스키마 차이, 원본 총 행 수가 1,000,000이 아님, 선택된 서울 행이 0건이면 실패한다.
- 엣지: null `province`는 서울이 아니므로 제외; 모든 배치의 선택이 비어 있으면 실패; 행 수가 배치 경계보다 작아도 처리; 서울이 없는 조각도 허용한다.
- 자원: 배치 8,192행, 임시 원본 한 개와 결과 파일 한 개. 추가 외부 서비스나 유료 의존성 없음.
- DoD: 선택한 모든 배치가 순서대로 기록되고 결과 스키마가 원본과 같다.

## I-03-a — 재독 검증과 최종 인계 (C-03)

- 위치: `scripts/download_seoul_personas.py`의 `validate(output: pathlib.Path, extraction: dict) -> dict`, `main() -> None`; 결과 `data/nemotron-personas-korea/seoul.parquet`, `metadata.json`, `README.md`.
- 기술: Python 3.13.1, PyArrow 24.0.0, 표준 라이브러리 `hashlib`, `json`, `collections`, `pathlib`.
- 결과 구조: Parquet는 I-02-a 원본 스키마. `metadata.json`은 `dataset`, `revision`, `split`, `filter`, `source_rows`, `seoul_rows`, `columns`, `district_counts`, `source_shards`(이름·크기·SHA-256·원본/서울 행 수), `output_sha256`, `license`, `source_url`을 포함한다. 모든 필드는 필수이고 null 기본값은 없다. README에는 원본 링크, 라이선스, 합성 데이터임, 필터, 재현 버전, 행 수 및 사용 예를 기록한다.
- 로직: 임시 Parquet의 각 행 그룹을 재독해 I-02-a의 해당 배치 해시와 비교한다. 원본 행과 서울 행 총수, 스키마, 모든 `province`, UUID의 비어 있지 않음·유일함, 지역별 집계, 출력 SHA-256을 검사·기록한다. 검사 성공 후 같은 디렉터리의 임시 산출물을 `Path.hardlink_to`로 최종 이름에 연결해 기존 경로를 덮어쓰지 않는다. 메타데이터 확정에 실패하면 이번 실행에서 만든 최종 데이터 링크도 제거한다.
- 오류: 행 그룹 수/해시/스키마/지역/UUID 불일치나 출력 쓰기 오류는 실패로 보고 임시 파일을 정리한다. 원본 전체 조각은 결과 디렉터리에 보관하지 않는다.
- 엣지: null 또는 빈 UUID, 중복 UUID는 실패; null `district`가 있다면 해당 값을 변형하지 않고 별도 집계한다; 최종 출력이 존재하면 중단한다.
- 자원: 지역 집계 및 UUID 집합을 메모리에 보관; 메모리 제약 수치 없음. 기존 결과를 대체하지 않는다.
- DoD: 아래 테스트 전부 통과 후 결과와 출처 안내가 존재한다.

## 테스트 명세

별도 구현 복제 테스트 파일을 만들지 않고 실제 원본/산출물의 경계에서 실행한다.

1. `원본_고정_완전성` — Given 고정 커밋의 공식 메타데이터, When 9개 조각을 읽으면, Then 이름·크기·SHA-256이 모두 일치하고 원본 1,000,000행을 읽는다. 픽스처: 공식 원본. C-01 완료 기준 ①–③.
2. `서울_부분집합_정확성` — Given 검증된 원본 조각, When 배치별 서울 필터 후 결과를 재독하면, Then 모든 선택 배치의 순서와 값이 행 그룹 SHA-256과 일치하고 선택 행 수와 저장 행 수가 일치한다. 빈 선택 배치와 서울이 없는 조각도 누락 없이 처리한다. 픽스처: 공식 원본 및 생성 결과. C-02 ①–④, C-03 ①.
3. `스키마_지역_ID` — Given 생성 Parquet, When 모든 행 그룹을 재독하면, Then 원본과 26개 열/타입이 같고 모든 행의 `province`가 서울이며 UUID가 비어 있지 않고 유일하다. 픽스처: 공식 원본 스키마 및 생성 결과. C-02 ②–③, C-03 ②.
4. `인계_검증` — Given 검증 성공, When 파일과 메타데이터 및 README를 열면, Then 행 수·지역별 분포·출처·라이선스·합성 여부·고정 리비전이 확인된다. 픽스처: 생성 결과. C-03 ③–④.
5. `실패_경계` — Given 다운로드 실패, 크기/해시 불일치, 손상, 스키마 불일치, 빈 서울 결과, 검증 실패 또는 기존 최종 파일, When 처리를 수행하면, Then 성공으로 표시하지 않고 기존 파일을 덮어쓰지 않는다. 픽스처: 네트워크 오류/변조가 실제로 발생한 경우에만 확인하며 저영향 일회성 작업을 위해 구현 복제 테스트를 추가하지 않는다. C-01–C-03 오류 경계.

## 활동 엔진 공통 규약

[개념 Map](01_concept_map.md)의 [C-04](01_concept_map.md#c-04--활동과-시간-단서-추출)~[C-08](01_concept_map.md#c-08--출처-재현성-품질-검증)과 [DR-12~DR-20](decisions.md)을 구현하는 단위(I-04-a~I-08-e, 21개)를 아래에 정의한다. 규칙 데이터(정규식·키워드·템플릿 수치·지명 사전 시드)의 정본은 [02a_rule_data.md](02a_rule_data.md)이고, 이 문서의 코드 블록은 서울 원본의 앞 표본(단서 추출 20,000명, 계획·프레임 3,000명 × 7일)에서 실제로 돌려 불변식을 확인한 **검증용 프로토타입의 소스를 그대로 옮긴 참조 구현**이다(서울 전원 185,228명 실행은 아직 하지 않았고, 프로토타입 자체는 프로젝트에 두지 않았다). 코드와 이 문서가 어긋나면 코드가 틀린 것이고, 문서가 틀렸다면 문서를 먼저 고친다.

### 구현 단위 목록과 추적

| 단위 | 개념 단위 | 위치 | 한 줄 요약 |
|---|---|---|---|
| [I-04-a](#i-04-a--규칙-로더와-텍스트-도구) | C-04 | `engine/rules.py`, `engine/text.py`, `scripts/install_rules.py` | 규칙 로더, 목록 문자열 파싱, 문장·절 분리, 마스킹 |
| [I-04-b](#i-04-b--시간요일빈도역할-단서-추출기) | C-04 | `engine/cues/timecues.py` | 요일·빈도·시간대어·시각·역할 단서와 등급 |
| [I-04-c](#i-04-c--모달리티-분류기) | C-04 | `engine/cues/modality.py` | 현재 습관/희망/과거/부정/가정 분류 |
| [I-04-d](#i-04-d--활동-후보-15개-대분류-목록-항목-병합) | C-04 | `engine/cues/activities.py` | 활동 후보, 15개 대분류, 목록 항목 병합 |
| [I-04-e](#i-04-e--장소-판정과-근무지-단서) | C-04 | `engine/cues/places.py` | 장소 판정(`place_scope`)과 근무지 단서 |
| [I-04-f](#i-04-f--인물-단위-추출과-출력-파일) | C-04 | `engine/cues/extract.py` | 인물 단위 추출과 `cues.parquet` 출력 |
| [I-05-a](#i-05-a--시간-눈금-키드-난수-스키마) | C-05 | `engine/config.py`, `engine/timegrid.py`, `engine/rng.py`, `engine/schema.py` | 시간 눈금, 키드 난수, 스키마, 오류 계층 |
| [I-05-b](#i-05-b--템플릿-선택-근거-분류-일별-앵커) | C-05 | `engine/plan/person.py`, `engine/plan/anchors.py` | 템플릿 선택, 근거 분류, 일별 앵커 |
| [I-05-c](#i-05-c--10분-슬롯-격자와-배치) | C-05 | `engine/plan/grid.py` | 10분 슬롯 격자와 배치 |
| [I-05-d](#i-05-d--하루-활동-구간-조립) | C-05 | `engine/plan/compose.py` | 하루 활동 구간 조립(주간 배정 포함) |
| [I-06-a](#i-06-a--지리-상수-3종) | C-06 | `engine/geo/gazetteer.py`, `engine/geo/adjacency.py`, `rules/v1/geo/*`, `scripts/geo_candidates.py` | 지리 상수 3종 |
| [I-06-b](#i-06-b--위치-종류-구-해석-근무지) | C-06 | `engine/place/locate.py` | 위치 종류와 구 해석, 근무지 |
| [I-06-c](#i-06-c--이동-구간-삽입) | C-06 | `engine/place/moves.py` | 이동 구간 삽입 |
| [I-07-a](#i-07-a--세그먼트-저장소) | C-07 | `engine/store/segments.py` | 세그먼트 저장소 |
| [I-07-b](#i-07-b--프레임-샘플러) | C-07 | `engine/frames/sample.py` | 프레임 샘플러 |
| [I-07-c](#i-07-c--프레임-내보내기와-조회) | C-07 | `engine/frames/export.py` | 프레임 내보내기와 조회 |
| [I-08-a](#i-08-a--실행-메타데이터와-재생) | C-08 | `engine/verify/runmeta.py` | 실행 메타데이터와 재생 |
| [I-08-b](#i-08-b--전수-자동-검사) | C-08 | `engine/verify/invariants.py` | 전수 자동 검사 |
| [I-08-c](#i-08-c--보고-지표-5종) | C-08 | `engine/verify/metrics.py` | 보고 지표 5종 |
| [I-08-d](#i-08-d--표본-검수와-합격선) | C-08 | `engine/verify/samplereview.py` | 표본 검수와 합격선 |
| [I-08-e](#i-08-e--단계-실행기와-명령행) | C-07, C-08 | `engine/pipeline.py`, `engine/cli.py` | 단계 실행기와 명령행 |

### 실행 환경과 기술 선택

- 언어·런타임: Python 3.13.1 (`/Library/Frameworks/Python.framework/Versions/3.13/bin/python3`). 이 PC에 이미 있고 기존 구현([I-01-a](#i-01-a--원본-정보와-조각-획득-c-01) 등)과 같다.
- 라이브러리(설치되어 있는 버전 그대로 고정): pyarrow 24.0.0(Parquet 입출력, 스키마), numpy 2.4.6(프레임 샘플러와 검사의 벡터 연산에만). 나머지는 표준 라이브러리(`re`, `ast`, `json`, `csv`, `hashlib`, `datetime`, `zoneinfo`, `dataclasses`, `collections`, `argparse`, `pathlib`, `tempfile`, `unittest`)다.
- 쓰지 않는 것: pytest·hypothesis(설치되어 있지 않고 네트워크 설치가 필요), pandas, 서드파티 `regex`, 외부 서비스, 유료 의존성. 테스트는 사용자의 기존 도구(`/Users/aiden/memguard`)와 같은 `unittest`로 쓴다([S-01](#self-결정-기록)).
- 실행 위치: 프로젝트 루트(`/Users/aiden/CBU_Hack`)에서 `python3 -m engine.cli …`. 테스트는 `python3 -m unittest discover -s tests -t . -v`.

### 파일 배치

```
engine/                  # 새 파이썬 패키지 (위 단위 목록의 위치)
rules/v1/                # 02a_rule_data.md에서 설치한 규칙 데이터 (scripts/install_rules.py)
scripts/install_rules.py, scripts/geo_candidates.py, scripts/download_seoul_personas.py(기존)
tests/                   # test_*.py, fixtures.py (합성 인물·4개 구 장난감 지리)
data/nemotron-personas-korea/seoul.parquet   # 입력 (기존, 수정하지 않는다)
data/runs/<run_id>/      # 실행 산출물 (아래)
```

실행 산출물(`run_id` = `v1-s20261030-20261102-7d`, 곧 `{rule_version}-s{seed}-{시작일 YYYYMMDD}-{일수}d`이고 `--limit N`이면 `-n{N}`을 붙인다):

```
data/runs/<run_id>/
  run.json                                   # I-08-a
  persons.parquet                            # I-07-a (계획 단계: person_idx, uuid, 거주 구, 템플릿, 근무지)
  cues/cues.parquet, cues/cue_persons.parquet  # I-04-f
  segments/<YYYY-MM-DD>/part-<NNNNN>.parquet   # I-07-a (파트당 5,000명)
  frames/<YYYY-MM-DD>.parquet                # I-07-c (선택)
  report/checks.json, metrics.json, diagnostics.json, sample_review.md, labels.template.json, labels.json, thresholds.json   # I-08-b~d
```

### 표기와 용어

- 시각은 **하루 기준 분(minute-of-day) 정수**다. 0~1440이고, 1440 이상은 다음 날 새벽(취침·야간 근무)이다. 슬롯 = 10분, 하루 144슬롯, 모든 세그먼트 경계는 10의 배수다([DR-17](decisions.md#dr-17--프레임과-시간-규약-및-결정성)).
- 요일은 `datetime.date.weekday()`(0=월 … 6=일), 평일 0~4, 주말 5~6, 주 키는 `date.isocalendar()`의 (연, 주).
- 구 이름은 내부에서 짧은 이름(`양천구`)이고 출력 열에서는 원본과 같은 `서울-양천구`다. 변환은 `schema.full_name(short)` / `short_name(full)` 하나로 한다. 구 목록은 `metadata.json`의 `district_counts` 키 25개다.
- 정렬은 항상 유니코드 코드포인트 순(`sorted`)이다. set을 순회하지 않고 정렬한 뒤 순회한다.
- 참조 구현이 쓰는 짧은 키 이름과 Parquet 열 이름의 대응:

| 참조 구현 키 | 열 이름 | 참조 구현 키 | 열 이름 |
|---|---|---|---|
| `mid` | `mention_id` | `scope` | `place_scope` |
| `code` | `activity_code` | `ptoken` | `place_token` |
| `kw` | `activity_label` | `districts` | `place_districts` |
| `clause` | `clause_text` | `explicit` | `place_explicit` |
| `clock` | `clock_min` | `grade` | `time_grade` |
| `freq` | `freq_min`, `freq_max` | `sa` / `st` / `sp_place` | `src_activity` / `src_time` / `src_place` |
| `res` | 거주 구(짧은 이름) | `occ` | `occupation` |
| `dist` / `frm` / `to` | `district` / `from_district` / `to_district` | `ACT` / `MOVE` | `kind` = `ACTIVITY` / `MOVE` |

### 참조 구현을 읽는 법

- 코드 블록의 **대문자 전역 상수**(`TEMPLATES`, `DPWIN`, `DEFDUR`, `DEFLABEL`, `HOME_HINT`, `VENUE_HINT`, `NEAR`, `HOME_CODES`, `HOPS`, `RES`, `ADJ`, `DISTRICTS`, 정규식 상수들)는 프로토타입의 모듈 전역이다. 구현에서는 위 대응표대로 `RuleSet`(규칙 JSON)과 `Geo`(지리 상수)를 **인자로 주입**하고, 함수 시그니처는 각 단위 절의 시그니처를 따른다.
- 규칙 JSON을 읽을 때의 변환: `templates.json`의 `per_week`가 문자열 `"EVERY"`이면 참조 구현의 `EVERY`(`None`)와 같다. JSON의 배열(`[360, 450]`)은 참조 구현의 튜플과 같은 값으로 쓴다. `time.json.freq_rules[]`의 기술자는 [I-04-b](#i-04-b--시간요일빈도역할-단서-추출기)의 표대로 함수로 바꾼다.
- `c04`/`plan`이라는 이름은 코드 출처를 구분하는 표기일 뿐 모듈 이름이 아니다. 참조 구현의 `prepare`가 부르는 `c04.extract_person`은 구현에서는 `cues.parquet` 읽기로 바뀐다([I-05-b](#i-05-b--템플릿-선택-근거-분류-일별-앵커)).

### 결정성 규약 (모든 단위)

1. 난수는 [`KeyedRng`](#i-05-a--시간-눈금-키드-난수-스키마)만 쓴다. Python `random`, `numpy.random`, `hash()`를 쓰지 않는다(`PYTHONHASHSEED`에 의존하면 안 된다).
2. 결과는 입력, 규칙 버전, 시드, uuid, 날짜의 순수 함수다. 전역 상태, 현재 시각, 실행 순서에 의존하지 않는다([DR-17](decisions.md#dr-17--프레임과-시간-규약-및-결정성) ⑥). 각 난수 스트림의 키에는 용도 문자열을 넣어 한 용도에서 난수를 더 뽑아도 다른 용도의 결과가 흔들리지 않게 한다.
3. 같은 결과를 두 번 만들었을 때 바이트가 같아야 하므로 Parquet 쓰기는 고정 옵션(`compression="zstd"`, `row_group_size` 명시, 정렬 고정)을 쓰고 타임스탬프나 실행 ID를 파일 내용에 넣지 않는다.

### 오류 정책 (공통)

| 예외 | 발생 | 처리 | 종료 코드 |
|---|---|---|---|
| `RuleError` | 규칙 파일 누락·JSON 오류·정규식 컴파일 실패·해시 불일치 | 즉시 중단, 파일·키를 메시지에 포함 | 3 |
| `InputError` | 원본 Parquet의 필수 열 누락·타입 불일치, 구 25개가 아님 | 즉시 중단 | 2 |
| `InvariantError` | 불변식 위반([I-08-b](#i-08-b--전수-자동-검사)) | 실행을 실패로 표시하고 `checks.json`에 위반을 남김, 완성본으로 확정하지 않음 | 4 |
| `RunError` | 실행 설정 오류, 이미 존재하는 `run_id`, 경로 문제 | 즉시 중단, 기존 산출물을 덮어쓰지 않음 | 5 |

- 한 인물의 목록 문자열 파싱 실패는 실패가 아니다. `cue_persons.list_parse_failed = true`로 기록하고 그 인물은 서술 단서만으로 계속한다(원본 185,228행은 실패 0건이었다).
- 실행은 `data/runs/.tmp-<run_id>/`에 쓰고 모든 검사가 끝난 뒤에만 `<run_id>/`로 이름을 바꾼다. 실패하면 임시 디렉터리를 지운다. 완성본 이름은 검증 전에 노출하지 않는다([I-03-a](#i-03-a--재독-검증과-최종-인계-c-03)와 같은 원칙).

### 자원과 실행 규칙

- 단일 프로세스, 인물 5,000명 단위 청크로 처리한다(`config.CHUNK_PERSONS = 5000`). 청크 안에서 날짜별로 계획을 만들고 날짜별 파트 파일로 바로 쓴 뒤 버린다. 병렬화는 v1 범위 밖이다(순수 함수라 나중에 청크 단위로 추가할 수 있다).
- 프로토타입 실측(Apple M5 Pro, 단일 코어): 단서 추출 인물당 1.1~2.4ms, 하루 계획 인물·일당 0.09~0.14ms, 프레임 144시각 샘플링 서울 전원 기준 하루 약 3초(배열 약 52MB). 서울 전원 7일 기준 추출 약 3~8분, 계획 약 2~3분, 샘플링과 검사 약 1분이다.
- **이 PC는 메모리·디스크 고갈로 두 번 커널 패닉이 난 적이 있어 `memguard` 규칙을 따른다.** 서울 전원 규모(`--limit` 없음) 실행은 반드시 `/Users/aiden/memguard/bin/memguard run --label "CBU_Hack:<단계>" --mem-gb 3 -- python3 -m engine.cli …`로 감싼다. 단계별 메모리 예산은 추출 2GiB 미만, 계획 1GiB 미만, 검증·샘플링 2GiB 미만으로 보며 `--mem-gb 3`을 쓴다. `--limit`을 준 개발·테스트 실행은 감싸지 않는다. 시작 전에 `memguard status`로 디스크 여유 12GiB 이상을 확인한다.
- 전체 프레임을 메모리에 쌓지 않는다. 프레임은 시각 1개(185,228행)씩 만들어 바로 쓴다([DR-18](decisions.md#dr-18--저장과-조회-방식)).

### 단계 간 데이터 계약

| 생산 → 소비 | 데이터 | 핵심 열 |
|---|---|---|
| [I-04-f](#i-04-f--인물-단위-추출과-출력-파일) → [I-05-b](#i-05-b--템플릿-선택-근거-분류-일별-앵커) | `cues.parquet` | `usable`, `activity_code`, `role`, 빈도·요일·시간대·시각 열, `place_*`, `mention_id` |
| [I-04-f](#i-04-f--인물-단위-추출과-출력-파일) → [I-06-b](#i-06-b--위치-종류-구-해석-근무지) | `cue_persons.parquet` | `workplace_hint_districts`, `workplace_hint_explicit` |
| [I-05-d](#i-05-d--하루-활동-구간-조립) → [I-06-c](#i-06-c--이동-구간-삽입) | 활동 구간 목록(메모리) | `start`, `end`, `code`, `detail`, `mid`, `tag`, `sa`, `st` |
| [I-06-c](#i-06-c--이동-구간-삽입) → [I-07-a](#i-07-a--세그먼트-저장소) | 위치·이동이 붙은 구간(메모리) | 위 열 + `dist`, `frm`, `to`, `sp_place` |
| [I-07-a](#i-07-a--세그먼트-저장소) → [I-07-b](#i-07-b--프레임-샘플러) | `segments/<날짜>/part-*.parquet` | `person_idx`, `start`, `end`, `kind`, 구 열 |
| [I-07-b](#i-07-b--프레임-샘플러) → [I-07-c](#i-07-c--프레임-내보내기와-조회), [I-08-b](#i-08-b--전수-자동-검사) | 프레임 청크(메모리) | `person_idx`, `district_id`, `state`, `code_id`, `row` |


## I-04-a — 규칙 로더와 텍스트 도구

- 추적: [C-04](01_concept_map.md#c-04--활동과-시간-단서-추출) 전 기준의 공통 기반. 관련 결정 [DR-15](decisions.md#dr-15--시간-신호-해석과-추출-방식), [DR-16](decisions.md#dr-16--활동-분류-체계).
- 위치: `engine/rules.py`, `engine/text.py`, `scripts/install_rules.py`.
- 기술: 표준 라이브러리(`json`, `csv`, `re`, `ast`, `hashlib`, `dataclasses`). 정규식은 플래그 없이 컴파일한다.
- 규칙 데이터와 참조 구현 상수의 대응(`RuleSet`이 컴파일해서 들고 있다):

| 참조 구현 상수 | 파일 · 키 |
|---|---|
| `FIELDS` | `text.json` `field_order` |
| `SENT_BREAK`, `CLAUSE_BREAK`, `NAME`, `NONTIME` | `text.json` `sentence_break`, `clause_break`, `name_mask`, `nontime_titles`(여러 개면 `\|`로 결합) |
| `WEEKEND_RE`, `WEEKDAY_RE`, `WEEKDAY_NAMED`, `WD_IDX`, `WEEKEND_FREQ`, `WEEKDAY_FREQ` | `time.json` `weekend`, `weekday`, `weekday_named`, `weekday_index`, `weekend_freq`, `weekday_freq` |
| `FREQ_RULES`, `KNUM`, `APPROX` | `time.json` `freq_rules`, `numerals`, `approx_numerals` (`NUMW` = `\d+\|한\|두\|세\|네\|다섯\|여섯\|일곱`) |
| `DAYPARTS`, `DP_WORD`, `CLOCK_A`, `CLOCK_24`, `CLOCK_B`, `ROLES` | `time.json` `dayparts`, `daypart_word_to_code`, `clock_a`, `clock_24`, `clock_b`, `roles` |
| `NEG`, `HYP`, `ASP_STRONG`, `ASP_VALUE`, `PAST` | `modality.json` `neg`, `hyp`, `asp_strong`, `asp_value_tier_b_only`, `past` |
| `CLAUSE_RULES`, `LABEL_RULES`, `TRIP`, `TRIP_CTX`, `PLACE_LIKE`, `DIST_RE`, `DIST_SPORT_RE`, `DISTRICT_NAMES`, `MERGE_STOP` | `activities.json` `clause_rules`, `label_rules`, `trip_clause`, `trip_context`, `place_like`, `district_pattern`, `district_sport_pattern`, `district_names`, `merge_stoplist` |
| `HOME_HINT`, `VENUE_HINT`, `NEAR`, `HOME_CODES` | `location.json` `home_hint`, `venue_hint`, `near_codes`, `home_codes` |
| `TEMPLATES`, `DEFDUR`, `DEFLABEL`, `DPWIN` | `templates.json` `templates`, `default_duration`, `default_label` / `time.json` `dayparts[].window` |

- 시그니처:

```python
# engine/rules.py
@dataclass(frozen=True)
class RuleSet:
    version: str; sha256: str                       # rule_set_sha256
    text: dict; time: dict; modality: dict; activities: dict; location: dict; templates: dict   # 읽은 JSON 그대로
    rx: SimpleNamespace                              # 컴파일된 정규식 (위 표의 상수 이름을 소문자로)
def load_rules(version: str = "v1", rules_dir: Path = RULES_DIR) -> RuleSet
def compute_rule_set_sha256(rules_dir: Path, version: str) -> str      # 경로 오름차순: path.encode()+b"\0"+bytes

# engine/text.py
@dataclass(frozen=True)
class Span: start: int; end: int; text: str          # start/end = 필드 원문 안의 코드포인트 오프셋, text = 앞뒤 공백을 뺀 부분
def parse_list_field(value: str | None) -> list[str] | None
def split_sentences(text: str | None, rules: RuleSet) -> list[Span]
def split_clauses(sentence: Span, rules: RuleSet) -> list[Span]
def mask_text(text: str, rules: RuleSet) -> str       # 길이 불변
```

- 데이터 구조: 위 시그니처. `rule_set_sha256`은 규칙 파일 전체(JSON 6 + CSV 3)로 계산한다. 구 인접표가 아직 없는 개발 중에는 있는 파일만으로 계산하고 `run.json`에 `adjacency_present`를 남긴다.
- 로직:
  1. `load_rules`: `rules_dir/version/`의 JSON 6개를 읽고, 정규식 키를 모두 `re.compile`하고, `RuleSet.sha256`을 계산한다. `time.json`의 `freq_rules[]`는 컴파일해서 보관한다.
  2. `parse_list_field`: `None`이거나 빈 문자열이면 `None`. `ast.literal_eval`을 `try`로 감싸 `ValueError`, `SyntaxError`, `RecursionError`, `MemoryError`를 모두 `None`으로 바꾼다. 결과가 `list`이고 모든 원소가 `str`일 때만 돌려준다(빈 목록 `[]`은 유효). 그 밖은 `None`.
  3. `split_sentences`: `SENT_BREAK`로 나눈다. 구분자는 버리고, 각 조각의 앞뒤 공백을 제거하되 오프셋은 **제거 후의 시작·끝**을 가리킨다. 빈 조각은 버린다. `None`은 `[]`.
  4. `split_clauses`: `CLAUSE_BREAK`로 나눈다. 길이 `text.json.min_clause_chars`(2) 미만 조각은 바로 앞 절에 합치고(앞이 없으면 버린다), 오프셋은 문장 오프셋 + 문장 안 오프셋이다.
  5. `mask_text`: `NONTIME` 패턴과 `NAME`(`씨` 앞의 2~4음절 이름)을 같은 길이의 `□`로 바꾼다. 위치가 변하지 않으므로 마스킹한 문자열의 오프셋을 원문에 그대로 쓴다. 방송명('6시 내고향', '뉴스 9', '톱10', 'N 시청')을 시각 단서로 세지 않게 하고, 사람 이름이 활동 키워드('수영')와 겹치는 것을 막는다([DR-15](decisions.md#dr-15--시간-신호-해석과-추출-방식) ④).
  6. `scripts/install_rules.py`: `02a_rule_data.md`에서 `## \`rules/v1/…\`` 제목 다음 코드 블록을 읽어 `rules/v1/…`에 `내용 + "\n"` 그대로 쓴다. 문서의 해시 표와 파일별 SHA-256이 다르면 쓰지 않고 `RuleError`(종료 코드 3). 이미 같은 내용이면 건너뛰고 다르면 덮어쓰지 않고 실패한다.
- 오류: 파일 없음, JSON 오류, 정규식 컴파일 실패(파일·키 포함), 해시 불일치 → `RuleError`. `parse_list_field`는 예외를 던지지 않는다.
- 엣지: `None`·빈 문자열·공백만 있는 필드는 `[]`; 종결 부호 없는 한 문장; `\r\n` 줄바꿈; 쉼표로 시작하는 문장; 이름 마스크가 '씨'가 없는 문장에는 영향 없음; 5,000자 이상의 긴 필드도 같은 규칙으로 처리.
- 자원: 규칙 로딩 1초 미만. 메모리 무시 가능.
- 테스트 명세:

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_rules_load_v1` | 02a에서 설치한 `rules/v1` | `load_rules("v1")` | JSON 6개·CSV 3개 중 있는 파일이 모두 로드되고, 정규식이 전부 컴파일되며, `sha256`이 02a의 `rule_set_sha256`과 같다 |
| `test_rules_bad_regex` | 임시 디렉터리에 `modality.json`의 `neg`를 `"("`로 바꾼 사본 | `load_rules` | `RuleError`, 메시지에 `modality.json`과 `neg` |
| `test_rules_missing_file` | `time.json`이 없는 사본 | `load_rules` | `RuleError` |
| `test_parse_list_field_ok` | `"['a', 'b']"`, `"[]"` | `parse_list_field` | `['a','b']`, `[]` |
| `test_parse_list_field_bad` | `None`, `""`, `"abc"`, `"['a', 3]"`, `"{'a': 1}"`, 깊이 중첩 문자열 | `parse_list_field` | 모두 `None`, 예외 없음 |
| `test_split_sentences_offsets` | `"가나다. 라마바!\n사아자"` | `split_sentences` | 3개, 각 `text[start:end] == span.text`, 빈 조각 없음 |
| `test_split_clauses_merge_short` | `"주말이면 산책을 하고, 가, 영화를 봅니다"` | `split_clauses` | 1글자 절 '가'가 앞 절에 합쳐져 오프셋이 이어진다 |
| `test_mask_text_same_length` | `"이수영 씨는 '6시 내고향'과 KBS 뉴스 9 시청"` | `mask_text` | 길이 동일, 이름·'6시 내고향'·'뉴스 9'·'9 시청' 자리가 `□` |
| `test_install_rules_hash_mismatch` | 02a 사본에서 한 JSON 블록을 한 글자 바꾼 것 | `install_rules` | 아무 파일도 쓰지 않고 `RuleError` |
| `test_install_rules_idempotent` | 이미 설치된 `rules/v1` | `install_rules` 재실행 | 파일 변경 없음, 종료 코드 0 |

- DoD: 위 테스트 전부 green. `rules/v1`이 02a의 해시 표와 일치. 규칙 파일 해시가 `run.json`에 기록 가능.

```python
# 참조 구현(검증용 프로토타입 소스 그대로) — spans, mask
def spans(pattern, text):
    pos = 0
    for m in pattern.finditer(text):
        if m.start() > pos:
            yield pos, text[pos:m.start()]
        pos = m.end()
    if pos < len(text):
        yield pos, text[pos:]


def mask(t):
    t = NONTIME.sub(lambda m: "□" * len(m.group(0)), t)
    return NAME.sub(lambda m: "□" * len(m.group(0)), t)
```

## I-04-b — 시간·요일·빈도·역할 단서 추출기

- 추적: [C-04](01_concept_map.md#c-04--활동과-시간-단서-추출) 기준 ②⑤, 테스트 E04-2·E04-5. 관련 결정 [DR-15](decisions.md#dr-15--시간-신호-해석과-추출-방식).
- 위치: `engine/cues/timecues.py`.
- 기술: 표준 `re`. 규칙은 `time.json`.
- 시그니처:

```python
@dataclass(frozen=True)
class TimeCues:
    day_scope: str                 # ANY | WEEKDAY | WEEKEND
    weekdays: tuple[int, ...]      # 0=월 … 6=일, 비어 있으면 "특정 요일 없음"
    freq_kind: str                 # NONE | DAILY | ALMOST_DAILY | PER_WEEK | WEEKLY | PROB_WEEK | OCCASIONAL
    freq_min: float | None; freq_max: float | None
    daypart: str | None            # DAWN MORNING FORENOON NOON AFTERNOON EVENING NIGHT
    clock_min: int | None          # 0..1620 (>=1440 이면 다음 날 새벽)
    grade: str                     # A | B | C | D | N
    role: str | None               # WAKE | BED | WORK_START | WORK_END
    inherited: tuple[str, ...]     # 앞 절에서 이어받은 단서 ("day", "daypart")
def extract_time_cues(masked_clause: str, rules: RuleSet) -> TimeCues
def inherit_context(ctx: Context, cues: TimeCues) -> tuple[TimeCues, Context]    # 문장 안 앞 절 → 뒤 절
```

- 등급([DR-15](decisions.md#dr-15--시간-신호-해석과-추출-방식) ④): `A` = 시간대어+N시(분)이거나 `HH:MM`; `B` = 시간대어 없는 N시이지만 역할(WAKE·WORK_START → 오전 1~11시, BED·WORK_END → 오후 1~11시, BED의 1~5시는 다음 날 새벽)로 확정; `C` = 시간대어만; `D` = 시각 없이 요일·빈도만; `N` = 없음. 한글 숫자 시각('여섯 시')은 `time.json.clock_korean_detect_only`로 **탐지만** 하고 건수를 센다(쓰지 않는다). 건수는 인물별 `korean_clock_hits`이며 세는 방법은 [I-04-f](#i-04-f--인물-단위-추출과-출력-파일)에 있다.
- 빈도 기술자 해석(`time.json.freq_rules[]`, 위에서부터 **처음 일치하는 것**이 이긴다): `min`/`max`는 고정값, `min_group`/`max_group`은 정규식 그룹의 수(없으면 min과 같음), `approx_group`은 `approx_numerals`의 범위, `ratio_n`/`ratio_w`는 주당 확률 n/w(`2주에 한 번` = 0.5), `p`는 고정 확률, `per_month_group`은 `min(1.0, N/4)`('한 달에 N번').
- 로직(참조 구현, 아래 코드와 같다):
  1. `주말마다|주말이면|주말에는|주말엔` → `WEEKEND`, 요일 [5,6], `WEEKLY(1,1)`. 아니면 `주말`/`평일` 중 **먼저 나온 것**의 범위; `월요일` 같은 요일 이름이 있으면 `weekdays`를 그 요일로 좁힌다. 빈도가 따로 없고 `평일마다|평일이면|평일에는|평일 아침/저녁…`이면 `PER_WEEK(3,5)`(설계 상수 [S-07](#self-결정-기록)).
  2. `freq_rules`를 순서대로 시험해 첫 일치를 채택한다. 순서가 중요하다('거의 매일'이 '매일'보다 먼저, '2주에 한 번'이 '주 N회'보다 먼저 걸리도록 `PER_WEEK` 패턴은 숫자 앞 글자가 `\d두세네`가 아닐 것을 요구한다).
  3. 시간대어는 `dayparts` 순서의 **첫 일치**. `낮`, `밤`은 한글에 둘러싸이지 않은 단독어일 때만(`낮추고`, `군밤`을 제외).
  4. `CLOCK_A`(시간대어+N시[+M분|반])를 변환표(`to_minutes`)로 분으로 바꾼다. 변환 불가(`아침 12시`)면 시각 없음. 없으면 `CLOCK_24`(`07:30`). 없으면 `CLOCK_B`를 보되 **역할이 있을 때만** 채택한다.
  5. 등급 결정: 시각이 있으면 A/B, 아니면 시간대어 C, 아니면 요일·빈도 D, 아니면 N.
  6. `inherit_context`: 한 문장 안에서 앞 절의 `day_scope`·`weekdays`·`daypart`를 뒤 절이 이어받는다. 뒤 절에 자기 `day_scope`가 없고 **자기 빈도도 없을 때만** 요일 범위를 받고, **빈도(freq)와 시각(clock)은 이어받지 않는다**(번짐 오염 방지). 문장이 끝나면 맥락을 비운다.

```python
# 참조 구현(검증용 프로토타입 소스 그대로) — num, to_minutes, role_of, time_cues, inherit
def num(s):
    return int(s) if s.isdigit() else KNUM[s]


def to_minutes(dp, h, m, half):
    mm = int(m) if m else (30 if half else 0)
    if dp in ("새벽", "아침", "오전"):
        hh = h if 0 <= h <= 11 else None
    elif dp == "낮":
        hh = 12 if h == 12 else (h + 12 if 1 <= h <= 5 else (h if h == 11 else None))
    elif dp == "오후":
        hh = 12 if h == 12 else (h + 12 if 1 <= h <= 11 else None)
    elif dp == "저녁":
        hh = h + 12 if 4 <= h <= 11 else None
    elif dp == "밤":
        hh = h + 12 if 6 <= h <= 11 else (24 if h == 12 else (24 + h if 1 <= h <= 5 else None))
    else:
        hh = None
    return None if hh is None else hh * 60 + mm


def role_of(c):
    for name, rx in ROLES:
        if rx.search(c):
            return name
    return None


def time_cues(c):
    """returns dict of cue attributes for one (masked) clause"""
    out = {"day_scope": "ANY", "weekdays": [], "freq_kind": "NONE", "freq": None, "daypart": None, "clock": None, "grade": "N", "role": role_of(c)}
    m = WEEKEND_FREQ.search(c)
    if m:
        out.update(day_scope="WEEKEND", weekdays=[5, 6], freq_kind="WEEKLY", freq=(1, 1))
    else:
        we, wd = WEEKEND_RE.search(c), WEEKDAY_RE.search(c)
        if we and (not wd or we.start() < wd.start()):
            out.update(day_scope="WEEKEND", weekdays=[5, 6])
        elif wd:
            out.update(day_scope="WEEKDAY", weekdays=[0, 1, 2, 3, 4])
        named = sorted({WD_IDX[x] for x in WEEKDAY_NAMED.findall(c)})
        if named:
            out["weekdays"] = named
            out["day_scope"] = "WEEKEND" if set(named) <= {5, 6} else ("WEEKDAY" if set(named) <= {0, 1, 2, 3, 4} else "ANY")
        if WEEKDAY_FREQ.search(c) and out["freq_kind"] == "NONE":
            out.update(freq_kind="PER_WEEK", freq=(3, 5))
        for kind, rx, fn in FREQ_RULES:
            fm = rx.search(c)
            if fm:
                out.update(freq_kind=kind, freq=fn(fm))
                break
    for code, rx, win in DAYPARTS:
        if rx.search(c):
            out["daypart"] = code
            break
    ca = CLOCK_A.search(c)
    if ca:
        mins = to_minutes(ca.group("dp"), int(ca.group("h")), ca.group("m"), ca.group("half"))
        if mins is not None:
            out.update(clock=mins, grade="A", daypart=DP_WORD[ca.group("dp")])
    if out["clock"] is None:
        c24 = CLOCK_24.search(c)
        if c24:
            out.update(clock=int(c24.group("h")) * 60 + int(c24.group("m")), grade="A")
    if out["clock"] is None:
        cb = CLOCK_B.search(c)
        if cb and out["role"]:
            h = int(cb.group("h"))
            mm = int(cb.group("m")) if cb.group("m") else (30 if cb.group("half") else 0)
            if out["role"] in ("WAKE", "WORK_START") and 1 <= h <= 11:
                out.update(clock=h * 60 + mm, grade="B")
            elif out["role"] in ("BED", "WORK_END") and 1 <= h <= 11:
                out.update(clock=(h + 12) * 60 + mm if not (out["role"] == "BED" and h <= 5) else (h + 24) * 60 + mm, grade="B")
    if out["grade"] == "N":
        if out["daypart"]:
            out["grade"] = "C"
        elif out["freq_kind"] != "NONE" or out["day_scope"] != "ANY":
            out["grade"] = "D"
    return out


def inherit(cur, new):
    """forward inheritance inside a sentence: day_scope/weekdays and daypart carry on; freq and clock never do"""
    merged = dict(new)
    inh = []
    if new["day_scope"] == "ANY" and new["freq_kind"] == "NONE" and cur["day_scope"] != "ANY":
        merged["day_scope"], merged["weekdays"] = cur["day_scope"], cur["weekdays"]
        inh.append("day")
    if new["daypart"] is None and cur["daypart"] is not None:
        merged["daypart"] = cur["daypart"]
        inh.append("daypart")
    if merged["grade"] == "N":
        merged["grade"] = "C" if merged["daypart"] else ("D" if merged["freq_kind"] != "NONE" or merged["day_scope"] != "ANY" else "N")
    merged["inherited"] = inh
    return merged
```

- 오류: 입력은 이미 마스킹된 문자열이어야 한다. 예외를 던지지 않는다(정규식 외 연산 없음). `None`이 들어오면 `TypeError`가 아니라 빈 문자열처럼 처리한다.
- 엣지: `3시간`·`10시간`·`톱10`·`뉴스 9 시청`은 시각 아님; `밤 12시`는 1440; `새벽 4시 반`은 270; 시간대어와 N시 사이에 따옴표가 있으면 A가 아님; 역할 없는 `9시 뉴스`는 N; 한 절에 `주말`과 `평일`이 둘 다 있으면 먼저 나온 쪽; `격주`는 PROB_WEEK(0.5).
- 자원: 절당 정규식 20여 번, 마이크로초~수십 마이크로초.
- 테스트 명세(`extract_time_cues(mask_text(문장))`의 기대값, 프로토타입에서 확인):

| 입력 문장 | 등급 | clock_min | daypart | day_scope | 빈도 | role | weekdays |
|---|---|---|---|---|---|---|---|
| 매일 새벽 6시면 어김없이 일어나 | A | 360 | DAWN | ANY | DAILY 7–7 | WAKE | |
| 오후 4시쯤 마포구 골목에 위치한 빵집에서 | A | 960 | AFTERNOON | ANY | NONE | | |
| 매일 저녁 9시 뉴스를 꼼꼼히 시청하며 | A | 1260 | EVENING | ANY | DAILY | | |
| 거실에서 9시 뉴스 프로그램을 시청하며 | N | | | ANY | NONE | | |
| 'KBS 뉴스 9 시청', '가족과 함께하는 | N | | | ANY | NONE | | |
| TV 앞에 앉아 '6시 내고향'을 시청하며 | N | | | ANY | NONE | | |
| 매일 아침 8시 정각에 출근해 먼지 하나 없는 | A | 480 | MORNING | ANY | DAILY | WORK_START | |
| 오후 6시 정각에 퇴근하는 안정적인 삶 | A | 1080 | AFTERNOON | ANY | NONE | WORK_END | |
| 밤 12시가 넘어서야 잠자리에 들며 | A | 1440 | NIGHT | ANY | NONE | BED | |
| 저녁 6시 30분에 저녁 식사를 | A | 1110 | EVENING | ANY | NONE | | |
| 매일 오전 10시, 예술의전당 산책로를 | A | 600 | FORENOON | ANY | DAILY | | |
| 밤 11시에 불을 끄고 잠자리에 들며 | A | 1380 | NIGHT | ANY | NONE | BED | |
| 아침 7시에 일어나 | A | 420 | MORNING | ANY | NONE | WAKE | |
| 새벽 4시 반에 눈을 뜨고 | A | 270 | DAWN | ANY | NONE | WAKE | |
| 07:30에 기상하여 | A | 450 | | ANY | NONE | WAKE | |
| 3시간 동안 독서를 하며 / 10시간 근무 / 가요 톱10 시절의 음악을 | N | | | ANY | NONE | | |
| 9시부터 6시까지 사무실 생활을 | N | | | ANY | NONE | | |
| 주 3회 수영을 하며 | D | | | ANY | PER_WEEK 3–3 | | |
| 일주일에 대여섯 번 집 밖에서 식사를 | D | | | ANY | PER_WEEK 5–6 | | |
| 2주에 한 번은 단골 분식집에서 / 한 달에 두 번 등산 모임에 | D | | | ANY | PROB_WEEK 0.5 | | |
| 매주 수요일 저녁 모임에 | C | | EVENING | WEEKDAY | WEEKLY 1–1 | | [2] |
| 평일 저녁에는 석촌호수를 | C | | EVENING | WEEKDAY | PER_WEEK 3–5 | | [0,1,2,3,4] |
| 주말마다 낚시터를 | D | | | WEEKEND | WEEKLY 1–1 | | [5,6] |
| 거의 매일 산책을 | D | | | ANY | ALMOST_DAILY 5–6 | | |
| 낮추고 음악을 틀어놓은 채 / 군밤을 먹으며 | N | | | ANY | NONE | | |
| 밤새 게임을 하며 | C | | NIGHT | ANY | NONE | | |
| 여섯 시에 일어나 | N | | | ANY | NONE | WAKE | |

  추가 테스트: `test_inherit_day_and_daypart_not_freq` — 문장 `주말마다 낚시터를 찾고, 고기를 구워 먹습니다`를 절별로 `time_cues` → `inherit`(앞 절 결과를 `cur`로 넘김)에 넣으면 둘째 절이 `day_scope = WEEKEND`, `freq_kind = NONE`(앞 절의 `WEEKLY`는 받지 않음), `grade = D`, `inherited = ["day"]`이다. 문장 `주말마다 낚시터를 찾고, 일주일에 한 번 고기를 구워 먹습니다`에서는 둘째 절이 자기 빈도(`PER_WEEK`)를 가지므로 `day_scope = ANY`(WEEKEND를 받지 않음)이다. `test_korean_clock_detect_only` — `persona = "여섯 시에 일어나 산책을 합니다. 일곱 시 반에 청소를 합니다."`(나머지 필드는 비움)를 `extract_person`에 넣으면 `korean_clock_hits == 2`이고, 두 절의 `time_cues`는 모두 `clock = None`, `grade = N`이다(첫 절만 `role = WAKE`).
- DoD: 표의 기대값 전부와 추가 테스트 green, E04-5 통과.

## I-04-c — 모달리티 분류기

- 추적: [C-04](01_concept_map.md#c-04--활동과-시간-단서-추출) 기준 ③, 테스트 E04-3. 관련 결정 [DR-15](decisions.md#dr-15--시간-신호-해석과-추출-방식) ③.
- 위치: `engine/cues/modality.py`.
- 시그니처: `def classify_modality(masked_clause: str, tier: str, cues: TimeCues, rules: RuleSet) -> str`  (반환: `PRESENT_HABIT`, `PAST`, `ASPIRATION`, `NEGATION`, `HYPOTHETICAL`, `UNKNOWN`).
- 로직(참조 구현): 우선순위 `NEGATION` → `HYPOTHETICAL` → `ASPIRATION` → `PAST` → 나머지. `ASPIRATION`은 `asp_strong`에 걸리거나, **Tier B일 때만** `asp_value`('~에 집중합니다/가치를 둡니다/추구/만족/우선순위…')에 걸릴 때. 아무것도 안 걸리면 Tier A는 `PRESENT_HABIT`, Tier B는 시각·빈도·요일·시간대어 단서(등급 ≠ N)나 역할이 있을 때만 `PRESENT_HABIT`, 없으면 `UNKNOWN`.
- 근거: 같은 문장 유형이라도 `hobbies_and_interests`의 '매일' 문장 중 희망형은 2.1%인데 `career_goals_and_ambitions`는 31.8%(마커 미검출분 포함 대부분)이므로 Tier별로 마커 집합을 다르게 쓴다([DR-15](decisions.md#dr-15--시간-신호-해석과-추출-방식)). '바라보며', '기대어', '계획 없이', '자극적이지 않은', '라면(식품)'이 오분류되지 않도록 마커를 좁혔다(프로토타입 1차 실행에서 확인한 오탐).

```python
# 참조 구현(검증용 프로토타입 소스 그대로) — modality
def modality(c, tier, cues):
    if NEG.search(c):
        return "NEGATION"
    if HYP.search(c):
        return "HYPOTHETICAL"
    if ASP_STRONG.search(c) or (tier == "B" and ASP_VALUE.search(c)):
        return "ASPIRATION"
    if PAST.search(c):
        return "PAST"
    if tier == "A":
        return "PRESENT_HABIT"
    has_cue = cues["grade"] != "N" or cues["role"] is not None
    return "PRESENT_HABIT" if has_cue else "UNKNOWN"
```

- 오류: 없음. 엣지: 빈 문자열은 Tier A면 `PRESENT_HABIT`, Tier B면 `UNKNOWN`; 마커가 둘 이상이면 위 우선순위.
- 테스트 명세:

| 문장 (Tier) | 기대 |
|---|---|
| 주말이면 북한산 둘레길을 걸으며 땀을 흘립니다 (A) | PRESENT_HABIT |
| 가끔은 계획 없이 동네 친구들과 만나 수다를 떱니다 (A) | PRESENT_HABIT |
| 거실 소파에 기대어 유튜브로 노래를 듣습니다 (A) | PRESENT_HABIT |
| 여행지에서 바라보는 풍경이 좋아 사진을 찍습니다 (A) | PRESENT_HABIT |
| 자극적이지 않은 사찰 음식을 즐겨 찾습니다 (A) | PRESENT_HABIT |
| 매운 음식은 먹지 않습니다 (A) | NEGATION |
| 젊은 시절에는 매일 새벽 5시에 일어나 운동을 했었습니다 (A) | PAST |
| 언젠가 제주도에서 한 달 살기를 하고 싶습니다 (A) | ASPIRATION |
| 9시부터 6시까지 갇혀 있는 사무실 생활을 상상하며 (B) | HYPOTHETICAL |
| 거창한 성공보다는 매일 정해진 시간에 출근하는 평범한 일상을 되찾는 것에 우선순위를 둡니다 (B) | ASPIRATION |
| 매일 아침 9시 정각에 책상에 앉아 오답 노트를 정리하며 (B) | PRESENT_HABIT |
| 회계 자격증 취득을 위해 매일 아침 9시 정각에 책상에 앉아 (B) | PRESENT_HABIT |
| 주말에 라면을 끓여 먹으며 휴식을 취합니다 (A) | PRESENT_HABIT |
| 만약 시간이 난다면 낚시를 가겠지만 (A) | HYPOTHETICAL |
| 송파구 일대 부동산 시장을 분석하며 컨설팅을 제공합니다 (B) | UNKNOWN |

- 자원: 절당 정규식 5회 안팎, µs 단위.
- DoD: 위 15행 green. 추가로 `test_tier_a_unknown_never` — 표의 15개 입력과 `매일 산책을 합니다`를 모두 `tier = "A"`로 분류하면 `UNKNOWN`이 하나도 없고 마지막 문장은 `PRESENT_HABIT`이다(단서 없는 `산책을 합니다`를 `tier = "B"`로 분류하면 `UNKNOWN`).

## I-04-d — 활동 후보, 15개 대분류, 목록 항목 병합

- 추적: [C-04](01_concept_map.md#c-04--활동과-시간-단서-추출) 기준 ②③⑥, 테스트 E04-2·E04-3·E04-6. 관련 결정 [DR-16](decisions.md#dr-16--활동-분류-체계), [DR-15](decisions.md#dr-15--시간-신호-해석과-추출-방식).
- 위치: `engine/cues/activities.py`. (절·목록 항목을 mention 행으로 만드는 흐름은 [I-04-f](#i-04-f--인물-단위-추출과-출력-파일)의 `extract_person`에 있다.)
- 규칙:
  1. **절 대분류**: `clause_rules`를 순서대로 시험해 일치한 **서로 다른 대분류를 최대 2개**(`max_mentions_per_clause`) 채택한다. 순서는 WORK(야간·교대 근무 키워드만) → STUDY → CULTURE → EXERCISE → MEAL → HOUSEHOLD → CARE → PERSONAL → ERRAND → SOCIAL → REST → HOBBY. 문화(CULTURE)를 운동(EXERCISE)보다 앞에 둔 것은 '고궁을 거닐며'가 문화로 먼저 잡히게 하기 위해서다.
  2. **역할 mention**: 절에 역할(WAKE·BED·WORK_START·WORK_END)이 있으면 대분류 키워드가 없어도 별도 mention을 만든다. WAKE·BED는 `SLEEP`, WORK_START·WORK_END는 `WORK`이며 `role`이 채워진다. 이 mention은 일과 배치가 아니라 앵커로만 쓰인다([I-05-b](#i-05-b--템플릿-선택-근거-분류-일별-앵커)).
  3. **mention 생성 조건**: 대분류 hit가 있고 단서가 하나라도 있을 때(시각·요일·빈도·시간대 등급 ≠ N, 역할, 장소 판정 ≠ NONE). 단서가 없는 서술 절은 mention이 되지 않는다(활동 어휘는 목록 항목이 맡는다).
  4. **서울 밖 여행 절**: 대분류가 없고 장소 판정이 `OUTSIDE_SEOUL`이면 `activity_code = OTHER`인 mention을 만들어 생략 사실을 센다. 단, 같은 문장 다른 절에 서울 장소가 있고 이 절의 서울 밖 근거가 '여행' 같은 일반 여행어뿐이면 만들지 않는다([S-11](#self-결정-기록)).
  5. **목록 항목**: `hobbies_and_interests_list`의 각 항목을 `label_rules` 첫 일치로 대분류한다. 대분류가 `TRIP`이면 항목 안에 서울 장소가 있을 때만 `CULTURE`로 바꾸고, 아니면 `OTHER` + 서울 밖(생략)으로 한다. 일치가 없으면 `OTHER`. 목록 항목은 `modality = PRESENT_HABIT`, 시각 단서 없음으로 시작한다. 기술 목록(`skills_and_expertise_list`)은 mention을 만들지 않는다([S-08](#self-결정-기록)).
  6. **병합**: 목록 항목의 내용 토큰(3음절 이상 한글·영숫자, `MERGE_STOP` 제외)이 **같은 대분류의** 서술 mention 절 안에 하나라도 있으면 병합한다. 목록 항목이 서술 mention의 시각·요일·빈도·모달리티(와 장소가 비었으면 장소)를 받고 `merged_from`에 그 mention의 id를 적는다. 서술 mention은 `dup_of = 목록 항목 id`, `excluded = true`, `excluded_reason = "DUP:<id>"`가 된다. 한 서술 mention은 한 목록 항목에만 병합된다.
  7. `OTHER`는 일과 배치에서 `HOBBY`로 취급한다([I-05-b](#i-05-b--템플릿-선택-근거-분류-일별-앵커)).
- 시그니처: `def label_category(label: str, rules: RuleSet) -> str` (반환: 15개 코드 중 하나 또는 `"TRIP"`); `def clause_categories(masked_clause: str, rules: RuleSet) -> list[tuple[str, int, str]]` (코드, 위치, 일치어, 최대 2개); `def merge_list_items(mentions: list[dict], rules: RuleSet) -> None` (제자리 갱신).

```python
# 참조 구현(검증용 프로토타입 소스 그대로) — label_category
def label_category(label):
    for code, rx in LABEL_RULES:
        if rx.search(label):
            return code
    return "OTHER"
```

```python
# 참조 구현(extract_person 안의 같은 반복문을 함수로 뺀 것) — clause_categories
def clause_categories(masked_clause, rules):
    cats = []
    for code, rx in CLAUSE_RULES:
        m = rx.search(masked_clause)
        if m:
            cats.append((code, m.start(), m.group(0)))
        if len(cats) == 2:
            break
    return cats
```

- 오류: 없음(규칙 오류는 로딩 때 `RuleError`). 엣지: 빈 라벨은 `OTHER`; 같은 키워드가 두 규칙에 걸려도 첫 규칙; 서술 mention이 여러 목록 항목과 겹치면 목록 순서상 첫 항목.
- 자원: 목록 항목 4.4개/인물 평균, 병합은 인물당 O(목록×서술 mention).
- 테스트 명세:

| 테스트 | 입력 | 기대 |
|---|---|---|
| `test_clause_categories` | '퇴근 후에는 성북구 동네 목욕탕에서 뜨거운 탕에 몸을 담그며' | [PERSONAL] (WORK 아님) |
| | '스마트폰 앱으로 피자를 주문해 식사를 즐깁니다' | [MEAL] (ERRAND 아님: '스마트폰') |
| | '송파구 일대 부동산 시장을 분석하며' | [] ('시장'이 ERRAND로 안 걸림) |
| | '이수영 씨는 서울에서 태어나 꼼꼼한 성격으로' | [] (이름 '수영' 마스킹) |
| | '오후에는 거실 소파에서 나훈아의 노래를 틀어놓고 짧은 낮잠을 자며' | [REST, HOBBY] |
| | '주말이면 가족들과 함께 신세계백화점 강남점에서 가벼운 쇼핑을 즐깁니다' | [ERRAND] |
| | '친구들과 함께 경복궁이나 창덕궁 같은 고궁을 거닐며' | [CULTURE, EXERCISE] |
| | '고양이를 돌보며 하루를 시작하고' | [CARE] |
| | '틈틈이 청주를 한 잔 곁들입니다' | [] |
| `test_label_category` | 북한산 둘레길 산책 / 유튜브 여행 영상 시청 / 리그 오브 레전드 게임 / 3쿠션 당구 / 베란다 가드닝 / 필름 카메라 출사 | EXERCISE / HOBBY / HOBBY / HOBBY / HOBBY / HOBBY |
| | K리그 경기 직관 / 바버샵 그루밍 / 동네 단골 일식집 방문 / 전국 노포 맛집 탐방 / 동네 분식집 순례 및 신메뉴 시식 | CULTURE / PERSONAL / MEAL / MEAL / MEAL |
| | 해외 휴양지 여행 / 가족 단위 캠핑 / 오래된 친구와의 전화 수다 / 단골 미용실 외모 관리 / 고궁 및 역사 유적지 탐방 / 나훈아 음악 감상 / 창덕궁 후원 산책 | TRIP / TRIP / SOCIAL / PERSONAL / CULTURE / HOBBY / CULTURE |
| `test_role_mention_without_keyword` | '매일 아침 8시 정각에 출근해' | `SLEEP/WORK` 아닌 `WORK` mention, `role = WORK_START`, 시각 A 480 |
| `test_no_mention_without_cue` | '혈압 관리를 위해 요가를 합니다'(단서 없음) | 서술 mention 없음 |
| `test_merge_list_item` | 서술 '매주 정해진 요일에 단골 미용실을 찾아…' + 목록 '단골 미용실 외모 관리' | 목록 항목이 `freq_kind=WEEKLY`를 받고 `merged_from` 채워짐, 서술은 `excluded`·`DUP:` |
| `test_merge_ignores_generic_tokens` | 목록 '목동 근린공원 천천히 걷기' + 서술 '…천천히 거닐며'(같은 EXERCISE) | 병합되지 않음('천천히'는 불용어) |
| `test_trip_in_seoul_sentence` | '창덕궁의 후원을 천천히 걸으며, … 정적인 여행을 즐깁니다'(한 문장) | OUTSIDE mention 0개 |

- DoD: 표 green, E04-6 통과. 취미 목록 전수에서 `OTHER` 비율 ≤ 10%(프로토타입 실측 5.5%)를 확인하는 `test_other_ratio_corpus`(실제 원본, 느린 테스트로 분리).

## I-04-e — 장소 판정과 근무지 단서

- 추적: [C-04](01_concept_map.md#c-04--활동과-시간-단서-추출) 기준 ④, 테스트 E04-4. 관련 결정 [DR-12](decisions.md#dr-12--서울-밖-활동의-범위와-판정), [DR-13](decisions.md#dr-13--장소-해석과-지리-상수).
- 위치: `engine/cues/places.py` (사전 객체는 [I-06-a](#i-06-a--지리-상수-3종)의 `Gazetteer`).
- 시그니처:

```python
@dataclass(frozen=True)
class PlaceInfo:
    scope: str            # SEOUL_DISTRICT | SEOUL_UNSPECIFIED | OUTSIDE_SEOUL | UNRESOLVED | NONE
    token: str | None; districts: tuple[str, ...]; explicit: bool    # explicit = 구 이름이 문장에 직접 나옴
def classify_place(masked_text: str, kw_pos: int | None, gaz: Gazetteer, rules: RuleSet) -> PlaceInfo
```

- 로직(참조 구현의 `place_info`): ① 사전 hit를 모은다 — `Gazetteer.find`(길이 긴 토큰 우선 단일 alternation, `guard`로 동음이의 제외, `ctx=TRIP`는 같은 절에 `trip_context`가 있을 때만), 구 이름 정규식(`서울 `+`강남`~`중`+`구`), `OO구민체육센터`. ② hit가 있으면: 서울 밖과 서울 hit가 **둘 다** 있고 활동 키워드 위치가 있으면 키워드에 **가장 가까운** hit를 택한다(동률은 서울 밖); 서울 hit만이면 `(일반어 여부, 후보 구 수, 키워드와의 거리, 명시 구 이름 우선)` 순으로 가장 구체적인 hit를 택한다(후보 1개 > 2~3개 > 많은 구 > 후보 없는 일반어). ③ hit가 없으면: `trip_clause`('여행·캠핑·드라이브·나들이·당일치기·펜션·리조트·글램핑·템플스테이·휴양')가 있고 서울 hit가 없으면 `OUTSIDE_SEOUL`(토큰 = 여행어). ④ 아니면 `place_like`('…공원·산·천·호수·시장·궁·역·도서관·미술관·박물관·체육센터·복지관·둘레길·숲길·해변·계곡·섬')에 걸리지만 사전에 없으면 `UNRESOLVED`. ⑤ 아니면 `NONE`. **접미사로 서울 밖을 추정하지 않는다**('여의도', '…거리'는 접미사 추정이 틀린다; [S-13](#self-결정-기록)).
```python
# 참조 구현(검증용 프로토타입 소스 그대로) — load_gaz, place_info
def load_gaz(path):
    rows = list(csv.DictReader(open(path, encoding="utf-8")))
    return {r["token"]: (r["scope"], [d for d in r["districts"].split("|") if d], r.get("guard", ""), r.get("ctx", "")) for r in rows}


def place_info(c, kw_pos):
    """returns (scope, token, districts). kw_pos = char position of the activity keyword (or None)"""
    hits = []                                   # (start, end, token, scope, districts, explicit)
    for m in GAZ_RE.finditer(c):
        scope, ds, _g, ctx = GAZ[m.group(0)]
        if ctx == "TRIP" and not TRIP_CTX.search(c):
            continue
        hits.append((m.start(), m.end(), m.group(0), scope, ds, False))
    for m in DIST_RE.finditer(c):
        hits.append((m.start(), m.end(), m.group(0), "SEOUL", [m.group(1) + "구"], True))
    for m in DIST_SPORT_RE.finditer(c):
        hits.append((m.start(), m.end(), m.group(0), "SEOUL", [m.group(1) + "구"], True))
    if hits:
        def rank(h):
            dist = abs(h[0] - kw_pos) if kw_pos is not None else h[0]
            if h[3] == "OUTSIDE":
                return (0, 0, dist)
            spec = 99 if not h[4] else len(h[4])
            return (1, spec if spec != 1 else 0, dist, 0 if h[5] else 1)
        has_out = any(x[3] == "OUTSIDE" for x in hits)
        has_seoul = any(x[3] == "SEOUL" for x in hits)
        if has_out and has_seoul and kw_pos is not None:
            hits.sort(key=lambda x: (abs(x[0] - kw_pos), 0 if x[3] == "OUTSIDE" else 1))
            h = hits[0]
            if h[3] == "SEOUL":
                seoul_hits = sorted([x for x in hits if x[3] == "SEOUL"], key=rank)
                h = seoul_hits[0]
        else:
            hits.sort(key=rank)
            h = hits[0]
        if h[3] == "OUTSIDE":
            return "OUTSIDE_SEOUL", h[2], [], False
        return ("SEOUL_DISTRICT" if h[4] else "SEOUL_UNSPECIFIED"), h[2], h[4], h[5]
    t = TRIP.search(c)
    if t and not any(h[3] == "SEOUL" for h in hits):
        return "OUTSIDE_SEOUL", t.group(0), [], False
    pl = PLACE_LIKE.search(c)
    if pl:
        return "UNRESOLVED", pl.group(0), [], False
    return "NONE", None, [], False
```

- 근무지 단서: 직업이 있는 인물(`occupation != '무직'`)의 `professional_persona` 절에서 처음 나오는 `SEOUL_DISTRICT` 판정(후보 구 1~3개)을 `workplace_hint`로 기록한다([I-04-f](#i-04-f--인물-단위-추출과-출력-파일)). 실측으로 직업 있는 인물의 약 40%(12,635명 중 5,052명)가 갖는다. 참조 구현(`extract_person`)은 직업 여부를 보지 않고 모든 인물의 힌트를 계산하므로, 구현은 `occupation == '무직'`이면 이 검사를 건너뛰고 `workplace_hint = None`으로 둔다.
- 오류: 없음. 엣지: 일반어('동네', '집 근처', '서울')는 `SEOUL_UNSPECIFIED`; 서울 안 지명과 서울 밖 지명이 한 문장에 섞이면 활동 키워드에 가까운 쪽; 구 이름이 본인 거주 구와 같아도 판정은 같다.
- 자원: 절당 사전 정규식 1회, 마이크로초 단위.
- 테스트 명세:

| 입력 (kw_pos) | 기대 `scope` : `token` (후보 구) |
|---|---|
| 퇴근 후에는 성북구 동네 목욕탕에서 … | SEOUL_DISTRICT : 성북구 (성북구), explicit |
| 송파구 일대 부동산 시장을 분석하며 | SEOUL_DISTRICT : 송파구, explicit |
| 집 근처 공원을 가볍게 산책하며 | SEOUL_UNSPECIFIED : 집 근처 |
| 주말이면 가평의 숲길이나 양평의 강변을 걸으며 | OUTSIDE_SEOUL |
| 강서구 식물원이나 한강 변을 걸으며 | SEOUL_DISTRICT : 강서구 (강서구), explicit(12개 구인 '한강'보다 우선) |
| 평소에는 좋아하는 일본 만화책을 쌓아두고 읽으며 | NONE ('일본'은 여행 문맥이 없어 무시) |
| 휴가철에는 가족들과 함께 일본 유후인의 온천 마을로 여행을 떠나 | OUTSIDE_SEOUL : 일본 |
| 틈틈이 청주를 한 잔 곁들입니다 / 고양이를 돌보며 하루를 시작하고 | NONE / NONE (동음이의 guard) |
| 전국의 숨은 노포를 찾아다니는 식도락 여행을 떠나며 | OUTSIDE_SEOUL : 전국 |
| 가끔은 남한산성 둘레길을 오르거나 잠실의 피부관리 숍에서 시간을 보내며 (kw=둘레길) | OUTSIDE_SEOUL : 남한산성 |
| 같은 문장 (kw=피부관리) | SEOUL_DISTRICT : 잠실 (송파구) |
| 서울식물원 산책로를 천천히 걸으며 | SEOUL_DISTRICT : 서울식물원 (강서구) ('서울'보다 긴 토큰 우선) |
| 서울 시내 무작정 버스 투어 | SEOUL_UNSPECIFIED : 서울 |
| 해외 자연 풍경 출사 | OUTSIDE_SEOUL : 해외 |
| 수리산 둘레길을 걸으며 | UNRESOLVED : 수리산 |
| 주말이면 북한산 둘레길을 걸으며 | SEOUL_DISTRICT : 북한산 (종로구, 성북구, 강북구, 도봉구, 은평구) |

  추가: `test_workplace_hint_employed_only` — `professional_persona = "김 씨는 송파구의 작은 회계 사무소에서 장부를 정리합니다."`인 행 둘을 `extract_person`에 넣어 `occupation = "회계 사무원"`이면 `workplace_hint == ("송파구", ["송파구"], True)`, `occupation = "무직"`이면 `None`. `test_classify_longest_token_wins` — `classify_place`가 '서울대공원'을 '서울'보다 긴 토큰으로 보고 `OUTSIDE_SEOUL`로 판정한다(같은 이름의 사전 테스트는 I-06-a에 따로 있다).
- DoD: 표 green, E04-4 통과.

## I-04-f — 인물 단위 추출과 출력 파일

- 추적: [C-04](01_concept_map.md#c-04--활동과-시간-단서-추출) 기준 ①②③④⑤⑥ 전체와 E04-1(모든 UUID 처리).
- 위치: `engine/cues/extract.py`.
- 시그니처:

```python
@dataclass
class PersonCues:
    mentions: list[dict]; workplace_hint: tuple[str, list[str], bool] | None    # (토큰, 후보 구, explicit)
    list_parse_failed: bool; korean_clock_hits: int
def extract_person(row: dict, rules: RuleSet, gaz: Gazetteer) -> PersonCues
def run_cues(parquet_path: Path, out_dir: Path, rules: RuleSet, gaz: Gazetteer,
             limit: int | None = None, batch_rows: int = 2048) -> dict          # 통계 dict 반환
```

- 입력: `seoul.parquet`의 `uuid`, `occupation`, 서술 11개 필드, `hobbies_and_interests_list`. 필수 열이 없으면 `InputError`.
- 흐름: 서술 필드를 `FIELDS` 순서로 문장 → 절로 나누고, 문장마다 맥락을 새로 시작하며, 절마다 ① 마스킹 ② 시간 단서 ③ 앞 절 맥락 이어받기 ④ 대분류(최대 2)와 역할 ⑤ 장소 판정 ⑥ 모달리티 ⑦ mention 생성 조건 확인 후 기록. 이어서 목록 항목 mention을 만들고 병합한다. 마지막으로 `mention_id`를 0부터 생성 순서대로 붙이고 `excluded`·`usable`을 계산한다. `korean_clock_hits`는 ① 마스킹을 마친 서술 절마다 `time.json.clock_korean_detect_only`가 일치한 횟수를 모두 더한 값이다(목록 항목은 세지 않고, 일치해도 시각 값은 만들지 않는다). `workplace_hint`는 `occupation != '무직'`일 때만 채운다([I-04-e](#i-04-e--장소-판정과-근무지-단서)).
- `usable` = `not excluded and (modality == PRESENT_HABIT or (modality == UNKNOWN and tier == "A"))` — 그리고 서울 밖이 아님.
- 출력 `cues/cues.parquet`(정렬: `person_idx`, `mention_id`; 행 그룹 = 청크 하나):

| 열 | 타입 | 설명 |
|---|---|---|
| `person_idx`, `uuid`, `mention_id` | int32, string, int32 | 원본 행 순서(0부터), 인물 안 생성 순서 |
| `kind`, `field`, `tier` | string | `NARRATIVE`/`LIST_ITEM`, 원본 열, `A`/`B`(목록 항목은 `A`) |
| `sentence_idx`, `clause_idx`, `char_start`, `char_end` | int16, int16, int32, int32 | 서술 절의 위치(원문 코드포인트 오프셋). 목록 항목은 `-1`, 항목 번호, `-1`, `-1` |
| `clause_text` | string | 절 원문(최대 300자). 목록 항목은 항목 문자열 |
| `activity_code`, `activity_label`, `role` | string, string, string? | 15개 코드(MOVE 제외), 일치어 또는 항목 문자열, 역할 |
| `modality`, `day_scope`, `weekdays`, `freq_kind`, `freq_min`, `freq_max` | string, string, list<int8>, string, float32?, float32? | 단서 |
| `daypart`, `clock_min`, `time_grade`, `inherited` | string?, int16?, string, list<string> | 단서 |
| `place_scope`, `place_token`, `place_districts`, `place_explicit` | string, string?, list<string>, bool | 장소(구는 짧은 이름) |
| `excluded`, `excluded_reason`, `dup_of`, `merged_from`, `usable` | bool, string?, int32?, int32?, bool | `OUTSIDE_SEOUL:<토큰>`, `DUP:<id>` |

- 출력 `cues/cue_persons.parquet`(인물당 정확히 1행 — E04-1): `person_idx`, `uuid`, `n_mentions`, `n_usable`, `n_excluded_outside`, `n_excluded_dup`, `n_modality_excluded`(PAST·ASPIRATION·NEGATION·HYPOTHETICAL), `n_unresolved`, `has_outside_nontravel`(서울 밖 mention이 `travel_persona` 밖에도 있음), `workplace_hint_districts` list<string>, `workplace_hint_token` string?, `workplace_hint_explicit` bool, `list_parse_failed` bool, `korean_clock_hits` int16.
- 오류: 필수 열 누락·타입 불일치 `InputError`. 한 인물에서 예외가 나면 그 인물의 uuid를 메시지에 넣어 `RunError`로 올린다(조용히 건너뛰지 않는다). 목록 파싱 실패는 기록만 하고 계속.
- 엣지: 모든 서술 필드가 빈 인물 → `cue_persons`에 `n_mentions` = 목록 항목 수(0일 수 있음)로 1행; 서울 행이 `--limit`보다 적어도 정상; 같은 uuid 중복 → `InputError`.
- 자원: 인물당 1.1~2.4ms(프로토타입 실측, 20,000명 기준), 서울 전원 약 3~8분, 메모리는 배치 2,048명 + Parquet 쓰기 버퍼 1GiB 미만. 서울 전원 실행은 `memguard run`.
- 픽스처 `tests/fixtures.EXAMPLE_ROW`: 원본(NVIDIA Nemotron-Personas-Korea, CC BY 4.0, 합성 페르소나) UUID `89eca80b88284888ad94c84f56777680`의 필드 6개를 그대로 옮긴 것이고 나머지 서술 필드는 빈 문자열이다.

```python
EXAMPLE_ROW = {
    "uuid": "89eca80b88284888ad94c84f56777680",
    "hobbies_and_interests": "매주 정해진 요일에 단골 미용실을 찾아 머리를 단정하게 다듬고, 친구들과 함께 경복궁이나 창덕궁 같은 고궁을 거닐며 역사 기록을 살핍니다. 오후에는 거실 소파에서 나훈아의 노래를 나직하게 틀어놓고 짧은 낮잠을 자며 하루의 긴장을 풉니다.",
    "hobbies_and_interests_list": "['고궁 및 역사 유적지 탐방', '나훈아 음악 감상', '단골 미용실 외모 관리', '오래된 친구와의 전화 수다', '목동 근린공원 천천히 걷기']",
    "sports_persona": "안상식 씨는 정해진 시간에 집 근처 공원을 천천히 거닐며 나무의 상태를 살피고, 무리한 운동보다는 가벼운 산책으로 하루의 활동량을 채우는 정적인 생활을 유지합니다.",
    "travel_persona": "안상식 씨는 고등학교 동창들과 함께 창덕궁의 후원을 천천히 걸으며 왕실의 기록을 살피고, 역사적 사건이 깃든 장소에서 옛이야기를 나누는 정적인 여행을 즐깁니다.",
    "professional_persona": "안상식 씨는 퇴직 후에도 목동 주민센터의 복잡한 서류 뭉치를 보며 흐뭇해하며, 동네 이웃들이 쩔쩔매는 민원 서류 절차를 막힘없이 짚어주는 해결사 역할을 자처합니다.",
    "persona": "안상식 씨는 목동에서 평생을 성실함과 규칙으로 무장해 온, 꼼꼼한 가계부와 나훈아의 노래가 어울리는 정갈한 노신사입니다.",
    "occupation": "무직", "age": 73, "military_status": "비현역", "family_type": "배우자와 거주", "district": "서울-양천구",
}
```

- 테스트 명세:

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_example_persona` | `tests/fixtures.EXAMPLE_ROW`(UUID `89eca80b…`의 실제 서술 5개 필드·목록) | `extract_person` | 서울 밖 mention 0개; `PERSONAL`(단골 미용실) 목록 항목이 `WEEKLY`를 받음; `REST`(낮잠) 서술 mention의 `daypart = AFTERNOON`; `CULTURE` mention의 `place_districts = (종로구,)`; `HOBBY`(나훈아 음악) 목록 항목이 `AFTERNOON`을 받음 |
| `test_every_person_has_cue_person_row` | 합성 인물 30명(서술이 모두 빈 인물 포함) | `run_cues` | `cue_persons` 30행, uuid 집합이 입력과 같음 (E04-1) |
| `test_mention_ids_dense_and_ordered` | 위 출력 | 읽기 | 인물별 `mention_id`가 0..n-1 연속, 정렬됨 |
| `test_usable_definition` | 모달리티·tier 조합 표 | 계산 | 정의와 일치(Tier B UNKNOWN은 false) |
| `test_excluded_have_reason` | 서울 밖·병합 mention | 읽기 | `excluded`이면 `excluded_reason`이 `OUTSIDE_SEOUL:`/`DUP:`로 시작 |
| `test_list_parse_failure_is_recorded` | 목록 필드가 `"abc"`인 인물 | `run_cues` | 중단 없이 `list_parse_failed = true` |
| `test_missing_column` | `uuid` 열이 없는 Parquet | `run_cues` | `InputError` |
| `test_evidence_span_roundtrip` | 서술 mention | `field_text[char_start:char_end].strip()` | `clause_text`와 같음 (E04-2) |
| `test_run_cues_deterministic` | 같은 입력 두 번 | `run_cues` | 두 `cues.parquet`의 바이트가 같음 |
| `test_corpus_smoke_20k` (느린 테스트) | 실제 원본 앞 20,000명 | `run_cues` | 인물당 평균 mention 12~16개, A/B 등급 인물 0.5~1.5%, 예외 없음 |

- DoD: 위 테스트 green, 서울 전원 실행의 `cue_persons` 행 수 = 185,228(E04-1), 통계가 `report/diagnostics.json`에 들어감.

```python
# 참조 구현(검증용 프로토타입 소스 그대로) — extract_person
def extract_person(row):
    mentions = []
    for field, tier in FIELDS:
        text = row.get(field) or ""
        for s0, sent in spans(SENT_BREAK, text):
            cur = {"day_scope": "ANY", "weekdays": [], "freq_kind": "NONE", "freq": None, "daypart": None, "clock": None, "grade": "N"}
            clauses = list(spans(CLAUSE_BREAK, sent))
            sent_seoul = any(place_info(mask(cl), None)[0] in ("SEOUL_DISTRICT", "SEOUL_UNSPECIFIED") for _c0, cl in clauses)
            for c0, clause in clauses:
                mc = mask(clause)
                cues = inherit(cur, time_cues(mc))
                cur = {**cur, **{k: cues[k] for k in ("day_scope", "weekdays", "daypart")}}
                cats = []
                for code, rx in CLAUSE_RULES:
                    m = rx.search(mc)
                    if m:
                        cats.append((code, m.start(), m.group(0), None))
                    if len(cats) == 2:
                        break
                role = cues["role"]
                if role:
                    rm = dict(ROLES)[role].search(mc)
                    cats.insert(0, ("SLEEP" if role in ("WAKE", "BED") else "WORK", rm.start(), rm.group(0), role))
                scope0, tok0, ds0, ex0 = place_info(mc, cats[0][1] if cats else None)
                if not cats and scope0 == "OUTSIDE_SEOUL":
                    if sent_seoul and TRIP.fullmatch(tok0 or ""):
                        continue                     # a bare trip word inside a sentence that names a Seoul place
                    cats = [("OTHER", 0, tok0 or "", None)]
                if not cats:
                    continue
                mod = modality(mc, tier, cues)
                for code, pos, kw, mrole in cats:
                    scope, tok, ds, explicit = place_info(mc, pos)
                    has_cue = cues["grade"] != "N" or mrole is not None or scope != "NONE"
                    if not has_cue:
                        continue
                    mentions.append({"field": field, "tier": tier, "kind": "NARRATIVE", "code": code, "kw": kw, "clause": clause.strip(),
                                     "modality": mod, "scope": scope, "ptoken": tok, "districts": ds, "explicit": explicit, **{k: cues[k] for k in ("day_scope", "weekdays", "freq_kind", "freq", "daypart", "clock", "grade", "inherited")}, "role": mrole})
    try:
        items = ast.literal_eval(row["hobbies_and_interests_list"])
    except Exception:
        items = []
    for it in items:
        code = label_category(it)
        scope, tok, ds, explicit = place_info(it, None)
        if code == "TRIP":
            if scope in ("SEOUL_DISTRICT", "SEOUL_UNSPECIFIED"):
                code = "CULTURE"
            else:
                code, scope = "OTHER", "OUTSIDE_SEOUL"
        mentions.append({"field": "hobbies_and_interests_list", "tier": "A", "kind": "LIST_ITEM", "code": code, "kw": it, "clause": it, "modality": "PRESENT_HABIT",
                         "scope": scope, "ptoken": tok, "districts": ds, "explicit": explicit, "day_scope": "ANY", "weekdays": [], "freq_kind": "NONE", "freq": None,
                         "daypart": None, "clock": None, "grade": "N", "role": None, "inherited": []})
    hint = None
    for s0, sent in spans(SENT_BREAK, row.get("professional_persona") or ""):
        for c0, clause in spans(CLAUSE_BREAK, sent):
            sc, tk, ds, ex = place_info(mask(clause), None)
            if hint is None and sc == "SEOUL_DISTRICT" and 1 <= len(ds) <= 3:
                hint = (tk, ds, ex)
    row["_workplace_hint"] = hint
    # ---- merge: a LIST_ITEM mention adopts the cues of a NARRATIVE mention of the same category that shares a content token
    narr = [m for m in mentions if m["kind"] == "NARRATIVE"]
    for li in [m for m in mentions if m["kind"] == "LIST_ITEM"]:
        toks = [t for t in re.findall(r"[가-힣A-Za-z0-9]{3,}", li["kw"]) if t not in MERGE_STOP]
        li["merged_with"] = None
        if not toks:
            continue
        for nm in narr:
            if nm["code"] == li["code"] and not nm.get("dup_of") and any(t in nm["clause"] for t in toks):
                for k in ("day_scope", "weekdays", "freq_kind", "freq", "daypart", "clock", "grade", "modality"):
                    li[k] = nm[k]
                if li["scope"] == "NONE":
                    li["scope"], li["ptoken"], li["districts"], li["explicit"] = nm["scope"], nm["ptoken"], nm["districts"], nm["explicit"]
                li["merged_with"] = nm["clause"][:40]
                nm["dup_of"] = li["kw"]
                break
    return mentions
```


## I-05-a — 시간 눈금, 키드 난수, 스키마

- 추적: [C-05](01_concept_map.md#c-05--인물별-활동-계획과-시간-상태) 기준 ①⑥, [C-07](01_concept_map.md#c-07--지역별-10분-프레임) 기준 ④(결정성의 기반). 관련 결정 [DR-17](decisions.md#dr-17--프레임과-시간-규약-및-결정성).
- 위치: `engine/config.py`, `engine/timegrid.py`, `engine/rng.py`, `engine/schema.py`.
- 기술: 표준 라이브러리(`hashlib`, `datetime`, `zoneinfo`, `dataclasses`), pyarrow 24.0.0(스키마).
- `config.py`: `SLOT_MIN = 10`, `SLOTS_PER_DAY = 144`, `DAY_MIN = 1440`, `CHUNK_PERSONS = 5000`, `KST = ZoneInfo("Asia/Seoul")`, 경로 상수(`ROOT`, `SEOUL_PARQUET = data/nemotron-personas-korea/seoul.parquet`, `METADATA_JSON = data/nemotron-personas-korea/metadata.json`, `RULES_DIR = rules`, `RUNS_DIR = data/runs`), 예외 계층 `EngineError ← RuleError, InputError, InvariantError, RunError`(종료 코드 3·2·4·5).
- `timegrid.py`: `snap10(m) = m // 10 * 10`; `day_type(date) -> "WEEKDAY" | "WEEKEND"`(`weekday() >= 5`); `iso_key(date) -> (iso_year, iso_week)`; `slot_of(m) = m // 10`; `minute_label(m) -> "HH:MM"`(1440 이상은 24:xx 그대로); `frame_times() -> range(0, 1440, 10)`; `ts_of(date, minute) -> datetime`(KST, tz-aware; 일광절약 없음).
- `rng.py`: **`KeyedRng`** — SHA-256으로 시드를 만들고 SplitMix64로 난수를 뽑는다([S-04](#self-결정-기록)). 시드 문자열은 `"\x1f".join([str(seed), *map(str, keys)])`의 UTF-8, 상태 초깃값은 해시 앞 8바이트(빅엔디언). 메서드: `u64()`, `randint(a, b)`(= `a + u64() % (b-a+1)`), `random()`(= `(u64() >> 11) / 2**53`), `step(lo, hi, step=10)`(= `lo + step * randint(0, (hi-lo)//step)`), `choice(seq)`, `sample(seq, k)`(부분 Fisher–Yates, `j = i + u64() % (n-i)`), `weighted(items, weights)`(누적합 비교). 정답 벡터는 [02a_rule_data.md](02a_rule_data.md)의 마지막 절. 참조 구현의 클래스 이름은 `Rng`이고 구현에서는 `KeyedRng`로 부른다.

```python
# 참조 구현(검증용 프로토타입 소스 그대로) — Rng
import hashlib

MASK = (1 << 64) - 1

class Rng:
    """SplitMix64 seeded from sha256(seed, keys...)"""

    def __init__(self, seed, *keys):
        h = hashlib.sha256("\x1f".join([str(seed)] + [str(k) for k in keys]).encode("utf-8")).digest()
        self.s = int.from_bytes(h[:8], "big")

    def u64(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & MASK
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def randint(self, a, b):
        return a + self.u64() % (b - a + 1)

    def random(self):
        return (self.u64() >> 11) / float(1 << 53)

    def step(self, lo, hi, step=10):
        return lo + step * self.randint(0, (hi - lo) // step)

    def choice(self, seq):
        return seq[self.u64() % len(seq)]

    def sample(self, seq, k):
        a = list(seq)
        for i in range(min(k, len(a))):
            j = i + self.u64() % (len(a) - i)
            a[i], a[j] = a[j], a[i]
        return a[:k]

    def weighted(self, items, weights):
        tot = float(sum(weights))
        x = self.random() * tot
        acc = 0.0
        for it, w in zip(items, weights):
            acc += w
            if x < acc:
                return it
        return items[-1]
```

- `schema.py`: `ACTIVITY_CODES`(15개, `activities.json.codes`와 같은 순서), 열거값 상수(`MODALITIES`, `FREQ_KINDS`, `DAYPARTS`, `GRADES`, `PLACE_SCOPES`, `STATE_KINDS`, `SRC_ACTIVITY = {text, structured, assumption}`, `SRC_TIME = {text, assumption}`, `SRC_PLACE = {text, dictionary, structured, assumption}`), `full_name(short)`/`short_name(full)`, 그리고 pyarrow 스키마 5개. `CUE_SCHEMA`와 `CUE_PERSON_SCHEMA`는 [I-04-f](#i-04-f--인물-단위-추출과-출력-파일)의 표와 같다. 나머지:

| 스키마 | 열 (타입) |
|---|---|
| `PERSON_SCHEMA` (`persons.parquet`) | `person_idx` int32, `uuid` string, `residence` string(`서울-양천구`), `age` int16, `occupation` string, `template` string, `workplace_district` string?, `workplace_src` string |
| `SEGMENT_SCHEMA` (`segments/<날짜>/part-*.parquet`) | `person_idx` int32, `start` int16, `end` int16, `kind` string(`ACTIVITY`/`MOVE`), `activity_code` string, `activity_detail` string, `district` string?(ACTIVITY), `from_district` string?, `to_district` string?(MOVE), `src_activity` string, `src_time` string, `src_place` string, `mention_id` int32(없으면 -1), `tag` string, `segment_id` int64 |
| `FRAME_SCHEMA` (`frames/<날짜>.parquet`) | `ts` timestamp[s, tz=Asia/Seoul], `district` string, `uuid` string, `activity_code` string, `activity_detail` string, `state_kind` string(`ACTIVITY`/`MOVE_OUT`/`MOVE_IN`), `segment_id` int64 |

  `segment_id = day_ordinal * 2**32 + row_in_day`. `day_ordinal = (date - date(2000, 1, 1)).days`, `row_in_day` = 그 날짜의 파트 파일들을 (파트 번호, 행 번호) 순으로 이은 0부터의 행 번호.
- 오류: 참조 구현에는 없지만 구현은 `KeyedRng.randint(a, b)`에서 `b < a`이면 `ValueError`, 빈 시퀀스 `choice`와 `lo`/`hi`가 `step`의 배수가 아닌 `step`도 `ValueError`로 막는다. 스키마에 없는 열거값을 쓰면 `InvariantError`(쓰기 시 검증).
- 엣지: 키에 한글·공백이 있어도 UTF-8 그대로 해시; 같은 키 순서가 달라지면 다른 스트림; `step`은 `lo`가 10의 배수일 때만 10의 배수를 낸다(호출자가 보장).
- 자원: `u64()`당 약 0.5µs. 메모리 무시 가능.
- 테스트 명세:

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_rng_golden_vectors` | 02a의 정답 벡터 6줄 | 같은 인자로 호출 | 값이 정확히 같다 |
| `test_rng_streams_independent` | `KeyedRng(1,'a')`, `KeyedRng(1,'b')` | 한쪽만 더 뽑아도 | 다른 쪽 출력 불변, 두 스트림은 서로 다름 |
| `test_rng_bounds_and_step` | 10만 번 `randint(3, 9)`, `step(360, 450)` | 범위·배수 검사 | 모두 범위 안, `step`은 10의 배수 |
| `test_rng_sample_distinct` | `sample(range(7), 3)` | 20회 | 항상 서로 다른 3개, 같은 키면 같은 결과 |
| `test_rng_weighted` | 가중치 `(0, 1)` / `(1, 0)` | 100회 | 항상 둘째 / 첫째 |
| `test_day_type_week` | 2026-11-02(월)~08(일) | `day_type` | 평일 5, 주말 2 |
| `test_iso_key_year_boundary` | 2027-01-01 | `iso_key` | `(2026, 53)` |
| `test_snap10_and_label` | 0, 9, 10, 1439 / 1470 | `snap10`, `minute_label` | 0, 0, 10, 1430 / `"24:30"` |
| `test_schema_roundtrip` | 스키마 5개 | 빈 테이블 쓰기·읽기 | 스키마 동일 |
| `test_district_names` | `양천구` / `서울-양천구` | `full_name`, `short_name` | 서로 역변환 |
| `test_segment_id_formula` | 2026-11-02, 행 7 | `segment_id` | `(date - 2000-01-01).days * 2**32 + 7` |

- DoD: 표 green. 02a 벡터와 정확히 일치.

## I-05-b — 템플릿 선택, 근거 분류, 일별 앵커

- 추적: [C-05](01_concept_map.md#c-05--인물별-활동-계획과-시간-상태) 기준 ②④, 테스트 E05-2·E05-4. 관련 결정 [DR-14](decisions.md#dr-14--구조화-필드의-사용과-근거-우선순위), [DR-19](decisions.md#dr-19--기본-일과-템플릿), [DR-15](decisions.md#dr-15--시간-신호-해석과-추출-방식).
- 위치: `engine/plan/person.py`(`build_person_context`, `usable`, `select_template`), `engine/plan/anchors.py`(`anchors`).
- 시그니처:

```python
def build_person_context(person: dict, cue_rows: list[dict], cue_person: dict | None, rules: RuleSet) -> dict    # 아래 prepare()
def anchors(P: dict, date: datetime.date, seed: int, rules: RuleSet) -> dict
```

- 입력: `person` = 원본 행(`uuid`, `age`, `occupation`, `military_status`, `family_type`, `district`), `cue_rows` = 그 인물의 `cues.parquet` 행(열 이름 ↔ 참조 구현 키 대응은 위 표), `cue_person`(근무지 단서). 참조 구현 `prepare`의 `ms = c04.extract_person(p)`는 `ms = [행을 키로 바꾼 dict …]`로, `hint = p.get("_workplace_hint")`는 `cue_person`의 `workplace_hint_*`로 바꾼다.
- 분류(참조 구현 `prepare`): ① `usable` mention만 쓴다. ② **앵커**: `role`이 있는 usable mention 중 요일 범위가 `ANY` 또는 해당 요일 유형인 것을 등급 A < B < C < 그 밖, `mention_id` 순으로 정렬해 첫 번째를 `(role, 요일 유형)`별로 보관한다. ③ **cued**: 역할이 없고 `SLEEP`·`WORK`가 아니며 빈도·시각·요일 범위 중 하나가 있는 usable mention(일정에 직접 올린다). ④ **pool**: 나머지 usable mention을 대분류별로 모은다(`OTHER`는 `HOBBY`로). 템플릿 칸의 활동 이름을 고르는 후보다. ⑤ 템플릿: 직업 있음(`occupation != '무직'`)이면 `WORK_DAY`(usable `WORK` mention에 야간·교대 근무 문구가 있으면 `SHIFT`), 무직이면 65세 이상 `HOME_65P`, 미만 `HOME_U65`. 현역은 `WORK_DAY`이고 근무지는 거주 구다([DR-19](decisions.md#dr-19--기본-일과-템플릿)). ⑥ **충돌**: 무직인데 usable `WORK_START`·`WORK_END` 앵커 mention이 있으면(현재형 '출근/퇴근') 서술을 우선해 `WORK_DAY`로 바꾸고 `conflict = "JOBLESS_WITH_WORK_CUE"`를 남긴다([DR-14](decisions.md#dr-14--구조화-필드의-사용과-근거-우선순위) ③; 20K 표본에서 무직 중 약 7.6%). 이 사례는 근무지 단서를 쓰지 않는다(무직이므로). 자녀 동거는 `family_type`에 '자녀' 포함. 근무지 단서는 직업 있는 인물만.
- 앵커 규칙(`anchors`, 날짜별 순수 함수): 난수는 `KeyedRng(seed, uuid, iso_year, iso_week, weekday, "anchors")` 하나를 **wake → bed → night_meal → work start** 순서로 쓴다. 기상·취침은 앵커 mention의 등급 A/B 시각을 10분 내림해 쓰고, 시각이 없고 시간대만 있으면 템플릿 범위와 시간대 창의 교집합(교집합이 비면 시간대 창)에서 뽑고, 아무것도 없으면 템플릿 범위에서 뽑는다. `SHIFT`가 아닌 템플릿은 기상을 [240, 720], 취침을 [1200, 1560]으로 자른다(1440 이상 = 다음 날 새벽). 근무 시작은 앵커 `WORK_START`의 시각 또는 템플릿 범위, 순근무 시간은 480분이고 `WORK_END` 시각이 있으면 `max(240, 종료 − 시작 − 60)`. 시작이나 종료를 시각 단서(등급 A/B)에서 가져왔으면 `work_src = "text"`, 아니면 `"assumption"`이며 [I-05-d](#i-05-d--하루-활동-구간-조립)가 WORK 구간의 `st`(src_time)로 쓴다. `SHIFT`는 평일(월~금)에 `fixed_work = (1320, 1800)`(22:00~다음 날 06:00)이다. `night_meal` ∈ [60, 180]은 야간 근무 중 식사 시작(다음 날 01:00~03:00)이다.

```python
# 참조 구현(검증용 프로토타입 소스 그대로) — usable, re_shift, prepare, snap, anchors
def usable(m):
    if m["scope"] == "OUTSIDE_SEOUL" or m.get("dup_of"):
        return False
    if m["modality"] == "PRESENT_HABIT":
        return True
    return m["modality"] == "UNKNOWN" and m["tier"] == "A"


def re_shift(m):
    return bool(c04.re.search(r"야간\s*근무|교대\s*근무", m["clause"]))


def prepare(p, seed):
    ms = c04.extract_person(p)
    for i, m in enumerate(ms):
        m["mid"] = i
    P = dict(uuid=p["uuid"], age=p["age"], occ=p["occupation"], res=p["district"].split("-", 1)[1], military=p.get("military_status", ""),
             family=p.get("family_type", ""), mentions=ms)
    us = [m for m in ms if usable(m)]
    shift = any(m["code"] == "WORK" and m["role"] is None and re_shift(m) for m in us)
    if P["occ"] != "무직":
        P["tpl"] = "SHIFT" if shift else "WORK_DAY"
    else:
        P["tpl"] = "HOME_65P" if P["age"] >= 65 else "HOME_U65"
    P["conflict"] = None
    if P["occ"] == "무직" and any(m["role"] in ("WORK_START", "WORK_END") for m in us):
        P["tpl"] = "WORK_DAY"                       # DR-14: the narrative wins, but the conflict is recorded
        P["conflict"] = "JOBLESS_WITH_WORK_CUE"
    anch = {}
    for role in ("WAKE", "BED", "WORK_START", "WORK_END"):
        c = [m for m in us if m["role"] == role]
        for dtp in (WD, WE):
            ok = [m for m in c if m["day_scope"] in ("ANY", dtp)]
            ok.sort(key=lambda m: ({"A": 0, "B": 1, "C": 2}.get(m["grade"], 3), m["mid"]))
            if ok:
                anch[(role, dtp)] = ok[0]
    P["anch"] = anch
    P["cued"] = [m for m in us if m["role"] is None and m["code"] not in ("SLEEP", "WORK") and (m["freq_kind"] != "NONE" or m["clock"] is not None or m["day_scope"] != "ANY")]
    P["pool"] = collections.defaultdict(list)
    for m in us:
        if m["role"] is None and m not in P["cued"] and m["code"] not in ("SLEEP", "WORK"):
            P["pool"]["HOBBY" if m["code"] == "OTHER" else m["code"]].append(m)
    P["child"] = "자녀" in P["family"]
    hint = p.get("_workplace_hint")
    P["wp_hint"] = hint[1] if (hint and P["occ"] != "무직") else None
    P["wp_explicit"] = bool(hint[2]) if hint else False
    return P


def snap(x):
    return x // 10 * 10


def anchors(P, date, seed):
    wd = date.weekday()
    dtp = WE if wd >= 5 else WD
    iso = date.isocalendar()
    T = TEMPLATES[P["tpl"]][dtp]
    r = Rng(seed, P["uuid"], iso[0], iso[1], wd, "anchors")

    def draw(lo_hi, win=None):
        lo, hi = lo_hi
        if win:
            lo2, hi2 = max(lo, win[0]), min(hi, win[1])
            lo, hi = (lo2, hi2) if lo2 <= hi2 else win
        lo, hi = (lo + 9) // 10 * 10, hi // 10 * 10
        return r.step(lo, max(lo, hi))

    wk = P["anch"].get(("WAKE", dtp))
    bd = P["anch"].get(("BED", dtp))
    wake = snap(wk["clock"]) if wk and wk["clock"] is not None and wk["grade"] in "AB" else draw(T["wake"], DPWIN.get(wk["daypart"]) if wk and wk["daypart"] else None)
    bed = snap(bd["clock"]) if bd and bd["clock"] is not None and bd["grade"] in "AB" else draw(T["bed"], DPWIN.get(bd["daypart"]) if bd and bd["daypart"] else None)
    if P["tpl"] != "SHIFT":
        wake = min(max(wake, 240), 720)
        bed = min(max(bed, 1200), 1560)
    A = dict(dtp=dtp, wd=wd, wake=wake, bed=bed, work=None, fixed_work=None, night_meal=snap(r.step(60, 180)))
    if T["work"]:
        ws, we = P["anch"].get(("WORK_START", dtp)), P["anch"].get(("WORK_END", dtp))
        text_start = bool(ws and ws["clock"] is not None and ws["grade"] in "AB")
        text_end = bool(we and we["clock"] is not None and we["grade"] in "AB")
        start = snap(ws["clock"]) if text_start else draw(T["work"]["start"])
        net = T["work"]["net"]
        if text_end:
            net = max(240, snap(we["clock"]) - start - 60)
        A["work"] = (start, net)
        A["work_src"] = "text" if (text_start or text_end) else "assumption"
    if T["fixed_work"] and wd <= 4:
        A["fixed_work"] = T["fixed_work"]
    return A
```

- 오류: 필수 키 누락 시 `InputError`. 엣지: 앵커 mention의 시각이 범위를 벗어나면 자르고 `diagnostics.anchor_clamped`를 센다; 두 앵커가 같은 등급이면 `mention_id`가 작은 것; 주말만 있는 앵커는 평일에 쓰지 않는다; 직업 있는데 서술에 근무 단서가 전혀 없으면 템플릿 그대로.
- 자원: 인물당 앵커 계산 수 µs.
- 테스트 명세:

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_select_template_matrix` | (무직,34)(무직,64)(무직,65)(직업,50)(현역)(직업+'야간 근무' mention) | `select_template` | `HOME_U65`, `HOME_U65`, `HOME_65P`, `WORK_DAY`, `WORK_DAY`(근무지=거주), `SHIFT` |
| `test_anchor_from_clock` | WAKE 앵커 A 300, `ANY` | 7일 `anchors` | 매일 `wake == 300` |
| `test_anchor_daypart_window` | WAKE 앵커 `DAWN`(시각 없음, 창 [240, 360]), 템플릿 `WORK_DAY` | 평일·주말 | 평일은 템플릿 범위 [360, 450]과의 교집합이 한 점이라 `wake == 360`, 주말(범위 [420, 540])은 교집합이 없어 시간대 창 [240, 360]에서 뽑음 |
| `test_anchor_clamps` | WAKE 100분, BED 100분 앵커 | `anchors` | `wake == 240`, `bed == 1200` |
| `test_anchor_day_scope` | WEEKEND 전용 WAKE 앵커 | 평일·주말 | 평일은 템플릿 범위, 주말만 앵커 |
| `test_anchor_work_end` | WORK_START 480(A), WORK_END 1080(A) | 평일 | 시작 480, 순근무 `1080 - 480 - 60 = 540`, `work_src == "text"` (시각 단서가 없으면 `"assumption"`) |
| `test_anchor_pure` | 같은 입력 | 호출 순서를 바꿔 반복 | 같은 dict |
| `test_shift_fixed_work` | SHIFT 인물, 월~일 | `anchors` | 월~금 `fixed_work == (1320, 1800)`, 토·일 `None` |
| `test_night_meal_range` | 1,000명 | `anchors` | `night_meal ∈ {60, 70, …, 180}` |
| `test_conflict_jobless_with_work_cue` | 무직 + usable `WORK_START` 앵커 mention(A 480) | `build_person_context` | 템플릿 `WORK_DAY`, `conflict == "JOBLESS_WITH_WORK_CUE"`, 평일 조립에 WORK 구간이 있음 (E05-4) |
| `test_cued_vs_pool_partition` | 빈도 있음/시각 있음/요일 범위 있음/아무 단서 없음 mention | `build_person_context` | 앞 셋은 `cued`, 마지막은 `pool`, 역할 mention은 `anch`, 서울 밖·병합·희망 mention은 어디에도 없음 |
| `test_workplace_hint_only_employed` | 무직 + 단서, 직업 + 단서 | `build_person_context` | 무직은 `wp_hint = None`, 직업은 후보 구 리스트 |

- DoD: 표 green, E05-2·E05-4 통과.

## I-05-c — 10분 슬롯 격자와 배치

- 추적: [C-05](01_concept_map.md#c-05--인물별-활동-계획과-시간-상태) 기준 ①②, 테스트 E05-1·E05-2.
- 위치: `engine/plan/grid.py`.
- 규칙: `Grid`는 144개 슬롯에 `None` 또는 spec(dict)을 담는다. `free(a, b)`는 `0 ≤ a`, `b ≤ 1440`이고 `[a, b)`의 모든 슬롯이 비었는지, `put`은 그 구간을 채움, `run_from(a)`는 `a`부터 이어지는 빈 길이(분). `place(grid, spec, dur, window, rng, anchor=None, min_dur=30)`:
  1. **앵커 배치**(`anchor`가 있을 때): 시작 = `snap10(anchor)`. 앵커에서 이어지는 빈 길이 `run`을 구해 `d = min(dur, run)`; `d ≥ min(min_dur, dur)`이고 `d > 0`이면 `[a, a+d)`에 배치, 아니면 실패(`None`). 시작 시각은 절대 옮기지 않는다.
  2. **창 배치**: 원래 길이 `dur`, 안 되면 `min(dur, 30)`(최소 10)으로 **한 번만** 줄여 다시 시도한다. 길이 `d`마다 시작 후보를 `window` 시작(올림해 10 배수)부터 `window 끝 − d`까지 10분 간격으로 만들고, `rng.randint(0, 후보 수 − 1)`로 고른 위치에서 시작해 **한 바퀴 돌며** 처음 비어 있는 곳에 배치한다.
  3. 둘 다 실패면 `None`(호출자가 진단 카운터를 올린다).
- 시그니처: `class Grid: free(a, b) -> bool; put(a, b, spec) -> None; run_from(a) -> int`; `def place(grid, spec, dur, window, rng, anchor=None, min_dur=30) -> tuple[int, int] | None`.

```python
# 참조 구현(검증용 프로토타입 소스 그대로) — Grid, place, dur_min_of
class Grid:
    def __init__(self):
        self.s = [None] * 144

    def free(self, a, b):
        return a >= 0 and b <= 1440 and all(self.s[k] is None for k in range(a // 10, b // 10))

    def put(self, a, b, sp):
        for k in range(a // 10, b // 10):
            self.s[k] = sp

    def run_from(self, a):
        n = 0
        k = a // 10
        while k < 144 and self.s[k] is None:
            n += 10
            k += 1
        return n


def place(grid, sp, dur, window, rng, anchor=None, min_dur=30):
    if anchor is not None:
        a = snap(anchor)
        run = grid.run_from(a)
        d = min(dur, run)
        if d >= min(min_dur, dur) and d > 0:
            grid.put(a, a + d, sp)
            return a, a + d
        return None
    lo, hi = window
    lo = (lo + 9) // 10 * 10
    for d in (dur, max(10, dur_min_of(dur, min_dur))):
        starts = list(range(lo, hi - d + 1, 10))
        if not starts:
            continue
        i0 = rng.randint(0, len(starts) - 1)
        for j in range(len(starts)):
            a = starts[(i0 + j) % len(starts)]
            if grid.free(a, a + d):
                grid.put(a, a + d, sp)
                return a, a + d
    return None


def dur_min_of(dur, min_dur):
    return min(dur, min_dur)
```

- 오류: 참조 구현에는 없지만 구현은 `put`에서 점유 검사를 추가해 이미 찬 슬롯을 덮으려 하면 `InvariantError`를 낸다(방어적 검사. 구간 전체를 먼저 검사해 아무 슬롯도 바꾸지 않은 채 낸다). 엣지: 창이 길이보다 짧으면 `None`; 앵커가 이미 찬 슬롯이면 `None`; 앵커 뒤 빈 길이가 10분뿐이고 `dur ≥ 30`이면 `None`; 끝 경계는 1440까지 허용.
- 자원: 슬롯 144개, 배치 1회 O(144).
- 테스트 명세 (`X`, `Y`는 서로 다른 임의의 spec dict, 난수는 `KeyedRng(1, "t")`, `place`의 `window`는 분 단위 (시작, 끝)):

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_grid_free_put_run` | 빈 `Grid`에 `put(0, 60, X)` | `free(0, 60)`, `free(60, 120)`, `run_from(0)`, `run_from(60)`, `free(1430, 1450)` | `False`, `True`, `0`, `1380`, `False`(끝 경계 1440을 넘음) |
| `test_place_anchor_shrinks_to_free_run` | `put(640, 700, X)` | `place(g, Y, 90, (0, 1440), rng, anchor=600)` | `(600, 640)`, 슬롯 60~63이 `Y` (앵커 뒤 빈 길이가 40이라 `d = 40 ≥ min(30, 90)`) |
| `test_place_anchor_fails_on_occupied` | `put(600, 660, X)` | `place(g, Y, 60, (0, 1440), rng, anchor=600)` | `None`, 격자 변화 없음 |
| `test_place_anchor_too_short` | `put(610, 700, X)` (앵커 뒤 빈 길이 10) | `place(g, Y, 60, (0, 1440), rng, anchor=600)` | `None`, 슬롯 60은 비어 있음 |
| `test_place_window_rotation_deterministic` | ① 빈 격자 둘 ② `put(600, 660, X)`한 격자 | `place(g, Y, 60, (600, 720), KeyedRng(1, "t"))` — ①은 두 격자에 각각, ②는 시드를 0~49로 바꿔 50번 | ① 두 결과가 같다 ② 언제나 `(660, 720)` (후보 시작 600~660 중 660만 비어 있어 한 바퀴 돌아 찾음) |
| `test_place_window_shrinks_once` | 빈 격자 | `place(g, Y, 120, (600, 630), rng)` | `(600, 630)` (120이 안 들어가 30으로 한 번 줄여 배치) |
| `test_place_window_too_small` | 빈 격자 | `place(g, Y, 120, (600, 620), rng)`, `place(g, Y, 60, (600, 600), rng)` | 둘 다 `None`, 격자 변화 없음 (30으로 줄여도 20분 창에 안 들어가고 더 줄이지 않음; 빈 창) |
| `test_place_end_boundary` | 빈 격자 | `place(g, Y, 30, (1410, 1440), rng)` | `(1410, 1440)` |
| `test_put_overlap_raises` | `put(600, 660, X)` | `put(630, 690, Y)` | `InvariantError`, 슬롯 66~68은 비어 있는 그대로 (쓰기 전에 검사) |
- DoD: 위 테스트 green.

## I-05-d — 하루 활동 구간 조립

- 추적: [C-05](01_concept_map.md#c-05--인물별-활동-계획과-시간-상태) 기준 ①②③④⑤⑥, 테스트 E05-1~E05-6. 관련 결정 [DR-14](decisions.md#dr-14--구조화-필드의-사용과-근거-우선순위), [DR-15](decisions.md#dr-15--시간-신호-해석과-추출-방식) ⑤, [DR-17](decisions.md#dr-17--프레임과-시간-규약-및-결정성), [DR-19](decisions.md#dr-19--기본-일과-템플릿).
- 위치: `engine/plan/compose.py`.
- 시그니처: `def compose_day(P: dict, date: datetime.date, seed: int, rules: RuleSet, log: Counter) -> tuple[list[dict], dict]`  (반환: 활동 구간 목록, 전날 앵커 `Pv`). 구간 dict 키: `start`, `end`, `kind="ACT"`, `code`, `detail`, `mid`, `tag`, `sa`(src_activity), `st`(src_time), `sp`(원 spec).
- 처리 순서(우선순위 순; 앞 단계가 채운 슬롯은 뒤 단계가 덮지 못한다). 난수는 `KeyedRng(seed, uuid, iso_year, iso_week, weekday, "compose")` 하나를 이 순서대로 쓴다.
  1. **수면과 이어짐**: 비교대 템플릿은 어제 취침이 1440을 넘으면 오늘 `[0, 취침−1440)`을 `취침 전 여가`(HOBBY, tag `late`)로 두고 그 뒤부터 기상까지 `SLEEP`; 어제 취침이 1440 이하이면 `[0, 기상)`이 `SLEEP`. 오늘 취침이 1440 미만이면 `[취침, 1440)`도 `SLEEP`. 수면 길이는 5~10시간으로 기상을 보정하고(`기상 ∈ [어제 취침 − 1140, 어제 취침 − 840]`을 다시 [240, 720]으로 자름; 바뀌면 `sleep_clamped`), `SHIFT`는 같은 날 `[취침, 기상)`(낮잠 아닌 낮 수면)이다. 어제 `fixed_work`가 1440을 넘으면 오늘 `[0, night_meal)` WORK, `[night_meal, +30)` MEAL(`식사(근무 중)`), 이어서 끝까지 WORK. 오늘 `fixed_work`가 있으면 `[시작, 1440)` WORK.
  2. **시각이 있는 cued 일정**(등급 A/B, 오늘 요일이 `weekly_days`에 포함): `mention_id` 순서로 앵커 배치. 길이는 `rng.step(대분류 기본 길이)`. 실패하면 `unmet`.
  3. **식사**: 아침(기상 + 30~60분 시작, 30분; 실패하면 창 배치), 점심(12:00~13:00 시작, 근무일 60분·그 밖 30~60분), 저녁(18:00~19:30 시작, 60분). 이미 그 시간대에 MEAL이 있으면 건너뛴다(cued 식사와 중복 방지). `SHIFT`는 점심 대신 근무 시작 전 4시간 안에 한 번.
  4. **근무**: 시작 슬롯부터 비어 있는 슬롯만 `순근무` 분이 찰 때까지 WORK로 채운다(점심 등 이미 찬 슬롯은 건너뜀). 모자라면 `work_short`.
  5. **시각 없는 cued 일정**(시간대 C, 빈도 D): 시간대 창(없으면 12:00~21:00)에 창 배치.
  6. **템플릿 유연 블록**(`templates.json`의 `flex` 순서대로; 희귀한 블록이 앞): 주당 횟수가 `EVERY`면 해당 요일 유형의 모든 날, 아니면 `KeyedRng(seed, uuid, iso_year, iso_week, "flex", 블록 번호)`로 해당 요일 유형 날짜에서 `randint(lo, hi)`일을 `sample`. `OUTING`은 `ERRAND`/`SOCIAL`/`CULTURE` 중 pool 크기 + 1로 가중 추첨, `NAP`은 `REST`. 활동 이름은 `pick_pool`(요일 범위가 맞는 pool mention, 시간대가 블록 창과 겹치면 가중치 3배)로 고르고, pool이 비면 기본 이름(`default_label`)과 `src_activity = assumption`. 길이는 `rng.step(dur)`.
  7. **자녀 돌봄**: 자녀 동거이고 평일이며 `SHIFT`가 아니면 `CARE` 60~120분을 [18:30, 21:30)에 배치(`src_activity = structured`).
  8. **채우기**: 남은 빈 구간마다 ① 구간이 기상 시각에서 시작하고 30분 이상이면 앞 30분은 `PERSONAL`(`기상 후 준비`) ② 구간이 취침 시각에서 끝나고 30분 이상 남으면 뒤 30분은 `PERSONAL`(`취침 준비`) ③ 남은 길이가 20분 이하이면 **바로 앞 슬롯의 활동에 흡수**(없으면 바로 뒤) ④ 30분이면 `REST`(`휴식`) ⑤ 그보다 길면 `HOBBY`(`자유 시간(기본)`). 모두 `assumption`.
  9. **구간화**: 연속 슬롯에서 같은 spec(또는 같은 `code`·`detail`·`mid`·`tag`)을 하나의 구간으로 합친다.
- 주간 배정(`weekly_days`): 난수 `KeyedRng(seed, uuid, iso_year, iso_week, "wk", mention_id)`. 후보 요일 = `weekdays`가 있으면 그것, 없으면 `day_scope`(WEEKEND [5,6], WEEKDAY [0..4], ANY 전체). 일수 `n` = `DAILY` 전부, `ALMOST_DAILY` `min(후보 수, randint(5,6))`, `PER_WEEK` `min(후보 수, randint(min,max))`, `WEEKLY` 1, `OCCASIONAL` `randint(0,1)`, `PROB_WEEK` `random() < min`이면 1 아니면 0, 빈도 없음 `min(후보 수, randint(1,2))`([S-07](#self-결정-기록)). 뽑을 요일은 `rng.sample(후보, n)`이고 **조립하는 날짜와 무관하게 그 주 전체에서 한 번** 정해지므로 어느 날부터 시작해도 같다.
- 출처 라벨: cued는 `sa = text`, `st = text`(시각 또는 시간대 단서가 있을 때) 아니면 `assumption`; pool로 이름을 고른 칸은 `sa = text`, `st = assumption`; 근무는 `sa = structured`; 나머지 기본값은 `assumption`.
- 템플릿 JSON 키와 참조 구현 리터럴의 대응: 점심 시작 `meal.lunch_start`=[720,780], 저녁 시작 `meal.dinner_start`=[1080,1170], 아침 `meal.breakfast_after_wake`=[30,60], 수면 `sleep.min_minutes/max_minutes/wake_clamp/bed_clamp`, 채우기 `filler.*`, 야간 식사 `night_meal`, 자녀 돌봄 `child_care`, pool 가중 `pool_daypart_weight`. 구현은 리터럴 대신 이 키를 읽는다.

```python
# 참조 구현(검증용 프로토타입 소스 그대로) — spec, weekly_days, label_of, pick_pool, compose_day
def spec(code, detail, sa="assumption", st="assumption", mid=-1, tag=""):
    return dict(code=code, detail=detail, sa=sa, st=st, mid=mid, tag=tag)


def weekly_days(P, m, iso, seed):
    r = Rng(seed, P["uuid"], iso[0], iso[1], "wk", m["mid"])
    elig = m["weekdays"] if m["weekdays"] else ([5, 6] if m["day_scope"] == "WEEKEND" else ([0, 1, 2, 3, 4] if m["day_scope"] == "WEEKDAY" else list(range(7))))
    k, fr = m["freq_kind"], m["freq"]
    if k == "DAILY":
        n = len(elig)
    elif k == "ALMOST_DAILY":
        n = min(len(elig), r.randint(5, 6))
    elif k == "PER_WEEK":
        n = min(len(elig), r.randint(int(fr[0]), int(fr[1])))
    elif k == "WEEKLY":
        n = 1
    elif k == "OCCASIONAL":
        n = r.randint(0, 1)
    elif k == "PROB_WEEK":
        n = 1 if r.random() < fr[0] else 0
    else:
        n = min(len(elig), r.randint(1, 2))
    return set(r.sample(elig, n))


def label_of(m):
    if m["kind"] == "LIST_ITEM":
        return m["kw"]
    pre = (m["ptoken"] + " ") if m["ptoken"] and m["scope"] in ("SEOUL_DISTRICT", "SEOUL_UNSPECIFIED") else ""
    return pre + m["kw"]


def pick_pool(P, code, window, dtp, rng):
    items = [m for m in P["pool"].get(code, []) if m["day_scope"] in ("ANY", dtp)]
    if not items:
        return None
    ws = []
    for m in items:
        w = 1.0
        if m["daypart"] and window:
            a, b = DPWIN[m["daypart"]]
            if a < window[1] and b > window[0]:
                w = 3.0
        ws.append(w)
    return rng.weighted(items, ws)


def compose_day(P, date, seed, log):
    A, Pv = anchors(P, date, seed), anchors(P, date - dt.timedelta(days=1), seed)
    wd, dtp = A["wd"], A["dtp"]
    iso = date.isocalendar()
    T = TEMPLATES[P["tpl"]][dtp]
    g = Grid()
    r = Rng(seed, P["uuid"], iso[0], iso[1], wd, "compose")
    SL = spec("SLEEP", "수면")
    # ---- sleep + carry-in
    if P["tpl"] == "SHIFT":
        g.put(A["bed"], A["wake"], SL)
    else:
        wake, bed_prev = A["wake"], Pv["bed"]
        lo_w, hi_w = bed_prev - 1440 + 300, bed_prev - 1440 + 600          # sleep length 5h..10h
        clamped = min(max(wake, lo_w), hi_w)
        clamped = min(max(clamped, 240), 720)
        if clamped != wake:
            log["sleep_clamped"] += 1
        wake = clamped
        s0 = 0 if bed_prev <= 1440 else bed_prev - 1440
        if s0 > 0:
            g.put(0, s0, spec("HOBBY", "취침 전 여가", tag="late"))
        g.put(s0, wake, SL)
        if A["bed"] < 1440:
            g.put(A["bed"], 1440, SL)
        A["wake"] = wake
    if Pv["fixed_work"] and Pv["fixed_work"][1] > 1440:
        end = Pv["fixed_work"][1] - 1440
        nm = Pv["night_meal"]
        WK = spec("WORK", "근무", sa="structured", tag="night")
        g.put(0, nm, WK)
        g.put(nm, nm + 30, spec("MEAL", "식사(근무 중)", tag="night_meal"))
        g.put(nm + 30, end, WK)
    if A["fixed_work"]:
        g.put(A["fixed_work"][0], 1440, spec("WORK", "근무", sa="structured", tag="night"))
    # ---- anchored cued events (clock A/B)
    def make(m, dtp_):
        code = "HOBBY" if m["code"] == "OTHER" else m["code"]
        return spec(code, label_of(m), sa="text", st="text" if (m["clock"] is not None or m["daypart"]) else "assumption", mid=m["mid"], tag="cued")

    cued_today = [m for m in P["cued"] if wd in weekly_days(P, m, iso, seed)]
    cued_today.sort(key=lambda m: (0 if m["grade"] in "AB" and m["clock"] is not None else 1, m["mid"]))
    deferred = []
    for m in cued_today:
        if m["grade"] in "AB" and m["clock"] is not None:
            lo, hi = DEFDUR.get(m["code"], (60, 120))
            d = r.step(lo, hi)
            if not place(g, make(m, dtp), d, None, r, anchor=m["clock"]):
                log["unmet"] += 1
        else:
            deferred.append(m)
    # ---- meals
    def meal_exists(lo, hi):
        return any(g.s[k] is not None and g.s[k]["code"] == "MEAL" for k in range(lo // 10, min(144, hi // 10)))

    brk = spec("MEAL", "식사(아침)", tag="breakfast")
    if not meal_exists(A["wake"], A["wake"] + 120):
        if not place(g, brk, 30, (A["wake"], A["wake"] + 120), r, anchor=A["wake"] + r.step(30, 60)):
            place(g, brk, 30, (A["wake"], A["wake"] + 180), r)
    if P["tpl"] == "SHIFT":
        if A["fixed_work"] and not meal_exists(A["fixed_work"][0] - 240, A["fixed_work"][0]):
            place(g, spec("MEAL", "식사(근무 전)", tag="dinner"), 60, (A["fixed_work"][0] - 240, A["fixed_work"][0]), r)
    else:
        lunch_dur = 60 if A["work"] else r.step(30, 60)
        if not meal_exists(720, 840):
            place(g, spec("MEAL", "식사(점심)", tag="lunch_work" if A["work"] else "lunch"), lunch_dur, (720, 780 + lunch_dur), r)
        if not meal_exists(1080, 1230):
            place(g, spec("MEAL", "식사(저녁)", tag="dinner"), 60, (1080, 1170 + 60), r)
    # ---- work (fill from start, skipping occupied slots)
    if A["work"]:
        start, net = A["work"]
        rem, k = net, start // 10
        WK = spec("WORK", "근무", sa="structured", st=A["work_src"], tag="day")
        while rem > 0 and k < 144:
            if g.s[k] is None:
                g.s[k] = WK
                rem -= 10
            k += 1
        if rem > 0:
            log["work_short"] += 1
    # ---- remaining cued events (daypart / frequency only)
    for m in deferred:
        lo, hi = DEFDUR.get(m["code"], (60, 120))
        d = r.step(lo, hi)
        win = DPWIN[m["daypart"]] if m["daypart"] else (720, 1260)
        if not place(g, make(m, dtp), d, win, r):
            log["unmet"] += 1
    # ---- template flex blocks
    for bi, b in enumerate(T["flex"]):
        elig = [0, 1, 2, 3, 4] if b["days"] == WD else [5, 6]
        if b["per_week"] is EVERY:
            days = set(elig)
        else:
            rr = Rng(seed, P["uuid"], iso[0], iso[1], "flex", bi)
            days = set(rr.sample(elig, min(len(elig), rr.randint(*b["per_week"]))))
        if wd not in days:
            continue
        code = b["code"]
        if code == "OUTING":
            cw = [len(P["pool"].get(c, [])) + 1 for c in ("ERRAND", "SOCIAL", "CULTURE")]
            code = r.weighted(["ERRAND", "SOCIAL", "CULTURE"], cw)
        if code == "NAP":
            code = "REST"
        pm = pick_pool(P, code, b["window"], dtp, r)
        sp = spec(code, label_of(pm), sa="text", mid=pm["mid"], tag="flex") if pm else spec(code, DEFLABEL.get(code, code), tag="flex")
        d = r.step(*b["dur"])
        if not place(g, sp, d, b["window"], r):
            log["flex_unplaced"] += 1
            log["flex_unplaced:" + b["code"] + ":" + dtp] += 1
    if P["child"] and dtp == WD and P["tpl"] != "SHIFT":
        place(g, spec("CARE", "자녀 돌봄", sa="structured", tag="child"), r.step(60, 120), (1110, 1290), r)
    # ---- filler: PERSONAL right after waking / before bed, short gaps absorbed, 30min = REST, longer = HOBBY
    k = 0
    while k < 144:
        if g.s[k] is None:
            j = k
            while j < 144 and g.s[j] is None:
                j += 1
            lo, hi = k * 10, j * 10
            # leading PERSONAL (30 min after wake) and trailing PERSONAL (30 min before bed)
            if lo <= A["wake"] < hi and hi - A["wake"] >= 30 and lo == A["wake"]:
                g.put(lo, lo + 30, spec("PERSONAL", "기상 후 준비", tag="filler_p"))
                lo += 30
            bed_t = A["bed"] if A["bed"] < 1440 else None
            if lo < hi and bed_t is not None and hi == bed_t and hi - lo >= 30:
                g.put(hi - 30, hi, spec("PERSONAL", "취침 준비", tag="filler_p"))
                hi -= 30
            run = hi - lo
            if run > 0:
                if run <= 20:
                    # absorb into the previous slot's spec if there is one, else into the next
                    ref = g.s[lo // 10 - 1] if lo >= 10 and g.s[lo // 10 - 1] is not None else (g.s[hi // 10] if hi < 1440 and g.s[hi // 10] is not None else None)
                    g.put(lo, hi, ref if ref is not None else spec("REST", "여유 시간", tag="filler"))
                    log["absorbed"] += 1
                elif run == 30:
                    g.put(lo, hi, spec("REST", "휴식", tag="filler"))
                else:
                    g.put(lo, hi, spec("HOBBY", "자유 시간(기본)", tag="filler"))
            k = j
        else:
            k += 1
    # ---- to segments (merge equal neighbours)
    segs = []
    for k in range(144):
        sp = g.s[k]
        if segs and segs[-1]["sp"] is sp:
            segs[-1]["end"] += 10
        elif segs and segs[-1]["code"] == sp["code"] and segs[-1]["detail"] == sp["detail"] and segs[-1]["mid"] == sp["mid"] and segs[-1]["tag"] == sp["tag"]:
            segs[-1]["end"] += 10
        else:
            segs.append(dict(start=k * 10, end=k * 10 + 10, kind="ACT", sp=sp, code=sp["code"], detail=sp["detail"], mid=sp["mid"], tag=sp["tag"], sa=sp["sa"], st=sp["st"]))
    return segs, Pv
```

- 진단 카운터(`diagnostics.json`에 합산): `sleep_clamped`, `unmet`(시각 일정 배치 실패), `work_short`, `flex_unplaced`(+ `flex_unplaced:<코드>:<요일유형>`), `absorbed`.
- 오류: 채우기 뒤에도 빈 슬롯이 남으면 `InvariantError`(발생하면 버그). 엣지: 앵커 일정이 수면과 겹치면 `unmet`; 어제 취침이 1470(24:30)이면 오늘 30분 `late`; `SHIFT` 토·일은 근무 없음이지만 금요일 밤 근무의 이어짐이 토요일 새벽에 있음; 주당 횟수 상한이 후보 일수보다 크면 후보 일수로 줄임; 주를 걸치는 요일 지정.
- 자원: 인물·일당 0.09~0.14ms(프로토타입 실측, 1,500명×7일).
- 테스트 명세(속성 검사 + 구조 사례; 특정 난수 결과는 단정하지 않는다):

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_compose_covers_day` | 합성 인물 1,000명(네 템플릿 모두) × 14일 | `compose_day` | 구간이 `[0, 1440)`을 겹침·공백 없이 덮고, 모든 경계가 10의 배수이며 `start < end` (E05-1) |
| `test_compose_pure_and_order_free` | 같은 인물·날짜 | 호출 2회, 인물 순서 뒤집기 | 결과 동일 (E05-6) |
| `test_compose_midnight` | 어제 취침 1470 / 오늘 기상 480 | 조립 | 오늘 `[0, 30)` `late`, 이어서 `SLEEP`; 전날 마지막은 `SLEEP`으로 끝남 (E05-1) |
| `test_compose_shift_nights` | SHIFT 인물, 월~일 | 7일 조립 | 월 22:00~24:00 WORK, 화 `[0, 06:00)` WORK에 30분 식사 포함, 토 새벽은 금요일 밤 근무의 이어짐, 토·일 22:00 이후 새 근무 없음, 수면은 낮 |
| `test_compose_anchor_wake` | WAKE A 300 | 조립 | 첫 `SLEEP`이 `[0, 300)`에서 끝남 |
| `test_compose_cued_clock_event` | EXERCISE 시각 960(A) DAILY | 7일 | 매일 960에 시작하는 EXERCISE (점유되지 않은 날), `st = text` (E05-2) |
| `test_compose_priority_clock_over_template` | 시각 일정이 점심 창과 겹침 | 조립 | 시각 일정이 그대로, 점심은 비켜 배치 (우선순위 [DR-14](decisions.md#dr-14--구조화-필드의-사용과-근거-우선순위)) |
| `test_compose_freq_counts` | PER_WEEK 3–3 mention | 20주 | 매주 정확히 3일 (E05-5) |
| `test_compose_weekend_scope` | WEEKEND·WEEKLY mention | 20주 | 매주 주말 둘 중 정확히 하루, 평일 0 (E05-5) |
| `test_compose_prob_week` | PROB_WEEK 0.5 | 400주 | 발생 비율 0.4~0.6 |
| `test_compose_weekly_independent_of_start` | 같은 주 | 수요일부터/월요일부터 조립 | 같은 요일 집합 (E05-6) |
| `test_compose_work_minutes` | WORK_DAY | 평일/주말 | 평일 WORK 합계 480 이상 500 이하(흡수 ≤ 20분), 주말 0 (E05-4) |
| `test_compose_sleep_bounds` | 임의 앵커 | 14일 | 연속 수면 300~600분 |
| `test_compose_pool_labels` | EXERCISE pool 항목 있음 / 없음 | 평일 | 있으면 이름이 pool 이름, `sa = text`; 없으면 `운동·산책`, `sa = assumption` (E05-3) |
| `test_compose_filler_rules` | 평일 | 조립 | 기상 직후 30분 `기상 후 준비`, 20분 이하 `filler` 구간 없음 |
| `test_compose_child_care` | 자녀 동거 평일 | 조립 | `CARE` 60~120분이 18:30~21:30 안, `sa = structured` |
| `test_compose_provenance_labels` | 위 사례들 | 구간 읽기 | `sa`/`st` 값이 정의된 열거값 안 (E05-3) |
| `test_compose_unplaced_is_logged` | 저녁이 가득 찬 사례 | 조립 | 예외 없이 `flex_unplaced` 증가 |

- DoD: 위 테스트 green, 서울 원본 앞 1,500명 × 7일에서 불변식 위반 0건, 인물 순서를 뒤집은 재생이 동일, E05-1~E05-6 통과.


## I-06-a — 지리 상수 3종

- 추적: [C-04](01_concept_map.md#c-04--활동과-시간-단서-추출) 기준 ④(장소 판정), [C-06](01_concept_map.md#c-06--활동-구와-이동) 기준 ⑤, 테스트 E04-4·E06-5. 관련 결정 [DR-13](decisions.md#dr-13--장소-해석과-지리-상수), [DR-12](decisions.md#dr-12--서울-밖-활동의-범위와-판정).
- 위치: `engine/geo/gazetteer.py`, `engine/geo/adjacency.py`, `rules/v1/geo/seoul_places.csv`, `rules/v1/geo/outside_places.csv`, `rules/v1/geo/district_adjacency.csv`(+ `district_adjacency.meta.json`), `scripts/geo_candidates.py`.
- 기술: 표준 라이브러리(`csv`, `re`, `json`, `collections.deque`).
- 시그니처:

```python
@dataclass(frozen=True)
class GazHit: start: int; end: int; token: str; scope: str; districts: tuple[str, ...]; explicit: bool
class Gazetteer:
    @classmethod
    def from_csv(cls, seoul_csv: Path, outside_csv: Path) -> "Gazetteer"
    def find(self, masked_text: str, trip_context: bool) -> list[GazHit]     # ctx=TRIP 항목은 trip_context일 때만

class Geo:
    def __init__(self, districts: Sequence[str], edges: Iterable[tuple[str, str]], residents: Mapping[str, int]) -> None
    @classmethod
    def from_files(cls, adjacency_csv: Path, metadata_json: Path) -> "Geo"
    districts: tuple[str, ...]                       # 25개, 정렬된 짧은 이름
    def hops(self, a: str, b: str) -> int            # 너비 우선 탐색, hops(a, a) = 0
    def adjacent(self, d: str) -> tuple[str, ...]    # 정렬
    def non_adjacent(self, d: str) -> tuple[str, ...]  # 자신과 인접 제외, 정렬
    def residents(self, d: str) -> int               # metadata.json district_counts
    def move_minutes(self, a: str, b: str) -> int    # 0홉 10, 1홉 20, 2홉 30, 3홉 이상 40
    def check_invariants(self) -> None               # 아래 불변식, 위반 시 RuleError
```

- 데이터 형식: 사전 CSV는 UTF-8, 머리글 `token,scope,districts,note,guard,ctx`. 서울 사전은 `scope = SEOUL`이고 `districts`에 서울 구 후보(짧은 이름, `|` 구분, 비면 '일반어'), 서울 밖 사전은 `scope = OUTSIDE`이고 `districts`가 빈다. `guard`는 토큰 뒤에 오면 일치로 보지 않는 정규식, `ctx = TRIP`은 같은 절에 여행 문맥이 있을 때만 유효. 인접표 `district_adjacency.csv`는 머리글 `a,b`, 한 줄이 무방향 간선 하나(`a < b` 사전순), `district_adjacency.meta.json`은 `source`, `retrieved`(내려받은 날짜), `method`, `boundary_rule`을 가진다.
- 인접표 불변식: 구가 정확히 25개(`metadata.json` 키와 같음); 간선 양끝이 모두 그 25개 안; 자기 간선·중복 간선 없음; 그래프가 연결됨; 각 구의 차수가 2 이상 9 이하; 알려진 쌍 — 인접: (종로구, 중구), (강남구, 서초구), (강서구, 양천구), (노원구, 도봉구), (마포구, 서대문구) / 비인접: (종로구, 양천구), (강동구, 강서구).
- 구축 절차(Phase 7의 데이터 작업이며 Phase 6에서 todo로 만든다):
  1. `scripts/geo_candidates.py`가 서울 원본의 취미 목록·서술에서 장소형 토큰을 빈도순으로 CSV에 낸다(접미사 `둘레길·산책로·공원·호수·한강·시장·도서관·체육센터·복지관·경로당·문화센터·박물관·미술관·궁·숲길·산·천·역·골목`, 동네 `…동`, 서울 밖 지명 후보). **`…거리`는 의성어 동사(삐걱거리·흥얼거리…)가 대량으로 걸려 제외한다**(프로토타입 빈도표에서 확인). 이 CSV는 사람이 검토할 후보일 뿐 분류 결과가 아니다.
  2. 사람이 위에서부터 서울 구 후보(그 지물이 지나는 **모든** 서울 구)를 적거나 서울 밖이면 서울 밖 사전에 올린다. 시드 292행([02a_rule_data.md](02a_rule_data.md))은 상위 빈도 토큰을 행정구역 상식으로 적은 초안이므로 공식 경계 자료로 대조해 `note`에 출처를 남기고 틀린 것을 고친다.
  3. 인접표는 공식 행정경계(통계청 SGIS 또는 행정안전부 법정경계)에서 "경계선이 길이를 가지고 맞닿으면 인접, 한강 중앙선을 경계로 하는 구도 인접"으로 계산한다. 출처와 내려받은 날짜를 meta에 적는다.
  4. 커버리지: 취미 목록의 장소형 토큰 언급 중 사전에 있는 비율을 `diagnostics.json`에 기록하고 95% 이상이어야 한다(시드는 이미 상위 100개 기준 96.2%).
  5. 완성되면 `rule_set_sha256`을 다시 계산해 02a와 `run.json`에 기록한다.
- 로직(`find`): 두 사전의 토큰을 합쳐(서울 우선) 길이 내림차순으로 정렬하고 `re.escape(토큰) + ("(?!" + guard + ")" if guard)`를 `|`로 이은 **정규식 하나**를 컴파일한다. `finditer`로 훑어 `ctx = TRIP` 항목은 `trip_context`가 거짓이면 버린다. 구 이름 정규식(`서울 `+`강남` 등+`구`, `…구민체육센터`)의 hit는 `explicit = True`로 [I-04-e](#i-04-e--장소-판정과-근무지-단서)가 따로 만든다.
- 오류: CSV 열 누락, 알 수 없는 `scope`, 같은 CSV 안의 중복 토큰, 서울 사전 `districts`에 25개 구에 없는 이름, 인접표 불변식 위반 → `RuleError`.
- 엣지: 토큰의 정규식 메타문자는 `re.escape`; 한 토큰이 다른 토큰의 접두(`서울` / `서울숲` / `서울대공원`)면 긴 쪽이 이김; 같은 토큰이 두 사전에 있으면 서울이 이김.
- 자원: 사전 292행, 컴파일 1회, 메모리 무시 가능.
- 테스트 명세:

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_gazetteer_loads_seed` | 02a 시드 CSV | `from_csv` | 서울 177행·서울 밖 115행, 후보 구가 모두 25개 안 |
| `test_gazetteer_longest_match` | '서울대공원', '남한산성에서' | `find` | '서울대공원'은 서울 밖(`서울`보다 긴 토큰), '남한산성' 서울 밖 |
| `test_gazetteer_guard` | '청주를 한 잔', '고양이를', '고양시에서' | `find` | 앞의 둘은 일치 없음, '고양시'만 서울 밖 |
| `test_gazetteer_trip_ctx` | '일본 만화책' / '일본 여행' | `find(trip_context=False/True)` | 일치 없음 / 서울 밖 |
| `test_gazetteer_duplicate_token` | 같은 CSV에 같은 토큰 2행 | `from_csv` | `RuleError` |
| `test_gazetteer_bad_district` | 서울 사전에 `없는구` | `from_csv` | `RuleError` |
| `test_geo_toy_hops` | 장난감 사슬 A–B–C–D | `hops`, `move_minutes` | (A,D) = 3홉, A→A 10, A→B 20, A→C 30, A→D 40 |
| `test_geo_symmetric` | 장난감 | 모든 쌍 | `hops`, `move_minutes`가 대칭 |
| `test_geo_nonadjacent` | 장난감 | `non_adjacent("A")` | `("C", "D")` |
| `test_geo_invariants_toy_violation` | 연결되지 않은 그래프 | `check_invariants` | `RuleError` |
| `test_geo_invariants_real` | 실제 인접표(Phase 7) | `check_invariants` | 25개 구·연결·차수 2~9·알려진 쌍 통과 |
| `test_gazetteer_coverage_corpus` (느린) | 실제 원본 취미 목록 | 접미사 탐지 토큰 언급 | 사전 적중 ≥ 95% |
| `test_geo_candidates_script` | 합성 입력 | `geo_candidates` | 빈도순 CSV, `…거리` 토큰 없음 |

- DoD: 위 테스트(실제 인접표·코퍼스 테스트 포함) green, 사전·인접표가 02a 해시 표와 일치하거나 새 해시가 02a에 기록됨.

## I-06-b — 위치 종류, 구 해석, 근무지

- 추적: [C-06](01_concept_map.md#c-06--활동-구와-이동) 기준 ①③④⑤, 테스트 E06-1·E06-3·E06-4·E06-5. 관련 결정 [DR-13](decisions.md#dr-13--장소-해석과-지리-상수), [DR-19](decisions.md#dr-19--기본-일과-템플릿), [DR-12](decisions.md#dr-12--서울-밖-활동의-범위와-판정).
- 위치: `engine/place/locate.py`.
- 시그니처:

```python
def location_kind(seg: dict, mention: dict | None, rules: RuleSet) -> str                       # HOME | WORK | VENUE
def resolve_cands(P: dict, cands: Sequence[str], seed: int, token: str, geo: Geo) -> str         # 후보 구 → 구 하나
def draw_district(P: dict, probs: tuple[float, float, float], rng: KeyedRng, geo: Geo) -> str
def workplace(P: dict, seed: int, geo: Geo) -> tuple[str, str]                                    # (구, src_place)
def locate(P: dict, segs: list[dict], Pv: dict, date, seed: int, wp: tuple[str, str], geo: Geo, rules: RuleSet) -> tuple[list[dict], str, str]
```

- 위치 종류(참조 구현 `locate`와 `location.json`):

| 구간 | 위치 종류 |
|---|---|
| `WORK`, 또는 tag가 `lunch_work`·`night_meal` | `WORK` |
| `SLEEP`, `REST`, `HOUSEHOLD`, `CARE` | `HOME` |
| `MEAL` | mention이 있고 (이름에 `venue_hint` 단어가 있거나 장소 판정이 `SEOUL_*`)이면 `VENUE`, 아니면 `HOME` |
| `EXERCISE`, `CULTURE`, `ERRAND`, `SOCIAL` | 이름에 `home_hint`(통화·전화·온라인·집에서·거실·베란다·홈트·화상·침대·소파)가 있으면 `HOME`, 아니면 `VENUE` |
| `HOBBY`, `STUDY`, `PERSONAL` | 이름에 `venue_hint`가 있거나 mention의 장소 판정이 `SEOUL_DISTRICT`이면 `VENUE`, 아니면 `HOME` |
| tag가 `breakfast`·`dinner`·`late`·`filler`·`filler_p`·`child`(WORK 제외) | `HOME`(위 규칙보다 우선) |
| 구간 끝 > 1380(23:00) (WORK 제외) | `HOME`(우선; 하루가 집에서 끝나도록) |

- 구 해석과 출처 라벨(`src_place`):

| 위치 종류 | 구 | `src_place` | 위치 식별자 `vid` |
|---|---|---|---|
| `HOME` | 거주 구 | `structured` | `H` |
| `WORK` | `workplace` 결과(사람 단위 상수, 날짜와 무관) | `workplace`의 두 번째 값 | `W` |
| `VENUE` + mention 장소가 `SEOUL_DISTRICT`이고 후보가 있음 | `resolve_cands` | 명시 구 이름이면 `text`, 사전이면 `dictionary` | `V:<구>:<토큰>` |
| `VENUE` + 그 밖 | `draw_district` | `assumption` | `V:<구>:<mention_id 또는 코드>` |

- `resolve_cands`([DR-13](decisions.md#dr-13--장소-해석과-지리-상수) ③): 후보에 거주 구가 있으면 거주 구; 아니면 거주 구와의 홉 수가 가장 작은 구; 동률이면 `KeyedRng(seed, uuid, "tie", 토큰).choice(정렬된 동률 목록)`.
- `draw_district`([DR-19](decisions.md#dr-19--기본-일과-템플릿) ⑤): `rng.weighted(["SAME","ADJ","OTHER"], probs)`. SAME = 거주 구, ADJ = 거주 구의 인접 구(정렬), OTHER = 거주 구도 인접 구도 아닌 구(정렬); 그 목록에서 `rng.weighted(목록, 구별 거주 인원)`. 근거리 활동(`near_codes`: MEAL·EXERCISE·ERRAND·HOUSEHOLD·PERSONAL·REST·CARE)은 `(1.0, 0, 0)`, 그 밖은 `(0.5, 0.3, 0.2)`. 난수 키 `KeyedRng(seed, uuid, iso_year, iso_week, "venue", mention_id(없으면 코드), tag)` — 같은 주에는 같은 곳, 주가 바뀌면 달라질 수 있다.
- `workplace`: 현역이면 거주 구(`assumption`); 근무지 단서가 있으면 `resolve_cands(후보)`(`text`/`dictionary`); 없으면 `draw_district(P, (0.30, 0.40, 0.30), KeyedRng(seed, uuid, "workplace"))`(`assumption`).
- 하루 시작 위치: 어제 `fixed_work`가 1440을 넘으면 `W`(근무지), 아니면 `H`(거주 구). 오늘 첫 구간의 위치가 이와 다르면(드묾) 첫 구간의 위치를 이 값으로 덮어쓴다.

```python
# 참조 구현(검증용 프로토타입 소스 그대로) — resolve_cands, draw_district, workplace, locate
def resolve_cands(P, cands, seed, token):
    if P["res"] in cands:
        return P["res"]
    best = min(HOPS[P["res"]][c] for c in cands)
    ties = sorted(c for c in cands if HOPS[P["res"]][c] == best)
    return ties[0] if len(ties) == 1 else Rng(seed, P["uuid"], "tie", token).choice(ties)


def draw_district(P, probs, rng):
    kind = rng.weighted(["SAME", "ADJ", "OTHER"], probs)
    if kind == "SAME":
        return P["res"]
    pool = sorted(ADJ[P["res"]]) if kind == "ADJ" else sorted(d for d in DISTRICTS if d != P["res"] and d not in ADJ[P["res"]])
    return rng.weighted(pool, [RES[d] for d in pool])


def workplace(P, seed):
    if P["military"] == "현역":
        return P["res"], "assumption"
    if P["wp_hint"]:
        return resolve_cands(P, P["wp_hint"], seed, "wp"), ("text" if P["wp_explicit"] else "dictionary")
    return draw_district(P, (0.30, 0.40, 0.30), Rng(seed, P["uuid"], "workplace")), "assumption"


def locate(P, segs, Pv, date, seed, wp):
    iso = date.isocalendar()
    ms = {m["mid"]: m for m in P["mentions"]}
    out = []
    for s in segs:
        m = ms.get(s["mid"])
        code, tag = s["code"], s["tag"]
        if code == "WORK" or tag in ("lunch_work", "night_meal"):
            kind = "WORK"
        elif code in HOME_CODES:
            kind = "HOME"
        elif code == "MEAL":
            kind = "VENUE" if (m and (VENUE_HINT.search(s["detail"]) or m["scope"].startswith("SEOUL"))) else "HOME"
        elif code in ("EXERCISE", "CULTURE", "ERRAND", "SOCIAL"):
            kind = "HOME" if HOME_HINT.search(s["detail"]) else "VENUE"
        elif code in ("HOBBY", "STUDY", "PERSONAL"):
            kind = "VENUE" if (VENUE_HINT.search(s["detail"]) or (m and m["scope"] == "SEOUL_DISTRICT")) else "HOME"
        else:
            kind = "HOME"
        if tag in ("breakfast", "dinner", "late", "filler", "filler_p", "child") and code != "WORK":
            kind = "HOME"
        if s["end"] > 1380 and kind != "WORK":
            kind = "HOME"
        if kind == "HOME":
            d, src = P["res"], "structured"
            vid = "H"
        elif kind == "WORK":
            d, src = wp
            vid = "W"
        else:
            if m and m["scope"] == "SEOUL_DISTRICT" and m["districts"]:
                d, src = resolve_cands(P, m["districts"], seed, m["ptoken"]), ("text" if m["explicit"] else "dictionary")
                vid = "V:" + d + ":" + (m["ptoken"] or "")
            else:
                probs = (1.0, 0.0, 0.0) if code in NEAR else (0.5, 0.3, 0.2)
                d, src = draw_district(P, probs, Rng(seed, P["uuid"], iso[0], iso[1], "venue", s["mid"] if s["mid"] >= 0 else code, s["tag"])), "assumption"
                vid = "V:" + d + ":" + str(s["mid"] if s["mid"] >= 0 else code)
        s2 = dict(s)
        s2.update(dist=d, sp_place=src, vid=vid)
        out.append(s2)
    prev_vid, prev_d = ("W", wp[0]) if (Pv["fixed_work"] and Pv["fixed_work"][1] > 1440) else ("H", P["res"])
    if out and out[0]["vid"] != prev_vid:
        out[0].update(vid=prev_vid, dist=prev_d)
    return out, prev_vid, prev_d
```

- 오류: 인접 구 목록이 비면 OTHER 목록으로 대체; 거주 구가 `Geo`에 없으면 `InputError`. 엣지: 후보 구가 1개면 그 구; 후보가 모두 거주 구에서 같은 홉이면 동률 규칙; 근무지 단서가 있는 인물이 무직이면 쓰지 않는다([I-05-b](#i-05-b--템플릿-선택-근거-분류-일별-앵커)).
- 자원: 구간당 수 µs.
- 테스트 명세:

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_location_kind_table` | 구간 사례 14개(위 표 한 줄에 하나) | `location_kind` | 표대로 (E06-3) |
| `test_late_segments_are_home` | 끝 1400의 EXERCISE | `locate` | `HOME` |
| `test_resolve_prefers_residence` | 후보 [종로구, 양천구], 거주 양천구 | `resolve_cands` | 양천구 (E06-5) |
| `test_resolve_min_hops` | 장난감: 거주 A, 후보 [C, D] | `resolve_cands` | C (홉 2 < 3) (E06-5) |
| `test_resolve_tie_is_deterministic` | 거주 A, 후보 [B, C]가 같은 홉이 되는 장난감 | 입력 순서를 바꿔 반복 | 같은 구, 입력 순서와 무관 (E06-5) |
| `test_draw_district_probabilities` | `(0.30, 0.40, 0.30)`, 서로 다른 uuid 10,000명 | `draw_district` | 같은 구/인접/그 밖 비율이 0.27~0.33 / 0.37~0.43 / 0.27~0.33 |
| `test_draw_district_near_is_residence` | `(1.0, 0, 0)` | 1,000회 | 항상 거주 구 |
| `test_draw_district_weights` | `OTHER` 목록 두 구의 거주 인원 1:9 | 10,000회 | 선택 비율 ≈ 1:9 |
| `test_workplace_rules` | 현역 / 단서 [송파구] / 단서 없음 | `workplace` | 거주 구·`assumption` / 송파구·`text` 또는 `dictionary` / 확률 추첨·`assumption` |
| `test_workplace_is_person_constant` | 한 인물, 14일 | `workplace` | 항상 같은 구 |
| `test_venue_stable_within_week` | 같은 mention, 같은 주 월·수 | `locate` | 같은 구; 다음 주에는 달라질 수 있음 |
| `test_start_of_day_location` | 어제 야간 근무 이어짐 | `locate` | 시작 위치 `W`, 첫 구간 WORK |
| `test_provenance_labels` | text·dictionary·structured·assumption 각 1개 | `locate` | `src_place`가 정의된 값 (E06-3) |
| `test_outside_never_assigned` | 합성 1,000명 × 7일 | `locate` | 모든 구가 25개 안, 서울 밖 mention은 구간이 되지 않음 (E06-4) |

- DoD: 위 테스트 green, E06-3·E06-4·E06-5 통과.

## I-06-c — 이동 구간 삽입

- 추적: [C-06](01_concept_map.md#c-06--활동-구와-이동) 기준 ①②, 테스트 E06-1·E06-2. 관련 결정 [DR-11](decisions.md#dr-11--이동-중-지역-귀속), [DR-13](decisions.md#dr-13--장소-해석과-지리-상수) ⑤, [DR-17](decisions.md#dr-17--프레임과-시간-규약-및-결정성) ④⑤.
- 위치: `engine/place/moves.py`.
- 시그니처:

```python
def carve_plan(out: list[dict]) -> list[tuple[int, int]]               # (out 인덱스, 줄일 수 있는 분) 뒤에서부터
def insert_moves(segs: list[dict], prev_vid: str, prev_d: str, geo: Geo, log: Counter) -> list[dict]
def plan_person_day(P: dict, date, seed: int, geo: Geo, rules: RuleSet, log: Counter) -> list[dict]   # compose_day → workplace → locate → insert_moves
```

- 규칙: 위치 식별자 `vid`가 바뀌는 경계마다 `MOVE`가 필요하다. 길이 `need = geo.move_minutes(출발 구, 도착 구)`(같은 구 10, 1홉 20, 2홉 30, 3홉 이상 40). **도착 구간의 시작 시각 `T`는 되도록 유지한다**(시각 단서가 있는 일정의 시작을 지킨다). 이동 시간은 이렇게 마련한다:
  1. `carve_plan`: `out`의 끝에서부터 걸으며 활동 구간만 대상으로 한다. 필러(`filler`·`filler_p`·`late`)는 **전부 소비할 수 있고 계속 앞으로 이어간다**. **시각 단서가 있는 구간(`st = text`)과 `WORK` 구간은 끝을 지켜야 하므로 줄이지 않고(0분) 거기서 멈춘다** — 이때 이동 시간은 전부 도착 구간 지연(`delay`)으로 채운다([DR-14](decisions.md#dr-14--구조화-필드의-사용과-근거-우선순위) ③ 시각 단서 우선, [DR-19](decisions.md#dr-19--기본-일과-템플릿)의 근무 8시간). 그 밖에 처음 만나는 구간은 길이 − 10분(수면은 길이 − 240분)까지 줄일 수 있고, 거기서 멈춘다. `MOVE`를 만나면 멈춘다.
  2. 줄일 수 있는 합 `avail`을 먼저 계산한다. `carved = min(need, avail)`, `delay = need − carved`.
  3. `delay > 0`인데 도착 구간이 `delay`를 빼고 10분 이상 남지 않으면 **이동을 취소**한다. 현재 장소에 머무르며 앞 구간을 도착 구간 끝까지 늘리고(앞 구간이 없으면 `REST` 필러를 둔다), 도착 구간은 버린다. `cancelled_by_move`를 센다. 근무 직후의 짧은 도착 구간이 취소되면 근무가 그만큼(최대 약 40분) 길어진다. 프로토타입(1,500명 × 7일)에서 취소는 인물·일당 0.042건, 도착 지연은 0.77건이었고 평일 근무는 중앙값 480분·p95 500분·최대 590분(시각 단서 근무)이었다. 3,000명 × 7일에서는 취소 0.041건, 도착 지연 0.78건, 평일 근무 최대 720분이었다(상한은 시각 단서가 정한 근무 길이를 따르므로 표본이 커지면 최댓값이 늘 수 있다).
  4. 아니면 계획대로 앞 구간들을 줄이고(길이 0이 된 구간은 제거), 도착 구간의 시작을 `T + delay`로 미룬다(`late_arrival`을 센다), `MOVE`를 `[T − carved, T + delay)`로 넣는다. `from_district` = 현재 구, `to_district` = 도착 구.
- 불변식(출력): 구간이 하루를 겹침·공백 없이 덮는다; 모든 경계가 10의 배수; 구가 바뀌는 `MOVE` ≥ 20분, 같은 구 `MOVE` ≥ 10분; `MOVE.from`은 바로 앞 활동 구간의 구와, `MOVE.to`는 바로 뒤 활동 구간의 구와 같다; 하루의 첫 구간 앞에 `MOVE`가 없다.
- 이동 중 귀속(프레임 단계, [I-07-b](#i-07-b--프레임-샘플러)): `MOVE [s, e)`의 중점 `m = s + (e − s)/2`에서 `[s, m)`은 출발 구, `[m, e)`는 도착 구. 중점 시각은 후반(도착 구)이다.

```python
# 참조 구현(검증용 프로토타입 소스 그대로) — move_len, carve_plan, insert_moves, plan_person_day
def move_len(a, b):
    h = HOPS[a][b]
    return 10 if h == 0 else (20 if h == 1 else (30 if h == 2 else 40))


FILLER_TAGS = ("filler", "filler_p", "late")


def carve_plan(out):
    """walk backwards over `out`: fillers are consumable; text-timed and WORK segments keep their end (protected);
    the first other segment may give up to (len-keep) and ends the walk"""
    plan = []
    for i in range(len(out) - 1, -1, -1):
        t = out[i]
        if t["kind"] != "ACT":
            break
        if t["st"] == "text" or t["code"] == "WORK":
            plan.append((i, 0))
            break
        filler = t["tag"] in FILLER_TAGS
        keep = 0 if filler else (240 if t["code"] == "SLEEP" else 10)
        plan.append((i, max(0, t["end"] - t["start"] - keep)))
        if not filler:
            break
    return plan


def insert_moves(segs, prev_vid, prev_d, log):
    out = []
    cur_vid, cur_d = prev_vid, prev_d
    for s in segs:
        if s["vid"] != cur_vid:
            need = move_len(cur_d, s["dist"])
            T = s["start"]
            plan = carve_plan(out)
            avail = sum(mt for _, mt in plan)
            carved = min(need, avail)
            delay = need - carved
            if delay > 0 and s["end"] - T - delay < 10:
                # cannot fit the relocation: cancel it, stay at the current place for the target's interval
                log["cancelled_by_move"] += 1
                if out and out[-1]["kind"] == "ACT":
                    out[-1]["end"] = s["end"]
                else:
                    out.append(dict(start=T, end=s["end"], kind="ACT", code="REST", detail="휴식", mid=-1, tag="filler", sa="assumption", st="assumption",
                                    sp_place="structured", dist=cur_d, vid=cur_vid, sp=None))
                continue
            rem = carved
            for i, mt in plan:
                take = min(rem, mt)
                out[i]["end"] -= take
                rem -= take
            out[:] = [t for t in out if not (t["kind"] == "ACT" and t["end"] == t["start"])]
            if delay > 0:
                log["late_arrival"] += 1
                s["start"] = T + delay
            out.append(dict(start=T - carved, end=T + delay, kind="MOVE", frm=cur_d, to=s["dist"], code="MOVE", detail="이동", mid=-1, tag="move",
                            sa="assumption", st="assumption", sp_place="assumption", dist=None, vid=None))
        out.append(s)
        cur_vid, cur_d = s["vid"], s["dist"]
    return out


def plan_person_day(P, date, seed, log):
    segs, Pv = compose_day(P, date, seed, log)
    wp = workplace(P, seed)
    loc, pvid, pd = locate(P, segs, Pv, date, seed, wp)
    return insert_moves(loc, pvid, pd, log)
```

- 오류: 불변식 위반은 [I-08-b](#i-08-b--전수-자동-검사)가 `InvariantError`로 올린다. 엣지: 첫 구간이 이동을 요구하면 `locate`가 위치를 덮어써 이동이 생기지 않음; 연속 이동(취소된 구간 뒤); 수면 직전 이동; 하루 끝(23:00 이후 구간은 `HOME`이므로 돌아오는 이동이 그 전에 끝남).
- 자원: 구간당 O(1) 평균.
- 테스트 명세(장난감 지리 A–B–C–D, 구간은 합성):

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_move_len_bands` | 장난감 | `move_len` / `geo.move_minutes` | 0·1·2·3홉 → 10·20·30·40 |
| `test_move_does_not_shorten_work` | [WORK 540][HOME 60], 이동 40분 | `insert_moves` | WORK 길이 불변, MOVE `[T, T+40)`, HOME 시작이 40분 뒤로 |
| `test_move_does_not_shorten_text_timed` | [EXERCISE `st = text` 60][HOME 60], 이동 20분 | `insert_moves` | EXERCISE 길이 불변, 도착 구간이 20분 지연 |
| `test_move_carves_previous` | [HOME 60분][VENUE(인접 구) 60분] | `insert_moves` | HOME이 20분 줄고 그 자리에 MOVE 20분, VENUE 시작 불변 (E06-2) |
| `test_move_consumes_fillers` | [HOME 30][filler 20][VENUE(2홉)] | `insert_moves` | 필러 소비, HOME이 10분 줄고 MOVE 30분 |
| `test_move_never_cuts_sleep_below_240` | 앞 구간 SLEEP 250분, 이동 40분 | `insert_moves` | 수면은 10분만 줄고 나머지 30분은 도착 지연 |
| `test_move_late_arrival` | 앞 구간(비필러) 길이 20, 이동 30분, 도착 구간 길이 60 | `insert_moves` | 앞 구간은 10분만 남고(10분 줄어듦), 도착 구간 시작이 20분 뒤로, MOVE `[T−10, T+20)`, `late_arrival` +1 |
| `test_move_cancel` | 도착 구간 길이 20, 지연 30분 필요 | `insert_moves` | 이동 취소, 앞 구간이 도착 구간 끝까지 늘어남, `cancelled_by_move` +1 |
| `test_same_district_move_is_10` | HOME → 같은 구 VENUE | `insert_moves` | MOVE 10분, `from == to` |
| `test_move_matches_neighbours` | 위 사례 전부 | 불변식 검사 | `MOVE.from`/`to`가 앞뒤 구간의 구와 같음 |
| `test_no_move_at_day_start` | 야간 근무 이어짐 / 일반 | `plan_person_day` | 첫 구간 앞에 MOVE 없음, 구간이 0에서 시작 (E06-2: 이동은 하루 경계를 넘지 않음) |
| `test_plan_person_day_invariants` | 서울 원본 앞 1,500명 × 7일(실제 단서) | `plan_person_day` | 위 불변식 위반 0건 (E06-1·E06-2) |
| `test_every_slot_exactly_one_district` | 위 결과 | 144시각 샘플링 | 각 시각 인물당 정확히 한 구 (E06-1) |
| `test_move_midpoint_halves` | MOVE 20·30·40·60분 | [I-07-b](#i-07-b--프레임-샘플러) 샘플링 | 아래 표 (E06-2) |

  | MOVE 길이 | 프레임 시각(시작 s 기준) | 출발(MOVE_OUT) 수 | 도착(MOVE_IN) 수 |
  |---|---|---|---|
  | 10분(같은 구만) | s | 1 | 0 |
  | 20분 | s, s+10 | 1 (s) | 1 (s+10) |
  | 30분 | s, s+10, s+20 | 2 | 1 |
  | 40분 | s … s+30 | 2 | 2 |
  | 60분 | s … s+50 | 3 | 3 |

- DoD: 위 테스트 green, E06-1·E06-2 통과, 서울 원본 앞 3,000명 × 7일에서 불변식 위반 0건.

## I-07-a — 세그먼트 저장소

- 추적: [C-07](01_concept_map.md#c-07--지역별-10분-프레임) 기준 ③(정본), 테스트 E07-3. 관련 결정 [DR-18](decisions.md#dr-18--저장과-조회-방식).
- 위치: `engine/store/segments.py`.
- 시그니처:

```python
def write_persons(run_dir: Path, rows: list[dict]) -> None                                  # persons.parquet (PERSON_SCHEMA)
def write_segment_part(run_dir: Path, date: datetime.date, chunk_no: int, rows: list[dict], row_offset: int) -> int   # 쓴 행 수
def read_day(run_dir: Path, date: datetime.date) -> pyarrow.Table                            # 파트를 번호 순으로 이어 붙인 하루치
def list_days(run_dir: Path) -> list[datetime.date]
```

- 규칙: `segments/<YYYY-MM-DD>/part-<NNNNN>.parquet`(청크 번호 0부터 5자리). 청크 안에서 (`person_idx`, `start`) 순으로 정렬해 쓴다. `row_offset`은 같은 날짜의 앞 파트 행 수 합이며 `segment_id = day_ordinal * 2**32 + row_offset + 파트 안 행 번호`. 따라서 같은 날짜의 파트는 **청크 번호 순서로만** 쓴다(어기면 `RunError`). Parquet 옵션은 고정: `compression="zstd"`, `row_group_size=65536`, `use_dictionary=True`, `write_statistics=True`.
- 변환: 내부 구간 dict → `SEGMENT_SCHEMA` 행. `ACT` → `kind = ACTIVITY`, `district = dist`; `MOVE` → `kind = MOVE`, `from_district = frm`, `to_district = to`; 구 이름은 `full_name`; `mention_id`가 없으면 -1.
- 오류: 같은 파트를 두 번 쓰면 `RunError`; 스키마·열거값 위반이면 `InvariantError`; 읽을 때 파트 번호가 비어 있으면(0..n-1이 아님) `InvariantError`.
- 엣지: 한 청크에 인물이 5,000명 미만(마지막 청크); 날짜 디렉터리가 임시 경로(`.tmp-…`) 아래에 만들어짐.
- 자원: 서울 전원 하루 약 350만 행(인물·일당 평균 18.7구간, 프로토타입 실측)이며 파트 38개. 쓰기 메모리는 청크 하나(5,000명 × 19구간 ≈ 10만 행).
- 테스트 명세:

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_write_read_roundtrip` | 합성 구간 3,000행 | 쓰고 `read_day` | 열·값 동일, 정렬됨 |
| `test_segment_id_unique_and_formula` | 파트 3개 | `read_day` | `segment_id` 유일, 공식과 일치 (E07-3) |
| `test_parts_must_be_in_order` | 파트 1을 먼저 씀 | `write_segment_part` | `RunError` |
| `test_part_written_twice` | 같은 파트 | 두 번 | `RunError` |
| `test_read_day_gap_in_parts` | 파트 0, 2만 있음 | `read_day` | `InvariantError` |
| `test_schema_enforced` | `kind = "X"` | 쓰기 | `InvariantError` |
| `test_write_deterministic` | 같은 입력 두 번 | 파일 바이트 | 동일 |
| `test_persons_roundtrip` | 5명 | `write_persons`, 읽기 | 동일 |

- DoD: 표 green.

## I-07-b — 프레임 샘플러

- 추적: [C-07](01_concept_map.md#c-07--지역별-10분-프레임) 기준 ①②③④⑤, 테스트 E07-1~E07-5. 관련 결정 [DR-17](decisions.md#dr-17--프레임과-시간-규약-및-결정성), [DR-18](decisions.md#dr-18--저장과-조회-방식).
- 위치: `engine/frames/sample.py`.
- 시그니처:

```python
def build_arrays(day_table: pyarrow.Table, district_id: Mapping[str, int]) -> dict[str, numpy.ndarray]
def sample_frame(a: dict[str, numpy.ndarray], t: int) -> tuple[ndarray, ndarray, ndarray, ndarray, ndarray]   # person_idx, district_id, state, code_id, row
def iter_frames(day_table: pyarrow.Table, district_id: Mapping[str, int], times: Iterable[int] = range(0, 1440, 10)) -> Iterator[tuple[int, FrameChunk]]
```

- 규칙([DR-17](decisions.md#dr-17--프레임과-시간-규약-및-결정성)): 시각 `t`(10의 배수)의 상태는 `start ≤ t < end`인 구간 하나다. `ACTIVITY`면 그 구간의 구; `MOVE`이면 중점 `mid = start + (end − start) // 2`에서 `t < mid`는 `MOVE_OUT`(구 = 출발 구), 아니면 `MOVE_IN`(구 = 도착 구). 구간 배열은 (인물, 시작) 순으로 정렬되어 있으므로 `flatnonzero` 결과가 곧 `person_idx` 오름차순이다. 결과 행 수가 인물 수가 아니거나 `person_idx`가 유일하지 않으면 `InvariantError`.
- 구현 지침: 배열 dtype은 참조 구현 그대로(`person_idx` int32, `start`·`end`·`mid` int16, `kind`·`code`·`district`·`from`·`to` int8). 활동 코드 id는 `ACTIVITY_CODES` 순서(`MOVE` 포함), 구 id는 정렬된 25개 구의 순번. `iter_frames`는 시각 하나씩 `FrameChunk`를 내고 이전 청크를 붙들지 않는다.

```python
# 참조 구현(검증용 프로토타입 소스 그대로) — build_arrays, sample_frame, frame_at, check_day
ACT_CODES = ["SLEEP", "REST", "MEAL", "PERSONAL", "WORK", "STUDY", "HOUSEHOLD", "CARE", "MOVE", "EXERCISE", "HOBBY", "SOCIAL", "CULTURE", "ERRAND", "OTHER"]
CODE_ID = {c: i for i, c in enumerate(ACT_CODES)}


def build_arrays(day_segments, district_id):
    """day_segments[i] = segment list of person i for ONE day -> numpy columns (rows ordered by person, start)"""
    pi, st, en, kd, cd, di, fr, to = [], [], [], [], [], [], [], []
    for i, segs in enumerate(day_segments):
        for sg in segs:
            pi.append(i)
            st.append(sg["start"])
            en.append(sg["end"])
            if sg["kind"] == "ACT":
                kd.append(0); cd.append(CODE_ID[sg["code"]]); di.append(district_id[sg["dist"]]); fr.append(-1); to.append(-1)
            else:
                kd.append(1); cd.append(CODE_ID["MOVE"]); di.append(-1); fr.append(district_id[sg["frm"]]); to.append(district_id[sg["to"]])
    a = dict(pi=np.array(pi, np.int32), st=np.array(st, np.int16), en=np.array(en, np.int16), kd=np.array(kd, np.int8), cd=np.array(cd, np.int8),
             di=np.array(di, np.int8), fr=np.array(fr, np.int8), to=np.array(to, np.int8))
    a["mid"] = (a["st"].astype(np.int32) + (a["en"].astype(np.int32) - a["st"].astype(np.int32)) // 2).astype(np.int16)
    return a


def sample_frame(a, t):
    """state of every person at minute t (multiple of 10): returns person_idx, district_id, state(0 ACTIVITY,1 MOVE_OUT,2 MOVE_IN), code_id, row_idx"""
    idx = np.flatnonzero((a["st"] <= t) & (t < a["en"]))
    is_move = a["kd"][idx] == 1
    out_move = is_move & (t < a["mid"][idx])
    dist = np.where(is_move, np.where(out_move, a["fr"][idx], a["to"][idx]), a["di"][idx])
    state = np.where(is_move, np.where(out_move, 1, 2), 0).astype(np.int8)
    return a["pi"][idx], dist, state, a["cd"][idx], idx


def frame_at(segs, t):
    for s in segs:
        if s["start"] <= t < s["end"]:
            if s["kind"] == "ACT":
                return s["dist"], "ACTIVITY", s
            mid = s["start"] + (s["end"] - s["start"]) // 2
            return (s["frm"], "MOVE_OUT", s) if t < mid else (s["to"], "MOVE_IN", s)
    raise AssertionError("no segment at %d" % t)


def check_day(segs, prev_nonzero=True):
    errs = []
    t = 0
    for i, s in enumerate(segs):
        if s["start"] != t:
            errs.append(("gap_or_overlap", i, s["start"], t))
        if s["end"] <= s["start"]:
            errs.append(("empty", i))
        if s["start"] % 10 or s["end"] % 10:
            errs.append(("grid", i, s["start"], s["end"]))
        if s["kind"] == "MOVE":
            L = s["end"] - s["start"]
            if s["frm"] != s["to"] and L < 20:
                errs.append(("cross_move_short", i, L))
            if s["frm"] == s["to"] and L < 10:
                errs.append(("same_move_short", i, L))
        t = s["end"]
    if t != 1440:
        errs.append(("not_covered", t))
    return errs
```

- 오류: 위 `InvariantError`. 엣지: 어떤 구에도 아무도 없는 시각은 해당 구 0명(빈 구 포함, 합계는 보존); 구간 하나가 144슬롯 전체를 덮는 인물; `t`가 10의 배수가 아니면 `ValueError`.
- 자원: 프로토타입 실측 외삽으로 서울 전원 하루 144프레임 약 2~3초, 배열 약 52MB/일. 메모리는 하루치 배열 + 프레임 1개(185,228행).
- 테스트 명세:

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_sampler_matches_reference` | 합성 500명 × 2일 | `sample_frame` vs 느린 참조 `frame_at` | 모든 t·모든 인물 일치 (E07-3) |
| `test_sampler_conservation` | 위 | 144프레임 | 행 수 = 인물 수, `person_idx` 유일, 구별 합계의 총합 = 인물 수 (E07-1·E07-2) |
| `test_sampler_boundaries` | 구간 `[s, e)` | t = s, e − 10, e, mid | s·e−10은 그 구간, e는 다음 구간, MOVE의 mid는 `MOVE_IN` (E07-3) |
| `test_sampler_empty_district` | 한 구에 아무도 없는 시각 | 구별 집계 | 그 구 0, 합계 보존 (E07-2) |
| `test_sampler_order_independent` | 파트 순서를 섞어 읽음 | 같은 시각 | 같은 결과(정렬 후 비교) (E07-4) |
| `test_sampler_split_equals_whole` | 인물 집합을 둘로 나눠 각각 샘플링 | 합치기 | 전체 샘플링과 같음 (E07-4) |
| `test_iter_frames_holds_one_chunk` | 합성 | `tracemalloc`으로 최대 보유 | 프레임 1개 크기 수준(전체의 10% 미만) (E07-5) |
| `test_sampler_rejects_bad_time` | t = 15 | `sample_frame` | `ValueError` |

- DoD: 표 green, 프로토타입과 같은 크기(합성 1,500명)에서 144프레임 샘플링이 0.1초 미만.

## I-07-c — 프레임 내보내기와 조회

- 추적: [C-07](01_concept_map.md#c-07--지역별-10분-프레임) 기준 ①②③④, 테스트 E07-1~E07-4와 사용자 조회 E2E. 관련 결정 [DR-18](decisions.md#dr-18--저장과-조회-방식).
- 위치: `engine/frames/export.py`.
- 시그니처:

```python
def export_day(run_dir: Path, date: datetime.date) -> Path                                   # frames/<날짜>.parquet
def query(run_dir: Path, date: datetime.date, minute: int, district: str | None = None) -> pyarrow.Table
def district_counts(run_dir: Path, date: datetime.date, minute: int) -> dict[str, int]
```

- 형식: `FRAME_SCHEMA`. 파일 하나가 하루이고 **행 그룹 하나가 시각 하나**(144개)다. 행 그룹 안은 (`district`, `person_idx`) 순으로 정렬한다. `ts = ts_of(date, minute)`, `uuid`는 `persons.parquet`에서, `activity_detail`은 세그먼트에서 가져온다. 쓰기는 `ParquetWriter`에 시각마다 `write_table` 한 번(행 그룹 한 개), `compression="zstd"`. `query`는 `minute // 10`번째 행 그룹만 읽고 `district`로 거른다.
- 규모: 하루 26,672,832행(서울 전원). 크기·시간은 첫 수직 슬라이스에서 측정해 구현 순서 Map에 기록한다([DR-18](decisions.md#dr-18--저장과-조회-방식)). 내보내기는 선택이고, 프레임은 언제든 세그먼트에서 다시 계산된다.
- 자원: 시각 청크 하나(185,228행)씩 쓰므로 메모리는 하루치 배열 + 청크 1개. 서울 전원 하루 내보내기의 시간·크기는 측정 전이라 수치를 단정하지 않는다.
- 오류: 이미 파일이 있으면 `RunError`(덮어쓰지 않음); 세그먼트 없는 날짜는 `RunError`.
- 엣지: 비어 있는 구는 `district_counts`에서 0으로 나온다; `minute`이 10의 배수가 아니면 `ValueError`.
- 테스트 명세:

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_export_row_group_per_slot` | 합성 300명 × 1일 | `export_day` | 행 그룹 144개, 각 300행 (E07-1) |
| `test_query_district_slice` | 위 | `query(t, d)` | 그 시각·그 구의 인물만, 모든 구의 합 = 300 (E07-2) |
| `test_export_matches_sampler` | 위 | 내보낸 값 vs `sample_frame` | 모든 시각·인물 일치 (E07-3) |
| `test_export_evidence_lookup` | 프레임 행 | `segment_id`로 세그먼트 조회 | `start ≤ t < end`, 같은 uuid·활동 (E07-3) |
| `test_export_deterministic` | 같은 입력 두 번 | 파일 바이트 | 동일 (E07-4) |
| `test_export_refuses_overwrite` | 이미 있는 파일 | `export_day` | `RunError` |
| `test_district_counts_include_empty` | 한 구에 아무도 없음 | `district_counts` | 25개 구 키 전부, 빈 구 0 |

- DoD: 표 green.


## I-08-a — 실행 메타데이터와 재생

- 추적: [C-08](01_concept_map.md#c-08--출처-재현성-품질-검증) 기준 ②⑥, 테스트 E08-2·E08-6. 관련 결정 [DR-20](decisions.md#dr-20--품질-합격선과-보관) ①.
- 위치: `engine/verify/runmeta.py`.
- 시그니처:

```python
def make_run_id(rule_version: str, seed: int, start: datetime.date, days: int, limit: int | None) -> str
def build_run_meta(args: Namespace, rules: RuleSet, source_meta: dict, stats: dict) -> dict
def write_run_meta(run_dir: Path, meta: dict) -> None
def result_hash(run_dir: Path) -> str                       # 세그먼트 결과 해시
def cues_hash(run_dir: Path) -> str
def replay_matches(run_dir: Path, workdir: Path) -> bool    # run.json 설정으로 다시 만들어 해시 비교
```

- `run_id` = `f"{rule_version}-s{seed}-{start:%Y%m%d}-{days}d"` + (`limit`이 있으면 `f"-n{limit}"`). 예: `v1-s20261030-20261102-7d`.
- `run.json` 필수 항목(E08-2): `run_id`, `rule_version`, `rule_set_sha256`, `adjacency_present`, `seed`, `start_date`, `days`, `timezone`(`"Asia/Seoul"`), `frame_minutes`(10), `source`(`dataset`, `revision`, `seoul_rows`, `output_sha256`는 `data/nemotron-personas-korea/metadata.json`에서 그대로), `persons`(처리 인원), `limit`, `chunk_persons`, `python`, `pyarrow`, `numpy`(버전 문자열), `argv`(실행 명령), `stages`(단계별 이름·초·행 수), `cues_sha256`, `result_sha256`, `generated_at`(KST ISO 문자열). 결과 해시 두 개(`cues_sha256`, `result_sha256`)에는 `generated_at`·경로 밖 정보가 들어가지 않으므로 재생 비교에 쓸 수 있다.
- 해시 정의: 대상 파일을 (날짜 오름차순, 파트 번호 오름차순)으로 늘어놓고 `상대경로.encode() + b"\0" + 파일 바이트`를 이어 SHA-256. `cues_hash`는 `cues/cues.parquet`와 `cue_persons.parquet`, `result_hash`는 `segments/**`(와 `persons.parquet`).
- `replay_matches`: `run.json`의 `rule_version`·`seed`·`start_date`·`days`·`limit`으로 `cues`와 `plan` 단계를 임시 작업 디렉터리에 다시 돌려 두 해시가 같은지 비교한다. `rule_set_sha256`이 현재 규칙 파일과 다르면 재생하지 않고 `RuleError`.
- 오류: 필수 항목 누락 `RunError`; `run.json` 읽기 실패 `RunError`. 엣지: `limit`이 없는 전체 실행의 `persons`는 185,228; 파일 내용이 같아도 파일 이름이 다르면 해시가 달라진다(경로가 해시에 들어감).
- 자원: 해시 계산은 파일을 4MiB씩 읽는다. 전체 7일 세그먼트 수백 MB를 읽어 수 초.
- 테스트 명세:

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_run_id_format` | (`v1`, 20261030, 2026-11-02, 7, None / 100) | `make_run_id` | `v1-s20261030-20261102-7d` / `…-n100` |
| `test_run_meta_required_fields` | 소규모 실행 | `build_run_meta` | 필수 항목 전부 있음, `timezone == "Asia/Seoul"`, `source.output_sha256`이 `metadata.json` 값과 같음 (E08-2) |
| `test_result_hash_stable` | 같은 실행 두 번 | `result_hash` | 같음 |
| `test_result_hash_detects_change` | 파트 파일 1바이트 변경 | `result_hash` | 달라짐 |
| `test_replay_matches` | 20명 × 3일 실행 | `replay_matches` | `True` (E08-6) |
| `test_replay_rejects_changed_rules` | 규칙 `neg` 패턴 변경 | `replay_matches` | `RuleError` (E08-6) |

- DoD: 표 green.

## I-08-b — 전수 자동 검사

- 추적: [C-08](01_concept_map.md#c-08--출처-재현성-품질-검증) 기준 ③, 테스트 E08-3. 관련 결정 [DR-20](decisions.md#dr-20--품질-합격선과-보관) ①, [DR-17](decisions.md#dr-17--프레임과-시간-규약-및-결정성).
- 위치: `engine/verify/invariants.py`.
- 시그니처: `def run_checks(run_dir: Path, geo: Geo, expected_persons: int, dates: Sequence[datetime.date]) -> dict`. 반환은 `{검사ID: {"ok": bool, "violations": int, "examples": [최대 5건]}}`이고 `report/checks.json`에 쓴다. 하나라도 `ok = false`이면 실행은 실패다(종료 코드 4, 완성본으로 확정하지 않음; `.failed-<run_id>`로 보존).
- 검사 목록(위반 0건이 합격선):

| ID | 내용 |
|---|---|
| CHK-01 | 인물·날짜마다 구간이 `start`로 정렬되고 첫 `start = 0`, 앞 `end` = 뒤 `start`, 마지막 `end = 1440`, 모든 경계가 10의 배수, `start < end` |
| CHK-02 | `MOVE`: 구가 바뀌면 ≥ 20분, 같은 구 ≥ 10분; `from_district`는 바로 앞 활동 구간의 구, `to_district`는 바로 뒤 활동 구간의 구와 같음; 하루의 첫·마지막 구간이 아님 |
| CHK-03 | 모든 구 값이 25개 구 이름 안(서울 밖 없음); `ACTIVITY`는 `district`만, `MOVE`는 `from`/`to`만 채워짐 |
| CHK-04 | 날짜마다 144시각 전부에서 프레임 행 수 = 인물 수, `person_idx` 유일, 구별 합계의 총합 = 인물 수 ([I-07-b](#i-07-b--프레임-샘플러) 사용) |
| CHK-05 | `persons.parquet`·`cue_persons.parquet`의 uuid가 유일하고 개수가 기대 인원(전체 실행은 185,228)과 같으며 집합이 서로 같음 |
| CHK-06 | `activity_code`·`kind`·`src_activity`·`src_time`·`src_place`가 정의된 열거값; `mention_id ≥ 0`이면 그 인물의 `cues.parquet`에 있는 mention이고 활동 코드가 호환됨 |
| CHK-07 | 하루를 이어 붙인 연속 `SLEEP` 길이가 240~600분(이동이 줄일 수 있는 하한 240분, 상한은 [DR-19](decisions.md#dr-19--기본-일과-템플릿)의 10시간) |
| CHK-08 | 날짜 경계의 위치 연속: D일 마지막 구간의 구 = D+1일 첫 구간의 구 |
| CHK-09 | 같은 `segment_id`가 두 번 나오지 않음, 공식(`day_ordinal·2^32 + row`)과 일치 |

  결정성(같은 설정 재생 일치)은 [I-08-a](#i-08-a--실행-메타데이터와-재생)의 `replay_matches`가 맡는다.
- 구현: 파트 파일을 하나씩 읽어 numpy로 검사한다(`CHK-01`은 파트 안에서 `person_idx` 변화 지점을 찾아 벡터로, `CHK-04`는 `iter_frames`로). 위반 예시는 (uuid 또는 person_idx, 날짜, 구간 번호)로 남긴다. 파트 하나가 메모리에 올라가는 범위에서 처리한다.
- 오류: 검사 도중 파일을 못 읽으면 `InvariantError`가 아니라 `RunError`. 엣지: 마지막 청크 인원 부족, 모든 구간이 하나뿐인 인물(수면 하루종일 같은 비정상은 CHK-07이 잡음).
- 자원: 서울 전원 7일 기준 수 분, 메모리 1~2GiB(`memguard run --mem-gb 3`).
- 테스트 명세(E08-3; 각각 깨끗한 합성 실행의 사본에 위반을 **주입**해 검사가 잡는지 본다):

| 테스트 | 주입하는 위반 | 기대 |
|---|---|---|
| `test_checks_pass_on_clean_run` | 없음 | 모든 검사 `ok` |
| `test_chk01_detects_gap` | 구간 하나 삭제 | CHK-01 위반 ≥ 1 |
| `test_chk01_detects_overlap` | 구간 `end`를 10분 연장 | CHK-01 |
| `test_chk01_detects_grid` | `end = 1425` | CHK-01 |
| `test_chk02_detects_short_cross_move` | `from ≠ to`인 MOVE를 10분으로 | CHK-02 |
| `test_chk02_detects_neighbour_mismatch` | MOVE의 `to`를 바꿈 | CHK-02 |
| `test_chk03_detects_unknown_district` | `district = "서울-부산구"` | CHK-03 |
| `test_chk04_detects_duplicate_person` | 한 인물의 구간을 다른 인물 번호로 복제 | CHK-04 |
| `test_chk04_detects_missing_person` | 한 인물의 모든 구간 삭제 | CHK-04, CHK-01 |
| `test_chk05_detects_duplicate_uuid` | `persons.parquet`에 uuid 중복 | CHK-05 |
| `test_chk06_detects_bad_enum` | `src_time = "X"` | CHK-06 |
| `test_chk06_detects_dangling_mention` | `mention_id = 99999` | CHK-06 |
| `test_chk07_detects_short_sleep` | 수면 구간을 200분으로 | CHK-07 |
| `test_chk08_detects_location_jump` | 다음 날 첫 구간의 구를 바꿈 | CHK-08 |
| `test_chk09_detects_duplicate_segment_id` | `segment_id` 중복 | CHK-09 |

- DoD: 표 green, 서울 원본 앞 3,000명 × 7일 실행이 CHK-01~CHK-09를 모두 통과.

## I-08-c — 보고 지표 5종

- 추적: [C-08](01_concept_map.md#c-08--출처-재현성-품질-검증) 기준 ⑤, 테스트 E08-5. 관련 결정 [DR-15](decisions.md#dr-15--시간-신호-해석과-추출-방식) ⑦, [DR-12](decisions.md#dr-12--서울-밖-활동의-범위와-판정) ④, [DR-20](decisions.md#dr-20--품질-합격선과-보관) ④.
- 위치: `engine/verify/metrics.py`. 시그니처: `def compute_metrics(run_dir: Path) -> dict` → `report/metrics.json`.
- 정의(비율은 0~1, 소수 6자리로 반올림하고 분자·분모를 함께 적는다. 합격/불합격은 없다):

| 키 | 정의 |
|---|---|
| `grounded_time_ratio` | `ACTIVITY` 구간 중 `src_time = text`인 구간의 슬롯 수 ÷ 전체 슬롯(인물 수 × 일수 × 144). 하위로 `by_grade`(그 구간의 mention `time_grade` A/B/C별 슬롯 수) |
| `outside_omitted` | `persons_with_outside` ÷ 인물 수 (서울 밖으로 생략된 mention이 하나라도 있는 인물), `outside_mentions` ÷ 병합 제외 mention 수, 그리고 `travel_persona` 안/밖으로 나눈 같은 두 값 |
| `assumption_ratio` | 슬롯 중 `src_activity = assumption`, `src_time = assumption`, `src_place = assumption`인 비율 각각 |
| `modality_excluded_ratio` | NARRATIVE mention 중 `PAST`·`ASPIRATION`·`NEGATION`·`HYPOTHETICAL` ÷ NARRATIVE mention 수 (`unknown_ratio`도 함께) |
| `unresolved_ratio` | `place_scope = UNRESOLVED` ÷ (`place_scope ≠ NONE`인 mention 수) |

  부가 정보(진단): `template_counts`, `conflict_persons`, `flex_unplaced_rate`(= `flex_unplaced` ÷ 계획된 유연 블록 수), `late_arrival_rate`, `cancelled_by_move_rate`, `list_parse_failed_persons`, `korean_clock_hits`.
- 참고(프로토타입 20K 표본, 기대 범위일 뿐 합격선이 아니다): `grounded_time_ratio` ≈ 0.9%, 서울 밖 생략 mention을 가진 인물은 대부분(여행 서술 때문에 96% 안팎), 모달리티 제외 ≈ 6%, `unresolved` ≈ 0.7%.
- 오류: 분모 0이면 값 `0.0`, `denominator = 0`으로 적고 예외를 내지 않는다. 엣지: `--limit` 실행은 해당 인원 기준; 날짜 일부만 있는 실행.
- 자원: 파트 단위 스트리밍, 수십 초.
- 테스트 명세(E08-5; 합성 2명 × 1일, 구간·mention을 직접 지어 손계산한 값과 대조):

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_metrics_hand_computed` | 인물 A: 구간 `st = text` 1시간 30분 / 인물 B: 없음, 2 × 144 = 288슬롯, mention 6개(서술 4, 그중 희망 1 · 서울 밖 1) | `compute_metrics` | `grounded_time_ratio = 9/288`, `outside_omitted.persons = 1/2`, `modality_excluded_ratio = 1/4` 등 손계산과 같음 |
| `test_metrics_zero_denominator` | 서울 밖 mention 0건, UNRESOLVED 0건 | 계산 | 예외 없이 `0.0`과 `denominator = 0` |
| `test_metrics_travel_split` | `travel_persona` 안/밖 서울 밖 mention | 계산 | 두 값이 따로 나옴 |
| `test_metrics_json_roundtrip` | 결과 | 쓰고 읽기 | 동일 |

- DoD: 표 green, 서울 원본 앞 3,000명 실행에서 5종이 계산되어 `metrics.json`에 쓰임.

## I-08-d — 표본 검수와 합격선

- 추적: [C-08](01_concept_map.md#c-08--출처-재현성-품질-검증) 기준 ④, 테스트 E08-4. 관련 결정 [DR-20](decisions.md#dr-20--품질-합격선과-보관) ②③.
- 위치: `engine/verify/samplereview.py`.
- 시그니처:

```python
def select_sample(cue_persons: pyarrow.Table, persons: pyarrow.Table, seed: int, per_stratum: int = 25) -> list[str]     # uuid 200개
def write_review(run_dir: Path, uuids: Sequence[str], rules: RuleSet) -> None       # sample_review.md, labels.template.json
def evaluate(run_dir: Path, thresholds: dict | None = None) -> dict                 # labels.json → report/thresholds.json
```

- 선정([DR-20](decisions.md#dr-20--품질-합격선과-보관) ②): 층 = (직업 있음 × 65세 이상 × `has_outside_nontravel`) 8개. 층 안에서 uuid를 `sha256(f"{seed}|{uuid}")`의 16진 문자열 오름차순으로 정렬하고 앞 25명을 뽑는다. 층에 25명 미만이면 전부 뽑고 부족분을 가장 큰 층에서 같은 방식으로 이어서 보충한다. 합계는 항상 200명(전체 인원이 200명 미만이면 전원).
- `sample_review.md`: 인물마다 ① 나이·직업·거주 구·템플릿 ② 서술 발췌(필드별 앞 240자) ③ mention 표(대분류, 모달리티, 시각 등급, 장소 판정, `usable`, 제외 사유, 절 원문) ④ 첫 화요일과 첫 토요일 계획표(구간, 활동, 구, 출처 3종). `labels.template.json`은 mention마다 `"<uuid>:<mention_id>": {"activity_ok": null, "time_ok": null, "modality_ok": null, "category_ok": null, "outside_ok": null}`를 담는다(시각 단서가 없으면 `time_ok`는 쓰지 않음). 검토자가 `labels.json`으로 채워 둔다.
- `evaluate`: `thresholds.json`의 기본값은 [DR-20](decisions.md#dr-20--품질-합격선과-보관) ③이다 — `time_ab_precision ≥ 0.95`(등급 A/B mention 중 `time_ok`), `activity_precision ≥ 0.90`(usable mention 중 `activity_ok`), `modality_accuracy ≥ 0.90`(NARRATIVE mention 중 `modality_ok`), `category_accuracy ≥ 0.90`(usable mention 중 `category_ok`), `outside_false_omit_rate ≤ 0.05`(서울 밖 사유로 제외된 mention 중 `outside_ok = false`). 값, 분자·분모, 합격 여부, 라벨이 비어 있는 개수(`unlabeled`)를 `report/thresholds.json`에 쓴다. 라벨이 하나라도 비면 결과는 `incomplete`이고 합격이라고 말하지 않는다.
- 실행 기록과 결과는 읽기 전용으로만 쓴다(`report/` 밖 파일을 바꾸지 않는다).
- 오류: `labels.json`의 키가 템플릿에 없으면 `RunError`; 라벨 값이 `true/false/null`이 아니면 `RunError`. 엣지: 층이 비어 있음; 같은 인물이 여러 층에 속하지 않음(층은 서로소); 표본 인원이 200 미만.
- 자원: 가벼움(인물 200명).
- 테스트 명세:

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_select_sample_size_and_strata` | 합성 `cue_persons` 1,000명 | `select_sample` | 200명, 8개 층 각 25명, 층 조건이 맞음 (E08-4) |
| `test_select_sample_deterministic` | 같은 입력, 행 순서만 섞음 | 두 번 | 같은 목록 |
| `test_select_sample_small_stratum` | 한 층이 10명 | `select_sample` | 그 10명 전부 + 가장 큰 층에서 15명 보충, 합 200 |
| `test_write_review_files` | 합성 실행 | `write_review` | md와 템플릿 생성, 템플릿 키 수 = 표본 mention 수, 첫 화요일·토요일 표 포함 |
| `test_evaluate_thresholds` | 라벨을 채운 사본 | `evaluate` | 정밀도·분자·분모가 손계산과 같고 합격 판정이 맞음 (E08-4) |
| `test_evaluate_incomplete` | 라벨 일부 `null` | `evaluate` | `unlabeled > 0`, 판정 `incomplete` |
| `test_evaluate_bad_key` | 템플릿에 없는 키 | `evaluate` | `RunError` |
| `test_review_is_read_only` | 실행 | `write_review` 전후 | `report/` 밖 파일 해시 불변 |

- DoD: 표 green. 첫 서울 전원 실행에서 200명 라벨링을 마쳐 `thresholds.json`을 만든다(합격선 미달이면 규칙 버전을 올리는 DR을 추가).

## I-08-e — 단계 실행기와 명령행

- 추적: [C-07](01_concept_map.md#c-07--지역별-10분-프레임) 기준 ⑤와 사용자 조회 E2E, [C-08](01_concept_map.md#c-08--출처-재현성-품질-검증) 전 기준. 관련 결정 [DR-18](decisions.md#dr-18--저장과-조회-방식), [DR-20](decisions.md#dr-20--품질-합격선과-보관).
- 위치: `engine/pipeline.py`(`run_cues`·`plan_all`·`verify_run`·`run_all`), `engine/cli.py`(`main(argv)`).
- 시그니처:

```python
@dataclass(frozen=True)
class RunConfig:
    seed: int; start: datetime.date; days: int
    rule_version: str = "v1"; limit: int | None = None
    out_root: Path = RUNS_DIR                                    # data/runs
    source_parquet: Path = SEOUL_PARQUET; source_meta: Path = METADATA_JSON    # 원본 Parquet과 metadata.json
    rules_dir: Path = RULES_DIR                                  # rules/ (그 아래 <rule_version>/)
    chunk_persons: int = CHUNK_PERSONS

def load_gazetteer(cfg: RunConfig) -> Gazetteer                  # rules_dir/<버전>/geo/{seoul,outside}_places.csv
def load_geo(cfg: RunConfig) -> Geo                              # rules_dir/<버전>/geo/district_adjacency.csv + cfg.source_meta
def run_cues(cfg: RunConfig, run_dir: Path, rules: RuleSet, gaz: Gazetteer) -> dict     # I-04-f의 run_cues를 부르고 단계 통계(초·행 수) 반환
def plan_all(cfg: RunConfig, run_dir: Path, rules: RuleSet, geo: Geo) -> dict          # cues/ + 원본 → segments/, persons.parquet; 진단 Counter를 dict로 반환
def verify_run(cfg: RunConfig, run_dir: Path, rules: RuleSet, geo: Geo) -> dict         # 검사·지표·표본 산출물을 report/에 쓰고 {"checks": …, "metrics": …} 반환
def run_all(cfg: RunConfig) -> int                                                      # 아래 종료 코드를 반환, 예외를 밖으로 던지지 않음
# engine/cli.py
def main(argv: Sequence[str] | None = None) -> int                                      # 명령행을 RunConfig로 바꿔 실행, 종료 코드 반환
```

- 명령(모두 프로젝트 루트에서 `python3 -m engine.cli <명령> …`):

| 명령 | 하는 일 |
|---|---|
| `install-rules` | `02a_rule_data.md`에서 `rules/v1`을 설치·해시 검증 ([I-04-a](#i-04-a--규칙-로더와-텍스트-도구)) |
| `cues` | [I-04-f](#i-04-f--인물-단위-추출과-출력-파일) 실행 → `cues/` |
| `plan` | `cues/`를 읽어 청크별 계획·위치·이동 → `segments/`, `persons.parquet` |
| `frames` | 날짜 하나의 프레임을 `frames/<날짜>.parquet`로 내보냄 ([I-07-c](#i-07-c--프레임-내보내기와-조회)) |
| `verify` | [I-08-b](#i-08-b--전수-자동-검사)·[I-08-c](#i-08-c--보고-지표-5종) 실행, [I-08-d](#i-08-d--표본-검수와-합격선) 산출물 작성 |
| `evaluate` | `labels.json`을 평가 ([I-08-d](#i-08-d--표본-검수와-합격선)) |
| `query` | `--run R --date D --time HH:MM [--district 서울-양천구] [--rows 50]`: 그 시각 프레임을 표로 출력(구별 인원수 포함) |
| `explain` | `--run R --uuid U --date D`: 그 인물의 구간 표와 각 구간의 출처 라벨·근거 mention(절 원문, 필드, 오프셋) 출력 (E08-1) |
| `run-all` | `cues` → `plan` → `verify`를 한 번에 |

- 공통 옵션: `--seed`(정수, 필수), `--start`(YYYY-MM-DD, 필수), `--days`(필수), `--rule-version`(기본 `v1`), `--limit N`(앞 N명만, 개발용), `--out-root`(기본 `data/runs`), `--run`(기존 실행 이름). 개발·테스트용으로 `--source`(원본 Parquet, 기본 `data/nemotron-personas-korea/seoul.parquet`), `--source-meta`(기본 같은 폴더의 `metadata.json`), `--rules-dir`(기본 `rules`)를 두며 각각 `RunConfig.source_parquet`·`source_meta`·`rules_dir`가 된다. `--rule-version`과 `--rules-dir`로 `<rules-dir>/<버전>/` 아래 규칙·사전·인접표를 읽는다.
- `run-all` 흐름: ① 규칙·사전·인접표 로드와 검증(실패 `RuleError`) ② `run_id` 계산, `<out-root>/<run_id>` 또는 `.tmp-<run_id>`가 이미 있으면 `RunError`(덮어쓰지 않음) ③ `.tmp-<run_id>/`에서 `cues` → `plan`(청크 5,000명 × 날짜별 파트 쓰기) → `verify` ④ 모든 검사가 `ok`이면 `<run_id>`로 이름 변경하고 종료 코드 0, 아니면 `.failed-<run_id>`로 이름을 바꾸고 4 ⑤ 예외로 중단되면 `.tmp-…`을 지우고 해당 종료 코드. `plan_all`은 청크마다 `build_person_context` → 날짜별 `plan_person_day`(진단 Counter 누적) → `write_segment_part`를 호출하고 `persons.parquet`는 마지막에 쓴다.
- 종료 코드: 0 성공, 2 입력 오류, 3 규칙 오류, 4 불변식 위반, 5 실행 설정 오류, 75 `memguard`가 대기를 포기한 경우(래퍼가 돌려줌), 그 밖의 예외 1.
- 자원: 서울 전원(`--limit` 없음)은 `memguard run --label "CBU_Hack:run-all" --mem-gb 3 -- python3 -m engine.cli run-all …`로 감싼다(공통 규약). 7일 기준 총 시간은 추출 3~8분 + 계획 2~3분 + 검사 수 분이다. `--limit`을 준 개발 실행은 감싸지 않는다.
- 오류: `--seed`·`--start`·`--days` 누락은 argparse 오류(종료 코드 2); `--days ≤ 0`은 `RunError`. 엣지: `--start`가 과거이든 미래이든 상관없음(요일만 쓴다); `--limit`이 인원보다 크면 전원.
- 테스트 명세(통합·E2E):

| 테스트 | Given | When | Then |
|---|---|---|---|
| `test_cli_run_all_small` | 합성 Parquet 30명과 그에 맞는 `metadata.json`(`--source`·`--source-meta`), 설치된 `rules/`의 사본에 테스트용 인접표(25개 구 이름 위에 불변식을 만족하게 만든 가짜 그래프)를 넣은 디렉터리(`--rules-dir`) | `run-all --limit 30 --days 3` | `<run_id>/`에 `run.json`·`cues/`·`segments/`·`report/`가 있고 검사 모두 `ok`, 종료 코드 0 (E2E) |
| `test_cli_refuses_existing_run` | 같은 `run_id`가 이미 있음 | `run-all` | 종료 코드 5, 기존 파일 불변 |
| `test_cli_failed_run_not_published` | 검사를 실패시키는 주입(예: 규칙으로 일부 구간 손상) | `run-all` | `<run_id>`는 없고 `.failed-…`만 있음, 종료 코드 4 |
| `test_cli_install_rules` | 임시 디렉터리(`--rules-dir`) | `install-rules` | `<임시>/v1/`에 규칙 파일이 설치되고 `rule_set_sha256`이 02a의 값(파일 8개 기준)과 같음, 종료 코드 0 (E2E) |
| `test_cli_frames` | 소규모 실행 | `frames --run R --date D` | `frames/D.parquet`가 생기고 같은 시각의 행 수가 `query`와 같음, 종료 코드 0 |
| `test_cli_evaluate` | 소규모 실행과 라벨을 채운 `labels.json` | `evaluate --run R` | `report/thresholds.json`이 생기고 `evaluate()`의 반환값과 같음, 종료 코드 0 |
| `test_cli_requires_seed` | `--seed` 없음 | 실행 | 종료 코드 2 |
| `test_cli_query` | 소규모 실행 | `query --time 14:00 --district 서울-양천구` | 그 시각 그 구의 행만, 구별 인원수 출력 (E2E 조회) |
| `test_cli_explain` | 소규모 실행 | `explain --uuid U --date D` | 구간마다 출처 3종과 근거 절 원문(있는 경우) 출력 (E08-1) |
| `test_integration_c04_c05` | `cues/` 파일 | `plan`이 읽음 | 인물 수·mention 참조가 일치 |
| `test_integration_c05_c06` | 활동 구간 | `plan_person_day` | 위치·이동이 붙고 불변식 통과 |
| `test_integration_c06_c07` | 세그먼트 파트 | 프레임 샘플링 | 합계 보존 |
| `test_integration_real_slice` (느린) | 실제 원본 앞 300명 × 7일 | `run-all --limit 300` | 검사 통과, 지표 5종, 표본 산출물 생성 |

- DoD: 표 green. 서울 전원 `run-all`이 `memguard` 아래에서 끝나고 `cue_persons` 185,228행, 검사 전부 통과, `thresholds.json` 생성 전까지는 "품질 판정 미완"으로 보고.

## 활동 엔진 테스트 추적표

개념 Map의 완료 기준별 테스트(E04-1~E08-6, 28개)와 위 구현 테스트의 1:1 대응이다. 개념 테스트 하나에 구현 테스트가 여럿일 수 있고, 모든 개념 테스트에 하나 이상이 있다.

| 개념 테스트 | 구현 테스트 (단위) |
|---|---|
| E04-1 | `test_every_person_has_cue_person_row`, 서울 전원 `cue_persons` 행 수 185,228 ([I-04-f](#i-04-f--인물-단위-추출과-출력-파일)) |
| E04-2 | [I-04-b](#i-04-b--시간요일빈도역할-단서-추출기) 표(등급 A/B 근거 행), `test_evidence_span_roundtrip`, `test_example_persona` ([I-04-f](#i-04-f--인물-단위-추출과-출력-파일)) |
| E04-3 | [I-04-c](#i-04-c--모달리티-분류기) 15행 표, `test_merge_list_item`, `test_merge_ignores_generic_tokens` ([I-04-d](#i-04-d--활동-후보-15개-대분류-목록-항목-병합)) |
| E04-4 | [I-04-e](#i-04-e--장소-판정과-근무지-단서) 표, `test_trip_in_seoul_sentence`, `test_gazetteer_*` ([I-06-a](#i-06-a--지리-상수-3종)) |
| E04-5 | [I-04-b](#i-04-b--시간요일빈도역할-단서-추출기) 표의 비시각 행('6시 내고향'·'뉴스 9'·'톱10'·'3시간'), `test_korean_clock_detect_only` |
| E04-6 | `test_clause_categories`, `test_label_category`, `test_other_ratio_corpus` ([I-04-d](#i-04-d--활동-후보-15개-대분류-목록-항목-병합)) |
| E05-1 | `test_compose_covers_day`, `test_compose_midnight` ([I-05-d](#i-05-d--하루-활동-구간-조립)), `test_grid_*`·`test_place_*` ([I-05-c](#i-05-c--10분-슬롯-격자와-배치)), `test_snap10_and_label` ([I-05-a](#i-05-a--시간-눈금-키드-난수-스키마)) |
| E05-2 | `test_anchor_*` ([I-05-b](#i-05-b--템플릿-선택-근거-분류-일별-앵커)), `test_compose_cued_clock_event`, `test_compose_priority_clock_over_template` ([I-05-d](#i-05-d--하루-활동-구간-조립)) |
| E05-3 | `test_compose_provenance_labels`, `test_compose_pool_labels` ([I-05-d](#i-05-d--하루-활동-구간-조립)) |
| E05-4 | `test_select_template_matrix`, `test_conflict_jobless_with_work_cue` ([I-05-b](#i-05-b--템플릿-선택-근거-분류-일별-앵커)), `test_compose_work_minutes` ([I-05-d](#i-05-d--하루-활동-구간-조립)) |
| E05-5 | `test_compose_freq_counts`, `test_compose_weekend_scope`, `test_compose_prob_week`, `test_compose_shift_nights`, `test_compose_midnight` ([I-05-d](#i-05-d--하루-활동-구간-조립)) |
| E05-6 | `test_rng_golden_vectors` ([I-05-a](#i-05-a--시간-눈금-키드-난수-스키마)), `test_compose_pure_and_order_free`, `test_compose_weekly_independent_of_start` ([I-05-d](#i-05-d--하루-활동-구간-조립)) |
| E06-1 | `test_plan_person_day_invariants`, `test_every_slot_exactly_one_district` ([I-06-c](#i-06-c--이동-구간-삽입)), CHK-03·CHK-04 ([I-08-b](#i-08-b--전수-자동-검사)) |
| E06-2 | `test_move_*`, `test_same_district_move_is_10`, `test_no_move_at_day_start`, `test_move_midpoint_halves` ([I-06-c](#i-06-c--이동-구간-삽입)) |
| E06-3 | `test_location_kind_table`, `test_provenance_labels` ([I-06-b](#i-06-b--위치-종류-구-해석-근무지)) |
| E06-4 | `test_outside_never_assigned` ([I-06-b](#i-06-b--위치-종류-구-해석-근무지)), UNRESOLVED 행 ([I-04-e](#i-04-e--장소-판정과-근무지-단서)) |
| E06-5 | `test_resolve_*`, `test_draw_district_*`, `test_workplace_rules` ([I-06-b](#i-06-b--위치-종류-구-해석-근무지)), `test_geo_*` ([I-06-a](#i-06-a--지리-상수-3종)) |
| E07-1 | `test_sampler_conservation` ([I-07-b](#i-07-b--프레임-샘플러)), `test_export_row_group_per_slot` ([I-07-c](#i-07-c--프레임-내보내기와-조회)) |
| E07-2 | `test_sampler_conservation`, `test_sampler_empty_district` ([I-07-b](#i-07-b--프레임-샘플러)), `test_query_district_slice`, `test_district_counts_include_empty` ([I-07-c](#i-07-c--프레임-내보내기와-조회)) |
| E07-3 | `test_sampler_matches_reference`, `test_sampler_boundaries` ([I-07-b](#i-07-b--프레임-샘플러)), `test_export_evidence_lookup` ([I-07-c](#i-07-c--프레임-내보내기와-조회)), `test_segment_id_unique_and_formula` ([I-07-a](#i-07-a--세그먼트-저장소)) |
| E07-4 | `test_sampler_order_independent`, `test_sampler_split_equals_whole` ([I-07-b](#i-07-b--프레임-샘플러)), `test_export_deterministic` ([I-07-c](#i-07-c--프레임-내보내기와-조회)) |
| E07-5 | `test_iter_frames_holds_one_chunk` ([I-07-b](#i-07-b--프레임-샘플러)) |
| E08-1 | CHK-06 테스트 ([I-08-b](#i-08-b--전수-자동-검사)), `test_compose_provenance_labels`, `test_provenance_labels`, `test_cli_explain` ([I-08-e](#i-08-e--단계-실행기와-명령행)) |
| E08-2 | `test_run_meta_required_fields` ([I-08-a](#i-08-a--실행-메타데이터와-재생)) |
| E08-3 | `test_chk01_*` … `test_chk09_*` 주입 테스트 ([I-08-b](#i-08-b--전수-자동-검사)) |
| E08-4 | `test_select_sample_*`, `test_write_review_files`, `test_evaluate_thresholds` ([I-08-d](#i-08-d--표본-검수와-합격선)) |
| E08-5 | `test_metrics_hand_computed` ([I-08-c](#i-08-c--보고-지표-5종)) |
| E08-6 | `test_replay_matches`, `test_replay_rejects_changed_rules` ([I-08-a](#i-08-a--실행-메타데이터와-재생)) |

통합 테스트(개념 단위 경계마다 최소 1개): C-03→C-04 `test_corpus_smoke_20k`, C-04→C-05 `test_integration_c04_c05`, C-05→C-06 `test_integration_c05_c06`, C-06→C-07 `test_integration_c06_c07`, C-04~C-07→C-08 `test_integration_real_slice`. E2E는 `test_cli_run_all_small`·`test_cli_query`·서울 전원 `run-all`이다. 한 번 green이 된 테스트는 종료까지 green을 유지한다(회귀 원칙).

## SELF 결정 기록

개념 Map과 DR이 정하지 않은 구현 세부를 이 문서가 정한 것들이다(외부 계약에 영향이 없고 되돌리기 쉽다). 바꾸려면 이 표와 해당 단위를 먼저 고친다.

| ID | 결정 | 이유 | 되돌리는 법 |
|---|---|---|---|
| S-01 | 테스트는 `unittest`, pytest·hypothesis 미사용 | 설치되어 있지 않고 설치에 네트워크가 필요하며 사용자의 기존 도구(`memguard`)도 `unittest` | pytest를 설치하고 `unittest.TestCase`를 그대로 실행(호환) |
| S-02 | numpy는 프레임 샘플러·검사에만, 나머지는 표준 라이브러리 | 벡터 연산이 필요한 곳은 거기뿐이고 의존성을 줄임 | 순수 파이썬 샘플러로 교체(느려짐) |
| S-03 | 규칙은 JSON·CSV 파일로 외부화하고 `rule_set_sha256`을 실행에 기록 | 규칙 버전을 추적하고 재생 가능 ([DR-15](decisions.md#dr-15--시간-신호-해석과-추출-방식)) | 코드 상수로 되돌리면 버전 추적 상실 |
| S-04 | 난수는 SHA-256 시드 + SplitMix64 `KeyedRng` | Python `random`의 알고리즘 변경에 독립, 키별 독립 스트림, 처리 순서 무관 ([DR-17](decisions.md#dr-17--프레임과-시간-규약-및-결정성) ⑥) | 다른 PRNG로 바꾸면 모든 결과가 달라짐 → 규칙 버전 상승 |
| S-05 | 문장은 `.!?` 뒤 공백·줄바꿈, 절은 쉼표·세미콜론으로만 나눔 | 서술 문장이 쉼표로 병렬 절을 잇는 형태가 지배적(프로토타입 확인), 연결어미 분리는 오류가 커짐 | 연결어미 규칙을 추가하면 규칙 버전 상승 |
| S-06 | 방송명('6시 내고향'·'뉴스 9'·'톱10'·'N 시청')과 사람 이름('…씨')을 마스킹 | 오탐 제거 ([DR-15](decisions.md#dr-15--시간-신호-해석과-추출-방식) ④) | `text.json`에서 패턴 삭제 |
| S-07 | 빈도 기본값: `평일…` → 주 3~5회, 주말 표현 → 주말 1회, 빈도 단서 없이 시각·요일만 있는 일정 → 주 1~2회 | [DR-15](decisions.md#dr-15--시간-신호-해석과-추출-방식) ⑤가 정하지 않은 빈도의 기본값. 데이터에 없는 수치라 설계 상수 | `time.json`/`compose.py` 수정 → 규칙 버전 상승 |
| S-08 | mention은 (대분류 hit)와 (시각·요일·빈도·역할·장소 단서)가 함께 있을 때만 만들고, 절당 대분류 최대 2개, 기술 목록은 mention을 만들지 않음 | 단서 없는 절은 활동 어휘로만 가치가 있어 목록 항목이 맡고, 과생성 방지 | 조건 완화는 `activities.json` + 규칙 버전 상승 |
| S-09 | `WORK` 대분류 mention은 역할 또는 야간·교대 근무 문구가 있을 때만 | 일반 업무 서술을 일과 구간으로 오인하지 않음 | 규칙 변경 |
| S-10 | 목록 항목 병합: 같은 대분류 + 3음절 이상 내용 토큰 공유 + 불용어 제외 | 서술의 시각·빈도 단서를 깨끗한 항목 이름에 붙임. '천천히' 같은 부사 병합 방지 | 불용어 조정 |
| S-11 | 같은 문장에 서울 장소가 있으면 '여행' 같은 일반 여행어만 있는 절을 서울 밖으로 보지 않음 | '창덕궁을 걸으며 … 정적인 여행' 오판 방지(프로토타입 확인) | 규칙 삭제 시 서울 밖 생략 비율 상승 |
| S-12 | 국가·해외·전국 등은 여행 문맥이 같은 절에 있을 때만 서울 밖(`ctx=TRIP`) | '일본 만화책' 오탐 방지 | CSV `ctx` 삭제 |
| S-13 | 장소 판정은 사전 조회만 쓰고 접미사로 서울 밖을 추정하지 않음. hit 우선순위: 서울 밖 vs 서울이 섞이면 활동 키워드에 가까운 쪽(동률은 서울 밖), 서울끼리는 후보가 적은 쪽·명시 구 이름 우선 | '여의도'(도)·'…거리'(의성어) 같은 접미사 오판 방지 | 사전 확장 |
| S-14 | `workplace_hint`: 직업 있는 인물의 `professional_persona`에서 처음 나온 서울 구 후보(1~3개) | 직업 있는 인물의 약 40%가 갖는 서술 근거로 근무지를 정함 ([DR-13](decisions.md#dr-13--장소-해석과-지리-상수) ③) | 단서 미사용 시 [DR-19](decisions.md#dr-19--기본-일과-템플릿) 확률만 사용 |
| S-15 | `usable`·cued·pool 분류 정의([I-05-b](#i-05-b--템플릿-선택-근거-분류-일별-앵커)) | 어떤 mention이 일과를 직접 정하고 어떤 것이 이름 후보인지의 경계 | 분류 조정 |
| S-16 | 슬롯 격자 우선순위: 수면 → 시각 일정 → 식사 → 근무 → 시간대·빈도 일정 → 템플릿 유연 블록(희귀한 것 먼저) → 자녀 돌봄 → 채우기 | [DR-14](decisions.md#dr-14--구조화-필드의-사용과-근거-우선순위) ③ 우선순위를 슬롯 배치로 구현. 희귀 블록을 먼저 두지 않으면 배치 실패가 늘어남(프로토타입 1,500명 × 7일: 인물·일당 미배치 유연 블록 0.126개 → 0.081개) | 순서 변경 → 규칙 버전 상승 |
| S-17 | 수면 5~10시간 보정, 취침이 24시를 넘으면 다음 날 앞에 `취침 전 여가` | 이틀 계획의 독립성과 자정 이어짐을 순수 함수로 보장 | — |
| S-18 | 채우기: 기상 직후·취침 직전 30분 `PERSONAL`, 20분 이하 틈은 앞 활동에 흡수, 30분은 `REST`, 그보다 길면 `HOBBY(자유 시간)` | 조각 난 구간 폭증 방지(인물·일당 구간 수 안정) | 흡수 한계 조정 |
| S-19 | 야간 교대: 월~금 22:00~다음 날 06:00, 낮 수면, 토·일 밤에는 새 근무 없음 | [DR-19](decisions.md#dr-19--기본-일과-템플릿)의 교대·야간 열을 최소 규칙으로 구현 | 템플릿 수정 |
| S-20 | 위치 종류 규칙과 23시 이후 `HOME` | 하루가 집에서 끝나야 날짜 경계의 위치가 연속(순수 함수 유지) | `location.json` 수정 |
| S-21 | 이동 삽입: 도착 구간의 시작은 되도록 유지하고 앞 구간에서 먼저 가져오되(필러는 연쇄 소비), **시각 단서가 있는 구간과 WORK 구간은 끝을 지켜 깎지 않고 도착을 늦춘다**. 모자라면 도착 지연, 도착 구간이 지연을 못 받으면 이동 취소 | 활동이 지워지는 연쇄 삭제를 막고 [DR-14](decisions.md#dr-14--구조화-필드의-사용과-근거-우선순위) ③ 시각 단서 우선과 [DR-19](decisions.md#dr-19--기본-일과-템플릿) 근무 8시간을 지킨다. 프로토타입(1,500명 × 7일): 처음 방식은 구간을 인물·일당 0.32개 지웠고, 근무를 깎는 방식은 평일 근무가 440~480분으로 줄었으며, 지금 방식은 평일 근무 중앙값 480분·도착 지연 0.77건·이동 취소 0.042건 | 정책 변경 → 규칙 버전 상승 |
| S-22 | 청크 5,000명, 날짜별 파트 파일, 청크 순서로만 쓰기 | 메모리 상한과 `segment_id` 결정성 | `CHUNK_PERSONS` 조정 |
| S-23 | 프레임은 numpy로 시각 하나씩, 행 그룹 = 시각 | [DR-18](decisions.md#dr-18--저장과-조회-방식)의 파생 뷰·청크 스트리밍 | — |
| S-24 | 실행 디렉터리를 `.tmp-…`에 쓰고 검증 후 이름 변경, 실패는 `.failed-…` | 검증 전 산출물 노출 금지 ([I-03-a](#i-03-a--재독-검증과-최종-인계-c-03)와 같은 원칙) | — |
| S-25 | 표본 검수 층 선정은 SHA-256 정렬, 부족한 층은 가장 큰 층에서 보충 | 시드 고정·순서 무관 선정 | — |
| S-26 | 규칙 데이터 부록을 별도 Map 파일 `02a_rule_data.md`로 둠 | 02가 지나치게 길어지지 않게 하고 데이터와 코드를 분리 | 02에 합치면 됨 |
| S-27 | `explain`·`query` 명령을 CLI에 둠 | E08-1·E2E 조회를 UI 없이 검증 | — |

## 남은 위험과 Phase 6으로 넘기는 것

- **선행 데이터 작업**: ① `scripts/install_rules.py`로 `rules/v1` 설치 ② 구 인접표와 서울 장소 사전의 공식 자료 대조(네트워크 필요, 출처 기록) ③ 사전 커버리지 95% 확인. 이 데이터가 없으면 [I-04-e](#i-04-e--장소-판정과-근무지-단서)·[I-06-b](#i-06-b--위치-종류-구-해석-근무지)·[I-06-c](#i-06-c--이동-구간-삽입)를 끝까지 돌릴 수 없다.
- **수직 슬라이스에서 측정할 것**([DR-18](decisions.md#dr-18--저장과-조회-방식)): 세그먼트 수·디스크 크기, 프레임 내보내기 크기·시간, 서울 전원 실행 시간. 이 문서의 수치는 프로토타입 외삽이다.
- **사람이 하는 일**: 표본 200명 라벨링([DR-20](decisions.md#dr-20--품질-합격선과-보관)). 합격선 미달이면 규칙 버전을 올리는 DR을 추가하고 재실행한다.
- **알려진 한계**: 시각 근거가 있는 시간은 전체의 1% 안팎이라 결과는 대부분 설계 상수다. 직업 1,748종은 교대·야간 문구 외에는 주간 근무로 본다. 학업 중인 무직은 구분하지 않는다. 지리 사전 시드는 초안이다. 공휴일은 주말로 바꾸지 않는다. 단일 프로세스라 서울 전원 7일이 10분 안팎 걸린다(병렬화는 v1 밖).

## I-09-a — 명동 관련 행 추출 및 재독 검증 (C-09)

- 위치: `scripts/extract_myeongdong_personas.py`; 입력 `data/nemotron-personas-korea/seoul.parquet`, `metadata.json`; 출력 `data/myeongdong-personas/myeongdong.parquet`, `myeongdong.csv`, `match_evidence.csv`, `metadata.json`, `README.md`. 기존 활동 엔진 구현과 독립 작업이다.
- 기술: 설치된 Python 3.13.1, PyArrow 24.0.0, 표준 라이브러리 `re`, `csv`, `json`, `hashlib`, `pathlib`, `tempfile`. 원본과 같은 Parquet 스키마 보존 및 Excel 등에서 열 수 있는 UTF-8 BOM CSV를 위해 선택한다.
- 시그니처: `matched_tokens(text: str | None) -> set[str]`, `file_sha256(path: Path) -> str`, `main() -> None`. 명령은 `python3 scripts/extract_myeongdong_personas.py`.
- 입력 구조: 원본 메타데이터 `seoul_rows=185228`, `output_sha256`과 고정 리비전을 확인한다. Parquet은 원본과 같은 26개 열이며 `age`는 int64, 나머지는 string이다. 원본의 각 필드 null 가능성은 그대로 보존한다.
- 선택 명세: 25개 문자열 열 각각에서 `[가-힣A-Za-z0-9·]*명동[가-힣A-Za-z0-9·]*` 토큰을 찾아 전체 토큰 목록을 분류한다. 포함 토큰은 `명동`, `명동의`, `명동이나`, `명동과`, `명동이`, `명동보다는`, `명동에서`, `명동처럼`, `명동성당`, `명동성당의`, `명동성당이나`, `명동성당을`, `명동성당에서`, `명동성당에`, `명동성당으로`, `명동역점에`, `명동역점에서`, `명동교자에서`, `명동삼겹살`, `명동집`이다. 광명동/광명동굴 5가지 형태와 이명동·권명동·정명동·명동건·명동준·명동언 이름 6가지는 제외한다. 관측하지 못한 토큰이 새로 발견되면 추측하지 않고 실패해 재검토한다.
- 근거 구조: 원본 행에는 새 열을 넣지 않는다. `match_evidence.csv`는 `uuid`, `matched_fields`(원본 열 순서대로 `|` 연결), `matched_tokens`(정렬 후 `|` 연결), `match_type`(`place_mention` 또는 `business_only`)을 필수로 보유한다. `명동교자에서`, `명동삼겹살`, `명동집`만 일치하면 `business_only`이고 이는 실제 소재지가 미확정임을 의미한다. 원문에 지명/시설 언급도 있으면 `place_mention`으로 둔다.
- 처리 단계: ① 기존 최종 출력 디렉터리가 있으면 덮어쓰지 않고 중단한다. 원본 파일 SHA-256과 메타데이터/스키마를 확인한다. ② 최대 2,048행 배치로 26개 원본 필드를 전부 읽고 토큰을 분류하여 포함된 행과 근거를 원본 순서로 모은다. ③ 서울 원본 전체 185,228건, 단순 부분 문자열 후보 552건, 본 결과 514건, 지명·시설 510건, 상호만 4건, 제외 38건, 미분류 토큰 0건인지 확인한다. ④ 임시 출력 디렉터리에 Parquet, 원본 26열의 UTF-8 BOM CSV, 근거 CSV, 메타데이터, README를 쓴다. ⑤ I-09-a 검증을 통과하면 디렉터리를 최종 이름으로 확정한다.
- 메타데이터: `dataset`, `revision`, `source_file`, `source_sha256`, `source_rows`, `substring_candidate_rows`, `selected_rows`, `place_mention_rows`, `business_only_rows`, `business_only_uuids`, `excluded_other_place_or_name_rows`, `unknown_tokens`, `matched_district_counts`, `columns`, `result_sha256`, `license`, `source_url`을 포함한다. CSV에서는 null을 빈 셀로 표기하고 Parquet은 원본 null을 보존한다.
- 에러 정책: 소스 부재, 해시·행 수·스키마 불일치, 알려지지 않은 명동 토큰, 의도한 분류·수량 차이, UUID 중복, 출력/재독 검증 실패, 기존 최종 출력은 오류를 내고 임시 결과를 확정하지 않는다. 원본 파일은 읽기만 한다.
- 엣지: 빈 문자열/null은 미일치; 하나의 인물이 여러 열에서 중복 언급해도 한 번만 출력; 이름이 명동을 포함하더라도 다른 열에 명동 장소가 있으면 포함; 상호와 장소를 함께 언급하면 `place_mention`; 명동 표현이 없는 주변 장소는 추측하지 않는다.
- 성능/자원: 입력은 2,048행 배치; 선택된 수백 행만 메모리에 보유. 네트워크 없음. 소스 파일 해시 계산은 전체 원본 한 번 읽음.
- 테스트 명세: 별도 구현 복제 테스트 파일 대신 실제 원본과 출력 경계에서 검증한다. `명동_독립선택` Given 실제 서울 원본, When 별도의 지명/시설/상호 정규식으로 전수 비교, Then 출력 UUID·순서가 동일하고 514건이다(E09-1). `명동_오탐과_상호` Given 실제 광명동굴·인명 및 상호만 4건, When 분류하면, Then 앞의 38건은 빠지고 4건은 포함·식별된다(E09-2). `명동_원본보존` Given 선택 원본 행, When Parquet 재독, Then 스키마·값·행 순서가 동일하다(E09-3). `명동_인계` Given Parquet과 CSV·메타데이터, When 재독하면, Then CSV가 원본 값에 대응하고 근거의 UUID가 정확히 일치하며 출처·상호 불확실성이 명시된다(E09-4). 필요한 픽스처는 실제 원본의 사례다.
- DoD: 위 실제 데이터 경계 검증 모두 통과; 원본은 수정하지 않음; 최종 출력 파일 재독 성공.

## 명동 지면 고도 구현 명세

독립 작업 C-10~C-12. 사용자 선택은 [DR-24](decisions.md#dr-24--명동-지형-자료의-종류와-범위)·[DR-25](decisions.md#dr-25--고도행정동-경계-자료의-출처와-조건), 구현 세부는 [DR-26](decisions.md#dr-26--지형-자료의-저장과-검증)이다. 기존 C-09와 활동 엔진은 선행 조건이 아니다.

명동 지면 고도 트랙의 I-10-a~I-12-a는 구현·검증 완료(2026-10-31). 본 문서의 draft 상태는 아직 구현되지 않은 활동 엔진 트랙을 반영한다.

### 공통 계약

- 언어/라이브러리: Python 3.13.1, Rasterio 1.4.3(GDAL 3.9.3), Shapely 2.1.2, NumPy 2.5.4 및 표준 라이브러리. `scripts/requirements-myeongdong-terrain.txt`에 세 패키지 버전을 고정한다. GeoTIFF 처리·유효 도형 검사·원본 값 배열 보존을 위한 선택이다.
- 경계 원본: `https://raw.githubusercontent.com/vuski/admdongkor/d5d5a821c8de6cc337d47eeca604d7af76271c9b/ver20261001/HangJeongDong_ver20261001.geojson`; SHA-256 `843ece3a0b3429742cd4a1b699cc13987a88b5e5401f382e7fdaaa194ea5e605`. GeoJSON FeatureCollection, 좌표 `OGC:CRS84`, `adm_cd2 == '1114055000'`, `adm_nm == '서울특별시 중구 명동'`인 Feature 1개.
- 고도 원본: `https://s3.opengeohub.org/global/dtm/v1.2/gedtm_rf_m_30m_s_20060101_20151231_go_epsg.4326.3855_v1.2.tif`; ETag `\"8bf9b35f45e9befaa9b91d877bf230d7-3107\"`(해시가 아님). EPSG:4326, 1 밴드 Float32, 간격 1/3600도, NoData `3.4028234663852886e+38`; 고도 m, EGM2008 지오이드(EPSG:3855). 재표본화 금지.
- 산출물: `data/myeongdong-terrain/terrain_gedtm30_v1.2.tif`, `boundary.geojson`, `metadata.json`, `README.md`. 세계 격자는 보관하지 않고 명동과 교차하는 원본 격자만 가져온다.
- 실패 원칙: 원본 식별값·범위 불일치, 내부 결측, 비유한 값, 검증 실패, 기존 출력은 명시적 오류로 종료하고 최종 디렉터리를 생성하거나 덮어쓰지 않는다. 임시 디렉터리는 종료 시 정리한다.

### I-10-a — 원본과 경계 확인 (C-10)

- 위치: `scripts/download_myeongdong_terrain.py`; `tests/test_myeongdong_terrain.py`.
- 시그니처: `sha256_file(path: Path) -> str`; `select_boundary(payload: bytes, expected_sha256: str) -> dict`; `fetch_boundary(url: str) -> bytes`; `check_source(url: str, expected_etag: str) -> None`; `validate_source(src: rasterio.io.DatasetReader) -> None`.
- 데이터 구조: `select_boundary`는 원본 Feature dict를 반환하며 `type`, `properties.adm_cd2`, `properties.adm_nm`, `geometry.type`, `geometry.coordinates`가 필수다. 원본 속성과 좌표를 보존한다.
- 로직: ① 경계 파일을 제공받거나 HTTP GET으로 최대 60MB까지 읽는다. ② SHA-256 검증 후 JSON과 CRS84를 확인하고 정확한 코드·이름이 유일한 Feature를 골라 Shapely로 Polygon/MultiPolygon 유효성을 확인한다. ③ 고도 URL HEAD에서 ETag를 확인하고 16바이트 Range 요청에 HTTP 206/Content-Range가 반환되는지 확인한다. ④ GDAL `/vsicurl/`로 원본을 열고 CRS·해상도·밴드·타입·결측값을 검증한다. 처리 전후 ETag를 다시 대조한다.
- 에러: HTTP 오류·초과 크기·해시 불일치·잘못된 JSON/CRS·누락/중복 코드·이름 불일치·빈/무효/면이 아닌 도형·부분 읽기 미지원·ETag/격자 불일치면 오류로 중단한다. CLI는 오류를 stderr에 알리고 0이 아닌 값으로 끝난다.
- 엣지: 빈 바이트·빈 FeatureCollection·중복 코드·빈 좌표·누락 속성은 실패한다. 유효한 다중 폴리곤은 허용한다. 동명이 지역은 행정동코드로 구분한다.
- 성능/자원: 경계 원본 최대 60MB, 고도 원본은 HTTP Range만 사용한다. 연결/GDAL 타임아웃 60초, GDAL 재시도 1회다.
- 테스트: `test_select_boundary_valid` Given 유효 원본 조각과 해시 When 선택 Then 지정 코드 하나와 원본 도형 보존(E10-1/2); `test_select_boundary_rejects_invalid` Given 해시 불일치·JSON/CRS 오류·누락/중복·다른 이름·무효/빈 도형 When 선택 Then 오류(E10-1/2); `test_validate_source_rejects_mismatch` Given 정상 격자 및 CRS·간격·타입·밴드·NoData가 다른 격자 When 검증 Then 정상만 통과(E10-3). 실제 원본 통합 확인은 T-38이다.
- DoD: 원본 식별·경계 선택 계약과 해당 테스트 모두 통과.

### I-11-a — 격자 추출과 GeoTIFF (C-11)

- 위치: `scripts/download_myeongdong_terrain.py`; `tests/test_myeongdong_terrain.py`.
- 시그니처: `crop_terrain(src: rasterio.io.DatasetReader, geometry: dict) -> tuple[numpy.ma.MaskedArray, affine.Affine]`; `write_raster(path: Path, data: numpy.ma.MaskedArray, transform: affine.Affine, crs: rasterio.crs.CRS, nodata: float) -> None`.
- 데이터 구조: 결과는 `(1, height, width)` Float32 masked array와 원본에 정렬된 affine 변환이다. 외부 셀의 마스크는 True이며 저장할 때 원본 NoData를 쓴다. CRS·격자·m 단위를 유지한다.
- 로직: ① 유효 경계의 외접 사각형이 원본 범위 안에 있는지 확인한다. ② `rasterio.mask.mask(src, [geometry], crop=True, all_touched=True, filled=False)`를 실행한다. ③ 같은 영역의 경계 교차 마스크를 만들어 경계 교차 셀 중 원본 결측이나 비유한 값이 하나라도 있으면 실패한다. ④ 단일 밴드 DEFLATE 압축 Float32 GeoTIFF로 원본 값·CRS·변환·NoData를 저장한다.
- 에러: 경계가 원본 범위 밖, 겹침 없음, 교차 픽셀 0, 교차 픽셀의 NoData·NaN·무한대, 쓰기 실패면 중단한다.
- 엣지: 경계에 닿기만 한 픽셀도 포함한다. 0m 및 음수 고도는 유효하다. 폴리곤의 구멍과 교차하지 않는 픽셀은 제외한다.
- 성능/자원: 사전 점검상 57×57픽셀, 유효 셀 1,462개. 예상값은 진단값이며 실제 검증 결과가 정본이다.
- 테스트: `test_crop_preserves_values_and_boundary` Given 값이 서로 다른 합성 격자와 경계 When 자름 Then 경계 교차 셀은 원본 값·좌표를 유지하고 나머지는 NoData(E11-1/2/4); `test_crop_rejects_invalid_coverage` Given 영역 밖·교차 셀 0·내부 NoData·NaN/Inf When 자름 Then 오류(E11-3); 실제 원본 대조와 저장 파일 재독은 T-38이다.
- DoD: 원본 셀과 격자 보존 및 지정된 실패 테스트 통과.

### I-12-a — 검증과 결과 패키지 (C-12)

- 위치: `scripts/download_myeongdong_terrain.py`; 최종 `data/myeongdong-terrain/`; 검증 `tests/test_myeongdong_terrain.py`.
- 시그니처: `verify_raster(path: Path, data: numpy.ma.MaskedArray, transform: affine.Affine, crs: rasterio.crs.CRS, nodata: float) -> dict`; `build_package(boundary_payload: bytes, output_dir: Path) -> None`; `main() -> None`.
- 저장 스키마: `boundary.geojson`은 단일 FeatureCollection. `metadata.json`은 `schema_version`(1), `created_at_utc`(ISO 8601 UTC), `area`(`name`, `admin_code`, `statistics_code`, `boundary_file`, `boundary_sha256`, `boundary_source_url`, `boundary_source_sha256`, `boundary_version`, `boundary_license`), `elevation_source`(`name`, `version`, `doi`, `url`, `etag`, `license`, `source_period`, `vertical_datum`, `unit`), `raster`(`file`, `sha256`, `crs`, `width`, `height`, `transform` 6수, `nodata`, `selection`, `valid_cells`, `min_m`, `max_m`)를 모두 가진다. 필드는 필수이며 null 불가. `README.md`는 출처·원본 기간·CC BY 4.0·SGIS 및 저장소 표기·30m 한계·경계 교차 셀·수직 기준을 명시한다.
- 로직: ① `python scripts/download_myeongdong_terrain.py [--boundary-file PATH] [--output-dir PATH]`에서 기본 출력은 위 경로. 경계 파일이 생략되면 고정 URL을 읽는다. ② 출력 존재 여부를 확인하고 같은 부모의 임시 디렉터리에 GeoJSON·GeoTIFF를 쓴다. ③ GeoTIFF를 다시 열어 CRS·변환·치수·NoData·마스크·모든 유효 고도값을 추출 결과와 정확히 대조하고 파일 해시 및 통계를 계산한다. ④ 원본 ETag를 재확인하고 메타데이터·README를 쓴다. ⑤ 네 파일 및 경계·고도 재독 검사 성공 시에만 임시 디렉터리를 최종 경로로 원자적 이동한다.
- 에러: 재독 오류·배열 불일치·메타데이터 쓰기 실패·기존 출력 경로·변경된 ETag는 CLI 오류이며 확정하지 않는다. 기존 디렉터리는 삭제·수정하지 않는다.
- 엣지: 0·음수도 통계에 포함한다. 기존 출력은 덮어쓰지 않는다. 메타데이터 해시의 자기참조는 만들지 않는다.
- 성능/자원: 경계 원본 60MB 이하, GeoTIFF는 명동 외접 범위만; 임시·최종 파일은 같은 부모 파일시스템에 둔다.
- 테스트: `test_verify_raster_detects_corruption` Given 정상 및 값·마스크·CRS가 바뀐 임시 GeoTIFF When 재독 대조 Then 정상만 통과(E12-1/3); `test_output_exists_fails` Given 기존 출력 When 패키지 생성 Then 수정 없이 오류(E12-4). T-38에서 실제 결과의 필수 필드·출처·라이선스 및 해시·통계를 독립 재계산하고(E12-1/2/3) E2E를 확인한다.
- DoD: E10-1~3·E11-1~4·E12-1~4의 단위·통합·실데이터 E2E 통과, 지정 파일 네 개 보관.

## I-90-a — 명동 3D 시각화 (C-90)

상태: 구현·검증 완료(2026-10-31). 원본 격자 전체 대조와 브라우저 회전·확대·고도 조회 및 320px 가로 넘침 없음 확인.

- 위치: `/Users/aiden/.codex/visualizations/2026/10/30/01a18db6-c08e-7591-9964-e33d04246a8e/myeongdong-terrain-3d.html`(대화형 HTML fragment). 입력 `data/myeongdong-terrain/terrain_gedtm30_v1.2.tif`, `metadata.json`; 출력은 프로젝트의 원본을 바꾸지 않는다.
- 기술과 이유: Python 3.13.1 + Rasterio 1.4.3 + NumPy 2.5.4로 원본을 읽어 값·마스크를 JSON으로 직렬화하고, 브라우저에서 D3 7.9.0의 `geoMercator`로 원본 경위도를 투영해 Plotly.js 2.35.2의 `mesh3d`로 회전·확대 가능한 셀 면을 그린다. 두 브라우저 라이브러리는 버전 고정된 허용 CDN을 사용한다.
- 데이터 계약: 직렬화 객체 `{width: int, height: int, transform: [a,b,c,d,e,f], values: Array<Array<number|null>>, min: number, max: number, valid: int}`. 너비·높이는 각각 57, 유효 셀 수는 1,462다. 값은 원본 Float32를 숫자로 보존하고 NoData만 `null`이다.
- 인터페이스: `initializeMyeongdongTerrain(root: HTMLElement, data: object) -> void`; 각 유효 `[row,col]`의 네 모서리를 `geoMercator().center([중심 경도,중심 위도]).scale(6371000*cos(중심 위도)).translate([0,0])`으로 투영한다. 출력 좌표는 x=동쪽 미터, y=북쪽 미터(투영 y 부호 반전), z=원본 고도 미터이다. `mesh3d`의 각 셀에 두 삼각형을 쓰고 `aspectmode: 'data'`로 수평·수직 길이 비율을 1×로 유지한다. 각 윗면의 높이는 그 셀 고도로 일정하며, 인접한 유효 셀의 공통 변에는 양쪽 원본 고도를 잇는 이음면 두 삼각형을 추가한다([DR-91](decisions.md#dr-91--3d-격자-이음면과-접근-가능한-조작)). 결측 사이에 면을 만들거나 새로운 고도 격자를 보간하지 않는다.
- 로직: ① GeoTIFF SHA-256을 메타데이터와 대조하고 크기·마스크·최소/최대·좌표계를 검증한다. ② 읽은 격자와 affine 변환을 HTML 내부에 포함한다(실행 시 파일/네트워크 데이터 fetch 없음). ③ 유효 셀마다 원본 면 두 삼각형을, 유효 이웃끼리는 연결 면을 만들고 고도별 색·축 제목·단위·해상도·수직 배율·출처를 표시한다. ④ Plotly의 기본 회전·확대·고도 조회와 키보드로도 조작할 수 있는 방향·확대 입력을 활성화하고 화면 폭 변경 시 크기를 조절한다. ⑤ 테마 토큰·글로벌 테마 변경 시 고도 색상과 축·격자색을 다시 계산한다([DR-93](decisions.md#dr-93--3d-색상의-화면-테마-대응)).
- 스키마/기본값: `values[row][col]`은 유효한 Float32 높이 또는 `null`이며 nullable 값은 원본 NoData에만 해당한다. 기준점은 그리드 중앙이고 CRS는 EPSG:4326, 고도 기준은 EGM2008, 1초각이다. 표시명은 명동 행정동, 단위는 m, 수직 배율 기본값 1×다.
- 오류: 해시·셀 수·좌표계·통계가 다르면 생성을 중단한다. CDN 또는 WebGL 실패 시 대화 내 표시 영역에 오류를 표시하며 성공이라고 보고하지 않는다. 빈 격자·비유한 값은 생성 실패, 마스크 된 셀은 표시하지 않는다.
- 엣지: 0m와 음수도 유효하다. 경계에 닿는 픽셀은 셀 전체가 보이므로 일부는 행정동 밖으로 걸친다. 바다·건물·도로는 새로 만들지 않는다. 폭 480px 미만에서는 3D 축을 숨겨 잘림을 방지하되 고도 범위·단위와 출처는 표시한다([DR-92](decisions.md#dr-92--좁은-화면의-3d-축-표시)). 폭 320~736px에서 라벨·색상바·그래프가 잘리지 않는다.
- 성능/자원: HTML fragment 1MB 이하, 유효 1,462셀·윗면 2,924삼각형과 유효 이웃 2,793쌍의 이음면 5,586삼각형, 원본을 재다운로드하지 않음. 표면과 그래프는 단일 인스턴스로 유지한다.
- 테스트 명세: `test_terrain_payload` Given 검증 원본 When HTML용 숫자/마스크 생성 Then SHA·형태·1,462개 값·통계가 일치한다(E90-1/2). `test_projection_contract` Given 격자 모서리 When 미터 좌표로 변환 Then 동쪽/북쪽 방향과 1× 축 비율이 일관된다(E90-2/3). `test_3d_render_interaction` Given 렌더링된 대화형 fragment When 브라우저에서 열고 방향·확대를 변경 Then 표면·축·출처가 보이고 화면이 실제 갱신된다. 유효 이웃 사이만 이음면이 생기며 고도 조회를 제공한다(E90-3/4). 실제 브라우저와 원본을 픽스처로 쓴다.
- 완료 정의: 원본과 표시 격자·마스크 대조 통과, 브라우저 렌더링·회전·확대·고도 조회 확인, 출처와 해상도 표시, 원본 불변.

## 명동 최종 스무딩 지형 구현 명세

### I-91-a — 표준 스무딩 격자와 최종 데이터 (C-91)

상태: 구현·검증 완료(2026-10-31). 유효 1,462셀의 독립 SciPy 값과 결과가 정확히 일치하며, 원본·출력 해시와 마스크·메타데이터를 확인했다.

- 위치: `scripts/finalize_myeongdong_terrain.py`, `tests/test_finalize_myeongdong_terrain.py`; 입력 `data/myeongdong-terrain/terrain_gedtm30_v1.2.tif`와 원본 `metadata.json`; 출력 `data/myeongdong-terrain/final/terrain_smoothed.tif`, `metadata.json`, `README.md`.
- 기술 선택: Python 3.13.1, Rasterio 1.4.3, NumPy 2.5.4. 이미 설치된 지리·배열 패키지로 수행하며, 단위 테스트는 표준 라이브러리 `unittest`, 실데이터 독립 대조는 이 PC의 SciPy 1.17.1을 별도 Python 환경에서 사용한다.
- 시그니처: `gaussian_smooth(data: numpy.ma.MaskedArray, sigma: float = 1.0, radius: int = 4) -> numpy.ma.MaskedArray`; `file_sha256(path: pathlib.Path) -> str`; `finalize(source_dir: pathlib.Path, output_dir: pathlib.Path) -> None`; `main() -> None`.
- 입력/출력 스키마: 입력은 2차원 Float32 masked array. 출력은 동일한 shape·마스크의 Float32 masked array. GeoTIFF는 입력의 width·height·EPSG:4326·affine·단일 Float32 밴드·NoData를 유지한다. 고도 단위 m, 수직 기준 EGM2008(EPSG:3855).
- 로직: ① 원본 GeoTIFF의 SHA-256을 원본 메타데이터와 대조하고 CRS·치수·마스크·유효 셀 수(1,462)·유효값 유한성을 검증한다. ② 정수 오프셋 `[-4,4]`의 1차원 가중치 `w[k]=exp(-0.5*(k/1.0)**2)`를 외적해 9×9 커널을 만든다. ③ 값·유효 마스크의 바깥을 0으로 패딩하고 각각 커널로 합성곱한 뒤, 유효 셀에서 `값 가중합 / 유효 가중치 합`을 계산한다. Float64 중간 연산 후 Float32로 저장한다. NoData 위치는 계속 NoData다. ④ 같은 부모의 임시 디렉터리에 결과를 쓰고 GeoTIFF 재독·마스크·고도·메타데이터·원본 불변을 확인한 뒤 최종 디렉터리로 이동한다.
- 데이터 계약: `metadata.json`은 `status: finalized`, `created_at_utc`, `source`(`raster_path`, `raster_sha256`, `metadata_path`, `dataset`, `version`, `license`), `processing`(`method`, `sigma_cells`, `radius_cells`, `missing_policy`, `display_interpolation`), `raster`(`file`, `sha256`, `crs`, `width`, `height`, `transform`, `nodata`, `valid_cells`, `min_m`, `max_m`, `mean_absolute_change_m`, `max_absolute_change_m`), `vertical_datum`, `unit`을 필수·non-null로 포함한다. README는 원본·출처·라이선스, 셀 단위 σ·반경·NoData 규칙, 지형 변화 및 원본 데이터 해상도 한계를 명기한다.
- 에러 정책: 잘못된 차원·σ≤0·반경<1·모든 값 결측·유효 셀의 NaN/Inf·원본 해시/격자 불일치·0 가중치·쓰기/재독 오류·기존 최종 디렉터리는 오류를 내고 최종 산출물을 덮어쓰지 않는다.
- 엣지: 상수 고도와 0/음수는 보존; 원본 NoData는 평균에 들어가지 않는다. 정규화는 경계·격자 외부에서 유효 가중치만 사용한다. 단일 유효 셀은 값이 유지된다. 동시 실행 중 기존 출력이 발견되면 실패한다.
- 성능/자원: 57×57 셀, 9×9 커널, 메모리 수 MB; 네트워크 요청·유료 서비스 없음.
- 테스트: `test_constant_and_mask` Given 상수 격자와 불규칙 결측 When 스무딩 Then 유효값은 상수이며 마스크가 보존된다(E91-2). `test_peak_diffuses_without_invalid_leak` Given 한 개 봉우리 및 결측 경계 When 스무딩 Then 봉우리는 낮아지고 유효 이웃은 올라가며 결측은 유지되고 값은 입력 범위 안이다(E91-2). `test_invalid_data_rejected` Given 전부 결측, 유효 NaN/Inf, 잘못된 차원·σ·반경 When 처리 Then 오류(E91-2). `test_source_errors_and_existing_destination` Given CRS 없는 원본 또는 이미 존재하는 결과 폴더 When 최종화 Then 오류이고 기존 파일은 보존된다(E91-1/2). `test_final_source_verification` Given 실제 원본 When 최종화·재독 및 별도 SciPy `gaussian_filter(..., sigma=1, truncate=4, mode='constant')`의 유효 가중치 정규화 결과와 비교 Then 모든 유효 셀·마스크·메타데이터·해시 일치(E91-1/3/4). 실제 원본과 합성 격자 사용.
- DoD: 원본 해시·무변경, 단위 4개 및 실제 독립 대조·재독·출처 검증 통과, 최종 지형 3개 파일 보관.

### I-92-a — 스무딩 격자의 연속 3D 표면 (C-92)

상태: 구현·검증 완료(2026-10-31). 유효 1,462셀의 중심 고도, 공유 모서리 1,594개 및 삼각형 5,848개를 대조하고 브라우저 조작과 고도 툴팁을 확인했다.

- 위치: `/Users/aiden/.codex/visualizations/2026/10/30/01a18db6-c08e-7591-9964-e33d04246a8e/myeongdong-terrain-final-3d.html`; 입력은 I-91-a의 최종 GeoTIFF와 메타데이터. 이전 `myeongdong-terrain-3d.html`은 수정하지 않는다.
- 기술 선택: I-90-a와 같은 D3 7.9.0 투영, Plotly.js GL3D 2.35.2·JavaScript·Python/Rasterio. 웹 데이터 요청 없이 최종 57×57 수치·마스크를 fragment에 내장한다.
- 인터페이스: `initializeMyeongdongTerrain(root: HTMLElement, data: object) -> void`. 내장 `{width,height,transform,values,min,max,valid}`는 최종 래스터의 각 Float32 유효값 또는 `null`을 그대로 담는다. 좌표는 I-90-a와 같이 실제 동쪽·북쪽 미터로 투영한다.
- 로직: ① 최종 래스터 해시·shape·마스크·통계를 확인하고 모든 값을 내장한다. ② 모든 유효 셀의 중심점에 원래 최종 고도를 둔다. ③ 하나 이상의 유효 셀에 접하는 각 격자 모서리는 접하는 유효 셀 고도의 산술 평균을 높이로 갖는 공유 정점 1개로 만든다. ④ 각 유효 셀의 중심과 네 공유 모서리를 삼각형 4개로 연결한다. 결측 셀에는 삼각형을 만들지 않는다. ⑤ `aspectmode: 'data'`로 수직 배율 1×, 회전·확대·고도 조회 및 원본·스무딩·보간 표기를 제공한다.
- 에러 정책: 결과 해시·마스크·셀 수·비유한 값 불일치면 fragment 생성 중단. 브라우저 라이브러리·WebGL 실패는 오류 텍스트 표시. HTML 크기 1MB 초과면 실패한다.
- 엣지: 1개 유효 셀은 자체 고도의 평면 4개로 표시된다. 결측 주변의 공유 모서리 평균은 유효 셀만 사용한다. 행정동 경계에 닿는 픽셀 전체를 표현하므로 픽셀 일부는 행정동 밖으로 걸친다. 480px 미만은 3D 축을 숨기고 범위·단위·출처는 보존한다.
- 성능/자원: 유효 1,462셀·삼각형 5,848개, HTML 1MB 이하, 단일 Plotly 인스턴스.
- 테스트: `test_final_visual_payload` Given 최종 래스터 When 내장 데이터 생성 Then 파일 SHA·3,249셀 값·마스크·affine·통계가 일치(E92-1/3). `test_surface_connectivity` Given 유효 셀과 모서리 집합 When 면을 생성 Then 셀당 4삼각형, 유효 이웃은 같은 정점을 쓰고 결측 셀은 없음(E92-2/3). `test_final_3d_browser` Given fragment When 브라우저에서 확인 및 방향·확대 조작과 표면 가리키기 Then 화면 변화·고도 툴팁·출처 및 보간 안내가 보이고 320px에서 가로 넘침이 없다(E92-3/4).
- DoD: 최종 래스터 전수 대조, 연속 메시 검증 및 실제 브라우저 상호작용·출처·보간 표시 통과.

## 명동 증강 구현 명세

C-130~C-133, DR-130~132. 런타임 Python 3.13.1, PyArrow 24.0.0, SciPy 1.17.1, NumPy 2.4.6, lxml 6.1.1(현재 환경); Parquet는 원본 Arrow 스키마를 그대로 보존할 수 있고 SciPy HiGHS는 정수 조건의 목표 오차를 계산한다. 날짜·모집단·제한은 보고서에 반드시 적는다.

### I-130-a — 공식 표 정규화 (C-130)
- 위치: `scripts/prepare_myeongdong_statistics.py`; 출력 `data/myeongdong-augmentation-inputs/statistics.json`.
- 시그니처: `parse_html_rows(path: Path, width: int) -> list[list[str]]`; `make_statistics(age5: Path, age19: Path, single: Path, dong: Path, marriage_zip: Path) -> dict`; `main() -> None`.
- 구조: `population`은 정렬된 850개 `{district: str, sex: str, age_lower: int, population: int, single_registered: int}`, 연령 하한은 19,20,...95. `marriage`는 서울의 20~24,...80~84,85+별 양성 `{sex, age_lower, never, married, divorced_widowed, total}`. `district_marriage`는 같은 필드에 `district`를 붙여 세 혼인값이 모두 공개된 셀만 담고, `suppressed_district_marriage`에는 비공개 구·성·연령 식별자만 담는다(DR-136). `myeongdong_context`는 동명·코드·전체·성별·5세 연령 원표, `sources`는 원본 URL·조회조건·시점·원본 응답 SHA-256, 정의를 포함한다. 모든 수치는 음이 아닌 정수, 혼인 `X`와 19세 혼인 수치는 저장하지 않는다.
- 흐름: HTML 표 데이터행의 셀 수 71/11/311/71로 구분하고 25개 구·성별 블록을 읽어 19~99세로 합산한다. 인구·1인세대 서울 총계와 구 합 및 성별 합을 확인한다. KOSIS ZIP의 CP949 CSV를 읽어 서울 20세 이상·85세 이상 셀 및 25구의 공개된 셀을 고르고 혼인상태 세 값의 합을 검증한다. 비공개 구별 셀은 수치를 복원하지 않는다. KOSIS의 `224860.0000` 같은 십진 표기는 소수부가 0일 때만 정수로 변환한다. 정규화된 숫자와 결과 해시를 보관해 웹 세션 식별자는 저장하지 않는다.
- 오류: 원표 손상·필드 누락·구 불일치·억제된 목표·합계 불일치 → 오류와 중단. 엣지: 19세는 별도 원표, 100세 이상 제외; 85+ 혼인 기준은 포함 범위 차이를 표시; 0 인구 셀은 보정에서 0으로 처리. 자원: 원표 합계 10 MB 미만, 메모리 300 MB 이내.
- 테스트: `test_statistics_parsing`(형식 및 억제), `test_statistics_real_sources`(25구·850셀·합계·혼인 합·동 코드·해시). DoD: 두 테스트 통과, 완전한 정규화 자료 존재.

### I-131-a — 정수 표본 보정 (C-131)
- 위치: `scripts/augment_myeongdong_personas.py`; 시그니처: `population_quotas(cells: list[dict], capacity: dict, n: int) -> tuple[dict, list[dict]]`; `allocate_donors(candidates: list[dict], stats: dict, n: int, seed: int) -> tuple[list[str], dict]`.
- 중간 산출물: `data/myeongdong-augmentation-inputs/selection.json`에 서울 원본·통계 해시, 시드, 선택 UUID와 진단을 보관한다. 최종 생성은 이 중간 산출물과 입력 해시를 대조해 그대로 사용한다. 검사와 저장 단계에서 같은 정수계획을 불필요하게 재실행하지 않는다.
- 자료: 각 후보는 `{uuid, district, sex, age, family_type, marital_status}`. 키 `(district, sex, age_lower)`; 혼인 범주는 미혼 / 배우자있음 / 사별·이혼. 혼자거주는 `family_type.startswith('혼자 거주')`로만 판정하고 주민등록세대와 다름을 명시한다.
- 흐름: 공식 인구비율의 정수 목표를 최대잔여(동률 키순)로 배정. 공급이 모자라면 동일 구·성에서 하한 차이 최소, 동률 낮은 연령으로 이동하고 원목표/보정목표를 기록한다. 성·혼인 연령집단(19,20~24,...80~84,85~99)마다 변수 `x[구·5세구간·혼자여부·혼인그룹]`의 정수 상한을 원본 후보 수로 설정하고 각 구·연령 총계를 고정한다. 혼인 집단 목표는 2025년 서울 성·연령 비율을 최대잔여 정수화(19세 제외); 혼자 목표는 2026년 각 구 셀의 `인물수×1인세대/거주인구` 합을 가장 가까운 정수로 정한다. 공개된 구별 혼인 목표는 같은 구와 성·혼인 연령집단의 2025년 비율을 그 집단의 보정 인원에 적용해 최대잔여 정수화한다. `scipy.optimize.milp`로 서울 혼인 목표 절대오차 합 → 그룹 혼자 목표 오차 → 공개 구별 혼인 목표 절대오차 합 → 셀 혼자 목표 절대오차 합의 순서로 앞선 정수 최적값을 고정하며 최소화한다(DR-136). 19세는 혼인 목표 없음. 모든 해의 정수성과 상한·고정 총계를 검증한다. 각 셀 후보는 SHA-256(`seed|uuid`) 순으로 필요한 만큼 선택한다.
- 오류: 인구 0/전체 0, 미분류 성·연령·가족·혼인, 중복 UUID, 동일 구·성의 대체 공급 부족, 최적화 실패·비정수·제약 위반 시 즉시 실패. 엣지: 목표 0, 원본 없는 조합, 19세 및 85~99세는 위 분기대로 처리. 자원: 인구 요약 185,228행의 6개 열만 읽으며 정수계획 최대 75개 셀×6범주/집단, 메모리 1.5 GB 미만 목표.
- 테스트: `test_population_quotas_sparse`(부족 이동·불가능·합계), `test_allocation_constraints`(가능/불가능 목표, 단계별 우선순위, 단일 출처·결정성), `test_district_marriage`(공개 구별 분포와 도시 목표 우선순위), `test_real_calibration`(실제 전수·목표 대비 오차). DoD: 네 검사 통과, 65,000개 고유 UUID와 진단 존재.

### I-132-a — 부모 단서 연결·저장 (C-132)
- 위치: `scripts/augment_myeongdong_personas.py`; 시그니처: `extract_parent_evidence(parent: dict, evidence: dict) -> list[dict]`; `assign_parents(parent_ids: list[str], donor_ids: list[str], seed: int) -> list[tuple[str, int, str]]`; `write_personas(source: Path, parents: Path, evidence: Path, assignments: list[tuple], output: Path) -> None`.
- 스키마: 서울 원본 26개 Arrow 필드는 이름·순서·타입·값을 그대로 사용. 뒤에 non-null `synthetic_id:string`, `donor_uuid:string`, `parent_uuid:string`, `variation_index:int32`, `parent_match_type:string`, `parent_evidence_json:string`, `parent_evidence_role:string`을 추가. `parent_evidence_json` 배열 원소는 `{field, token, start, end, sentence_start, sentence_end, sentence}`이며 오프셋은 부모 원문 문자열의 유니코드 코드포인트 기준 반개구간. `parent_evidence_role`은 항상 `unverified_parent_context_visit_candidate`.
- 흐름: 514 부모 UUID 정렬 후 seed 기반 셔플해 앞 236명의 횟수를 127, 나머지 126으로 한다. 선택된 고유 출처 UUID를 시드로 셔플해 부모 슬롯에 1:1 할당하고 같은 부모와 출처 UUID가 일치하면 다음 다른 슬롯과 교환. UUID5 고정 네임스페이스에 부모 UUID·인덱스·출처 UUID를 넣어 생성 ID를 만든다. 매칭 근거 표의 필드에서 추출기의 허용 토큰 정규식에 일치하는 각 범위를 찾아 문장 경계(`.`, `!`, `?`, 줄바꿈)까지 원문을 보존한다. 텍스트는 부모에 대한 단서로만 라벨링한다. 서울 원본 2,048행씩 읽으며 선택 인물 원본 값을 그대로 복사해 Parquet로 쓴다.
- 오류: 입력 부모 수/UUID/단서 누락, 토큰 범위 불일치, 자체 연결 교환 불가, 쓰기 실패 또는 기존 출력 디렉터리 존재 → 중단. 엣지: 한 부모가 여러 필드·단서 보유 시 모두 기록; 상호만 언급은 `business_only`; 조사상 19세라도 제한 없음. 자원: 선택 UUID와 할당 정보만 메모리 보관, Arrow 배치 2,048; 본문 최대 2 GB 목표.
- 테스트: `test_evidence_unicode_and_business`(복수 필드·한글 오프셋·상호 전용), `test_assign_parent_slots`(정확 배분·충돌 회피·시드 재현), `test_written_schema_and_evidence`(입력과 26필드 대조·고유 ID). DoD: 테스트 통과, 원본 26열 불변과 부모별 할당이 검증됨.

### I-133-a — 재독 검증 및 자료 인계 (C-133)
- 위치: `scripts/augment_myeongdong_personas.py`; `verify_output(output: Path, source: Path, parents: Path, evidence: Path, stats: dict, diagnostics: dict, selected_ids: list[str]) -> dict`; `build_dataset(source: Path, parents: Path, evidence: Path, statistics: Path, selection: Path, ontology: Path, output: Path, expected_n: int, expected_parent_count: int) -> dict`. 산출물 `data/myeongdong-augmented/personas.parquet`, `diagnostics.json`, `statistics.json`, `ontology_contract_snapshot.md`, `README.md`. `python3 -m scripts.augment_myeongdong_personas build`가 해시 확인된 중간 배분을 사용한다.
- 구조: diagnostics는 입력 파일과 생성·정규화 코드 해시·통계 및 온톨로지 문서 SHA-256·버전·시드·행 수·부모당 수·구성별 인구 목표/보정/실현·혼인과 1인세대 대리 목표/실현/오차·최적화 상태·구간 이동을 포함한다. README는 재현 명령·데이터셋 CC BY 4.0 표시·통계 원표 URL과 시점·대상/정의 차이·미보정 항목·온톨로지 미완성 및 명동 단서의 지위 설명.
- 흐름: 임시 디렉터리에 결과와 정규화 통계·온톨로지 문서 스냅샷을 쓰고 Parquet를 다시 읽어 개수·고유 ID·부모 할당·출처 UUID의 원본 26필드 전수 동일성·근거 문자범위·분포와 진단을 독립 집계한다. 성공 시 임시 디렉터리를 최종 이름으로 옮기며 기존 최종 경로는 덮어쓰지 않는다. 실패 시 최종 파일을 만들지 않는다.
- 오류: 참조·데이터·목표/실현·개수·해시 불일치, 디스크 부족, 기존 출력 또는 예외 → 실패와 원인. 엣지: 누락 텍스트·중복 부모·출처 mismatch 감지, 셀 목표 0 정상. 자원: Parquet 2,048행 배치, UUID·요약만 보관, 목표 메모리 1.5 GB 미만.
- 테스트: `test_validation_rejects_corruption`, `test_e2e_small`, `test_full_dataset_validation`. DoD: 합성 E2E 및 실제 65,000행 재독 검사 통과, 메타데이터와 설명서 존재. 원천의 의미적 오류가 없다고 보증하지 않는다.

## 온톨로지 기반 정방향·역산 구현체 Map

이 절은 [C-100~C-105](01_concept_map.md#온톨로지-기반-정방향-기록과-목표-역산--c-100c-105)의 Phase 5 설계다. 아래 경로는 **구현 예정 위치**다. 수학·직렬화·타입의 정확한 필드/제약/null 규칙과 알고리즘 절차는 [02b_inverse_algorithm.md](02b_inverse_algorithm.md)에 둔다. 기존 활동 엔진을 구현하거나 수정하지 않는다. 명동 어댑터는 확인된 [온톨로지 초안](01a_persona_ontology.md) §7.4의 `simulate`를 native trace로 연결하는 계약이며 실제 코드는 아직 없다.

### 공통 기술·지원 타입·오류

- 로컬에서 확인한 **Python 3.13.1, NumPy 2.4.6, SciPy 1.17.1**을 고정한다. 설치나 외부 서비스 없이 기존 환경을 쓴다. 나머지는 표준 `dataclasses`, `typing`, `json`, `hashlib`, `pathlib`, `datetime`, `zoneinfo`, `multiprocessing`, `time`, `unittest`다. PyArrow 24.0.0은 실제 데이터 어댑터가 필요할 때만 사용한다.
- Python은 기존 프로젝트와 일치한다. SciPy DE는 미분이 없는 혼합 공간 탐색에 사용하며 전역 최적을 보장하지 않는다. JSON/JSONL은 입력·실패·규칙 추적을 직접 재독하기 위한 내부 기록이다([DR-105~106](decisions.md#dr-105--역산의-수치-탐색과-내부-설정)).
- `CompiledModel={manifest:ModelManifest, adapter:ModelAdapter, rule_order:tuple[str,...], variables:Mapping[str,VariableSpec], mechanisms:Mapping[str,MechanismSpec], native_runner:NativeRunner|None}`.
- `ValidatedRequest={raw:InverseRequest, request_hash:Sha256, baseline:PathValues, scenario_initial:Mapping[str,State], scenario_exogenous:Mapping[str,PathValues]}`.
- `Coordinate={control_id:str, block_index:int, encoded_lo:float, encoded_hi:float, integral:bool, domain:ControlDomain}`. `EncodedSpace={coordinates:tuple[Coordinate,...], fixed_controls:Mapping[str,Scalar]}`; fixed_controls의 key는 control ID와 block index의 canonical 결합이다. 좌표 순서는 ID, block index다.
- `Archive=Mapping[str,Evaluation]`이며 key는 전체 제어 경로의 내용 해시다. 완전 평가만 순위에 넣고 partial/실패는 journal에 보존한다.
- `EvaluationBudget={simulations_used:int, trace_bytes:int, deadline_monotonic:float, search_limit:int, total_limit:int, scope_limit:int|None}`. scope_limit은 현재 restart의 누적 호출 상한이며 없으면 None이다. 프로세스 실행 전에 전체/탐색/해당 scope 중 적용되는 최솟값을 확인해 예약하며 실패도 사용량에 포함한다. 재시작 시 과거 사용량을 차감한다.
- `InverseError(code:str, message:str, context:Mapping[str,Scalar])`의 하위 타입: `InvalidRequest`, `ModelNotReady`, `EvaluationFailed`, `BudgetExceeded`, `ArtifactCorrupt`, `RequestBusy`. 보고서로 바꿀 때 원래 code/context를 보존한다. 모델/장부 손상은 전체 중단, 후보별 계산 실패는 격리한다.
- 아래 테스트의 G/W/T는 Given/When/Then이다. 구현 예정 파일 `tests/inverse_fixtures.py`에는 손계산 대기열, u1*u2 상호작용, 임계값, 유한 혼합 변수, 지연 피드백, 실패/timeout 어댑터만 둔다. 구현을 그대로 복제하는 테스트는 만들지 않는다.

### I-100-a — 실행 모델 계약과 컴파일 (C-100)

- 위치: `inverse_engine/contracts.py`, `inverse_engine/model.py`, `inverse_engine/adapters/base.py`.
- 기술 선택: 공통 Python 3.13.1 표준 라이브러리. typed contract와 위상 정렬로 충분하며 그래프 DB를 요구하지 않는다.
- 인터페이스: `compile_model(adapter:ModelAdapter)->CompiledModel`; `validate_manifest(manifest:ModelManifest)->None`; `model_digest(manifest:ModelManifest)->str`. 어댑터 시그니처는 02b §14 그대로다.
- 데이터 구조/스키마: 02b §14의 VariableSpec·MechanismSpec·ModelManifest. state/derived는 정확히 한 writer를 갖고 input 역할은 writer가 없어야 한다. parameters는 선언 ID를 정확히 덮는다.
- 로직: 스냅샷/해시 확인→ID/타입/단위/역할/출처/writer 확인→같은 단계 derived 의존 DAG 생성→ID 순서로 안정 Kahn 정렬→규칙 순서와 모델 해시 반환. native_trace 모드는 등록 NativeRunner 존재와 trace 계약을 확인한다. state 읽기는 x_t이므로 지연 피드백으로 허용한다.
- 에러: 누락/중복 writer·알 수 없는 참조·단위 없음·순간 순환·native runner 없음→`ModelNotReady`; 파일 변조→`ArtifactCorrupt`. 원인 ID를 반환하며 모델을 자동 보완하지 않는다.
- 엣지케이스: lagged self-feedback 허용; 의미 관계만 있는 그래프/빈 실행 모델 거절; 0·음수는 타입에 따라 허용; null·NaN·Inf 거절; incomplete dependency는 가지치기 비활성화.
- 성능/자원: DAG O(V+E), 안정 ID 정렬 O(V log V). 추가 수치 SLA 없음; 실행 비용은 요청 예산 적용.
- 테스트 명세: `test_model_roles_and_units` G 역할/타입/단위 충돌 W 컴파일 T 정상만 통과(E100-1); `test_model_requires_mechanisms` G writer 누락/중복 W 컴파일 T ID 오류(E100-2); `test_time_dependencies` G 지연 피드백/derived cycle W 컴파일 T 전자만 통과(E100-3); `test_semantic_edge_not_causal` G prefers 관계만 있음 W 컴파일 T 규칙 합성 없음(E100-4); `test_model_snapshot_identity` G 소스 변조/native 누락 W 컴파일 T 무결성/준비 오류(E100-5). 임시 스냅샷 fixture 사용.
- DoD: E100-1~5 green, 입력 열거 순서에 무관한 rule_order, 원본과 기존 엔진 불변.

### I-101-a — 요청·제어·목표·거리 (C-101)

- 위치: `inverse_engine/request.py`, `inverse_engine/controls.py`, `inverse_engine/scoring.py`.
- 기술 선택: Python 3.13.1 + NumPy 2.4.6. 계약 검사는 표준 라이브러리, 좌표는 float64와 정수 mask를 쓴다.
- 인터페이스: `validate_request(model:CompiledModel, request:InverseRequest)->ValidatedRequest`; `encode_space(model:CompiledModel, request:ValidatedRequest)->EncodedSpace`; `decode_controls(space:EncodedSpace, z:numpy.ndarray, baseline:PathValues)->PathValues`; `change_distance(candidate:PathValues, request:ValidatedRequest)->float`; `target_violations(metrics:Mapping[str,float], targets:tuple[TargetSpec,...])->tuple[float,...]`.
- 데이터 구조/스키마: 02b §14. controls는 변경 허용 control ID의 부분집합이고 baseline은 모든 control ID의 H개 구간을 덮는다. 목록 밖 control과 mutable_blocks 밖의 구간은 고정한다. 좌표는 가변 블록에만 만들고 고정 경로는 baseline에서 복사한다. scenario initial/path를 불변 mapping으로 읽는다.
- 로직: hash/역할/시간/scenario 중복 검사→목표·단위·tolerance·거리 행렬 검사→02b §7.5의 좌표 생성→decode시 baseline의 허용 포트만 대체→02b §4의 D/초과량 계산. 범위 밖 z를 조용히 clipping하지 않는다.
- 에러: 비제어 포트·잘못된 snapshot·빈 targets/search scenarios·NaN·음수 tolerance·범주 비용 오류·정수점 없음·블록 공백·단위 다른 목표→`InvalidRequest`와 field path. 기준 u0가 현재 domain 밖이면 거리 기준으로 유지하되 후보로는 부적합 표시한다.
- 엣지케이스: controls=[] 또는 mutable_blocks=[]는 고정 경로; lo=hi는 가변 블록에 그 상수를 적용; δ0/y*0 허용; bool과 int 구분; 시간 블록 분할로 같은 경로의 D가 변하지 않음; 외생 방문 수요를 자동 control로 승격하지 않음. baseline이 가변 블록 내 상수 조건이나 domain을 어기면 D0 성공 후보로 채택하지 않는다.
- 성능/자원: path O(H·제어수), 수치 index |k|≤2^53−1. 별도 SLA 없음.
- 테스트 명세: `test_only_control_ports_change` G 금지 포트 W decode T 거절/고정 유지(E101-1); `test_target_units_windows_tolerance` G 다른 KPI 정의/단위/기간과 δ0 W 검증 T 충돌 거절/정확 판정(E101-2); `test_change_distance_semantics` G 단위·범주·블록 분할 W D T 알려진 값/분할 불변(E101-3); `test_action_domain_and_constraints` G 격자/빈 정수범위/연동 W 검증 T 유효점만 통과(E101-4); `test_saved_target_current_scenario` G 과거a100·목표60/현재a120 W 요청 T 각각 유지(E101-5).
- DoD: E101-1~5 green, 임의 도메인 기본값 없음, 목표/기준/현재 조건을 각각 재직렬화 가능.

### I-103-a — 장부·추적·캐시 (C-103)

- 위치: `inverse_engine/journal.py`, `inverse_engine/artifacts.py`, `inverse_engine/trace.py`.
- 기술 선택: Python 3.13.1 JSON/JSONL·SHA-256·`os.open(O_CREAT|O_EXCL)` 잠금·`os.replace`. 서버 없이 내부 기록/실패 보존.
- 인터페이스: `Journal.open(root:Path, request:ValidatedRequest, model:CompiledModel)->Journal`; `journal.begin(experiment_key:str, inputs:Mapping[str,object])->str`; `journal.append(attempt_id:str, event:TraceEvent)->None`; `journal.finish(attempt_id:str, result:RunResult)->None`; `journal.lookup(experiment_key:str)->RunResult|None`; `journal.record_evaluation(evaluation:Evaluation)->None`; `TraceReader.open(artifact_dir:Path)->TraceReader`; `verify_package(artifact_dir:Path)->None`.
- 데이터 구조/스키마: 02b §5·§14. attempt ID는 UUID4, experiment key는 내용 해시. 재현 hash에서 실행 시각/attempt ID 제외. state_at(k)는 초기 스냅샷에 step<k의 state write만 반영하며 같은 단계 derived를 따로 읽는다.
- 로직: request 잠금→입력 snapshot 확정→begin 먼저 기록→event 직렬화와 budget 차감→파일 hash/행 수 확인→manifest 원자적 완료. lookup은 complete 패키지를 재독 검증한다. 규칙이 변하지 않은 경우도 실행/분기 기록을 남긴다.
- 에러: 잠금 충돌→`RequestBusy`; 디스크/byte 한도→incomplete와 `BudgetExceeded`; hash/행수 불일치→`ArtifactCorrupt`. 죽은 PID의 lock은 같은 request의 명시적 resume에서만 해제하고 살아 있는 lock은 빼앗지 않는다.
- 엣지케이스: event>16MiB면 단독 segment지만 전체 한도 준수; partial/failed cache 금지; 같은 y/다른 u 보존; 새 constraint는 저장 trace로 다시 계산; 같은 실험의 재검증은 새 attempt.
- 성능/자원: segment16MiB, RAM은 현재 event/해시/재구성 state. `max_trace_bytes` 적용, top-level report≤64KiB. 큰 details는 별도 예산 내 파일.
- 테스트 명세: `test_journal_attempt_lifecycle` G 정상/실패 W 저장 T 조건·원인 보존(E103-1); `test_trace_reconstructs_states` G 손계산3단계 W 재구성 T 각 q 일치(E103-2); `test_incomplete_and_corruption` G 중단/변조 W lookup T cache 거절(E103-3); `test_cache_context_identity` G 모델/x0/w/u/seed/KPI/환경 변경 W 조회 T 각각 miss, 목표만 변경하면 재채점(E103-4); `test_recorded_run_replay` G complete W 독립 실행 T metrics/trace 일치(E103-5, I-102-a와 통합). 임시 디렉터리 fixture 사용.
- DoD: E103-1~5 green, 부분 파일이 완료로 보이지 않음, 입력/실패 추적 가능.

### I-102-a — 정방향 실행·안정 난수·프로세스 (C-102)

- 위치: `inverse_engine/forward.py`, `inverse_engine/noise.py`, `inverse_engine/worker.py`.
- 기술 선택: Python 3.13.1 + NumPy 2.4.6; multiprocessing spawn, deadline에 worker 종료. adapter는 명시적 로컬 registry로 로드하며 데이터 문자열을 eval하지 않는다.
- 인터페이스: `run_forward(model:CompiledModel, request:ValidatedRequest, controls:PathValues, scenario:Scenario, journal:Journal, budget:EvaluationBudget, *, use_cache:bool)->RunResult`; `simulate_steps(model:CompiledModel, initial:State, controls:PathValues, exogenous:PathValues, scenario:Scenario, grid:TimeGrid, emit:Callable[[TraceEvent],None])->None`; `keyed_uniform(seed:int, rule_id:str, entity_id:str, step:int, event_slot:str, draw_index:int)->float`.
- 데이터 구조/스키마: 불변 x_t, 매단계 비우는 derived, x_(t+1) 쓰기 버퍼. outputs의 keys/dtypes는 writes와 일치. 부모는 예산/파일/manifest를 관리하고 worker는 입력과 events를 교환한다.
- 로직: 캐시 확인→budget 예약/begin→worker 실행. rules 모드는 순서대로 reads를 모아 evaluate_rule→outputs 검사→event→전체 rule 완료 후 상태 일괄 교체. native_trace 모드는 NativeRunner를 한 번 호출해 동일 trace를 받으며 ID/타입/순서/시점/쓰기 완전성을 검사한다. trace로 KPI 계산→유한값 검사→finish. noise는 02b §6의 SHA-256 키 규약을 그대로 따른다.
- 에러: 출력 누락/NaN/Inf/규칙 exception/명시적 발산→`EvaluationFailed`; deadline→worker 종료/incomplete; 입력 파일 변경→전체 `ArtifactCorrupt`. 정상 계산의 trajectory 제약 위반은 evaluation error와 구별한다.
- 엣지케이스: H1, hold state, 분기와 개체 순서 변화, q clamp, native 기록 누락, timeout 중 partial. 큰 유한 숫자만으로 발산을 추정하지 않는다.
- 성능/자원: RAM O(상태+derived), trace는 실행 규칙 수에 비례, 동시 worker1. 호출/시간/byte 예산 강제. 메모리 SLA는 adapter별 실측 대상이다.
- 테스트 명세: `test_forward_deterministic_order` G 열거 순서 변경 W 재생 T 동일 결과/hash(E102-1); `test_forward_time_feedback` G 지연 q W2단계 T x_t만 읽음(E102-2); `test_common_noise_and_input_immutability` G 제어로 분기변경 W paired run T 같은 key 난수/원본불변(E102-3); `test_forward_analytic_metrics` G q fixture W 실행 T 손계산일치(E102-4); `test_forward_failures_and_timeout` G 잘못된 output/NaN/exception/무한대기/native 누락 W 실행 T 오류별 기록/종료(E102-5).
- DoD: E102-1~5와 E103-5 green, 두 실행 mode의 공통 trace/KPI 계약 일치, timeout 뒤 부모 응답 가능.

### I-104-a — 공통 평가·제약·후보 archive (C-104, C-101)

- 위치: `inverse_engine/evaluator.py`, `inverse_engine/archive.py`.
- 기술 선택: Python 3.13.1 + NumPy 2.4.6. 모든 solver가 같은 판정 함수를 쓴다.
- 인터페이스: `evaluate_candidate(z:numpy.ndarray, space:EncodedSpace, model:CompiledModel, request:ValidatedRequest, journal:Journal, budget:EvaluationBudget, provenance:str)->Evaluation`; `hard_residuals(evaluation:Evaluation)->numpy.ndarray`; `score_V(evaluation:Evaluation)->float`; `archive_best(archive:Mapping[str,Evaluation], *, feasible_only:bool)->Evaluation|None`; `safe_prunable_controls(model:CompiledModel, request:ValidatedRequest)->tuple[str,...]`.
- 데이터 구조/스키마: Evaluation은 02b §14. target 순서=scenario ID→metric ID. hard 순서=input constraint ID(각 scenario ID) 후 scenario ID→trajectory constraint ID. partial을 정상 빈 벡터로 다루지 않는다.
- 로직: decode/마스크/D→모든 scenario input constraints→위반시 sim 없이 기각 기록→scenario별 정방향 실행→현재 목표/constraints 재채점→전체 완료 후 feasibility/V→archive/장부. dependency 불완전이면 prune=[], 완전해도 목표와 모든 제약의 closure·baseline domain 적합성을 검사한다.
- 에러: failed/partial은 SciPy 내부에서만 inf sentinel, JSON은 null+error. 계산 오류는 격리, 손상은 중단, budget은 archive 보존 후 상위로 전파한다.
- 엣지케이스: baseline D0, δ0, 중복 경로, 새 목표로 cache 활용, 모든 scenario에 같은 제어, 숨은 상호작용/공유용량, 범위 밖 baseline으로 가지치기 금지.
- 성능/자원: 후보당 최대 S 정방향 호출, archive는 참조를 보관하며 전체 상태를 후보마다 RAM에 복사하지 않는다.
- 테스트 명세: `test_evaluator_constraints_and_mask` G 금지/input/trajectory 위반 W 평가 T 포트 고정·제약잔차(E104-1); `test_target_before_change_cost` G 값싼미달/비싼달성 W 정렬 T 달성우선(E104-2); `test_zero_change_solution` G 기준조건 목표 W 평가 T D0(E104-3); `test_interaction_not_pruned` G u1*u2/공유용량/불완전의존 W prune T 영향제어 유지(E104-4 일부); `test_partial_and_budget_accounting` G S2의 두번째 timeout W 평가 T partial/호출수2(E104-6 일부).
- DoD: 위 테스트 green, solver와 보고서가 같은 feasibility/거리 계약 사용.

### I-104-b — 전수 열거와 2단계 혼합 탐색 (C-104)

- 위치: `inverse_engine/search.py`.
- 기술 선택: Python 3.13.1 + NumPy 2.4.6 + SciPy 1.17.1. 로컬 설치본의 DE `integrality`, `constraints`, `rng` 시그니처를 확인했다. 고정 설정은 02b §7.5다.
- 인터페이스: `search(model:CompiledModel, request:ValidatedRequest, journal:Journal, budget:EvaluationBudget)->SearchResult`; `enumerate_controls(space:EncodedSpace)->Iterator[numpy.ndarray]`; `run_de_phase(phase:Literal["reach","change"], initial:numpy.ndarray, evaluate:Callable[[numpy.ndarray],Evaluation], space:EncodedSpace, request:ValidatedRequest, budget:EvaluationBudget)->None`.
- 데이터 구조/스키마: `SearchResult={archive:Mapping[str,Evaluation], selected_key:str|None, optimality:str, termination_reason:str, finite_certificate:SnapshotRef|None}`. certificate는 model/request/domain/scenario hash·조합수·평가수·실패수·최선D를 포함한다.
- 로직: 02b §7/§10대로 baseline/warm→D0 단축→budget 안의 finite enumeration 또는 A(V,hard)→B(D,hard+target). 목적/제약 callback은 memoized evaluator 공유. constraint 벡터 차원은 고정하고 failed일 때 내부 inf로 채운다. A의 첫 feasible은 generation callback에서 종료. 최종 순위는 solver.success가 아닌 archive다.
- 에러: budget은 best archive 반환, 계산실패는 기록 후 계속, 계약/무결성 오류는 중단. finite에서 오류/partial이 있으면 증명 없음. SciPy success여도 직접 판정 미달이면 미발견.
- 엣지케이스: d0, 여러 최소해의 key tie-break, 임계값·정수·범주, 모든 hard 위반, 범위 밖 baseline seed 제외, warm 후보>P이면 앞 P개만 사용. B가 개선하지 못해도 A의 feasible 보존.
- 성능/자원: P=max(5,10d), 최대3 restart/phase, worker1, 전체 simulation cap. 전수는 N·S가 남은 탐색 예산 안일 때만. 수치 최적성 SLA 없음.
- 테스트 명세: `test_search_mixed_and_multiple_solutions` G 유한혼합/연속q W 탐색 T 합법/목표충족, finite 최소D는 독립 enumeration과 일치(E104-4); `test_no_feasible_vs_proven` G 유한무해/적은예산의 연속문제 W 탐색 T 전자만 proof(E104-5); `test_search_budget_resume` G 중단과 동일/다른hash W resume T 예산·호환기록만 사용(E104-6); `test_zero_baseline_search_shortcut` G 기준이 통과 W search T 불필요한 제어변경 없음(E104-3 통합). DE의 우연한 전역 최적 발견을 합격 기준으로 삼지 않는다.
- DoD: E104-1~6 전체 green, phase B의 feasible 보존, feasibility/optimality 상태 일치.

### I-105-a — 재실행 검증·보고·공개 호출 (C-105)

- 위치: `inverse_engine/verify.py`, `inverse_engine/report.py`, `inverse_engine/pipeline.py`.
- 기술 선택: Python 3.13.1. 원래 평가/기록 함수를 재사용하며 설명 생성 서비스를 넣지 않는다.
- 인터페이스: `verify_candidate(candidate:Evaluation, model:CompiledModel, request:ValidatedRequest, journal:Journal, budget:EvaluationBudget)->VerificationResult`; `build_report(search_result:SearchResult, verification:VerificationResult|None, request:ValidatedRequest, journal:Journal, budget:EvaluationBudget)->InverseReport`; `solve_inverse(adapter:ModelAdapter, request:InverseRequest, output_root:Path)->InverseReport`.
- 데이터 구조/스키마: `VerificationResult={status:verified|failed|incomplete, replay_runs:tuple[RunResult,...], validation_runs:tuple[RunResult,...], mismatches:tuple[str,...]}`. report는 02b §14, details는 §9의 표. 공개 함수는 실제 외부 제어를 실행하지 않는다.
- 로직: selected를 cache 없이 모든 search scenario에 재실행→KPI/trace/hard/target 검사→추가 validation scenario 실행→status/optimality 매핑→제어전후·지표·제약·event근거·자원 보고. solve_inverse는 validate→journal→search→verify→report. 후보 미발견이면 verify 생략하고 가까운 결과/미달량만 보고한다.
- 에러: 재생/추가조건 실패→validation_failed, 예산부족→validation_incomplete, 준비/요청오류→해당status. 보고서 저장 실패는 성공 반환 대신 partial journal 경로를 가진 오류다.
- 엣지케이스: validation=[]는 추가 검증 없음 명시; D0도 실제 replay 필수; top5 대안은 원본 평가 참조지만 selected 하나만 최종 검증됨; 가짜 경로기여율 생성 없음; 실패시 selected=null/optimality=none.
- 성능/자원: 최종검증 S_search+S_validation회 예약. top-level report≤64KiB, details는 별도 budget 내 파일. 새 반사실/시나리오를 임의 생성하지 않는다.
- 테스트 명세: `test_verify_bypasses_cache_and_detects_change` G 캐시만 정상인변조 W 검증 T 실제실행/실패(E105-1); `test_holdout_scenario_failure` G 탐색에서만 통과 W 별도검증 T 실패/원인(E105-2); `test_report_units_and_guarantees` G finite/heuristic/zero W 보고 T 단위/D/최소성 범위 정확(E105-3); `test_report_path_evidence` G 상호작용trace W 설명 T 실제event만 참조(E105-4); `test_validation_budget_exhausted` G 검증timeout W 전체호출 T incomplete(E105-5).
- DoD: E105-1~5 및 아래 통합/E2E green, 합성모델 통과와 명동실제연결 상태를 구분.

### 경계별 통합·E2E 명세

| 경계 | 테스트 | Given / When / Then |
|---|---|---|
| C-100→C-101 | `test_model_to_request` | 역할 선언 모델 / 잘못된 제어 / 모델 기준 거절 |
| C-100·C-101→C-102 | `test_request_to_forward` | q 모델과 a120 / baseline 실행 / q76 및 입력불변 |
| C-102→C-103 | `test_forward_to_journal` | 3단계피드백 / 실행·재독 / 매 상태·규칙·hash 복원 |
| C-103→C-101 | `test_saved_target_current_scenario` | 과거q60 / 현재a120의 목표로 채택 / 목표와 현재조건 분리 |
| C-100·C-101·C-103→C-104 | `test_history_to_inverse` | 다른 날씨의 과거 제어 / warm start / 현재조건 재평가 |
| C-104→C-102 | `test_inverse_evaluates_original` | 임계값/상호작용 / 후보평가 / 원래F 또는 검증cache 참조 |
| C-104·C-103→C-105 | `test_candidate_to_verification` | 후보·기록 / 독립재생 / 목표·hash·D 대조 |
| C-100·C-101→C-105 | `test_verification_uses_frozen_contract` | 탐색후모델/목표변조 / 검증 / hash차이 거절 |
| C-105→C-102 | `test_verify_bypasses_cache_and_detects_change` | 후보/cache / 검증 / 새attempt·같은experiment key |
| 핵심 E2E | `test_forward_record_target_inverse_replay` | 가상q와충격 / 기록→목표고정→역산→재실행 / 목표60±1·제어만변경·report추적 |
| 실패 E2E | `test_inverse_failure_reporting` | 유한무해/연속미발견/미준비adapter / 전체flow / 서로다른status·근거 |

현재 테스트와 엔진 파일은 생성하지 않았다. 문서 예시의 수학 확인은 구현 테스트 통과와 구별한다.

## 명동 건물과 주변 지형 3D 지도 구현 명세

독립 작업 C-93~C-95. 범위·높이·도로는 [DR-97](decisions.md#dr-97--제공-건물과-도로의-지도-반영)·[DR-98](decisions.md#dr-98--주변-지형-확장과-기존-확정-지형-보존), 정렬과 복구는 [DR-99](decisions.md#dr-99--확장-격자와-제공-공간자료의-정렬)을 따른다. 사용자 제공 폴더는 읽기만 한다.

### I-93-a — 주변 지형 다운로드와 결합 (C-93)

상태: 완료(2026-10-31). 확정 1,462셀 불변, 주변 1,787셀 확장, 독립 SciPy 결과 3,249셀 일치.

- 위치: `scripts/build_myeongdong_3d_map.py`; 입력 `data/myeongdong-terrain/final/terrain_smoothed.tif` 및 해당 `metadata.json`; 출력 `data/myeongdong-3d-map/terrain_extended.tif`, `terrain_metadata.json`.
- 기술: Python 3.13.1, Rasterio 1.4.3(GDAL 3.9.3), NumPy 2.5.4, 기존 `scripts/finalize_myeongdong_terrain.py`의 `gaussian_smooth`(σ=1, 반경 4). 선정 이유는 C-91과 동일한 고도 원본·격자·스무딩 규칙을 재사용하기 위함이다.
- 시그니처: `extend_terrain(final_dir: Path, output_dir: Path) -> None`; `blend_extended(raw_with_halo: numpy.ma.MaskedArray, final: numpy.ma.MaskedArray, radius: int = 4) -> numpy.ma.MaskedArray`. CLI `python scripts/build_myeongdong_3d_map.py extend`.
- 원본/스키마: 고도 원본은 [I-10-a](#i-10-a--원본과-경계-확인-c-10)의 GEDTM30 v1.2 URL·ETag·EPSG:4326·1초각·Float32·NoData. 확장 결과는 C-91과 같은 57×57 affine·좌표·NoData이며 전체 3,249개 유효 셀이다.
- 로직: ① C-91 원본 해시·shape·CRS·1,462 유효 셀을 검증한다. ② 고도 서버의 고정 ETag와 HTTP 206을 확인하고 `/vsicurl/`에서 C-91의 57×57 격자와 정확히 일치하도록 원본 4셀 halo를 더한 65×65 창을 읽는다. ③ 창 중앙의 기존 유효 셀에 C-91 값을 덮어 하이브리드 입력을 만들고 σ=1·반경4 가우시안 정규화 연산 후 중앙 57×57을 선택한다. ④ 이전 유효 셀은 C-91 값을 다시 넣어 비트 단위로 보존하고 외부 셀은 계산값을 쓴다. ⑤ 임시 GeoTIFF 작성·재독 및 원본 ETag 재확인 후 확장 파일을 확정한다.
- 메타데이터: `status`, `source_url`, `source_etag`, `source_license`, `source_period`, `final_source_sha256`, `file`, `sha256`, `crs`, `transform`(6수), `width`, `height`, `valid_cells`, `preserved_cells`, `added_cells`, `min_m`, `max_m`, `sigma_cells`, `radius_cells`, `outside_method`, `vertical_datum`, `unit`은 필수·non-null.
- 에러: 기존 결과가 있으면 덮어쓰지 않음. 기존 해시·원본 ETag/격자 불일치, 데이터 없는 영역·NaN/Inf, 65×65 범위 외, 좌표 정렬 오차 1e-6픽셀 초과, 다운로드·쓰기·재독 실패는 오류이며 미검증 결과를 확정하지 않는다.
- 엣지: 확장 대상 원본 NoData는 실패한다. 기존 값은 음수·0을 포함해 수정하지 않는다. 출력 경로 충돌 시 기존 파일 보존. 요청 종료·오류 시 임시 파일 정리.
- 성능/자원: 원격 세계 원본 전체가 아닌 65×65 창의 COG 블록만 HTTP Range로 읽는다. 다운로드 타임아웃 60초. 결과는 3,249개 셀.
- 테스트: `test_blend_preserves_confirmed_cells` Given 정상·경계·마스크 입력 When 확장 Then 기존 값과 마스크 및 추가 영역의 유한값 확인(E93-2/3); `test_blend_rejects_bad_input` Given 차원·결측·비유한 값 When 확장 Then 오류(E93-3); `test_extend_real_source` Given 실제 고정 원본 When 확장·재독 Then ETag·기존 1,462셀·외부 1,787셀·CRS·출력 SHA와 SciPy 독립 계산 일치(E93-1~4).
- DoD: 단위·실제 원본 통합 검증 전부 green, 기존 파일 해시 불변.

### I-94-a — 건물·도로 정렬과 재현 데이터 (C-94)

상태: 완료(2026-10-31). 원본 건물 713항목과 결과 709객체를 전수 추적했고 도로 347개 끝점 11,426개를 독립 고도 계산과 대조했다.

- 위치: `scripts/build_myeongdong_3d_map.py`, `tests/test_myeongdong_3d_map.py`; 입력 `/Users/aiden/Downloads/myeongdong-3d/myeongdong_data.json`, `osm_raw.json`; 출력 `data/myeongdong-3d-map/map_data.json`, `map_data.js`, `source/myeongdong_data.json`, `source/osm_raw.json`.
- 기술: Python 3.13.1, NumPy 2.5.4, Rasterio 1.4.3, Shapely 2.1.2. 실제 원본의 좌표·토폴로지 검사 및 삼각형과 발자국의 정확한 교차에 사용한다.
- 시그니처: `to_local(lon: float, lat: float, center: dict) -> tuple[float, float]`; `build_surface(raster: numpy.ndarray, transform: affine.Affine, center: dict) -> Surface`; `reconstruct_buildings(buildings: list[dict], osm: dict, center: dict) -> list[dict]`; `terrain_range_for_polygon(geometry: shapely.geometry.base.BaseGeometry, surface: Surface) -> tuple[float, float]`; `project_roads(roads: list[dict], surface: Surface) -> list[dict]`; `prepare_map(input_dir: Path, output_dir: Path) -> None`; CLI `python scripts/build_myeongdong_3d_map.py prepare`.
- 좌표: 제공 원본 기준점 `lat=37.563`, `lon=126.98525`; `x=(lon-lon0)*111320*cos(lat0*pi/180)` 동쪽 미터, `z=-(lat-lat0)*111132` 남쪽 미터, `y=EGM2008 지면 고도` 미터. 제공 건물·도로의 `[x,z]`는 변환하지 않으며 지형 경위도에만 위 식을 적용한다.
- 메시/스키마: `Surface`는 `vertices: list[(x,y,z)]`, `triangles: list[(i,j,k)]`, 각 삼각형의 x/z 폴리곤 및 공간 인덱스를 포함한다. 확장 래스터 3,249셀 각각 중심에 원래 높이를 두고 주변 유효 셀 평균의 공유 모서리와 4삼각형으로 잇는다. `map_data.json`은 `metadata`, `terrain`(`width`, `height`, `transform`, `values`), `buildings[]`(`id`, `source_ids`, `name`, `height`, `minHeight`, `base`, `footprint`), `roads[]`(`id`, `name`, `segments`)를 가진다. 지형 메시를 저장하지 않고 같은 공유 모서리 규칙으로 브라우저에서 복원한다([DR-150](decisions.md#dr-150--지도-데이터의-지형-격자-표현)). 높이·바닥은 미터 Float, `footprint`는 `[x,z]` 외곽 ring, `segments`는 `[x,y,z]` 양끝점 목록이다. 각 필드는 non-null이며 이름은 빈 문자열을 허용한다.
- 건물 로직: ① 입력 파일 SHA-256과 건물 713개·도로 347개·기준점을 확인하고 정확한 입력 사본을 보관한다. ② raw OSM relation 8814915의 외곽 way 634096161·634096168을 연결한 유효한 단일 Polygon으로 대체한다(두 건의 이름·높이·minHeight가 같아야 함). ③ ID 772877690의 중복 4건은 외곽·높이·minHeight가 같은지 검사하고 원본 4건의 출처를 남기면서 최초 하나만 사용한다. ④ 그 외 발자국은 사용자가 제공한 꼭짓점과 높이를 변경하지 않는다. 유효하지 않거나 영역 밖인 건물은 숨기지 않고 오류로 종료한다. ⑤ 각 건물 발자국과 지면 삼각형의 교차를 구하고 각 교차 경계점에서 삼각형 평면을 평가해 최소·최대 고도를 산출한다. 바닥 `base=min`이고 지붕은 `base+height`, 구조 하단은 `base+minHeight`다. 다각형 면적 전체가 지면에 덮이는지 확인한다.
- 도로 로직: 각 제공 LineString을 확장 격자 사각형으로 잘라 내부 조각을 보존한다. 각 지면 삼각형과 선의 교차를 구해 양끝점의 선형 지면 고도를 계산하고 z-fighting 방지용 0.25m를 더한다. 직렬화 좌표는 0.001m 단위로 반올림한다(최대 표시 오차 0.5mm). 실제 도로의 폭·교량·지하 높이가 아니다. 347개 ID가 각 1개 이상의 조각을 가져야 한다.
- 에러: 경도·위도 변환 불일치, 중복 내용 불일치, OSM 노드·관계 누락·유효하지 않은 Polygon, 지면 경계 밖 건물, 빈 도로, 미피복 면적, `height<=minHeight`, 값 비유한, 입력 해시 불일치, 결과 경로 충돌·쓰기 실패는 중단한다. 기존 입력과 확정 지형은 수정하지 않는다.
- 엣지: 3개 `minHeight>0` 구조의 하부는 지면보다 의도적으로 높다. 이름 없음은 공란으로 두고 임의 이름을 만들지 않는다. 사각형 밖까지 연장된 5개 도로는 경계에서 자르며 ID는 보존한다. 시뮬레이션 현실처럼 추정 높이를 해석하지 않는다.
- 성능/자원: 예상 지형 정점 3,249개 중심+3,364개 모서리, 12,996삼각형; 건물 709개(713항목−관계 결합 1−중복 3), 도로 347개. Shapely STRtree로 삼각형 후보를 제한한다.
- 테스트: `test_coordinate_contract` Given 제공 node 및 픽셀 경위도 When 변환 Then 제공 x·z와 센티미터 반올림 범위 내 일치(E94-2). `test_relation_and_duplicate_repair` Given 실제 원본 조각 When 복구 Then 관계가 닫힌 정확한 원본 Polygon이고 4회 반복된 외곽은 한 번, 각 높이와 source ID가 보존된다(E94-1). `test_repair_rejects_inconsistent_source` Given 불완전한 관계 또는 다른 높이의 중복 When 복구 Then 오류(E94-1). `test_ground_and_roads` Given 합성 평면 지형과 발자국/도로 When 정렬 Then 밑면 최저 지형·도로 선형 고도·범위 절단이 맞고 범위 밖 오류(E94-2/3). 실제 전수 ID·높이·피복·도로·해시 검사는 `test_map_real_data`로 수행(E94-1~4).
- DoD: 모든 제공 항목의 대응과 높이·좌표·지면·도로가 검증되고 OSM/고도 출처와 변경 이력이 남는다.

### I-95-a — 로컬 3D 지도 및 대화 미리보기 (C-95)

상태: 완료(2026-10-31). 로컬 정적 서버와 대화 미리보기에서 렌더링·조작 및 320px 표시를 확인했다. `file://` 직접 자동 테스트는 앱의 URL 보안 정책으로 차단돼 검증하지 못했다.

- 위치: `data/myeongdong-3d-map/index.html`, `viewer.css`, `viewer.js`, `map_data.js`, `vendor/three.min.js`, `vendor/OrbitControls.js`, `vendor/LICENSE.txt`, `README.md`; 대화 미리보기 `/Users/aiden/.codex/visualizations/2026/10/30/01a18db6-c08e-7591-9964-e33d04246a8e/myeongdong-buildings-terrain-3d.html`.
- 기술: 브라우저의 WebGL과 Three.js 0.146.0/OrbitControls(MIT)를 버전 고정해 로컬 파일로 포함한다. 대화 미리보기는 동일 버전의 허용 CDN 라이브러리와 자체 내장 데이터를 사용한다. 기존 Rasterio/NumPy/Shapely로 전처리하며 외부 유료 서비스·계정 없이 실행한다.
- 인터페이스: `initMyeongdongMap(root: HTMLElement, data: MapData) -> void`; `new THREE.BufferGeometry()`의 지형, 각 건물의 `THREE.ExtrudeGeometry` 및 도로의 `THREE.LineSegments`; `OrbitControls`로 회전·확대, 클릭 `Raycaster`로 건물 선택. 렌더링 데이터는 I-94-a와 같다.
- 화면 계약: 지도가 화면 대부분을 차지한다. 조용한 청록빛 배경·연한 지형·동일한 건물 색·작은 선택 강조색을 사용한다. 왼쪽 정렬의 명동 3D 지형 이름, 간결한 회전·확대 안내, 선택 건물의 이름과 제공 높이, 전체보기와 확대/축소 버튼, 키보드용 건물 선택 목록, 출처가 보인다. 사용자 요청에 따라 건물 높이의 근거별 색상·분류·필터를 만들지 않는다. 뷰는 실제 x/y/z 미터를 1×로 유지한다.
- 로직: ① 로컬 HTML은 JS·데이터·MIT 라이선스를 함께 포함한 경로로 읽고 인터넷 없이 동작한다. ② 모든 지형 삼각형을 그린다. ③ 각 건물 폴리곤을 `base+minHeight`에서 `base+height`까지 수직으로 올린다. ④ 도로의 지형 고도에 투영된 선분을 표시한다. ⑤ 클릭한 건물의 이름·높이를 `textContent`로 나타내 데이터에 포함된 문자열이 HTML로 실행되지 않게 한다. ⑥ 브라우저 크기에 반응하고 GPU 오류는 사용자에게 표시한다. ⑦ 대화 위젯이 제공되는 경우 카메라·목표점·선택 건물을 비공개 상태로 저장·복원한다([DR-151](decisions.md#dr-151--로컬-지도-렌더링과-보기-상태)).
- 에러: 라이브러리·데이터 누락이나 WebGL 초기화 실패 시 오류 문구를 표시한다. 외부 데이터 호출이나 외부에 공개하는 동작은 없다.
- 엣지: 이름 없는 건물은 `이름 없음`; 동일 건물의 두 원본 ID는 하나의 복구 결과에 추적된다. 터널 중심선 역시 표면에 투영된 것으로 명시한다. 320px 폭에서도 화면·버튼·출처가 잘리거나 가로 스크롤이 생기지 않는다.
- 성능/자원: 지형 12,996삼각형, 건물 709개, 도로 347개. 화면 픽셀 비율을 2로 제한하고 리사이즈에 맞춘다. 대화 fragment는 1MB 이하.
- 테스트: `test_rendered_map` Given 원본과 최종 데이터 When 렌더링 Then 실제 3D 장면에 지형·709개 건물·347개 도로가 표시되고 좌표·비율이 일치한다(E95-1). `test_viewer_interaction` Given 브라우저 When 회전·확대·건물 선택 Then 화면이 변하고 해당 이름·높이가 표시된다(E95-2). `test_map_attribution_and_offline` Given 출력 폴더 When 정적 서버 또는 로컬로 열기 Then 인터넷 데이터 요청 없이 이용 가능하고 OSM·GEDTM30·SGIS 출처 및 한계가 표시된다(E95-3/4). `test_mobile_view` Given 폭 320px When 그림과 버튼 확인 Then 잘림·가로 스크롤 없이 동작한다(E95-4).
- DoD: 자료 계약·로컬 3D 렌더·상호작용·라이선스·반응형 검증 통과, 인계 README 포함.

## 명동 실사 텍스처 지도 구현 명세

독립 작업 C-152~154. 사용자가 정한 10m 위성영상과 확인된 건물만의 외벽 실사 범위는 [DR-152~155](decisions.md#dr-152--건물-지붕의-실제-위성영상-선택)를 따른다. 기존 `data/myeongdong-3d-map/`과 확정 지형을 수정하지 않고 `data/myeongdong-3d-map-photo/`에 별도 결과를 쓴다.

### I-152-a — 위성 RGB 공간자료 취득·변환 (C-152)

상태: 완료. 실제 COG 창의 해상도·피복·해시와 독립 좌표·보간 검증 통과.

- 위치: `scripts/prepare_myeongdong_photo_map.py`, `tests/test_myeongdong_photo_map.py`; 입력 `data/myeongdong-3d-map/map_data.json`, Earth Search STAC item `S2B_52SCG_20261028_0_L2A`의 `visual` COG; 출력 `data/myeongdong-3d-map-photo/satellite_source.tif`, `satellite.png`, `satellite_metadata.json`.
- 기술/이유: Python 3.13, Rasterio 1.4.3 및 GDAL, NumPy 2.5.4. COG의 작은 지도 영역만 읽고 원래 좌표를 보존한 뒤 Rasterio의 좌표계 변환을 이용한다.
- 시그니처: `map_bounds(data: dict) -> tuple[float,float,float,float]`는 지도 경위도 `(west,south,east,north)`를 반환한다. `fetch_satellite(data: dict, destination: Path) -> dict`는 원본·표시 이미지·메타데이터를 쓰고 메타데이터를 반환한다. `build_texture_bundle(destination: Path, photo_specs: list[dict]) -> None`은 이미지들을 `texture_data.js`의 데이터 URL로 패키지한다. CLI `python scripts/prepare_myeongdong_photo_map.py satellite`로 위성자료를 만들고 `photos`로 사진을 준비한다.
- 원본: `https://sentinel-cogs.s3.us-west-2.amazonaws.com/sentinel-s2-l2a-cogs/52/S/CG/2026/10/S2B_52SCG_20261028_0_L2A/TCI.tif`, Sentinel-2B, 2026-10-28T02:27:13.169Z, EPSG:32652, 픽셀 10m×10m, RGB 8bit. STAC 메타데이터와 실제 CRS·해상도·범위가 다르면 중단한다. 원본 저작권 고지는 Contains modified Copernicus Sentinel data (2026)다.
- 구조: 원본 창 GeoTIFF는 3채널 UInt8과 원래 affine·CRS를 보존한다. 출력 PNG는 지도의 EPSG:4326 사각 범위를 256×256으로 양선형 재투영한 RGB이며 출력 픽셀 수는 원본 10m 관측 해상도를 높이지 않는다. 메타데이터는 `scene_id`, `datetime`, `source_url`, `source_crs`, `source_resolution_m`, `bounds_wgs84`(4수), `image_width`, `image_height`, `source_sha256`, `image_sha256`, `attribution` 필수·non-null.
- 흐름: 지도 affine·57×57 크기에서 네 경계를 구함 → 원본 창에 2픽셀 여유를 두어 요청 → 3밴드·CRS·10m·데이터 피복 검증 → 창과 지리참조 저장 → 지도의 사각 영역으로 256×256 reprojection → 픽셀·파일 재독 및 해시·출처 저장 → 지도용 데이터 URL에 PNG 포함. 인터넷 접속은 준비 단계에만 필요하다.
- 오류/엣지: 네트워크·원본 CRS/해상도·피복·밴드·파일 크기·쓰기 실패, 기존 출력 폴더 충돌은 실패한다. 0 값은 정당한 어두운 RGB일 수 있으므로 NoData mask로 판단한다. 사각 경계의 좌표는 픽셀 바깥 경계로 해석한다. 자원: 전체 110km 원본을 내려받지 않고 57셀 영역만 포함한 수백 픽셀 창을 읽으며 브라우저 이미지는 256×256이다.
- 테스트: `test_bounds` Given 지도 변환·크기 When 경계 계산 Then 각 꼭짓점이 기대 위치와 일치(E152-2). `test_satellite_real` Given 고정 공개 COG When 영역 준비 Then 센서·10m·범위·밴드·해시·출처와 25개 지점의 독립 좌표·보간 대조가 검증된다(E152-1~4). `test_bad_imagery` Given 결측/CRS 오류 When 처리 Then 실패하고 유효한 단색 채널 0은 보존한다(E152-2). 지붕 UV는 I-154-a에서 확인한다.
- DoD: 실제 COG와 파일 재독·좌표·출처 검증 green.

### I-153-a — 실제 외벽 사진과 벽면 대응 (C-153)

상태: 완료. 사진 네 장의 대상·저자·날짜·이용조건·해시 및 벽면 방향 검증 통과.

- 위치: 같은 준비 스크립트와 결과의 `photos/`, `photos_metadata.json`; 출처는 Wikimedia Commons의 `Myeongdong Cathedral front.jpg`, `Bank of Korea Museum 20240219.jpg`, `서울중앙우체국.jpg`, `Young Nak Church 2007.jpg` 각각의 저자·날짜·라이선스가 표시된 파일 페이지.
- 기술/이유: 사진 원본의 Commons 1280px 미리보기를 변형 없이 저장하고 JavaScript의 UV 좌표로 필요한 부분을 사용하는 Three.js 0.146.0. `photos --photo-cache DIR`로 이미 다운로드해 원문과 해시를 확인한 사진을 받아들일 수 있으며, 생략하면 공개 원본 URL에서 다시 받는다. 내용이나 다른 건물을 생성하지 않는다.
- 인터페이스: `validate_photo_specs(data: dict, specs: list[dict]) -> None`. 각 `PhotoSpec`은 `id: string`, `filename: string`, `title: string`, `author: string`, `date: string`, `license: string`, `license_url: string|null`, `source_page: string`, `edge_indices: number[]`, `camera_direction: [number,number]`, `uv_crop: [left,top,right,bottom]`, `orientation_basis: string`, `sha256: string` 필수. 각각의 파일과 metadata는 결과 `photos/`와 `photos_metadata.json`에 저장한다.
- 대응: 대성당 `way/210335697`, 화폐박물관 `way/348489095`, 우체국 `way/775236382`, 영락교회 본당 `way/296668636`; 이미지의 대상·원본 OSM 태그 및 공간 위치 대조. 처음 둘은 사진의 GPS, 나머지는 주변 도로와 건물 배치로 사진 방향을 근사한다. 명시한 벽면의 접선 방향으로 원본 사진을 부분 투영한다. 사진이 없는 다른 건물·면은 종전 단색을 유지한다.
- 오류/엣지: 사진 파일 누락·빈 이미지·메타데이터 누락·건물 ID 중복/부재·유효 범위 밖 edge 인덱스·빈 면·UV 범위 역전·용도에 맞는 라이선스 확인 실패는 중단한다. 인접 건물이나 다른 면으로 사진을 재사용하지 않는다. 자원: 사진 네 장과 선택된 벽면 삼각형만 추가.
- 테스트: `test_photo_specs` Given 4장 메타데이터·709건물 When 확인 Then 대상 ID·사진과 면·출처가 1:1이고 없는 건물은 제외(E153-1/2/3). `test_invalid_photo_spec` Given 누락·범위외 edge 또는 UV When 확인 Then 실패(E153-2/3). `test_photo_credits` Given 산출물 When 재독 Then 사진별 저자·출처·라이선스·수정·SHA 보존(E153-1/4).
- DoD: 사진·건물·면 대응과 권리·한계에 대한 검사 green.

### I-154-a — 지붕 및 외벽 텍스처의 3D 지도 (C-154)

상태: 완료. UV·이미지 일치·원본 불변 및 실제 브라우저 선택·회전·모바일 검증 통과. 앱의 `file://` URL 보안 차단으로 직접 열기는 자동 검증하지 못했다.

- 위치: 결과의 `index.html`, `viewer.js`, `viewer.css`, `texture_data.js`, `preview_metadata.json`, 기존 `map_data.js`와 `vendor/`, `README.md`; 대화 미리보기는 크기 제한을 만족할 때 별도 fragment로 생성한다. `preview_metadata.json`은 대화 미리보기의 250px 사진별 건물 ID·제공 URL·SHA-256을 기록한다.
- 기술: Three.js 0.146.0과 OrbitControls, 원본 RGB 및 사진의 데이터 URL 텍스처. 외부 이미지 호출이나 로그인 없이 기존 지도와 같이 로컬로 실행한다. 대화 미리보기에는 동일 사진의 Commons 250px 제공본을 쓰고 데이터·사진 자체·벽면·출처는 유지한다([DR-158](decisions.md#dr-158--대화-미리보기-사진-크기-재조정)).
- 시그니처: `initMyeongdongMap(root: HTMLElement, data: MapData, textures: TextureData) -> void`; `roofUV(x: number,z: number,bounds: [west,south,east,north],center: {lat:number,lon:number}) -> [number,number]`; `facadeGeometry(building: Building, spec: PhotoSpec) -> THREE.BufferGeometry`. `TextureData`는 `satellite: data URL`, `photos: {id,data,edge_indices,camera_direction,uv_crop}[]`, `bounds_wgs84`를 갖는다.
- 흐름: 지형과 기존 건물·도로는 재사용하고 각 지붕의 로컬 x/z를 경위도·위성 이미지 UV로 정렬 → 사진 지정 건물의 지정된 edge만 높이 범위에 대응하는 사진 UV 벽면을 원래 벽에 겹쳐 표시 → 나머지 벽은 기존 색상 → 선택 강조는 사진·지붕 텍스처를 해치지 않음 → 사진이 있는 건물은 선택 시 사진의 벽면 방향과 건물 크기에 맞게 가까이 이동(수평거리 `max(60m, 최대폭·깊이·높이×1.75)`, 최소 카메라 거리 35m). 초기 1.05배에서 건물의 상하가 잘려 1.75배로 조정했다. 원본 `본당`은 OSM 관계로 확인된 `영락교회 본당`으로 표시([DR-156](decisions.md#dr-156--실사-외벽-건물의-선택-시점)) → 회전·확대·건물 선택·이미지 로딩 오류 표시. 10m 원본과 네 건물 한계를 화면과 README에 명시한다.
- 오류/엣지: 텍스처 이미지 디코드·WebGL 오류는 표시하고 성공으로 표기하지 않는다. 이미지 UV는 지도의 바깥 경계 기준이며 원본 위치 밖이 되지 않아야 한다. 320px에서 잘리지 않는다. 성능: 사진 네 장, 709건물, 지도 격자 3,249셀, 픽셀 비율 최대 2. 위젯은 1MB 미만.
- 테스트: `test_roof_uv` Given 지도 네 모서리·가운데 When UV 변환 Then 영상 네 모서리·중심과 맞는다(E154-1). `test_facade_geometry` Given 명시된 4건 When 기하 생성 Then 지정 edge 이외에 사진이 없고 UV는 crop 범위 안이다(E153-2/3). 두 테스트는 `tests/test_myeongdong_photo_viewer.cjs`로 실행한다. `test_photo_map_browser`(수동 CUA) Given 로컬 정적 서버 When 이미지 로드·회전·확대·건물 선택 Then 텍스처 5개와 기존 709건·347도로가 표시되고 320px에서 동작한다(E154-1/2). `test_photo_map_provenance` Given 결과 When 원본·출처·해시 비교 Then 기존 데이터는 불변이고 모든 사진의 출처·변형·해상도 한계가 보인다(E154-3/4).
- DoD: 실데이터·좌표·개별 사진 출처·브라우저 및 회귀 green, 새 폴더로 인계. 앱 브라우저가 `file://`를 차단하는 한 직접 파일 열기는 자동 검증하지 못함을 기록한다.

## 명동 유동인구 반영 구현 명세

Python 3.13.1, PyArrow 24.0.0(기존 Parquet·대량 join), 표준 `csv`, `decimal`, `hashlib`, `json`, `unittest`. 사용자 CSV는 수정하지 않는다. 시스템 Python은 번들 Python에 없는 PyArrow 작업에만 사용하며 원자료 검산은 번들 Python에서도 독립 수행한다. 고정 출력은 `data/myeongdong-traffic/`; 기존 경로가 있으면 중단한다.

### I-140-a — CSV와 그래프 원자료 검증 (C-140)
- 위치: `scripts/prepare_myeongdong_traffic.py`; `load_quarters(path: Path) -> list[dict]`; `periods_from_quarters(rows: list[dict]) -> dict[str,dict]`.
- 스키마: UTF-8 BOM CSV 30열을 `csv.DictReader`로 읽고 코드·연도·분기 및 성/연령/시간/요일 카운트는 원래 이름과 정수값으로 보존한다. 22개 20211~20262 연속 분기, 상권 구분 D, 코드 3120028, 이름 명동(명동거리) 필수. 각 수치는 유한한 비음수 정수; 결측·중복·기간 불일치·열 누락은 실패한다. 네 주변합과 총계의 차이는 ±10 이내를 허용하고 원래 차이를 기록하며 초과는 실패한다.
- 흐름: `2025_all`은 20251~20254의 동일 수치 열을 각각 더한 합성 기간이고 2025 그래프의 연령 비율은 연령값 합으로 정규화한다. 그래프의 6개 시간점은 각 분기 시간대 값을 구간 길이(6,5,3,3,4,3시간)로 나누어 검산한다. 이미지 파일 해시와 원본 CSV 해시·상권 원표 URL, 기간, 분기별 주변합 차이를 기록한다.
- 오류/엣지: UTF-8 실패, 빈 입력·중복·음수·비정수·합계 차이 초과·누락 분기 → 실패; 2025 전체 합산은 총계와 주변합 차이를 유지; 시점 내 원본 값 0은 합계가 양수라면 허용. 자원: 22행, 메모리 100 MB 이하 목표.
- 테스트: `test_csv_validation`(정상·누락·중복·음수·합계 오차), `test_2025_figures`(연령별 6.3/22.5/23.2/21.9/14.5/11.6%, 분기 6구간 숫자 검산), `test_source_dataset`(22행·원본 해시·연속 시점). 완료: 테스트 green.

### I-141-a — 10분 상대 프로필 (C-141)
- 위치: 같은 파일. `build_profiles(periods: dict[str,dict]) -> tuple[dict,list[dict]]`.
- 스키마: `traffic_reference.json`에 기간별 원표, 기간·연령·성별·요일·시간대 별 `share`와 `source_total_difference`, 시간대의 `source_count_per_hour`를 둔다. `time_profile_10min.parquet`는 non-null `period:string`, `slot:int16`, `start_minute:int16`, `end_minute:int16`, `time_band:string`, `slot_share:float64`, `relative_intensity:float64`, `assumption:string`.
- 흐름: 각 독립 주변합으로 각 비율을 정규화(0이면 실패). 각 시간대의 `slot_share = band_count / sum(band_counts) / (band_minutes/10)`; `relative_intensity = slot_share * 144`. 23기간×144슬롯을 출력하고 가정은 `uniform_within_time_band`이다. 요일×시각, 연령×시각, 성별×연령은 만들지 않는다. 절대 인원, 도착, 속도, 밀도, 개인 일정을 생성하지 않는다.
- 오류/엣지: 음수/0 주변합 → 실패, 0인 개별 구간은 슬롯 강도 0; `slot=143`은 23:50~24:00; 시각 범위 벗어남 실패. 자원: 3,312행, 제한 없음.
- 테스트: `test_ten_minute_boundaries`(각 경계·마지막 슬롯), `test_profile_reconciliation`(모든 기간 슬롯 합 1·원 구간 합·상대 강도 평균 1·요일 및 연령 합). 완료: green.

### I-142-a — 증강 인물의 연령 보조 가중치와 완성 (C-142)
- 위치: 같은 파일. `compute_age_factors(group_counts: dict[str,int], periods: dict[str,dict]) -> dict[str,dict]`; `persona_weights(personas: Path, periods: dict[str,dict]) -> tuple[pyarrow.Table,dict]`; `validate_delivery(output: Path, source: Path, personas: Path) -> dict`; `main() -> None`.
- 스키마: `persona_traffic_weights.parquet` 65,000행은 `synthetic_id:string`(non-null), `age:int64`, `age_band:string`, `weight_status:string`, 기간마다 nullable `weight_<period>:float64` 23열. 19세는 status `age19_unresolved`와 23개 null. 20~99세는 status `adult20plus_relative_only`, 60~99세는 60대 이상 범주. 23기간의 각 다섯 성인 연령별 목표는 `source_age_count / sum(source_age20plus_counts)`; 가중치는 `target_share / (synthetic_group_n / synthetic_adult20plus_n)`.
- 흐름: 기존 인물 Parquet의 synthetic_id·age만 배치로 읽고 ID 유일·19~99세 및 65,000명 검증. 위 정식으로 구한 가중치를 23열로 구성. `traffic_reference.json`은 2025 합산과 분기 수치·상권 정보·독립 주변분포 및 지도 없는 구간 가정; `validation.json`은 입력/출력/코드/온톨로지 SHA-256, 그룹별 목표·가중합·오차, 원본 주변합 오차, 파일 스키마·검사 결과. 사용자 CSV·이미지와 생성 당시 온톨로지 및 연결 계약 스냅샷을 변경 없이 결과 디렉터리 `sources/`에 복사한다. 결과는 임시 디렉터리에 쓰고 검증 후 final rename한다. 시뮬레이션은 아직 작성되지 않았으므로 사용 방법·R-09/10/12 관계를 README와 [자료 계약](05_myeongdong_traffic.md)에 명시한다.
- 오류/엣지: 중복/빈 synthetic_id, 나이 누락·범위 외, 20+ 연령군 중 원표는 양수인데 합성 그룹 0, 서로 다른 ID 수, 출력 경로 기존 존재 → 실패. 연령 그룹 원표 0은 가중치 0. 원본 인물 데이터와 온톨로지는 읽기만 한다. 자원: 65,000×23 실수와 3,312 프레임, 메모리 1 GB 이하 목표.
- 테스트: `test_age_weights`(정규화·미지원 19세·양수 그룹 결핍), `test_full_delivery`(기존 ID 65,000와 1:1, 각 23기간 가중 연령 분포 정확, 19세 null, 10분 합, 원본/출력 해시, 기존 원본 SHA 불변), `test_ontology_boundary`(상대 강도·가중치가 도착수·인/㎡가 아니라고 명시). 완료: 셋 green 및 CSV·이미지·Parquet·설명서 인계.

## 명동 변수 근거 조사 구현체 — I-240-a~I-242-a

이 작업의 구현물은 소스 코드가 아닌 Markdown 조사 보고서 [06_myeongdong_variable_research.md](06_myeongdong_variable_research.md)다. 각 항목은 `번호 | 변수 | 확인값/단위 | 공간·시점 | 출처 | 해석·한계`를 포함하며, 미확인은 `미확인`으로 표기한다. Markdown 본문과 링크가 인터페이스이므로 프로그램 함수·API·데이터베이스 스키마와 라이브러리 버전은 해당 없다. 보고서 작성 외 리소스 제한은 없다. 어떤 오류도 근거 없는 값을 채우는 방식으로 해결하지 않는다([DR-240](decisions.md#dr-240--명동-변수-자료-조사의-경계)).

### I-240-a — 공간·시설 보고 (C-240)

- 위치/기술: 보고서의 2·3·4·16 항목; Markdown과 원본 OSM JSON·GeoJSON.
- 인터페이스: 네 행을 공통 조사표 형식으로 작성한다. 입력은 중구 보도자료, 사용자가 제공한 OSM 노드와 행정동 경계, 중구 광고시설 보도자료다.
- 로직: 중구 도로 폭을 길 이름과 보도 연도로 제시한다. 노점 수는 중구 집계 연도와 지역을 보존한다. `amenity=bench` 노드 수를 세고 OSM 사각 범위와 행정동 경계 내 수를 각각 제시한다. 광고는 시설 종류·거리별 설치 수를 기록하고 시각 자극도는 미확인으로 둔다.
- 오류/엣지: 경계 밖 벤치는 구분한다. 비어 있거나 태그가 없는 시설은 실제 부재로 단정하지 않는다. 총 도로 폭을 장애물 제외 폭으로 기록하지 않는다. 출처 접근이 실패하면 확인한 2차 출처를 명시하거나 미확인으로 둔다.
- 검증: E240-1 네 번호 대조, E240-2 출처·범위·시점 대조, E240-3 원본 JSON과 경계에서 별도로 벤치 집계, E240-4 총 폭·OSM 등록 수·광고 개수의 한계 검토.
- DoD: 네 검증이 모두 통과하고 보고서의 해당 항목을 읽을 수 있다.

### I-241-a — 상점 운영·경제 사례 보고 (C-241)

- 위치/기술: 보고서의 6·14·15 항목; Markdown과 공개 웹 자료.
- 인터페이스: 세 행을 공통 조사표 형식으로 작성한다. 금액은 원, 임금은 원/시 또는 원/월, 시각은 현지 시각으로 기록한다.
- 로직: 영업시간·가격은 상호와 상품을 확인한 사례로 제시한다. 인건비는 노동부의 2026년 법정 최저 시급·월 환산액과 관찰된 개별 채용 사례를 구분한다. 근무시간·주휴 포함 여부 등 공개된 조건을 같이 기록한다.
- 오류/엣지: 폐업·이전 가능성이 있는 옛 상점 자료는 현재 전체 상권으로 일반화하지 않는다. 법정 최저임금을 실제 평균이나 모든 부대비용을 포함한 사업주 총인건비로 표기하지 않는다. 미확인 상권 평균은 미확인으로 둔다.
- 검증: E241-1 세 번호 대조, E241-2 상호별 운영시간·메뉴 출처 대조, E241-3 노동부 최저임금과 환산 조건 대조, E241-4 보고서 외 시뮬레이션 데이터의 변경 부재 확인.
- DoD: 네 검증이 모두 통과하고 사례와 평균이 구분된다.

### I-242-a — 교통·방문·소득 보고 (C-242)

- 위치/기술: 보고서의 9·11·12 항목; Markdown과 공개 웹 자료.
- 인터페이스: 세 행을 공통 조사표 형식으로 작성한다. 배차는 역·방면·요일·시간·분, 관광객 수는 기간과 대상, 소득은 직접 확인 여부를 표시한다.
- 로직: 명동역 4호선 시간표의 연속 출발 시각 차이를 계산해 지정 시간대의 범위를 제시한다. 외국인 수는 기관 발표에서 밝힌 기간 그대로 표시한다. 서울시 상권 소득 데이터의 제공 상태를 확인한다. 서울 여행 지출을 보조 자료로 표시할 때 소득이나 명동 내 지출과 다름을 명시한다.
- 오류/엣지: 열차 시간표가 분 단위면 차이도 분 단위 근사로 표시한다. 관광객의 중복 제거 방식이나 경계가 확인되지 않으면 그렇게 적는다. 총 방문 수를 새로운 시간당 도착 인원으로 변환하지 않는다. 소득 정보가 없다면 null/미확인으로 둔다.
- 검증: E242-1 세 번호 대조, E242-2 원 시간표와 간격 대조, E242-3 원 자료의 기간·대상·한계 대조, E242-4 소득과 여행 지출의 구분 및 공공 자료 중단 확인.
- DoD: 네 검증이 모두 통과하고 불명확한 값이 미확인으로 남는다.

통합·E2E: 열 항목 10개의 번호·단위·출처와 각 범위·시점·한계를 함께 읽어 확인한다. 원본과 보고서 사이의 경계·기간 차이 및 누락을 수동 검토한다. 문서 조사라 새로운 자동 테스트 프로그램은 만들지 않는다.

## 페르소나 반응 온톨로지 구현체 Map — C-201~C-216

이 절은 [개념 Map C-201~C-216](01_concept_map.md#페르소나-반응-온톨로지--c-201c-216)의 Phase 5 안내다. 구현체 단위는 두 문서로 나뉜다([DR-214](decisions.md#dr-214--반응-온톨로지의-pythonrust-분담), [DR-215](decisions.md#dr-215--온톨로지-정의-한-벌-공유와-rust-코드-위치)). Python 쪽(정의 검증, 프로필 추출, LLM 사전 후보, 지형·보행망·장소·상점 자료, 관광·소득·경쟁 상권, 시나리오 번들, 지표 후처리, 검증 보고)은 [02c_reaction_impl_map.md](02c_reaction_impl_map.md), Rust 런타임(반응 함수, 틱 진행, 혼잡, 방문 실현, 알아차림, 상점 반응, 스냅샷, plot)은 [02d_rust_engine.md](02d_rust_engine.md)에 있다. 두 언어 사이 파일 계약의 정본은 [02c §2](02c_reaction_impl_map.md#2-python--rust-파일-계약)다. 근거 추출(02c I-202-a)이 활동 엔진의 I-04-a~I-04-c(`engine/rules.py`, `engine/text.py`, `engine/cues/timecues.py`, `engine/cues/modality.py`)를 재사용하므로 그 단위의 인터페이스를 바꿀 때는 02c를 함께 고친다.

## 첨부 명동 UI 구현체 (I-250-a~I-253-a)

공통: Node 24.15.0(검증 환경), TypeScript 5.9.3, React 19.2.4, ~~Three.js 0.187.0~~ Three.js·타입 0.186.0([DR-243](decisions.md#dr-243--threejs와-타입-버전-선택)), Vite 8.0.5, Tailwind 4.2.2. 정확한 전이 의존성은 갱신한 `apps/myeongdong-ui/pnpm-lock.yaml` 정본. C-227 완성이나 실데이터 연결을 뜻하지 않는다. 결정 [DR-241~243](decisions.md#dr-241--첨부-화면의-내재화와-엔진-연결-준비).

### I-250-a — 독립 실행 (C-250)

- 위치: `apps/myeongdong-ui/{src,public,index.html,package.json,pnpm-lock.yaml,tsconfig.json,vite.config.ts,README.md,.gitignore}`. `.gitignore`는 재생성 가능한 `node_modules/`, `dist/`만 제외한다(SELF).
- 기술: 첨부 React/Vite 및 잠금 버전 유지; Three.js·타입은 DR-243에 따라 0.186.0으로 고정해 lock 갱신. Vite 설정은 React·Tailwind 플러그인만 적용, 개발 서버는 로컬 바인딩. HTML `lang=ko`, 제목 명시. 로컬 Pretendard 및 시스템 폴백.
- 인터페이스/스키마: `App(): JSX.Element`, `#root`, `pnpm dev`, `pnpm build`, `pnpm typecheck`, `pnpm test`. 첨부 자산은 동일 바이트이며 출력 경로는 Vite 기본 `dist/`.
- 흐름: 파일 복사 → 빌드 설정 정리 → GLB·JSON·글꼴 로드 → 화면 렌더. pnpm이 원본 0.187 lock 항목을 갱신 전에 거부하므로 해당 두 항목만 임시 제거한 뒤 정책을 유지한 채 pnpm으로 새 lock 항목을 생성한다(DR-243). 출처·실행법·미연결 상태를 README에 기록한다.
- 오류: GLB 적재 실패 시 기존 한국어 오류 표시. 패키지 설치·컴파일 실패 시 성공으로 표기하지 않는다.
- 엣지: 오프라인에서 외부 글꼴 요청 없음; 좁은 화면에서도 경고를 볼 수 있음. 미제공 자산은 오류 표시.
- 성능: 별도 수치 제약 없음. 엔진의 재생 성능 기준은 후속 C-227 범위.
- 테스트: `E250-assets` 원본과 복사된 `public/`, `src/data/` 해시 일치; `E250-build` 수정 lock의 frozen 설치, typecheck/build; `E250-browser` 로컬 화면·GLB·선택·회전·예시 표시·외부 요청 확인.
- DoD: 위 검사 통과, 앱·문서 이식, 원본과 기존 지도 미변경.

### I-251-a — 예시 데이터 격리 (C-251)

- 위치: `src/preview/pedestrians.ts`, `src/preview/scenarios.ts`, `src/App.tsx`, `src/index.css`.
- 기술: TypeScript 5.9.3 + Three.js 0.186.0(DR-243), 기존 계산과 색상 유지.
- 인터페이스: `createPreviewPedestrians(scene: THREE.Scene, bounds: THREE.Box3, obstacles: THREE.Box3[], boundary: [number, number][], count?: number): PreviewPedestrianSystem`; `updatePreviewPedestrians(system: PreviewPedestrianSystem, delta: number, elapsed: number): void`; 시나리오 모듈은 `BuildingType`, `BuildingMeta`, `scenarioVariables`, `defaultScenarioChoices`, `mockMonthlySales`, `expectedFootTraffic`, `expectedMonthlySales`를 내보낸다.
- 자료: 원본 `BuildingMeta`와 16개 선택지·기본값을 유지한다. mock 수치는 추정 근거가 없는 예시이며 저장·전송하지 않는다.
- 흐름: GLB에서 건물 장애물 생성 → preview 시스템 생성 → 애니메이션에서 update 호출. 조건 선택 → 예시 계산. 화면에 엔진 미연결·예시 보행/지표를 명시한다. 화면 검증에서 상단 경고가 시간 표시를 가리는 문제를 발견했으므로 경고는 시간 슬라이더 바로 위에 두고 데스크톱·작은 화면에서 가림을 확인한다. 브라우저에서 확인된 Three.js Clock 지원 중단 경고는 동일 역할의 `THREE.Timer.update/getDelta/getElapsed`로 교체하고 종료 시 dispose한다(SELF).
- 오류: 실제 엔진 데이터가 없어도 예시 표시; 지표 부재를 실제 수치로 대체하지 않는다.
- 엣지: 경계 밖/장애물은 기존처럼 재목표화; 건물 미선택 시 선택지·상세 숨김; 모든 값은 현재 메모리 상태만 변경.
- 성능: 기존 700 인스턴스 및 7.5배 표시 크기 유지, 별도 수치 제약 없음.
- 테스트: `E251-browser` GLB 로드 후 예시·미연결 안내, 건물 선택과 선택지 변화, 화면 축소 확인; 구조 검사로 보행·수치 생성이 preview에 있음을 확인한다.
- DoD: 기존 모양과 조작 보존, 예시 출처 명시, 동작·구조 검사 통과.

### I-252-a — Rust 프레임 해석기 (C-252)

- 위치: `src/engine/frames.ts`, `tests/frames.test.mjs`, `tests/fixtures/frame_1400.bin`.
- 기술: TypeScript 5.9.3, 브라우저 `TextDecoder`·`DataView`; 테스트 Node 24 내장 test runner. 새 런타임 의존성 없음.
- 인터페이스: `decodeFrameChunk(input: ArrayBuffer | Uint8Array): FrameChunk`; `FrameChunk = {header: FrameHeader, frameCount: number, readFrame(index: number): Frame}`; `Frame = {tick: number, records: FrameRecord[]}`. 레코드 필드 `body`, `x_m`, `z_m`, `heading_rad`, `gaze_rad`, `focal_range_m`, `focal_angle_deg`, `activity`, `flags`는 전부 number.
- 자료: JSON 머리말 + LF, `format=mdsim-frames-v1`, `record_bytes=14`, `origin=[0,0]`, `pos_scale_m=0.05`, 순서대로 `body:u32,x:i16,z:i16,heading:u8,gaze:u8,focal_range_m:u8,focal_angle_deg:u8,activity:u8,flags:u8`. 각 프레임은 little endian `tick:u32,n:u32`와 14바이트×n; 각도는 `u8 * 2π/256` 라디안. 머리말 `dt_s>0`, `render_every` 양의 정수, `n_frames` 비음수 정수, 틱 시작·끝은 비음수 정수 및 반열린 구간.
- 흐름: LF까지 UTF-8 JSON 해석 → 형식·필드·범위 검사 → 남은 바이트를 한 프레임씩 스캔해 틱·바이트 오프셋만 저장 → 반열린 틱 구간 안의 모든 render_every 배수 틱이 순서대로 빠짐없이 존재하는지, 프레임 수·종단 확인 → `readFrame` 요청 시에만 레코드 객체를 생성. Rust `step.rs`의 렌더 조건과 청크 종료 규약을 따른다.
- 오류: 머리말 없음/손상, 미지원 버전/필드/스케일/원점, 중간 절단, 잔여 바이트, 헤더 불일치, 비정상 틱은 `Error`; 범위 밖 인덱스는 `RangeError`.
- 엣지: 빈 청크·빈 프레임 허용; 음수 x/z는 signed; `Uint8Array`의 offset 준수; 자료 길이보다 큰 count 거부; 한 번에 전체 레코드 객체 생성 금지.
- 성능: 스캔 메모리는 O(프레임 수), 요청 프레임만 O(몸 수); 별도 상한 없음.
- 테스트: `E252-rust-fixture` Rust가 만든 600프레임의 헤더·처음 비어 있지 않은 레코드 기대값; `E252-signed-angle` 합성 음수 좌표·각도·비트; `E252-empty-index` 빈 프레임/잘못된 인덱스; `E252-malformed` 잘못된 형식·필드·헤더·잘림·여분·순서; `E252-subarray` 비영(非零) 버퍼 오프셋.
- DoD: 위 테스트 green, 브라우저 빌드 성공, 실제 UI에 연결됐다고 주장하지 않음.

### I-253-a — 좌표 조사 (C-253)

- 위치: `scripts/audit_myeongdong_ui_coordinates.py`, `apps/myeongdong-ui/README.md`.
- 기술: Python 3 표준 라이브러리(json·struct·hashlib·statistics·math), 입력 기본값은 내재화 GLB와 `data/myeongdong-3d-map/map_data.json`.
- 인터페이스: `python3 scripts/audit_myeongdong_ui_coordinates.py [--glb PATH] [--map PATH]`; JSON을 stdout에 기록, 오류는 stderr와 0이 아닌 종료 코드.
- 자료: GLB node extras.id, primitive POSITION accessor min/max, 정본 건물 id/footprint. 두 축 각각 `model = slope * map + intercept` 최소제곱; 고도는 분석 대상이 아니다.
- 흐름: GLB 헤더와 JSON chunk 검사 → ID별 accessor 외접 중심과 정본 외접 중심 대조 → 공통 ID 수·축별 상수/기울기·잔차 median/p95/max·입력 SHA-256 출력. 변환행렬이 있는 노드는 거부해 숨은 오차를 막는다.
- 오류: 파일 없음·JSON/GLB 오류·중복 ID·변환 노드·중첩 건물 노드(부모 변환을 추측하지 않음)·공통 건물 부족/축 분산 0은 오류.
- 엣지: 공통이 아닌 ID는 개수만 세고 제외; 중심 오차는 외곽/지형 정확도나 범위 밖 적용을 보증하지 않는다. 지면은 GLB 평면, 정본 지형 격자라는 차이를 README에 기록한다.
- 성능: 건물 수에 선형, 별도 수치 제약 없음.
- 테스트: `E253-real` 현재 GLB와 지도 실행 결과 공통 663, x/z 잔차 및 비영 원점 확인; GLB ground의 y 범위와 변환이 평면이고 정본 지형 격자의 고도 범위가 비영임을 별도 확인; `E253-errors` 잘못된 GLB·공통 ID 부족 거부; README 근거·한계 확인.
- DoD: 결과 재현·오류 검사 통과, 검증 전 변환을 UI에 적용하지 않음.

## 명동 5,044명 재생 구현체 (I-260-a~I-264-a)

공통: [DR-260~261](decisions.md#dr-260--실제-명동-입력의-5044명-직접-재생). Rust 2021의 기존 `mdsim-core`·`mdsim-runtime`, Python 3.13 + pyarrow 24.0.0 + NumPy 2.4.6 + NetworkX 3.6.1 + PyYAML 6.0.3(설치된 로컬 라이브러리), UI React 19.2.4/Three.js 0.186.0/Vite 8.0.5. 고유 run 디렉터리를 사용해 이전 사용자 실행을 덮어쓰지 않는다.

### I-260-a — 구조화 프로필 변환 (C-260)
- 위치: `scripts/prepare_myeongdong_5044.py`, 생성 입력 폴더의 `profiles.parquet`·`profile_attrs.parquet`·`source.json`.
- 시그니처: `select_profiles(table, n: int = 5044) -> list[dict]`; `profile_from_row(row: dict, income_spec: dict) -> dict`; CLI `python3 scripts/prepare_myeongdong_5044.py --out DIR [--count N]`.
- 자료: 원본 `synthetic_id`의 SHA-256 16진값 정렬 후 앞 N개를 선택하고 결과 행은 `persona_id`순으로 저장한다(02c §2.4). `P:<synthetic_id>`는 유일. Rust 프로필 형식·자료형은 02c §2.4와 fixture Parquet 스키마. 출력에 원문 이름·성별·나이·소득 열을 넣지 않고 별도 attrs에만 넣는다.
- 로직: 원본/의존 파일 해시 → 선택 → 02c §1.10의 나이 매듭 선형보간, 지정 능력·지각 기본값, 기본 관심 0.2, sens 1, 성향 0.5, 이동 속도 1.34×나이 배율 → I-215-a 소득/방문 예산 통계 → 주말 10:00~22:30 가용창 → 텍스트 히트 없음과 기본 목적(기타 1.0). 직접 입장이므로 출발지 접근 시간은 가정이며 방문 여부 추정에 쓰지 않는다.
- 오류/엣지: 원본 ID 누락·중복, 개수 초과, 음수/무효 나이·예산 또는 입력 스키마 불일치는 실패. N=0은 거부; 동일 입력·N이면 같은 ID. 정확히 5,044개를 요구.
- 제약: 전체 65k 표본 선택 후 5,044행 변환, 3GB 메모리 이내.
- 테스트: `test_select_unique_deterministic`, `test_profile_age_and_budget`, `test_input_contract_5044` 각각 고정 원본/경계/실표 및 Rust validate. DoD: 전부 green, 가정과 원본 해시 기록.

### I-261-a — 명동 공간 번들 (C-261)
- 위치: `scripts/prepare_myeongdong_5044.py`, 생성 입력 `nodes`, `edges`, `buildings`, `places`, `portals`, `access`, `competitors` Parquet와 번들 manifest·시나리오.
- 시그니처: `build_space(osm: dict, map_data: dict, businesses: dict, boundary: dict) -> dict[str, list[dict]]`; `write_bundle(out: Path, tables: dict, definitions: Path) -> Path`.
- 자료: OSM 노드/way와 행정동 경계, 지도 `buildings` 원본 ID·외곽·높이, 3D UI 업종 JSON. 기존 `sim/runs/fixture` 정의는 최신 엔진이 요구하는 `route.defer_min`이 없어 부적합하므로 현재 실행 파일의 `mdsim fixture`로 새 임시 fixture를 만들고 그 5개 개발 정의 YAML과 Arrow 스키마를 별도 사본으로 사용한다. `--fixture-root`로 이 경로를 지정하며 `development_fixture_definitions`·`structured_profile_defaults`·`forced_arrivals`·`surface_only`·`building_proxy_places`를 manifest에 남긴다.
- 로직: OSM 중 02c의 지상 보행 허용 way만 사용(접근 금지·area·지하 제외), 2개 연속 OSM 노드 사이를 간선으로 만들고 같은 OSM ID는 공유한다. 경계와 닿는 간선만 포함해 연결 성분이 가장 큰 것만 유지한다. 길이·경사(확장 지형 격자의 양 끝 보간)·유형별 기본 폭은 02c §1.10을 따른다. 공통 건물 중 카페·음식·의류·편의점은 건물당 1개 장소 대리값으로 만들고 가장 가까운 보행 간선에 연결한다. 경계 안 OSM 지하철 출구와 버스 정류장은 가장 가까운 연결 노드에 연결하고 거리·제외를 기록한다. 접근과 경쟁은 직접 입장에 쓰이지 않으며 접근 30분 가정과 빈 경쟁 목록을 둔다.
- 오류/엣지: 입력 누락·중복 ID·면적 0·연결 간선 없음·출입구 또는 장소 없음·참조 미일치·해시 불일치 시 중단. 단일 성분 외 간선과 지하층 수를 기록하며 통과 불가 좌표는 제외 사유를 남긴다.
- 제약: OSM 3MB·인물 5,044, RAM 3GB. 테스트: `test_network_refs`, `test_ground_truth_building_ids`, `test_portal_near_road`, `test_bundle_validate`(실제 원본과 Rust). DoD: 전부 green, 입력 수·범위·한계 기록.

### I-262-a — 고정 인원 런타임 (C-262)
- 위치: `sim/crates/mdsim-runtime/examples/replay_myeongdong_5044.rs`, 앱 `public/runs/<run_id>/`의 결과. 기존 Rust 기능을 호출하는 별도 example, 기존 엔진 파일 변경 없음.
- 시그니처: `cargo run -p mdsim-runtime --release --example replay_myeongdong_5044 -- <root> <defs> <bundle> <scenario> <out> [expected_count]`; `run(root, defs, bundle, scenario, out, expected_count) -> Result<()>`. expected_count 기본 5,044, 작은 확인용 입력에서는 명시한다.
- 자료: scenario `14:00~17:00`, `dt_s=0.5`, `render_every_ticks=2`; 각 프로필 순번 i의 arrival tick = `floor(i * n_ticks / 5044)`, 한 프로필당 1몸, `replicate=0`, 출입구는 ID 고정 해시로 선택, 체류 기한은 도착+65분과 종료 중 작은 값. 목표 5,044는 동시 인원 보정이 아니다.
- 흐름: 정의·번들·해시 검사 → 실제 `Space::build` → 5,044개 `SpawnKind::Domestic` 정렬 → `Sim::run_until`로 1,200틱씩 → `FileSink`의 실제 10분 프레임과 원자적 상태 JSON(`running`/`complete`/`failed`, 계획·누적 입장, 완료 프레임 경로/해시, 공간 변환, 경고) → 마지막에는 실제 `write_run`으로 사건·결정·몸·지표 manifest 생성. `run_status.json`은 UI용이며 실패를 성공으로 바꾸지 않는다.
- 오류/엣지: 출력 폴더가 비어 있지 않거나 프로필 수 불일치/출입구 없음/계약·I/O 오류는 실패하고 원인 표시. 0명·일부 조각·완료 전 탐색은 정지/대기. 존재하는 사용자 파일은 덮어쓰지 않는다.
- 제약: 3GB 메모리, 개발 확인은 작은 인원·짧은 창부터, 전체 15분 목표. 테스트: `test_forced_spawns`(N명 한 번씩·틱 범위), `test_engine_small_replay`(실제 Rust 결과), `test_full_run_5044`(manifest 몸 수 및 파일 해시). DoD: 각 검사 green, 진행 중에도 결과 표시 가능.

### I-263-a — GLB 정합 (C-263)
- 위치: `scripts/audit_myeongdong_ui_coordinates.py`, 생성 `alignment.json`, `tests/test_myeongdong_ui_coordinate_audit.py`.
- 시그니처: `audit(glb_path: Path, map_path: Path) -> dict`, 결과에 기존 `center_fit_model_x/z`, 각 입력 SHA-256, `vertices_count`, `vertices_p95_m`, `vertices_max_m` 추가.
- 로직: 663개 공통 building node의 POSITION Float32 중 고유 x/z 좌표를 읽고 중심 적합의 역변환 뒤 대응 footprint 선분까지 최단 거리를 전수 계산한다. 최대 0.2m 초과·GLB 변환 중첩·JSON/BIN 손상 시 실패. UI는 검증된 해시/변환만 사용한다.
- 오류/엣지: 공통 건물 부족, 잘림, 지원 외 accessor·stride, 비유한 좌표, 최대 오차 초과 거부. 지형 높이는 이 검사의 범위 밖.
- 제약: 약 4천 점·수백 폴리곤 선분, 별도 상한 없음. 테스트: `test_ground_truth_vertices`(현재 4,014점·최대 <0.2m), `test_invalid_transform`, 기존 감사 테스트. DoD: 오차 통과 및 해시가 실행 manifest와 일치.

### I-264-a — 조각 재생 UI (C-264)
- 위치: `apps/myeongdong-ui/src/engine/{replay.ts,agents.ts}`, `src/App.tsx`, `src/index.css`, `tests/replay.test.mjs`, README.
- 시그니처: `loadRun(base: string): Promise<RunStatus>`, `loadChunk(status, index): Promise<FrameChunk>`, `frameAt(status, seconds): Promise<Frame>`; `createEngineAgents(scene, capacity: number)`, `renderEngineFrame(system, frame, alignment)`.
- 자료: 상태 JSON의 `status`, `window_start_s`, `window_end_s`, `dt_s`, `render_every_ticks`, `planned_bodies`, `spawned_bodies`, `frames: [{path,sha256,tick_start,tick_end}]`, `alignment`, `limitations`. 엔진 레코드 x/z는 변환 후 GLB 지면 y=0에 둔다. 활동 상태 색, 몸당 화면 크기는 과시를 위해 고정 확대임을 설명한다.
- 흐름: 상태 폴링 → 완료된 10분 조각만 SHA-256·형식 검사 후 LRU 최대 2개 → 틱으로 해당 프레임 선택 → InstancedMesh 업데이트. 재생/정지, 초 단위 이동, 1×/10×/60× 배속. 생성 중에는 이미 완료된 구간만 재생하고 끝에서 대기하고, 완료된 실행의 끝에서는 정지하며 재생을 다시 누르면 처음부터 시작한다. 사용자가 이동하면 이전 비동기 요청이 최신 프레임을 덮지 못한다. DR-262에 따라 충분한 구간이 있으면 15:00, 전체 카메라 거리의 60%에서 시작하고 초기 구간이 더 짧으면 완료된 마지막 프레임에서 시작한다. 현재까지 입장은 `ceil(frame.tick * planned_bodies / n_ticks)`로 계산해 실제 도착 스케줄과 맞추고, 현재 인원은 프레임 레코드 수로 표시한다. 시간·현재 인원·계획/누적 입장·명동 가상 실험·직접 입장·미검증 경고를 표시한다. 기존 미리보기 보행/임의 매출 수치는 실제 재생 모드에서 표시하지 않고 조건은 결과에 영향을 주지 않는다고 알린다.
- 오류/엣지: HTTP/JSON/해시/조각·좌표 불일치 시 사용자에게 오류, 임의 결과로 대체하지 않음. 프레임 0명, 마지막 프레임, 시간 이동, 중복 요청, 느린 네트워크, 화면 unmount를 처리한다.
- 제약: 2조각 이하 캐시, 가능한 30fps; 측정 불가 시 값 기록. 테스트: `test_frame_selection_boundaries`, `test_race_newest_seek`, `test_replay_actual_chunk`, 320px·데스크톱 실제 로딩·재생·정지·이동·현재 수 대조 E2E. DoD: 모두 green·브라우저를 사용자에게 띄움.

## 직접 실행 API와 기록 연결 (I-270-a~I-273-a)

공통 DR-270·271. Python 3.13 표준 HTTP 서버·PyArrow 24·PyYAML 6, 기존 Rust 2021 엔진, React 19·Vite 8. 사용자 PC의 127.0.0.1만 사용한다.

### I-270-a — 로컬 실행 서비스 (C-270)
- 위치: `scripts/myeongdong_ui_server.py`, `apps/myeongdong-ui/vite.config.ts`; 실행 입력 `sim/runs/ui_jobs/<id>/input`, 프레임 `apps/myeongdong-ui/public/runs/<id>`. Vite watcher는 `public/runs/**`를 제외하고 전용 미들웨어가 `/runs/<id>/run_status.json`과 `/runs/<id>/frames/frame_HHMM.bin`을 직접 읽는다(DR-274).
- 인터페이스: `GET /api/health`; `POST /api/runs` JSON `{pedestrian_only: boolean, discount_pct: 0|10|20}` → 202 `{run_id,replay_base,state}`; `GET /api/runs/<id>` → 준비·계산·완료·실패 및 실제 완료 틱; `GET /api/runs/<id>/metrics` → I-272 결과. Vite `/api`가 `127.0.0.1:5174`로 프록시한다.
- 로직: 건물별 설정 최대 1,252건을 담을 JSON 256KiB·정확한 키·값과 localhost Origin 검사 → UUID 실행 ID·비어 있는 고유 디렉터리 → 별도 입력 생성 → `memguard run --mem-gb 3`으로 Rust example 오프라인 release 빌드 → `sim/Cargo.toml`, `sim/Cargo.lock`, `sim/crates`의 Cargo.toml·build.rs·.rs 파일을 빌드 전후 SHA-256으로 비교 → 바이너리를 실행 전용 파일로 복사하고 원본·사본 해시 비교 → 빌드 출처를 source/job에 기록 → `memguard`로 그 사본 실행 → 원자적 job 상태 저장. `building`은 별도 상태이며 동시 실행 중 409; 빌드 중 소스 변경, 프로세스 종료와 완료 manifest 불일치는 failed. 셸 없이 고정 인자 배열을 사용한다. 재시작 시 이전 미완료 작업은 중단으로 표시한다.
- 오류: 잘못된 JSON/미지원 값 400, 출처 거부 403, 실행 중 409, 결과 미완료 409, 없는 ID 404, 엔진·해시·I/O 실패는 명시된 failed/500. 세션이나 외부 URL·명령은 입력받지 않는다.
- 엣지: 빈 body·큰 body·알 수 없는 필드·경로 탐색·동시 요청·빌드 중 소스 변경·엔진 기동 실패·서비스 재시작·상태 파일 생성 전 처리·프레임 파일 갱신 중 대기 조건 유지.
- 제약: 로컬 1개 실행, 3GiB, 4 Rust 스레드; 서비스 바깥 공개 금지. 테스트: `test_api_validation`, `test_one_job_at_a_time`, `test_small_actual_engine`, `test_failure_status` Given/When/Then 각각 유효·무효 요청, 중복 요청, 12명 실행, 실패 가시화. DoD: 모두 green, 서비스에서 새 실행 가능.

### I-271-a — 조건 적용 번들 (C-271)
- 위치/시그니처: `scripts/myeongdong_ui_server.py`, `prepare_input(base: Path, dest: Path, options: Options) -> Path`.
- 구조: 원본의 `input/`, `defs/`, `bundle/`, `source.json`을 별도 복사. `scenario.yaml.pedestrian_only=[]` 또는 `[{start:"14:00",end:"17:00"}]`; `places.parquet.discount_pct`는 0/10/20. `bundle/manifest.json`의 장소·시나리오 SHA-256과 입력별 bundle ID 갱신, 옵션과 가정을 source에 기록. 원본은 쓰지 않는다.
- 흐름/오류: 복사 → 조건 검증 → 해당 열 수정(원 Arrow 스키마 유지) → hash 계산 → manifest/source 기록 → Rust `Bundle::load` 및 `scenario_entry` 검증. 입력 누락·기존 목적지·해시/범위 불일치 시 중단.
- 엣지: 기본값·각 할인값·보행 전용 켜기/끄기·빈 장소·기존 출력·원본 변경 여부. 제약: 5044 프로필 유지, 161개 가정 장소만 할인, 3시간 창 고정.
- 테스트: `test_isolated_bundle_options`, `test_rust_validate_changed_bundle`, `test_input_unchanged`; DoD: 수정값/해시·Rust 검사 green.

### I-272-a — 결과 요약 (C-273)
- 위치/시그니처: `scripts/myeongdong_ui_server.py`, `summarize_run(output: Path, input: Path) -> dict`.
- 구조: manifest status complete, events·metrics_10min 해시 확인. events의 `enter` 수·`purchase` 수와 `value` 합을 전체 및 `place_id`별 집계하고 10분 지표의 `enters`·`purchases`·`revenue_krw` 합과 일치해야 한다. 장소 `store_ref`를 건물 ID로 사용한다. 엔진 결과에 없는 건물은 null/미연결.
- 오류/엣지: 미완료 409; 파일 누락·해시·합계 불일치·중복 매핑은 오류; 구매 0은 0, 161개 모델 외 건물은 미연결. 제약: 사건 약 7만 행, 최대 3GiB.
- 테스트: `test_actual_events_match_metrics`, `test_unmodeled_building`, `test_hash_tamper`; DoD: 실제 출력 비교 및 오류 green.

### I-273-a — 실행 UI (C-270·272·273)
- 위치: `apps/myeongdong-ui/src/engine/{client,manualRun,useReplay,useEngineRun}.ts`, `src/App.tsx`, `src/index.css`, README.
- 인터페이스: `startRun(options): Promise<Job>`, `getJob(id): Promise<Job>`, `getMetrics(id): Promise<Metrics>`; `useReplay(base: string)`은 URL 기본 실행 또는 새 고유 실행을 구독한다.
- 로직: DR-286의 수동 실행·초안 흐름을 따른다. 서비스 오류는 원인을 표시하고 자동 반복하지 않는다. 상태 폴링 → 첫 실제 상태가 만들어진 새 실행만 재생으로 전환 → complete면 검증 요약 호출. 준비 중 이전 결과가 보이는 동안은 명시적으로 이전 결과라고 표시한다. 3시간 전체 실험의 방문·구매·구매액 및 매칭된 건물 수치와 그 실행에 실제 적용된 조건을 표시한다. 입력에서 새 조건을 선택해도 기존 결과에 적용됐다고 표시하지 않는다. 직접입장/합성/물리 미검증을 알려 준다. 오류는 그대로 보이며 기존 실행으로 성공 표시하지 않는다.
- 엣지: 서비스 중단·중복 시작·준비 중 첫 조각 없음·느린 상태·실패·건물 미매칭·0·모바일 320px·새 실행 전환 시 이전 프레임·결과 제거. 계산 중에는 현재 실행의 조건과 사용자가 새로 고른 초안을 각각 구분해 표시한다.
- 제약: 두 청크 이하 캐시, 3시간 합성 실험을 실측·월 예측으로 부르지 않음. 테스트: `test_client_status`, `test_run_switch`, `test_actual_metrics`, DR-286 수동 실행·초안 테스트. DoD: 타입·UI 테스트·빌드 및 브라우저 사용 green.

## 원본 조작과 건물별 설정 (I-280-a~I-282-a)

공통 DR-280~282, 02c I-204-c, 기존 Python 3.13·PyArrow 24·Rust·React. 기존 2개 옵션의 실행 결과는 계속 읽는다.

### I-280-a — 원본 변수 카탈로그 UI (C-280)
- 위치: `src/engine/{client,manualRun,useEngineRun}.ts`, `src/App.tsx`, `src/index.css`, `src/preview/scenarios.ts`.
- 계약: 원본 v01~v16의 세 선택지. 옵션 `{pedestrian_hours:0|4|8,width_delta_m:0|0.5|1,stall_scale:0.8|1|1.2,bench_count:0|10|20,discount_pct:0|10|20,hours_extension:0|1|2,income_multiplier:0.9|1|1.1,price_multiplier:0.9|1|1.1,visibility_level:'low'|'base'|'high',building_overrides: Record<string,BuildingOverride>}`. 기존 `{pedestrian_only:boolean,discount_pct}`는 0 또는 8시간과 기본값으로 읽되 이전 실행의 실제 기록값은 별도 표시한다.
- 로직: 원본 16개 중 연결된 9개만 표시·변경 가능하다. v07·08·09·10·11·13·14는 DR-284에 따라 렌더링하지 않는다. 초기 v03 100%는 가정 노점 40개. 변경은 초안만 갱신하고 실행 버튼 클릭 시에만 전송한다(DR-286). 옵션 비교는 건물별 내용을 포함한다.
- 오류/엣지: 이전 실행 옵션 필드 누락, 무효 옵션·빠른 연속 변경·서버 중단·계산 중 마지막 선택, 320px 스크롤. 제약: 실측·예측으로 표현하지 않는다.
- 건물 설정 패널은 긴 내용을 스크롤하므로 종류 아이콘을 패널 내부에 배치해 잘리지 않게 하고, 모바일에서는 본문 옆에 둔다(되돌리기 쉬운 SELF 배치).
- 860px 이하에서 건물 선택 시 조건 패널을 접어 건물 패널을 가리지 않게 한다. 조건 패널은 다시 열 수 있다(DR-285).
- 테스트: `test_catalog_16`(원본 카탈로그 16·실제 노출 9), `test_option_migration`, `test_options_comparison`, 데스크톱·320px 조작. DoD: 미연결 항목은 보이지 않고 지원 값이 전달된다.

### I-286-a — 수동 실행·초안 보호 (C-286)
- 위치: `src/engine/useEngineRun.ts`, `src/engine/manualRun.ts`, `src/App.tsx`, `tests/conditions.test.mjs`, README. React 19·TypeScript 5.7 기존 의존성 유지.
- 인터페이스: `useEngineRun().start(options: RunOptions): Promise<void>`만 `startRun(options)`을 호출한다. `sameOptions(a,b): boolean`은 9개 값과 건물별 설정을 비교한다. `createRunGuard(): {begin(busy:boolean):boolean; end():void}`는 동일 요청과 진행 중 중복 시작을 차단한다. `applyOptions(next)`는 React 초안만 변경하고 사용자 편집 플래그를 기록한다.
- 흐름: 초기 결과를 한 번 불러와 사용자가 아직 편집하지 않았으면 초안을 채움 → 편집 시 표시만 갱신 → 패널 위쪽의 실행 버튼 클릭 → 기존 실행/요청 중이 아니면 즉시 중복 잠금을 잡고 현재 초안을 POST → 준비·진행·완료를 기존처럼 표시. 편집 후의 비동기 job/metrics 전환은 초안을 덮어쓰지 않는다. 진행 중 편집도 유지하며 자동 실행·예약을 만들지 않는다. 초안이 실행 중 또는 표시 중인 조건과 다르면 실행 전이라고 표시한다.
- 오류/엣지: 실행 중·중복 클릭은 요청을 추가하지 않는다. 서버 409/실패는 기존 오류로 표시하고 초안 유지. 실행 시작 요청 중 편집한 값도 유지. 결과 화면 로드 중 사용자가 먼저 편집해도 비동기 응답은 이를 덮어쓰지 않는다. 같은 조건 재실행은 사용자가 클릭하면 허용. 브라우저 닫을 때 초안 보존은 요구하지 않는다.
- 자원: 새 서버나 외부 의존성 없음. 검증 시 실제 5,044명 계산을 임의로 시작하지 않고 모의 HTTP API로 클릭 1회·요청 1회와 진행 중 초안 보존을 확인한다.
- 테스트: `test_manual_duplicate_guard` Given 미해결 요청 When 두 번 호출 Then 한 번 전송, `test_options_comparison` Given 전체/건물 값 When 비교 Then 변경만 감지, E286-1~5 Given 기존 실행 When 여러 조건·건물 편집 Then POST 0, When 실행 버튼 클릭 Then POST 1, When 진행 중 편집 및 완료 Then 초안 유지·추가 POST 0. DoD: 테스트·타입 검사·빌드와 브라우저 확인 green.

### I-281-a — 변수 반영 입력 빌더 (C-281)
- 위치/시그니처: `scripts/myeongdong_scenarios.py`, `scripts/myeongdong_ui_server.py`; `validate_options(value)->dict`, `prepare_input(base:Path,dest:Path,options:dict)->Path`.
- 로직: v01 시나리오 창, v02 `inside_boundary && (ped_zone || road_type==pedestrian)` 대상 full 폭·sidewalk 폭 증가, v16 경계 내 간선 광고와 상점 visibility ±0.2 clamp. v03 경계 내 ped_zone을 ID 순으로 이어 32/40/48개 등간격 노점, v04 같은 간선의 15m부터 30m 간격 0/10/20개 벤치. 노점은 16~23시·가격 5000·면적 3·음식 3:기념품 1·서비스 1·수용 15 가정. v05·06·15는 상점 할인·종료·가격을 변경한다. v12는 원본 synthetic_id와 소득 사양 해시를 대조한 뒤 02c `budget_from_income(I*multiplier,group)`으로 money comfort/limit/sensitivity와 attrs 소득을 다시 쓴다.
- 스키마: 원본 Arrow 스키마 유지, 변경된 각 Parquet와 시나리오의 SHA·행 수, bundle_id와 가정 기록. 정확히 5,044 고유 프로필과 직접 입장을 유지한다.
- 오류/엣지: 범위·열·원본 해시·참조 오류와 배치 공간 부족은 실패. 기본 노점도 40개 가정, 기간 밖 영업시간은 효과 0 가능, 가격·시계성 clamp, 음수·중복 거부.
- 테스트: 9개 변수의 정상·경계별 데이터 대조, 노점·벤치 위치 결정론, 원본 불변, Rust validate와 작은 실제 실행. DoD: 전부 green.

### I-282-a — 건물별 업종/상점 설정 (C-282)
- 위치: `scripts/myeongdong_ui_server.py`, `src/engine/client.ts`, `src/App.tsx`; `building_overrides[id] = {type:'cafe'|'restaurant'|'fashion'|'convenience'|'medical'|'original', discount_pct:null|0|10|20, price_multiplier:null|0.9|1|1.1, hours_extension:null|0|1|2}`.
- 카탈로그: GLB 해시 확인 및 ID 중심 추출, 정본은 footprint, GLB 전용은 C-263 역변환으로 중심을 추정. 최근접 실제 간선 ≤80m면 엔진 지정 가능, 아니면 시각 설정만 허용하며 미반영 안내. GLB 전용은 미검증 가정 표시.
- 장소: 기존 `store_ref`는 해당 행만 수정, 새 건물은 `S:<id>`를 기존 엔진 스키마의 간선 투영·대리 업종으로 추가한다. 4종은 기존 OFFER 기본값, 의료는 복사된 개발 정의에 일반 방문 클래스·category를 추가하고 해당 정의 해시를 갱신한다. 의료 구매/진료 모델은 없음을 표시한다. 건물별 비-null 설정은 전체 설정보다 우선한다.
- 의료 선택 시 비적용 할인·가격 설정을 비우고 그 선택지를 숨기되 장소 영업시간은 표시한다. 기존 장소가 없는 건물은 종류 선택 후에만 개별 조건을 보인다(DR-284).
- 오류/엣지: 중복 ID·불명확한 위치·보행망 밖 건물·지원하지 않는 종류·앵커가 엔진에서 제외될 때 성공으로 표시하지 않는다. GLB 외곽 미검증은 별도 가정이다.
- 테스트: `test_catalog_alignment`, `test_two_building_overrides`, `test_medical_generic_visit`, `test_unmodeled_visual_only`, Rust validate·실행과 브라우저 개별 선택. DoD: 사용자 지정값이 해당 건물에만 적용되고 재실행 후 유지된다.

## 가설 검증과 시연 (I-290-a~I-292-a)

### I-290-a — 격리된 CLI 실험 실행기 (C-290)
- 위치: `scripts/myeongdong_hypotheses.py`; Python 3.13·PyArrow 24·PyYAML 6과 기존 Rust 바이너리. 외부 패키지 추가 없음.
- 인터페이스: `python3 scripts/myeongdong_hypotheses.py --batch <id> --cases <ids> --seed-index <0|1>`; 각 실행은 새 `ui-<uuid>`의 `sim/runs/ui_jobs/<id>/input`, `job.json`, `engine.bin`과 `apps/myeongdong-ui/public/runs/<id>/`를 생성한다. 고정 참조 실행과 원본은 수정하지 않는다.
- 스키마: batch JSON은 고정 바이너리·원본 해시·후보 정의·시드 및 케이스별 `{run_id,options,state,summary}`. source는 `engine_build`와 실험 ID·시드·원본을 기록한다. job은 기존 `/api/runs/<id>` 계약을 따른다.
- 흐름: 참조 바이너리 해시 확인 → baseline/단일 수정 옵션 정규화 → `prepare_input` → 시드 및 scenario entry·SHA·bundle ID 일관 갱신 → 각 건을 `memguard run --priority low --mem-gb 3`으로 순차 실행 → 5,044 고유 인원·프레임·실행 hash 및 events/metrics 대조 → 배치 결과를 원자적으로 저장. 사용자 작업은 슬롯을 우선한다.
- H7의 사양 원본 해시가 달라진 경우 DR-292의 110% 기존 프로필·속성 사본을 해시·동일 페르소나·기존 출처 검증 후 사용한다. scenario의 소득 설정과 프로필 hash·bundle ID·출처를 함께 갱신한다.
- 오류: 입력·프로필·시드·해시·엔진 불일치, 실패한 프로세스·불완전 기록은 그 케이스 failed로 남기고 성공 수치를 표시하지 않는다. 기존 경로 덮어쓰기 금지. 중단 시 완료 결과는 유지한다.
- 엣지: 건물 장소 없거나 보행망 밖이면 실패, 0 사건·0 분모는 상대비율 없음, 동일 이름 중복·중복 실행·시드 범위 오류 거부. 성능: 엔진 1개씩, `RAYON_NUM_THREADS=4`, 약 120MB/실행.
- 테스트: 입력 계약·시드 일치·단일 변경·빈 분모 단위, 실제 CLI baseline/후보의 manifest와 사건·프레임 `mdsim verify` 통합. DoD: 모든 실행 추적 및 일관성 검사 green.

### I-291-a — 결과·후보 선택 (C-291)
- 위치: `scripts/myeongdong_hypotheses.py`, `output/myeongdong-hypotheses/<batch>/report.json`·`report.md`.
- 가설/주 지표: H1 할인 20%→구매 건수 증가; H2 광고 높음→인지 사건 증가; H3 보행전용 8시간→시간평균 유효 면적 증가; H4 벤치 20→휴식 시작 증가; H5 보행 폭 +1m→시간평균 유효 면적 증가; H6 밀레오레 업종 카페→해당 장소 입장 변화; H7 소득 110%→구매 건수 증가. 효과 크기·반대 방향·0은 그대로 기록한다.
- 계산: 인지·휴식은 `events.parquet` 사건 수, 입장·구매·구매액은 `summarize_run`의 검증값, 유효 면적은 `metrics_10min`의 `edge_effective_area_m2`를 시간별 전체 간선 합산 후 18개 구간 평균한다. 건물 지표는 `store_ref`의 사건을 사용한다. 절대 차이와 기준이 0이 아닌 경우에만 상대 차이를 낸다.
- 보조 지표: 할인·소득은 구매액도 함께 표시한다. 광고 인지 비교는 각 10분 구간의 평균 재실 인원에 10분을 곱해 합산한 인·분과 인지 사건/1,000 인·분을 함께 기록한다. 건물 업종은 대상 장소 구매·구매액을 함께 표시한다.
- 두 번째 시드는 별도 배치에서 병렬 준비하되 실제 엔진은 memguard로 직렬 실행한다. 결과 통합은 두 배치의 고정 엔진·기준 입력·시드·후보 정의가 일치할 때만 허용한다.
- 짝 비교 전 동일한 원본 출처와 실제 입력 manifest의 파일별 해시를 대조한다. 단일 변수에 따른 변경 역할만 허용한다: 할인·벤치·업종은 places, 광고는 edges·places, 보행 전용은 없음, 폭은 edges, 소득은 profiles·profile_attrs. 그 외 파일 차이는 해당 비교를 무효 처리한다.
- 기준과 변경 실행의 `agents.parquet`에서 persona_id·arrival_tick·portal_in·mode 전부를 비교해 같은 5,044명의 입장 계획을 확인한다.
- 기준과 첫 시드의 동일 입력을 별도 배치에서 반복해 인물·사건·프레임 해시를 비교한다(DR-293). 다른 시드의 방향 비교는 이 반복과 별개다.
- 선정: 실제 차이가 충분하고 설명 가능한 사례를 우선하며 첫 시드와 두 번째 시드에서 방향이 같아야 한다. 사건 지표는 기준/변화 사건 수가 각각 10 이상이고 절대 차이 5건 이상인 사례를 우선한다. 공간 지표는 기하학적 가정의 직접 결과로 분류한다. 모든 후보와 미선정 사유를 공개해 사후 선택을 감춘 확증으로 표시하지 않는다.
- 시연은 조건을 만족한 건물·할인·소득·공간 사례 순으로 최대 4개를 표시한다. 기준을 만족하지 못하면 빈자리를 임의의 사례로 채우지 않는다.
- 테스트: 동일 설정/바이너리/시드의 쌍과 다른 단일 필드 검사, 0분모·음수 방향·희소 사건·2시드 반전 판정, 사건 합계 대조. DoD: 결과와 미선정 이유가 보고서에 남는다.

### I-292-a — 시연 화면 (C-292)
- 위치: `apps/myeongdong-ui/public/demo/index.html`, `apps/myeongdong-ui/public/demo/results.json`, 기존 `src/App.tsx` 링크. 기존 앱의 일반 재생과 수동 실행은 유지한다.
- 인터페이스: JSON은 `{schema_version,batch_id,engine,baseline,candidates,selected,limitations}`; 각 사례에 설명·변경값·시드별 기준/변경 지표·기준/변경 `ui-<uuid>` 실행 ID가 있다.
- Vite SPA가 `/demo/`를 메인 앱으로 되돌리므로 시연 주소와 앱 링크는 `/demo/index.html`을 사용한다.
- 흐름: 검증 결과를 정적으로 기록 → 시연 화면이 JSON을 읽어 추천 가설과 검증 결과·모든 후보를 표시 → 기존 `/`의 두 기록 링크를 열어 실제 프레임과 지표를 재생한다. 페이지 열기·링크 클릭은 POST하지 않는다.
- 건물 사례 링크의 `building=(way|relation|node)/<숫자>`를 3D 모델 로드 후 찾아 선택하고 해당 건물의 bounding sphere를 기준으로 카메라를 배치한다. 일치하지 않거나 부적합한 ID는 자동 선택하지 않는다(DR-294).
- DR-297에 따라 사용자에게 보이는 시연·재생 문구에는 요청받은 제외 표현을 쓰지 않고, 수치는 시뮬레이션·모형 기록으로 표시한다. 내부 보고서의 출처는 보존한다.
- 오류/엣지: JSON 누락·불완전 실행·0분모·추천 0개는 이유와 함께 표시, 임의 성공 수치 금지. 좁은 화면 표는 가로 스크롤 또는 세로 전환. 웹 접근은 loopback, 새 의존성 없음.
- 테스트: JSON 스키마와 링크 ID/지표 대조, 브라우저 추천·비추천·두 재생 링크·320px/데스크톱·POST 0. DoD: 사용자가 실제 기록 기반으로 즉시 시연할 수 있다.

## 시연 영상 (I-293-a~I-294-a)

### I-293-a — 브라우저 녹화 (C-293)
- 위치: `output/myeongdong-hypotheses/demo-20261031-a/video/`와 브라우저 CUA 세션. 고정된 기존 3D 기록과 시연 페이지를 조작한다.
- 계약: 1280×720 브라우저 뷰포트의 탭만 캡처하고 실제 시점별 프레임을 장면별 디렉터리에 저장한다. 실제 캡처 픽셀 크기는 3D 1280×720, 스크롤바가 있는 비교 페이지 1265×712로 기록하며 편집 시 1280×720에 비율 보존해 맞춘다. `{scene,run_id,frame_count,capture_rate,dimensions,description}` manifest를 기록한다. 최소 기준·변경 건물, 다른 조건 결과·비교 페이지, 결론을 담는다.
- 흐름: 별도 테스트 탭 생성 → 시연 페이지 → 기준 재생 및 건물 선택 → 변경 재생 → 할인/소득/보행면적 비교 → 각 장면의 화면 변화를 연속 캡처한다. 아무 엔진 실행 버튼도 누르지 않는다.
- 오류/엣지: 로딩 실패·프레임 0개·비교값 불일치 시 해당 장면을 다시 캡처한다. 사용자의 기존 탭과 기존 작업은 보존한다. 자원: 초당 약 8–12프레임, 고해상도 WebGL 탭 하나.
- 테스트: 장면 URL과 실행 ID 대조, 연속 프레임 변화·촬영 시간 검증. DoD: 필요한 실제 장면이 모두 보관된다.

### I-294-a — MP4 편집 (C-294)
- 위치: `scripts/edit_myeongdong_demo_video.py`, `output/myeongdong-hypotheses/demo-20261031-a/demo-video.mp4` 및 영상 제작 메타데이터. Pillow와 로컬 FFmpeg 8.1.1을 사용한다.
- 흐름: 실제 캡처 장면에 읽기 쉬운 한글 자막을 합성하고 컷 길이를 맞춘다. 검증 결과의 2개 시드와 모형 결과임을 텍스트에 담고 DR-297에서 제외한 표현은 넣지 않는다. H.264 1280×720 무음 MP4로 내보낸다(DR-296).
- 오류/엣지: 인코딩 실패·잘린 자막·영상 없는 결과를 실패 처리한다. 음성·배경음·외부 이미지나 음악을 사용하지 않는다. 성능: 1개 인코딩, 4스레드 이하.
- 테스트: ffprobe에서 H.264·오디오 트랙 없음·길이·해상도 확인, 시작·각 컷 중간·마지막 프레임 추출/육안 확인, 수치 vs `report.json` 대조. DoD: 실제 화면 기반 편집 MP4가 재생된다.

## 최종 소스 배포 구현 (I-300-a~I-302-a)

### I-300-a — 선정 파일과 문서 (C-300)

- 위치: `.gitignore`, `README.md`, `apps/myeongdong-ui/` 소스·설정·테스트 코드, `scripts/`, `sim/crates/`, `sim/Cargo.toml`, `sim/Cargo.lock`, `tests/`, `docs/maps/`.
- 기술 선택: 기존 Python 3, Node.js 24/pnpm 잠금 파일, Rust 2021/Cargo 잠금 파일을 유지한다. 신규 런타임 의존성 없음.
- 인터페이스와 데이터: 도메인 데이터와 지도·글꼴·바이너리 테스트 픽스처는 하나도 포함하지 않는다. Node package/TypeScript 설정 JSON은 소스 설정이므로 포함한다.
- 흐름: 코드·테스트·정본 선정 → 데이터·자산·결과·환경·빌드 파일 제외 → `README.md`에 누락 경로와 제약을 기재한다.
- 에러·엣지: 필수 소스 파일 누락 또는 금지 데이터 존재 시 업로드하지 않는다. 파일 크기는 GitHub 단일 파일 100 MB 미만으로 제한한다.
- 테스트: Given 선정 폴더, When 파일 목록·비밀 패턴 검사, Then 필수 소스가 있고 금지 데이터·생성 산출물·자격 증명이 없다.
- 성능/자원: 각 파일 100 MB 미만; 그 외 제약 없음. DoD: 테스트 통과, 출처와 제한 기록.

### I-301-a — 메모리 보호 경로 선택 (C-301)

- 위치: `scripts/myeongdong_ui_server.py`의 `main() -> None`, `apps/myeongdong-ui/README.md` 및 루트 README. 기술·의존성은 I-300-a와 같다.
- 인터페이스: CLI `--memguard PATH`는 `pathlib.Path`이며 기본값은 `Path.home() / 'memguard/bin/memguard'`(기존 컴퓨터에서는 종전과 동일). `RunManager(..., guard=args.memguard, ...)`에 전달한다. 기존 `--input`, `--port`는 그대로다.
- 흐름: CLI에서 경로를 읽고 서버를 생성한다. 보호 파일이 없으면 기존 health 오류를 유지한다. 빌드/실행 시 guard 실행과 3GB 인수는 변경하지 않는다.
- 에러·엣지: 존재하지 않는 경로는 health `ready: false`; 지정된 정상 경로는 `ready: true`(다른 전제 충족 시). 경로가 생략되면 기존 기본값 사용. 오프라인 Cargo 미준비 시 기존 빌드 실패가 노출된다.
- 테스트: Given 로컬 선택 입력·유효한 guard, When 서버 실행과 health 조회, Then ready true; Given 없는 guard, Then ready false. 로컬 데이터를 일시적으로 제공한 환경에서 기존 UI 타입·테스트·빌드 및 Python 입력→Rust validate 성공. 데이터 없는 저장소는 바로 빌드할 수 없음을 명시한다.
- 성능/자원: 기존 guard의 3GB 예약 인수를 유지한다(강제 메모리 제한이 아님). DoD: 설명서와 경계 검사 통과.

### I-302-a — GitHub 브랜치 (C-302)

- 위치: `hackathon-B-20261030/trade-area-qa`의 `feat/` 브랜치. 기존 `main`의 두 파일은 보존한다.
- 인터페이스: GitHub Git tree/commit/ref. 첫 소스 커밋의 부모는 확인한 `main` HEAD이고, 이후 검증 결과를 기록하는 커밋은 그 소스 커밋의 자손이다. 트리는 main tree를 기반으로 선정 파일을 추가한다.
- 흐름: 준비된 파일 및 검사 결과 확인 → blob/tree/commit 구성 → 새 브랜치 생성 → 원격 ref/tree/부모를 읽어 검증한다. 충돌·권한 오류 시 덮어쓰지 않고 중단한다.
- 에러·엣지: 브랜치 이름 중복은 새 이름 선택; `main` 변경 시 다시 확인; 원격 쓰기 실패는 실패로 보고. 성능/자원: 파일당 100 MB 미만.
- 테스트: Given GitHub에 생성한 브랜치, When ref/tree/commit을 다시 조회, Then HEAD가 생성한 커밋이며 기존 원격 파일과 필수 소스가 있고 첫 소스 커밋의 부모가 확인한 main HEAD다. DoD: 해당 검증 전부 통과.

### I-303-a — main README (C-303)

- 위치: 원격 `main`의 `README.md`. 원격 `main`을 부모로 한 `docs/source-branch-readme` 브랜치를 만들고 README만 추가하는 PR을 연다.
- 인터페이스: README에 정확한 소스 브랜치명, GitHub 브랜치 링크와 `git switch feat/myeongdong-simulation-source`를 적고 데이터 미포함을 명시한다.
- 흐름: 소스 브랜치 확인 → main 현재 HEAD 재확인 → README만 담은 커밋·브랜치 생성 → main 대상 PR → 보호 규칙에 따라 병합 → main의 README 조회.
- 에러·엣지: main이 바뀌면 새 HEAD를 기준으로 구성; 브랜치 중복은 다른 이름을 사용; PR 병합이 보호 규칙에 막히면 강제 우회하지 않고 상태를 보고한다. 성능/자원: 제약 없음.
- 테스트: Given main README PR, When diff 확인, Then README만 추가; Given 병합된 main, When 파일 조회, Then 브랜치명·전환 명령 및 데이터 미포함이 정확하다. DoD: 두 검사 통과.

## 사례별 대조 영상과 Manim (I-310-a~I-312-a)

### I-310-a — 실제 화면 캡처 (C-310)
- 위치: `output/myeongdong-hypotheses/demo-20261031-a/video-manim/`; CUA 브라우저 탭만 캡처. 입력은 `report.json`의 building·discount·income·pedestrian 각 `pairs[0]` 기준·변경 ID.
- 계약: 장면 `{case,side,run_id,fps,duration,frame_count}`과 `frame_%04d.jpg`; 1280×720 기준 8fps, 기준·변경 각각 5~7초. 건물은 동일 건물 선택, 다른 세 사례는 해당 조건과 엔진 결과를 확인한다.
- 흐름: 보고서에서 ID를 읽어 별도 탭을 열고 재생 → 8개 화면을 연속 캡처 → manifest를 쓴다. 새 실행 버튼은 누르지 않는다.
- 오류/엣지: 로딩 실패·ID/조건 불일치·프레임 부족은 중단 후 해당 장면 재촬영. 성능: 한 번에 WebGL 탭 1개.
- 테스트/DoD: Given 각 저장 결과, When 실제 UI에서 열어 캡처, Then 기준·변경 8개 모두 표시 ID와 보고서 일치 및 프레임이 움직임.

### I-311-a — Manim 대조 장면 (C-311)
- 위치: `scripts/myeongdong_manim_scenes.py`; 보고서 `report.json`; 결과 `video-manim/manim/`. 기술: Python 3.10, Manim Community 0.19.0, Cairo/Pango 및 로컬 한글 글꼴, 격리 venv.
- 인터페이스: Manim CLI 각 Scene 이름 `Building`, `Discount`, `Income`, `Pedestrian`; 입력 JSON의 `candidates[].pairs[0:2].before/after`와 `unit`.
- 흐름: 각 사례 제목·조건·단위 → 0부터 같은 비율의 두 시드 기준/변경 막대와 실제 숫자를 애니메이션으로 표현 → 정확한 해석 주의를 표시.
- 오류/엣지: 결과/시드 누락, 음수·잘못된 척도, 글꼴 미표시, 렌더 실패 시 중단. 성능: 1280×720·24fps·CPU 순차 실행, memguard 보호.
- 테스트/DoD: Given 보고서 두 시드, When Manim 렌더, Then 네 무음 장면·올바른 4개 값과 단위·잘리지 않은 한글·0 기준 막대가 확인된다.

### I-312-a — 연결·자막·검증 (C-312)
- 위치: `scripts/edit_myeongdong_manim_video.py`, `output/myeongdong-hypotheses/demo-20261031-a/demo-video-manim.mp4`; FFmpeg 8.1.1과 Pillow.
- 흐름: 사례별 기준 UI→변경 UI→Manim 장면을 순서대로 연결, UI에 조건·지표 한국어 자막 추가, 1280×720·24fps H.264로 통일해 무음 MP4 출력. 기존 영상은 덮어쓰지 않는다.
- 오류/엣지: 입력 누락·보고서 해시 불일치·자막 잘림·영상/오디오 트랙 오류는 실패. 제외 표현을 표시하지 않고 모형 결과임을 밝힌다.
- 테스트/DoD: ffprobe·전 구간 디코딩, 12개 컷 추출 육안 검사, 수치와 기록 대조·실제 재생 움직임·무음·정상 길이 확인.

## Changelog

- 2026-10-31: 배포 선정·이식 경로·원격 검증 I-300-a~I-302-a 추가.

- 2026-10-31: DR-260·261의 실제 명동 구조화 입력과 5,044 직접 입장 재생 I-260-a~I-264-a 추가.

- 2026-10-31: I-250-a~I-253-a 이식, 정책 통과 frozen 설치·타입/빌드, 프레임 및 좌표 테스트, 320px·데스크톱 브라우저 검증 완료. 원본 UI의 모바일 조건 패널 숨김과 16번 기본값 차이는 연결 전 과제로 남겼다.
- 2026-10-31: 사용자 결정 DR-243에 맞춰 Three.js·타입 0.186.0과 잠금 파일 갱신을 기록했다.
- 2026-10-31: DR-241·242에 따라 I-250-a~I-253-a의 계약과 검증을 추가했다.

- 2026-10-31: 페르소나 반응 온톨로지 C-201~C-216의 구현체 Map 안내 절을 추가했다(Python 02c, Rust 02d, DR-214·DR-215). 다른 절은 바꾸지 않았다.
- 2026-10-31: 자료 조사 보고서 I-240-a~I-242-a의 계약과 검증 명세를 추가했다.

- 2026-10-31: I-152~154의 실제 영상·25개 좌표·사진 4장·UV·709개 건물·347도로·브라우저 및 원본 불변 검증 완료. 직접 `file://` 열기는 앱 보안 정책상 자동 검증하지 못했다.

- 2026-10-31: Sentinel-2 원본 창·사진별 건물/벽면/라이선스·3D UV·검증의 I-152-a~I-154-a 명세 추가.

- 2026-10-31: I-140-a~I-142-a 구현 및 원표·10분 프로필·65,000명 연령 상대 가중치 전수 재독 검증 완료.
- 2026-10-31: 명동 상권 유동인구의 10분 상대 프로필·인물 연령 가중치·온톨로지 연결 작업 계획 추가.

- 2026-10-31: 정방향·역산 I-100-a, I-101-a, I-102-a, I-103-a, I-104-a/b, I-105-a의 계약·흐름·오류·엣지·자원·테스트·DoD를 추가했다. 엔진 구현 없이 설계만 작성했다.

- 2026-10-31: I-130-a~I-133-a 구현·테스트 및 실제 65,000행 통계·온톨로지 최소 계약 재독 검증 완료. 공개된 구별 혼인 셀과 비공개 제외는 DR-136에 따른다.
- 2026-10-31: 명동 65,000명 증강의 통계·온톨로지 계약·구현 및 검증 계획 추가.

- 2026-10-31: 대화형 명동 지면 고도 시각화 I-90-a와 원본·표현·검증 계약 추가.
- 2026-10-31: 명동 최종 스무딩 지형 I-91-a와 연속 3D 표면 I-92-a의 입출력·알고리즘·검증 명세 추가.
- 2026-10-31: I-91-a·I-92-a의 합성·실제 데이터 및 3D 브라우저 검증 완료.
- 2026-10-31: 사용자 제공 건물·도로와 주변 지형의 3D 지도 I-93-a~I-95-a 구현·검증 계약 추가.
- 2026-10-31: I-93-a~I-95-a 구현 및 데이터·메시·브라우저 검증 완료. `file://` 직접 검증은 앱 보안 정책 차단으로 별도 기록했다.
- 2026-10-31: I-90-a 실제 원본 3,249셀·유효 이웃 대조, 브라우저 3D 표시·회전·확대·고도 툴팁 및 작은 화면 검증 완료.
- 2026-10-31: 미리보기에서 격자 틈과 작은 화면 문제를 발견해 DR-91의 원본 셀 이음면 및 키보드 조작 계약을 추가했다.
- 2026-10-31: 명동 지면 고도 작업 I-10-a~I-12-a의 원본·계약·오류·테스트 명세 추가.
- 2026-10-31: I-10-a~I-12-a 구현 및 실데이터 원본값·행정동 교차·해시·메타데이터 검증 완료.
- 2026-10-30: 최초 구현체 Map 작성.
- 2026-10-31: 독립 명동 추출 작업 I-09-a와 실제 데이터 경계 검증 명세 추가.
- 2026-10-31: I-09-a 구현 후 전체 원본과 결과의 행·스키마·순서, CSV, 근거 및 해시를 검증했다.
- 2026-10-30: 반복 전송 실패 시 임시 결과 보존 절차 추가.
- 2026-10-30: 원본 9개 및 최종 결과 재독 검증 완료. 연결 중단 자동 재시도도 실제 확인했다.
- 2026-10-30: 범위 안내 추가(C-01~C-03 한정, C-04~C-08 구현체는 작성 전). I-01~I-03 내용은 바꾸지 않았다.
- 2026-10-31: 활동 엔진 C-04~C-08의 구현체 I-04-a~I-08-e(21개), 공통 규약, 단계 간 데이터 계약, 활동 엔진 테스트 추적표(E04-1~E08-6 28개 ↔ 구현 테스트), SELF 결정 기록(S-01~S-27)을 추가했다. 규칙 데이터는 별도 Map [02a_rule_data.md](02a_rule_data.md)로 분리했다. 코드 블록은 서울 원본의 앞 표본(추출 20,000명, 계획·프레임 3,000명 × 7일)에서 불변식을 확인한 프로토타입 소스를 그대로 옮긴 것이다(전원 실행은 하지 않았고, 프로젝트에는 코드를 만들지 않았다). I-01~I-03 내용은 바꾸지 않았다. 상태 draft(구현체 Map 확인 전).
- 2026-10-31: 구현 순서 Map(Phase 6) 작성 중 발견한 빈칸을 보강했다. I-08-e에 `RunConfig`·`load_gazetteer`·`load_geo`·`run_cues`·`plan_all`·`verify_run`·`run_all`·`main` 시그니처와 개발·테스트용 옵션 `--source`·`--source-meta`·`--rules-dir`를 추가했고, `config.py` 경로 상수에 `METADATA_JSON`을 추가했다. 전용 테스트가 없던 CLI 명령 셋의 테스트(`test_cli_install_rules`, `test_cli_frames`, `test_cli_evaluate`)를 I-08-e에 추가했고, I-06-a와 이름이 겹친 I-04-e의 추가 테스트를 `test_classify_longest_token_wins`로 바꿨다. 그 밖의 빈칸(테스트 픽스처, 느린 테스트 표지, 지리 자료 구축 방법)은 [03_build_order.md](03_build_order.md)의 '구현체 Map 보강' 표에 해당 todo와 함께 적었다.
- 2026-10-31: 사례별 실제 녹화·Manim 애니메이션·무음 자막 영상 I-310-a~I-312-a 계획 추가.
