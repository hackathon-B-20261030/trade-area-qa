# 명동 상권 시뮬레이션 — 소스 코드

명동 3D 화면, Python 로컬 API와 Rust 시뮬레이션 엔진의 소스다. 이 브랜치에는 사용자의 요청에 따라 **데이터를 포함하지 않았다**. 기존 실행 결과는 실측 방문자나 수요 예측이 아닌 가정 기반 시뮬레이션이며 엔진의 물리 검증은 완료되지 않았다.

## 구성

| 경로 | 내용 |
| --- | --- |
| `apps/myeongdong-ui/` | React/Three.js 화면과 프런트엔드 테스트 코드 |
| `scripts/` | Python API, 시나리오 및 데이터 준비 코드 |
| `sim/` | Rust 엔진, CLI, 보고서 코드와 테스트 |
| `docs/maps/` | 설계, 구현·검증 계획 및 결정 기록 |
| `tests/` | Python 데이터·시나리오·통합 테스트 코드 |

## 데이터 제외와 실행 조건

입력 데이터, 원본·생성 데이터, 온톨로지 사전, 지도 GLB, 글꼴, 실행 기록, 프런트엔드의 지도·상점 JSON, 시연 결과 및 바이너리 테스트 픽스처는 모두 제외했다. 따라서 이 저장소만 복제해서는 화면을 빌드하거나 시뮬레이션을 실행할 수 없다. 실행하려면 원본 작업 환경에서 필요한 자료를 별도로 제공해야 한다.

주요 누락 경로는 다음과 같다.

- `apps/myeongdong-ui/src/data/building-business-data.json`과 `apps/myeongdong-ui/src/data/myeongdong-boundary-model.json`
- `apps/myeongdong-ui/public/myeongdong.glb`, `apps/myeongdong-ui/public/fonts/`, `apps/myeongdong-ui/public/demo/results.json`
- `apps/myeongdong-ui/tests/fixtures/frame_1400.bin`과 시뮬레이션 실행 기록
- 명동 5,044명 입력 번들, `data/` 아래의 원본·가공 데이터와 `ontology/v1/lexicon.yaml`

일부 기존 통합 테스트는 위 자료 외에도 원본 결과 및 `/private/tmp` 경로 파일을 전제로 한다. 소득 배율 변경에는 원본 페르소나와 입력 번들이 기록한 동일한 사양 해시가 필요하다. 현재 사양 파일은 그 해시와 달라 별도 복원 없이 사용할 수 없다.

## 의존성 및 실행

데이터가 복원된 환경에서는 Node.js 24, pnpm 11, Rust/Cargo, Python 3과 별도의 `memguard run` 호환 실행 파일이 필요하다. memguard는 이 저장소에 포함되지 않는다. 3 GB는 memguard의 예약 인수로 강제 메모리 상한이 아니다.

```bash
python3 -m venv .venv
source .venv/bin/activate
python -m pip install -r requirements-ui.txt
pnpm -C apps/myeongdong-ui install --frozen-lockfile
cargo fetch --locked --manifest-path sim/Cargo.toml
```

명동 입력 번들과 필요한 화면 자산을 복원한 뒤, 각각 다른 터미널에서 실행한다.

```bash
python scripts/myeongdong_ui_server.py \
  --input /path/to/prepared-5044-input \
  --memguard /path/to/memguard --port 5174
```

```bash
pnpm -C apps/myeongdong-ui dev --host 127.0.0.1 --port 5173
```

브라우저에서 `http://127.0.0.1:5173/`을 연다. 엔진은 실행 시 잠금 파일을 사용해 오프라인 빌드하며 두 서버는 로컬 루프백 주소에서만 사용한다.

## 검증

자료와 의존성을 복원한 환경에서는 다음 명령을 사용할 수 있다. 일부 통합 테스트는 위에 적은 별도의 로컬 결과 경로가 필요하다.

```bash
pnpm -C apps/myeongdong-ui typecheck
pnpm -C apps/myeongdong-ui test
pnpm -C apps/myeongdong-ui build
cargo test --manifest-path sim/Cargo.toml --workspace --locked
```

자세한 배경과 한계는 [화면 설명](apps/myeongdong-ui/README.md), [엔진 설명](sim/README.md), [결정 기록](docs/maps/decisions.md)을 참고한다.
