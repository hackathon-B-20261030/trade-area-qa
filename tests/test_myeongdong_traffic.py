import csv
import json
import math
import shutil
import tempfile
from pathlib import Path
import unittest

import pyarrow.parquet as pq
from scripts.prepare_myeongdong_traffic import (SOURCE,AGE,ADULT_AGE,TIME,BANDS,PERSONAS,OUTPUT,CONTRACT,ONTOLOGY,
    load_quarters,periods_from_quarters,build_profiles,compute_age_factors,age_band,digest)


class TrafficSourceTests(unittest.TestCase):
    def mutate(self, mutator):
        tmp=tempfile.TemporaryDirectory();self.addCleanup(tmp.cleanup);p=Path(tmp.name)/'source.csv'
        with SOURCE.open(encoding='utf-8-sig',newline='') as f:
            reader=csv.DictReader(f);header=reader.fieldnames;rows=list(reader)
        mutator(rows,header)
        with p.open('w',encoding='utf-8-sig',newline='') as f:
            w=csv.DictWriter(f,fieldnames=header);w.writeheader();w.writerows(rows)
        return p

    def test_csv_validation(self):
        self.assertEqual(len(load_quarters(SOURCE)),22)
        cases=[lambda rows,h:rows.pop(),lambda rows,h:rows[1].update({'기준년분기':rows[0]['기준년분기']}),
               lambda rows,h:rows[0].update({'10대':'-1'}),lambda rows,h:rows[0].update({'10대':'1.5'}),
               lambda rows,h:rows[0].update({'월':'1'}),lambda rows,h:rows[0].update({'TRDAR_CD':'1'})]
        for case in cases:
            with self.subTest(case=case),self.assertRaises(ValueError):load_quarters(self.mutate(case))

    def test_2025_figures(self):
        periods=periods_from_quarters(load_quarters(SOURCE)); counts=periods['2025_all']['counts']
        s=sum(counts[k] for k in AGE)
        self.assertEqual([round(100*counts[k]/s,1) for k in AGE],[6.3,22.5,23.2,21.9,14.5,11.6])
        q1=periods['20251']['counts'];q4=periods['20254']['counts']
        self.assertAlmostEqual(q1['11~14시']/3,145400.33333333334)
        self.assertAlmostEqual(q4['14~17시']/3,156729.33333333334)
        self.assertEqual([end-start for _,start,end in BANDS],[360,300,180,180,240,180])

    def test_source_dataset(self):
        periods=load_quarters(SOURCE)
        self.assertEqual(periods[0]['period'],'20211');self.assertEqual(periods[-1]['period'],'20262')
        self.assertTrue(all(x['area_code']=='3120028' for x in periods))
        self.assertTrue(all(abs(v)<=4 for x in periods for v in x['source_total_difference'].values()))


class TrafficProfileTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.ref,cls.slots=build_profiles(periods_from_quarters(load_quarters(SOURCE)))

    def test_ten_minute_boundaries(self):
        slots=[s for s in self.slots if s['period']=='2025_all']
        self.assertEqual(len(slots),144)
        self.assertEqual([(s['slot'],s['start_minute'],s['end_minute']) for s in slots],[(i,10*i,10*(i+1)) for i in range(144)])
        for label,start,end in BANDS:
            self.assertEqual(slots[start//10]['time_band'],label)
            self.assertEqual(slots[end//10-1]['time_band'],label)
        self.assertEqual(slots[-1]['end_minute'],1440)

    def test_profile_reconciliation(self):
        self.assertEqual(len(self.ref['periods']),23)
        self.assertEqual(len(self.slots),23*144)
        for period,ref in self.ref['periods'].items():
            rows=[x for x in self.slots if x['period']==period]
            self.assertAlmostEqual(sum(x['slot_share'] for x in rows),1,places=12)
            self.assertAlmostEqual(sum(x['relative_intensity'] for x in rows)/144,1,places=12)
            for label in TIME:
                self.assertAlmostEqual(sum(x['slot_share'] for x in rows if x['time_band']==label),ref['time_band_share'][label],places=12)
            for field in ('sex_share','age_share','age20plus_share','weekday_share'):
                self.assertAlmostEqual(sum(ref[field].values()),1,places=12)
        invalid=periods_from_quarters(load_quarters(SOURCE));invalid['20211']['counts'].update({x:0 for x in AGE})
        with self.assertRaises(ValueError):build_profiles(invalid)


class TrafficPersonaTests(unittest.TestCase):
    def test_age_weights(self):
        periods=periods_from_quarters(load_quarters(SOURCE))
        counts={'10대':10,'20대':1,'30대':2,'40대':3,'50대':4,'60대이상':5}
        factors=compute_age_factors(counts,periods)
        for period,entry in factors.items():
            total=math.fsum(counts[a]*entry['relative_factor'][a] for a in ADULT_AGE)
            self.assertAlmostEqual(total,15,places=12)
            for a in ADULT_AGE:
                self.assertAlmostEqual(counts[a]*entry['relative_factor'][a]/total,entry['target_share'][a],places=12)
        self.assertEqual(age_band(19),'10대');self.assertEqual(age_band(20),'20대')
        self.assertEqual(age_band(99),'60대이상')
        with self.assertRaises(ValueError):age_band(100)
        with self.assertRaises(ValueError):compute_age_factors({**counts,'20대':0},periods)

    def test_full_delivery(self):
        if not OUTPUT.exists():self.skipTest('Run full build first')
        report=json.loads((OUTPUT/'validation.json').read_text(encoding='utf8'));v=report['validation']
        self.assertTrue(v['verified']);self.assertEqual(v['personas'],65000)
        self.assertEqual(v['adult20plus'],64343);self.assertEqual(v['age19_unresolved'],657)
        self.assertEqual(v['quarter_count'],22);self.assertEqual(v['period_count'],23)
        self.assertEqual(v['slot_count'],23*144)
        for summary in v['period_checks'].values():
            for band in ADULT_AGE:
                self.assertAlmostEqual(summary['target_age20plus_share'][band],summary['weighted_age20plus_share'][band],places=12)
        self.assertEqual(report['input_sha256']['source_csv'],digest(SOURCE))
        self.assertEqual(report['input_sha256']['personas_parquet'],digest(PERSONAS))
        for name,h in report['output_sha256'].items():self.assertEqual(digest(OUTPUT/name),h)
        self.assertEqual(pq.ParquetFile(OUTPUT/'persona_traffic_weights.parquet').metadata.num_rows,65000)

    def test_ontology_boundary(self):
        if not OUTPUT.exists():self.skipTest('Run full build first')
        text=(OUTPUT/'README.md').read_text(encoding='utf8'); contract=CONTRACT.read_text(encoding='utf8')
        for word in ('도착률','인/㎡','주변분포','19세','null'):
            self.assertIn(word,text)
        self.assertIn('R-09',contract);self.assertIn('R-10',contract);self.assertIn('R-12',contract)
        self.assertIn('상권',contract);self.assertIn('교차 분포',contract)
        ontology=ONTOLOGY.read_text(encoding='utf8')
        self.assertIn('### 7.6 명동 상권 유동인구 자료 연결',ontology)
        self.assertIn('05_myeongdong_traffic.md',ontology)

if __name__=='__main__':unittest.main()
