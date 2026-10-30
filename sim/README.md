# mdsim — 명동 에이전트 틱 시뮬레이션 엔진 (Rust)

설계 정본은 `docs/maps/`다. 개념은 01b(C-220~C-228), 구현은 02d, 순서는 03a, 결정은 decisions.md(DR-220~DR-225, DR-229)에 있다. Python ↔ Rust 파일 계약의 정본은 02c §2다. 역산은 구현하지 않았다(DR-225).

## 흐름

페르소나 프로필(Python이 만든 `profiles.parquet`) → 온톨로지 정의(`ontology/v1/*.yaml`)로 요구·제공 계산 → 0.5초 틱 에이전트 시뮬레이션(시야·알아차림·시선·결정·이동·줄·상호작용) → 실행 기록(Parquet) → 그림 13장과 `report.html`.

## 크레이트

| 크레이트 | 하는 일 |
|---|---|
| `mdsim-core` | 정의 읽기와 식 평가, 키드 난수(Python과 비트 일치), 번들·시나리오 읽기, Parquet 도우미, 합성 고정 자료 |
| `mdsim-runtime` | 연속 공간, 환경, 반응 함수, 상황 구성, 방문지 선택, 지각, 행동 계층, 이동, 틱 실행기, 기록, probe |
| `mdsim-plot` | 의존성 없는 SVG 그림과 HTML 보고서 |
| `mdsim-cli` | `mdsim` 실행 파일 |

## 쓰는 법

```bash
cargo build --release -p mdsim-cli
B=target/release/mdsim

# 합성 고정 자료(02c §2 형식): ontology/v1, data/reaction-inputs, data/reaction-bundles/<id>
$B fixture --out runs/fixture

# 확인(정의·번들·시나리오 대조)
$B validate --root runs/fixture --defs runs/fixture/ontology/v1 --bundle runs/fixture/data/reaction-bundles/<id> --scenario <시나리오>

# 실행: 실행 기록 표, 렌더 프레임, 체크포인트, plots/, report.html
$B run --root runs/fixture --defs runs/fixture/ontology/v1 --bundle <번들 폴더> --scenario <시나리오 파일> --out runs/my_run

# 체크포인트에서 이어 달리기(처음부터 돈 것과 바이트가 같다)
$B run … --out runs/my_run --resume runs/my_run/checkpoints/ckpt_1430.json.gz

# 결정 하나 평가(검증용, 난수 없음), 그림 다시 그리기, 기록 해시 확인
$B probe --defs … [--bundle … --root …] --in situations.jsonl --out result.jsonl
$B plot --run runs/my_run --root … --defs … --bundle … --scenario …
$B verify --run runs/my_run
```

실제 명동 번들이면 `--root`는 저장소 루트, `--defs`는 `ontology/v1`, `--bundle`은 `data/reaction-bundles/<bundle_id>`다. 실패하면 표준 오류에 `{"code", "stage", "message"}` 한 줄을 쓰고 종료 코드(2 정의, 3 계약, 4 불변식, 5 설정, 1 입출력)로 끝난다.

## 테스트

```bash
cargo test --workspace
```

## 측정 (합성 고정 자료, 8스레드, release, 2026-10-31)

- 확인용 1시간·392명: 2.0초, 최대 메모리 60MB (`runs/fixture_quick`)
- 기본 3시간·5,064명: 16.7초, 628MB, 최대 동시 612명 (`runs/fixture_default`). 목표 동시 2,000에 못 미치는 것은 체류 보정 빈칸이다(02d §6)
- 부하 확인(대표 비율 ×3.4): 16,544명·최대 동시 2,402명 85초, RSS 462MB
- 행사일 1시간 (`runs/fixture_event`), 3개월 상점 적응 평일·주말 30일 (`runs/fixture_monthly`, 13초)
- VISIT 6.5만 명: 0.2초, 0.65GB

`runs/ui_jobs`·`runs/myeongdong_*`는 화면(apps/myeongdong-ui)이 만드는 실행이라 이 문서의 명령과 무관하다.
