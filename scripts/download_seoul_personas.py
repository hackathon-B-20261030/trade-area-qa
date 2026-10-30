#!/usr/bin/env python3
"""Download and verify the Seoul subset of NVIDIA's Korean personas dataset."""

from __future__ import annotations

import hashlib
import json
import tempfile
import time
from collections import Counter
from pathlib import Path

import pyarrow as pa
import pyarrow.compute as pc
import pyarrow.parquet as pq
import requests


DATASET = "nvidia/Nemotron-Personas-Korea"
REVISION = "ada0f5b53a38bb5a30cce09358adde883c1ab63a"
SOURCE_URL = f"https://huggingface.co/datasets/{DATASET}"
ROOT = Path(__file__).resolve().parent.parent
OUTPUT_DIR = ROOT / "data" / "nemotron-personas-korea"
OUTPUT_PATH = OUTPUT_DIR / "seoul.parquet"
METADATA_PATH = OUTPUT_DIR / "metadata.json"
EXPECTED_SOURCE_ROWS = 1_000_000
BATCH_SIZE = 8192
CHUNK_SIZE = 4 * 1024 * 1024
EXPECTED_COLUMNS = [
    "uuid", "professional_persona", "sports_persona", "arts_persona",
    "travel_persona", "culinary_persona", "family_persona", "persona",
    "cultural_background", "skills_and_expertise", "skills_and_expertise_list",
    "hobbies_and_interests", "hobbies_and_interests_list",
    "career_goals_and_ambitions", "sex", "age", "marital_status",
    "military_status", "family_type", "housing_type", "education_level",
    "bachelors_field", "occupation", "district", "province", "country",
]


def fetch_manifest(session: requests.Session) -> list[dict]:
    url = f"https://huggingface.co/api/datasets/{DATASET}/revision/{REVISION}"
    response = session.get(url, params={"blobs": "true"}, timeout=(15, 180))
    response.raise_for_status()
    data = response.json()
    if data.get("sha") != REVISION:
        raise ValueError("Dataset revision does not match the requested snapshot")
    shards = [x for x in data["siblings"] if x["rfilename"].endswith(".parquet")]
    expected = [f"data/train-{i:05d}-of-00009.parquet" for i in range(9)]
    if sorted(x["rfilename"] for x in shards) != expected:
        raise ValueError("Dataset shard names are missing, duplicated, or unexpected")
    for shard in shards:
        if not shard.get("size") or not shard.get("lfs", {}).get("sha256"):
            raise ValueError(f"Missing official size or SHA-256: {shard['rfilename']}")
    return sorted(shards, key=lambda x: x["rfilename"])


def download_shard(session: requests.Session, item: dict, destination: Path) -> None:
    name = item["rfilename"]
    url = f"{SOURCE_URL}/resolve/{REVISION}/{name}"
    for attempt in range(1, 4):
        digest = hashlib.sha256()
        size = 0
        try:
            with session.get(url, stream=True, timeout=(15, 180)) as response:
                response.raise_for_status()
                with destination.open("wb") as file:
                    for chunk in response.iter_content(chunk_size=CHUNK_SIZE):
                        if chunk:
                            file.write(chunk)
                            digest.update(chunk)
                            size += len(chunk)
            if size != item["size"] or digest.hexdigest() != item["lfs"]["sha256"]:
                raise ValueError(f"Size or SHA-256 mismatch for {name}")
            return
        except (requests.RequestException, OSError, ValueError) as exc:
            destination.unlink(missing_ok=True)
            if attempt == 3:
                raise RuntimeError(f"Could not download verified shard {name}") from exc
            print(f"Retrying {name} after {type(exc).__name__}", flush=True)
            time.sleep(attempt)


def table_digest(table: pa.Table) -> str:
    table = table.replace_schema_metadata(None).combine_chunks()
    sink = pa.BufferOutputStream()
    with pa.ipc.new_stream(sink, table.schema) as stream:
        stream.write_table(table)
    return hashlib.sha256(sink.getvalue().to_pybytes()).hexdigest()


def extract(session: requests.Session, manifest: list[dict], output: Path) -> dict:
    source_rows = 0
    seoul_rows = 0
    row_group_digests: list[str] = []
    source_shards = []
    schema = None
    writer = None
    try:
        with tempfile.TemporaryDirectory(prefix="nemotron-seoul-", dir="/private/tmp") as tmp:
            temporary_shard = Path(tmp) / "source.parquet"
            for number, item in enumerate(manifest, 1):
                print(f"Downloading and checking shard {number}/9", flush=True)
                download_shard(session, item, temporary_shard)
                parquet = pq.ParquetFile(temporary_shard)
                current_schema = parquet.schema_arrow
                if current_schema.names != EXPECTED_COLUMNS:
                    raise ValueError("Original column names or order differ from the dataset card")
                for field in current_schema:
                    expected_type = pa.int64() if field.name == "age" else pa.string()
                    if field.type != expected_type:
                        raise ValueError(f"Unexpected original type: {field}")
                if schema is None:
                    schema = current_schema
                    writer = pq.ParquetWriter(output, schema, compression="snappy")
                elif not schema.equals(current_schema, check_metadata=False):
                    raise ValueError("Original shard schemas do not match")

                shard_rows = 0
                shard_seoul_rows = 0
                for batch in parquet.iter_batches(batch_size=BATCH_SIZE):
                    shard_rows += batch.num_rows
                    province = batch.column(batch.schema.get_field_index("province"))
                    selected = batch.filter(pc.equal(province, "서울"))
                    if selected.num_rows:
                        table = pa.Table.from_batches([selected], schema=schema)
                        row_group_digests.append(table_digest(table))
                        writer.write_table(table)
                        shard_seoul_rows += selected.num_rows
                source_rows += shard_rows
                seoul_rows += shard_seoul_rows
                source_shards.append({
                    "name": item["rfilename"],
                    "size_bytes": item["size"],
                    "sha256": item["lfs"]["sha256"],
                    "rows": shard_rows,
                    "seoul_rows": shard_seoul_rows,
                })
                temporary_shard.unlink()
                print(f"Verified shard {number}/9: {shard_rows:,} rows, {shard_seoul_rows:,} Seoul", flush=True)
    finally:
        if writer is not None:
            writer.close()

    if source_rows != EXPECTED_SOURCE_ROWS or not seoul_rows:
        raise ValueError(f"Unexpected source or Seoul row count: {source_rows}, {seoul_rows}")
    return {
        "schema": schema,
        "source_rows": source_rows,
        "seoul_rows": seoul_rows,
        "source_shards": source_shards,
        "row_group_digests": row_group_digests,
    }


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(CHUNK_SIZE), b""):
            digest.update(chunk)
    return digest.hexdigest()


def validate(output: Path, extraction: dict) -> dict:
    parquet = pq.ParquetFile(output)
    if not parquet.schema_arrow.equals(extraction["schema"], check_metadata=False):
        raise ValueError("Output schema differs from the source")
    if parquet.num_row_groups != len(extraction["row_group_digests"]):
        raise ValueError("Output row-group count differs from the filtered batches")
    districts = Counter()
    ids: set[str] = set()
    output_rows = 0
    for i in range(parquet.num_row_groups):
        table = parquet.read_row_group(i)
        if table_digest(table) != extraction["row_group_digests"][i]:
            raise ValueError(f"Filtered row contents or order changed in row group {i}")
        province = table.column("province")
        if province.null_count or not pc.all(pc.equal(province, "서울")).as_py():
            raise ValueError("Output contains a non-Seoul or missing province")
        for uuid in table.column("uuid").to_pylist():
            if not uuid or uuid in ids:
                raise ValueError("Output contains a missing or duplicated UUID")
            ids.add(uuid)
        districts.update(x if x is not None else "<null>" for x in table.column("district").to_pylist())
        output_rows += table.num_rows
    if output_rows != extraction["seoul_rows"] or len(ids) != output_rows:
        raise ValueError("Output row count does not match the selected source rows")
    return {
        "dataset": DATASET,
        "revision": REVISION,
        "split": "train",
        "filter": 'province == "서울"',
        "source_rows": extraction["source_rows"],
        "seoul_rows": output_rows,
        "columns": EXPECTED_COLUMNS,
        "district_counts": dict(sorted(districts.items())),
        "source_shards": extraction["source_shards"],
        "output_sha256": file_sha256(output),
        "license": "CC BY 4.0",
        "source_url": SOURCE_URL,
    }


def main() -> None:
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    if OUTPUT_PATH.exists() or METADATA_PATH.exists():
        raise FileExistsError("Final output or metadata already exists; refusing to overwrite")
    with tempfile.TemporaryDirectory(prefix=".seoul-build-", dir=OUTPUT_DIR) as tmp:
        temporary_output = Path(tmp) / "seoul.parquet"
        temporary_metadata = Path(tmp) / "metadata.json"
        with requests.Session() as session:
            manifest = fetch_manifest(session)
            extraction = extract(session, manifest, temporary_output)
        print("Re-reading and validating the complete Seoul subset", flush=True)
        metadata = validate(temporary_output, extraction)
        temporary_metadata.write_text(
            json.dumps(metadata, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        OUTPUT_PATH.hardlink_to(temporary_output)
        try:
            METADATA_PATH.hardlink_to(temporary_metadata)
        except BaseException:
            OUTPUT_PATH.unlink(missing_ok=True)
            raise
        print(f"Complete: {metadata['seoul_rows']:,} Seoul rows, {len(metadata['district_counts'])} districts", flush=True)
        print(f"Output: {OUTPUT_PATH}", flush=True)


if __name__ == "__main__":
    main()
