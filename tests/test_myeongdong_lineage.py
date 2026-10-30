import csv
import json
from pathlib import Path
import tempfile
import unittest

import pyarrow as pa
import pyarrow.parquet as pq

from scripts.augment_myeongdong_personas import (assign_parents, extract_parent_evidence,
                                                write_personas, EVIDENCE_ROLE)


class LineageTests(unittest.TestCase):
    def test_evidence_unicode_and_business(self):
        p={'travel_persona':'오늘은 명동의 거리로 간다. 내일은 쉰다.',
           'culinary_persona':'명동집 음식 이야기를 한다.'}
        e={'matched_fields':'travel_persona|culinary_persona','matched_tokens':'명동의|명동집','match_type':'place_mention'}
        items=extract_parent_evidence(p,e)
        self.assertEqual(len(items),2)
        for x in items:
            self.assertEqual(p[x['field']][x['start']:x['end']],x['token'])
            self.assertEqual(p[x['field']][x['sentence_start']:x['sentence_end']],x['sentence'])
        self.assertEqual(items[0]['sentence'],'오늘은 명동의 거리로 간다.')
        b={'matched_fields':'culinary_persona','matched_tokens':'명동집','match_type':'business_only'}
        self.assertEqual(extract_parent_evidence(p,b)[0]['token'],'명동집')
        with self.assertRaises(ValueError):
            extract_parent_evidence(p,{**b,'matched_tokens':'명동교자에서'})

    def test_assign_parent_slots(self):
        parents=['a','b','c'];donors=['a','b','c','d','e','f','g','h']
        result=assign_parents(parents,donors,12)
        counts={p:sum(1 for row in result if row[0]==p) for p in parents}
        self.assertEqual(sorted(counts.values()),[2,3,3])
        self.assertEqual(result,assign_parents(parents,donors,12))
        self.assertTrue(all(parent!=donor for parent,_,donor in result))
        self.assertEqual(len({x[2] for x in result}),8)
        with self.assertRaises(ValueError):assign_parents(['a'],['a'],12)

    def test_written_schema_and_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            t=Path(tmp); schema=pa.schema([('uuid',pa.string()),('age',pa.int64()),('persona',pa.string())])
            src=t/'source.parquet';parents=t/'parent.parquet'; ev=t/'evidence.csv'; out=t/'output.parquet'
            pq.write_table(pa.Table.from_pylist([{'uuid':'s1','age':23,'persona':'일반 원문'},{'uuid':'s2','age':31,'persona':'다른 원문'}],schema=schema),src)
            pq.write_table(pa.Table.from_pylist([{'uuid':'p1','age':40,'persona':'명동의 거리를 걷는다.'}],schema=schema),parents)
            with ev.open('w',encoding='utf-8-sig',newline='') as f:
                w=csv.DictWriter(f,fieldnames=['uuid','matched_fields','matched_tokens','match_type']);w.writeheader()
                w.writerow({'uuid':'p1','matched_fields':'persona','matched_tokens':'명동의','match_type':'place_mention'})
            write_personas(src,parents,ev,[('p1',1,'s1'),('p1',2,'s2')],out)
            result=pq.read_table(out); rows=result.to_pylist()
            self.assertEqual(result.schema.field('age').type,pa.int64())
            self.assertEqual([r['uuid'] for r in rows],['s1','s2'])
            self.assertEqual([r['persona'] for r in rows],['일반 원문','다른 원문'])
            self.assertEqual(rows[0]['donor_uuid'],'s1')
            self.assertEqual(rows[0]['parent_evidence_role'],EVIDENCE_ROLE)
            self.assertEqual(json.loads(rows[0]['parent_evidence_json'])[0]['sentence'],'명동의 거리를 걷는다.')
            self.assertNotEqual(rows[0]['synthetic_id'],rows[1]['synthetic_id'])

if __name__ == '__main__':unittest.main()
