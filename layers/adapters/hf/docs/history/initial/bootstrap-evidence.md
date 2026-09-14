> 역사 기록: 독립 저장소 시점의 요구·상태·측정이다. 현재 배치와 사용법은 [HF 안내](../../../README.md)를 따른다. 원본 전체는 이관 시 보존한 Git bundle에 있다.

# 독립 프로젝트 초기화 기록

작업일: 2026-09-13. 범위는 폴더·별도 Git·계획 문서·인수인계입니다.

## 생성 범위

- `F:\dev\p4hfadapter`를 새 디렉터리로 생성했습니다.
- `git init -b main`으로 독립 저장소를 초기화했습니다. P4의 하위 디렉터리/worktree가 아닙니다.
- 모델별 Python 실행·Rust/P4 경계·양자화·검증·향후 통합 계획을 문서로 작성했습니다.
- P4 파일을 이동/삭제하지 않았고 새 문서는 이 저장소에 작성했습니다.
- `.gitignore`와 UTF-8/LF 문서 규칙을 준비했습니다. 모델/credentials/로컬 산출물은 추적하지 않습니다.

## P4 보존 기준

초기 HEAD: `d122125bafeaa6d32790761669f1bfa5868d8078`.
초기 비무시 tracked/untracked 파일 1,034개의 SHA256과 Git status/HEAD를 `.local/p4-before.json`에 기록했습니다.
기존 dirty 파일은 아래와 같았습니다. 이 목록은 관측이며 이 프로젝트가 만든 변경이 아닙니다.

```text
 M README.md
 M docs/distributed-batching-roadmap.md
 M docs/document-map.md
 M test/benchmarks/cluster-inference/README.md
 M test/benchmarks/cluster-inference/placement-policy.test.ts
 M tools/cluster-inference/placement-policy.ts
?? docs/external-analysis-improvement-plan.md
?? test/benchmarks/cluster-inference/model-loading-policy.test.ts
?? tools/cluster-inference/audit-model-loading.ts
?? tools/cluster-inference/model-loading-policy.ts
```

## 최종 점검

- `python .local/verify_docs.py`: Markdown 15개, 내부 링크 41개, P4 참조 링크 13개 점검, 오류 0.
- UTF-8/LF·최종 개행·코드 fence·필수 문서/README 색인 검사를 통과했습니다.
- 공식 외부 링크 13개를 수록했습니다. 이 로컬 검사기는 외부 웹 페이지를 다시 조회하지 않습니다.
- `git rev-parse --show-toplevel`은 `F:/dev/p4hfadapter`, `--git-dir`은 `.git`입니다.
- `.local/`, 가중치, `.venv/`, `target/`, `.env` 무시와 `.env.example` 예외를 확인했습니다.
- Git 작성자는 기존 전역 설정을 그대로 사용했습니다. 별도 remote와 push는 없습니다.
- 초기 커밋은 이 저장소의 전체 비무시 신규 파일만 포함합니다. 정확한 해시는 `git log -1`로 확인합니다.

이는 문서 검증이며 Python/Rust/모델/양자화/분산 실기 시험 결과가 아닙니다.

## P4 동시 변경 관측

16:22 KST 재확인 시 P4 HEAD가 `484b856ee7e53aea5b850b654c45da53cb0724a6`으로 바뀌었습니다.
새 커밋 제목은 `feat(outer): plan model loading from typed fleet profiles`였습니다.
초기 1,034개 대비 비무시 파일은 1,037개였고 내용 변경 6개와 새 경로 3개를 관측했습니다.
재확인 당시 dirty는 README·로드맵·문서 안내도와 미추적 `docs/batching-code-review.md`,
`docs/external-analysis-improvement-plan.md`였습니다.

따라서 **P4 전체 작업 트리/HEAD가 초기와 동일하다는 판정은 아닙니다.**
이 초기화 작업은 P4에 읽기 명령만 실행했으며, 쓰기·Git 초기화·문서 생성·커밋 대상은 p4hfadapter뿐입니다.
별도 작업 중 발생한 P4 변경은 되돌리거나 이 프로젝트로 가져오지 않았습니다.
초기 hash 목록과 비교 결과는 무시된 `.local/p4-before.json`, `.local/p4-preservation-result.json`에 있습니다.
이 로컬 감사 자료는 다른 컴퓨터의 clone에는 포함되지 않으므로 위 관측 요약을 인수인계 근거로 남깁니다.

## 미수행

P4 수정·통합 빌드, Python/Rust 구현·패키지 설치, 모델 다운로드·양자화·추론,
원격 배포·프로세스 조작·Git remote 생성·push는 수행하지 않았습니다.
