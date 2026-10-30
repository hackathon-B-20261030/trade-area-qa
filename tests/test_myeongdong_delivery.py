import csv
import json
from pathlib import Path
import tempfile
import unittest

import pyarrow as pa
import pyarrow.parquet as pq

from scripts.augment_myeongdong_personas import (allocate_donors,assign_parents,write_personas,
                                                verify_output,build_dataset)
from scripts.prepare_myeongdong_statistics import sha256


def setup_fixture(root):
    source=root/'source.parquet';parents=root/'parents.parquet';evidence=root/'evidence.csv'
    stats_path=root/'statistics.json';selection=root/'selection.json';ontology=root/'ontology.md'
    records=[]
    for gu in ('가구','나구'):
        for i in range(3):
            records.append({'uuid':gu+str(i),'district':'서울-'+gu,'sex':'여자','age':22,
                            'family_type':'혼자 거주' if i==0 else '부모와 동거',
                            'marital_status':'배우자있음' if i==0 else '미혼','persona':'출처 원문 '+str(i)})
    schema=pa.schema([('uuid',pa.string()),('district',pa.string()),('sex',pa.string()),('age',pa.int64()),
                      ('family_type',pa.string()),('marital_status',pa.string()),('persona',pa.string())])
    pq.write_table(pa.Table.from_pylist(records,schema=schema),source)
    ps=[{'uuid':'p1','persona':'명동에서 전시를 본다.'},{'uuid':'p2','persona':'명동집 음식'}]
    pq.write_table(pa.Table.from_pylist(ps),parents)
    with evidence.open('w',encoding='utf-8-sig',newline='') as f:
        w=csv.DictWriter(f,fieldnames=['uuid','matched_fields','matched_tokens','match_type']);w.writeheader()
        w.writerow({'uuid':'p1','matched_fields':'persona','matched_tokens':'명동에서','match_type':'place_mention'})
        w.writerow({'uuid':'p2','matched_fields':'persona','matched_tokens':'명동집','match_type':'business_only'})
    cells=[{'district':g,'sex':'여자','age_lower':20,'population':100,'single_registered':50} for g in ('가구','나구')]
    marriage=[{'sex':'여자','age_lower':20,'total':100,'never':50,'married':50,'divorced_widowed':0}]
    stats={'population':cells,'marriage':marriage,'limitations':['fixture'],
           'myeongdong_context':{'all_ages_total':1}}
    stats_path.write_text(json.dumps(stats,ensure_ascii=False),encoding='utf8')
    ids,diag=allocate_donors(records,stats,4,7)
    selection.write_text(json.dumps({'source_sha256':sha256(source),'statistics_sha256':sha256(stats_path),
                                      'seed':7,'selected_donor_uuids':ids,'diagnostics':diag},ensure_ascii=False),encoding='utf8')
    ontology.write_text('draft minimal contract',encoding='utf8')
    return source,parents,evidence,stats_path,selection,ontology,stats,ids,diag


class DeliveryTests(unittest.TestCase):
    def test_e2e_small(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,parents,evidence,sp,selection,ontology,stats,ids,diag=setup_fixture(root)
            out=root/'delivery'
            report=build_dataset(source,parents,evidence,sp,selection,ontology,out,4,2)
            self.assertTrue(report['validation']['verified'])
            self.assertEqual(report['validation']['rows'],4)
            self.assertEqual(report['validation']['parent_count'],2)
            self.assertEqual(report['validation']['business_only_variations'],2)
            self.assertEqual(sha256(out/'personas.parquet'),report['hashes']['personas_parquet'])
            self.assertTrue((out/'statistics.json').exists())
            self.assertTrue((out/'ontology_contract_snapshot.md').exists())
            with self.assertRaises(FileExistsError):
                build_dataset(source,parents,evidence,sp,selection,ontology,out,4,2)

    def test_validation_rejects_corruption(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);source,parents,evidence,sp,selection,ontology,stats,ids,diag=setup_fixture(root)
            out=root/'people.parquet'
            write_personas(source,parents,evidence,assign_parents(['p1','p2'],ids,7),out)
            data=pq.read_table(out);rows=data.to_pylist();rows[0]['age']=99
            pq.write_table(pa.Table.from_pylist(rows,schema=data.schema),out)
            with self.assertRaisesRegex(ValueError,'Original values changed'):
                verify_output(out,source,parents,evidence,stats,diag,ids)

    def test_full_dataset_validation(self):
        path=Path('data/myeongdong-augmented')
        if not path.exists():self.skipTest('Run full build first')
        report=json.loads((path/'diagnostics.json').read_text(encoding='utf8'))
        self.assertTrue(report['validation']['verified'])
        self.assertEqual(report['validation']['rows'],65000)
        self.assertEqual(report['validation']['unique_synthetic_ids'],65000)
        self.assertEqual(report['validation']['unique_donors'],65000)
        self.assertEqual(report['validation']['parent_count'],514)
        self.assertEqual(report['validation']['parents_with_extra'],236)
        self.assertEqual(report['validation']['original_columns_verified'],26)
        self.assertEqual(sum(len(g['district_marriage']) for g in report['calibration']['groups']),659)
        self.assertEqual(pq.ParquetFile(path/'personas.parquet').metadata.num_rows,65000)
        self.assertEqual(sha256(path/'personas.parquet'),report['hashes']['personas_parquet'])

if __name__ == '__main__':unittest.main()
