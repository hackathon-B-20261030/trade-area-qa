# AGENTS.md

이 파일은 이 저장소에서 작업하는 AI 코딩 에이전트(Claude Code, Codex, Cursor 등)와 사람 모두를 위한 작업 규칙입니다.

## 프로젝트 개요

- 저장소: https://github.com/hackathon-B-20261030/trade-area-qa
- 상태: 초기 세팅 단계 (기술 스택 미정 — 결정되면 이 섹션을 갱신하세요)

## Git 워크플로우 (필수)

`main` 브랜치는 GitHub Ruleset으로 보호되어 있습니다. **`main`에 직접 커밋하거나 푸시하지 마세요.**

1. 작업 시작 전 최신 `main`에서 새 브랜치를 만듭니다.
   ```bash
   git switch main
   git pull origin main
   git switch -c <type>/<짧은-설명>
   ```
2. 브랜치 이름 규칙: `<type>/<설명>` (kebab-case)
   - `feat/` 새 기능
   - `fix/` 버그 수정
   - `docs/` 문서
   - `refactor/` 리팩터링
   - `chore/` 설정, 빌드, 의존성 등
   - 예: `feat/trade-area-search`, `fix/login-redirect`
3. 작업 브랜치에서 커밋하고 원격에 푸시합니다.
   ```bash
   git push -u origin <브랜치명>
   ```
4. GitHub에서 `main` 대상으로 Pull Request를 생성합니다.
   ```bash
   gh pr create --base main --fill
   ```
5. PR이 병합되면 로컬 브랜치를 정리합니다.

### 에이전트 규칙

- 현재 브랜치가 `main`이면 코드를 수정하기 전에 반드시 새 브랜치를 먼저 만드세요.
- `git push --force`로 `main`을 덮어쓰지 마세요.
- 사용자가 명시적으로 요청하기 전에는 커밋, 푸시, PR 생성을 하지 마세요.

## 커밋 메시지

[Conventional Commits](https://www.conventionalcommits.org/) 형식을 따릅니다.

```
<type>: <요약>

<본문 (선택)>
```

예: `feat: 상권 검색 API 추가`, `fix: 빈 검색어 처리 오류 수정`
