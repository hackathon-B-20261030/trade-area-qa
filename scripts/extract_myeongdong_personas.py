#!/usr/bin/env python3
"""Extract Myeongdong-related rows from the verified Seoul persona subset."""

from __future__ import annotations

import csv
import hashlib
import json
import re
import shutil
import tempfile
from collections import Counter
from pathlib import Path

import pyarrow as pa
import pyarrow.parquet as pq


ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "data/nemotron-personas-korea/seoul.parquet"
SOURCE_METADATA = ROOT / "data/nemotron-personas-korea/metadata.json"
OUTPUT = ROOT / "data/myeongdong-personas"
DATASET = "nvidia/Nemotron-Personas-Korea"
REVISION = "ada0f5b53a38bb5a30cce09358adde883c1ab63a"
SOURCE_ROWS = 185_228
EXPECTED_COLUMNS = [
    "uuid", "professional_persona", "sports_persona", "arts_persona",
    "travel_persona", "culinary_persona", "family_persona", "persona",
    "cultural_background", "skills_and_expertise", "skills_and_expertise_list",
    "hobbies_and_interests", "hobbies_and_interests_list",
    "career_goals_and_ambitions", "sex", "age", "marital_status",
    "military_status", "family_type", "housing_type", "education_level",
    "bachelors_field", "occupation", "district", "province", "country",
]

TOKEN = re.compile(r"[가-힣A-Za-z0-9·]*명동[가-힣A-Za-z0-9·]*")
PLACE_TOKENS = frozenset({
    "명동", "명동의", "명동이나", "명동과", "명동이", "명동보다는", "명동에서", "명동처럼",
    "명동성당", "명동성당의", "명동성당이나", "명동성당을", "명동성당에서", "명동성당에",
    "명동성당으로", "명동역점에", "명동역점에서",
})
BUSINESS_TOKENS = frozenset({"명동교자에서", "명동삼겹살", "명동집"})
EXCLUDED_TOKENS = frozenset({
    "광명동", "광명동굴", "광명동굴의", "광명동굴이나", "광명동굴처럼",
    "이명동", "권명동", "정명동", "명동건", "명동준", "명동언",
})

# Independent expression for an end-to-end check of the selected UUIDs.
REFERENCE = re.compile(
    r"(?<![가-힣A-Za-z0-9·])명동(?:성당|역점|교자|삼겹살|집)?"
    r"(?:이나|보다는|에서|으로|처럼|의|과|을|이|에)?(?![가-힣A-Za-z0-9·])"
)


def matched_tokens(text: str | None) -> set[str]:
    return set(TOKEN.findall(text or ""))


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(4 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    if OUTPUT.exists():
        raise FileExistsError(f"Output already exists; refusing to overwrite: {OUTPUT}")

    metadata = json.loads(SOURCE_METADATA.read_text(encoding="utf-8"))
    if (
        metadata.get("dataset") != DATASET
        or metadata.get("revision") != REVISION
        or metadata.get("seoul_rows") != SOURCE_ROWS
        or metadata.get("columns") != EXPECTED_COLUMNS
    ):
        raise ValueError("Unexpected Seoul source metadata")
    source_stat = SOURCE.stat()
    source_hash = file_sha256(SOURCE)
    if source_hash != metadata.get("output_sha256"):
        raise ValueError("Seoul source SHA-256 does not match its metadata")

    source = pq.ParquetFile(SOURCE)
    schema = source.schema_arrow
    if source.metadata.num_rows != SOURCE_ROWS or schema.names != EXPECTED_COLUMNS:
        raise ValueError("Unexpected source row count or columns")
    for field in schema:
        expected = pa.int64() if field.name == "age" else pa.string()
        if field.type != expected:
            raise ValueError(f"Unexpected source field type: {field}")

    text_columns = [field.name for field in schema if pa.types.is_string(field.type)]
    accepted = PLACE_TOKENS | BUSINESS_TOKENS
    classified = accepted | EXCLUDED_TOKENS
    selected: list[dict] = []
    evidence: list[dict[str, str]] = []
    reference_ids: list[str] = []
    candidate_rows = 0
    input_rows = 0
    unknown_tokens: set[str] = set()
    district_counts: Counter[str] = Counter()

    for batch in source.iter_batches(batch_size=2048):
        columns = batch.to_pydict()
        for i in range(batch.num_rows):
            input_rows += 1
            uid = columns["uuid"][i]
            if not uid or columns["province"][i] != "서울":
                raise ValueError("Source has missing UUID or non-Seoul row")
            hits: set[str] = set()
            fields: list[str] = []
            raw_hit = False
            reference_hit = False
            for name in text_columns:
                value = columns[name][i]
                if not value or "명동" not in value:
                    continue
                raw_hit = True
                tokens = matched_tokens(value)
                unknown_tokens.update(tokens - classified)
                valid = tokens & accepted
                if valid:
                    fields.append(name)
                    hits.update(valid)
                if REFERENCE.search(value):
                    reference_hit = True
            candidate_rows += int(raw_hit)
            if reference_hit:
                reference_ids.append(uid)
            if hits:
                row = {name: columns[name][i] for name in EXPECTED_COLUMNS}
                selected.append(row)
                district_counts[row["district"]] += 1
                evidence.append({
                    "uuid": uid,
                    "matched_fields": "|".join(fields),
                    "matched_tokens": "|".join(sorted(hits)),
                    "match_type": "place_mention" if hits & PLACE_TOKENS else "business_only",
                })

    selected_ids = [row["uuid"] for row in selected]
    business_ids = [e["uuid"] for e in evidence if e["match_type"] == "business_only"]
    if unknown_tokens:
        raise ValueError(f"Unclassified source tokens: {sorted(unknown_tokens)}")
    if input_rows != SOURCE_ROWS or candidate_rows != 552:
        raise ValueError("Unexpected source or substring candidate count")
    if len(selected) != 514 or len(business_ids) != 4 or len(set(selected_ids)) != 514:
        raise ValueError("Unexpected result, business-only, or unique UUID count")
    if selected_ids != reference_ids:
        raise ValueError("Independent reference expression disagrees with selected UUIDs")

    staged = Path(tempfile.mkdtemp(prefix=".myeongdong-build-", dir=ROOT / "data"))
    try:
        table = pa.Table.from_pylist(selected, schema=schema)
        parquet_path = staged / "myeongdong.parquet"
        pq.write_table(table, parquet_path, compression="snappy")
        reread = pq.read_table(parquet_path)
        if not reread.equals(table, check_metadata=True):
            raise ValueError("Parquet schema, values, or row order changed")

        csv_path = staged / "myeongdong.csv"
        with csv_path.open("w", encoding="utf-8-sig", newline="") as file:
            writer = csv.DictWriter(file, fieldnames=EXPECTED_COLUMNS)
            writer.writeheader()
            writer.writerows(selected)
        with csv_path.open("r", encoding="utf-8-sig", newline="") as file:
            reader = csv.DictReader(file)
            if reader.fieldnames != EXPECTED_COLUMNS:
                raise ValueError("CSV column names or order changed")
            csv_rows = list(reader)
        expected_csv = [
            {key: "" if value is None else str(value) for key, value in row.items()}
            for row in selected
        ]
        if csv_rows != expected_csv:
            raise ValueError("CSV values or row order changed")

        evidence_path = staged / "match_evidence.csv"
        evidence_columns = ["uuid", "matched_fields", "matched_tokens", "match_type"]
        with evidence_path.open("w", encoding="utf-8-sig", newline="") as file:
            writer = csv.DictWriter(file, fieldnames=evidence_columns)
            writer.writeheader()
            writer.writerows(evidence)
        with evidence_path.open("r", encoding="utf-8-sig", newline="") as file:
            reader = csv.DictReader(file)
            if reader.fieldnames != evidence_columns or list(reader) != evidence:
                raise ValueError("Evidence CSV does not match the selection")

        result_metadata = {
            "dataset": DATASET,
            "revision": REVISION,
            "source_file": "data/nemotron-personas-korea/seoul.parquet",
            "source_sha256": source_hash,
            "source_rows": input_rows,
            "filter": "Mention of Myeongdong, Myeongdong Cathedral, Myeongdong Station branch, or an accepted Myeongdong business name; exclude other places and personal names",
            "substring_candidate_rows": candidate_rows,
            "selected_rows": len(selected),
            "place_mention_rows": len(selected) - len(business_ids),
            "business_only_rows": len(business_ids),
            "business_only_uuids": business_ids,
            "excluded_other_place_or_name_rows": candidate_rows - len(selected),
            "unknown_tokens": sorted(unknown_tokens),
            "matched_district_counts": dict(sorted(district_counts.items())),
            "columns": EXPECTED_COLUMNS,
            "result_sha256": file_sha256(parquet_path),
            "license": metadata["license"],
            "source_url": metadata["source_url"],
        }
        (staged / "metadata.json").write_text(
            json.dumps(result_metadata, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        (staged / "README.md").write_text(
            "# 명동 관련 서울 합성 페르소나\n\n"
            "NVIDIA의 [Nemotron-Personas-Korea](https://huggingface.co/datasets/nvidia/Nemotron-Personas-Korea) "
            "데이터셋(CC BY 4.0)을 서울 지역으로 선택한 뒤 명동 관련 언급을 기준으로 추출한 514행입니다. "
            f"원본 리비전은 `{REVISION}`이며, 원본의 26개 열과 순서를 보존합니다.\n\n"
            "- `myeongdong.parquet`: 원본 타입과 값을 유지하는 정본.\n"
            "- `myeongdong.csv`: 열람용 UTF-8 BOM CSV. CSV에서 null은 빈 셀입니다.\n"
            "- `match_evidence.csv`: UUID별로 일치한 열, 토큰, 분류.\n"
            "- `metadata.json`: 원본·출력 해시, 행 수, 구별 거주자 수와 판정 정보.\n\n"
            "지명이나 명동성당·명동역점 언급 510행과 상호명만 언급한 4행을 포함합니다. "
            "상호명만 있는 행의 실제 매장 소재지는 확인되지 않았으며 "
            "`match_evidence.csv`의 `business_only`와 메타데이터에 표시합니다. "
            "광명동굴·광명동 및 인명만 일치한 38행은 제외합니다. "
            "명동에 거주한다고 단정할 수 없고, 명동을 언급하지 않은 다른 명동 장소는 추정하지 않습니다.\n\n"
            "NVIDIA 데이터셋의 행은 실제 개인의 기록이 아닌 합성 페르소나입니다. "
            "이 파일은 CC BY 4.0 원본의 서울 부분집합에서 행을 선택한 파생물입니다.\n",
            encoding="utf-8",
        )

        after_stat = SOURCE.stat()
        if (source_stat.st_size, source_stat.st_mtime_ns) != (after_stat.st_size, after_stat.st_mtime_ns):
            raise ValueError("Source changed during extraction")
        if OUTPUT.exists():
            raise FileExistsError(f"Output appeared during extraction: {OUTPUT}")
        staged.rename(OUTPUT)
    finally:
        shutil.rmtree(staged, ignore_errors=True)

    print(f"Complete: {len(selected):,} rows, including {len(business_ids)} business-only rows")
    print(f"Output: {OUTPUT}")


if __name__ == "__main__":
    main()
