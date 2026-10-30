import json
from pathlib import Path
import unittest

from scripts.augment_myeongdong_personas import allocate_donors, population_quotas


def person(uid, gu, married, alone):
    return {'uuid':uid,'district':'서울-'+gu,'sex':'여자','age':22,
            'family_type':'혼자 거주' if alone else '부모와 동거','marital_status':married}


def fixture():
    cells=[{'district':gu,'sex':'여자','age_lower':20,'population':100,'single_registered':50} for gu in ('가구','나구')]
    marriage=[{'sex':'여자','age_lower':20,'total':100,'never':50,'married':50,'divorced_widowed':0}]
    people=[person('a1','가구','배우자있음',True),person('a2','가구','미혼',False),
            person('a3','가구','미혼',False),person('a4','가구','배우자있음',True),
            person('b1','나구','미혼',True),person('b2','나구','배우자있음',False),
            person('b3','나구','미혼',True),person('b4','나구','배우자있음',False)]
    return {'population':cells,'marriage':marriage},people


class AllocationTests(unittest.TestCase):
    def test_population_quotas_sparse(self):
        cells=[{'district':'가구','sex':'여자','age_lower':20,'population':80},
               {'district':'가구','sex':'여자','age_lower':25,'population':20}]
        a=('가구','여자',20);b=('가구','여자',25)
        q,m=population_quotas(cells,{a:2,b:3},5)
        self.assertEqual(q,{a:2,b:3})
        self.assertEqual(len(m),2)
        self.assertTrue(all(x['from']==list(a) and x['to']==list(b) for x in m))
        with self.assertRaises(ValueError): population_quotas(cells,{a:1,b:1},5)
        with self.assertRaises(ValueError): population_quotas([],{},5)

    def test_allocation_constraints(self):
        stats,people=fixture()
        ids,report=allocate_donors(people,stats,4,7)
        self.assertEqual(len(ids),4)
        self.assertEqual(len(set(ids)),4)
        self.assertEqual(ids,allocate_donors(people,stats,4,7)[0])
        g=report['groups'][0]
        self.assertEqual(g['marriage_actual'],g['marriage_target'])
        self.assertEqual(g['single_proxy_actual'],g['single_proxy_target'])
        self.assertTrue(all(c['actual']==c['adjusted_target'] for c in report['cells']))
        # An infeasible marriage margin is not manufactured: the optimum is reported.
        stats['marriage'][0].update(never=0,married=100)
        limited=[p for p in people if p['uuid'] not in ('a4','b4')]
        _,report=allocate_donors(limited,stats,4,7)
        self.assertEqual(report['groups'][0]['marriage_actual']['married'],2)
        self.assertEqual(report['groups'][0]['stages'][0]['objective'],4)
        with self.assertRaises(ValueError): allocate_donors(people+[people[0]],stats,4,7)

    def test_district_marriage(self):
        stats,people=fixture()
        stats['district_marriage']=[
            {'district':'가구','sex':'여자','age_lower':20,'total':100,'never':100,'married':0,'divorced_widowed':0},
            {'district':'나구','sex':'여자','age_lower':20,'total':100,'never':0,'married':100,'divorced_widowed':0},
        ]
        _,report=allocate_donors(people,stats,4,7)
        group=report['groups'][0]
        self.assertEqual(group['marriage_actual'],group['marriage_target'])
        self.assertEqual(group['single_proxy_actual'],group['single_proxy_target'])
        self.assertEqual(len(group['district_marriage']),2)
        self.assertEqual(next(s for s in group['stages'] if s['stage']=='marriage_district')['objective'],4)

    def test_real_calibration(self):
        path=Path('data/myeongdong-augmentation-inputs/selection.json')
        if not path.exists(): self.skipTest('Run allocation first')
        obj=json.loads(path.read_text(encoding='utf8'))
        ids=obj['selected_donor_uuids']; diag=obj['diagnostics']
        self.assertEqual(len(ids),65000);self.assertEqual(len(set(ids)),65000)
        self.assertEqual(diag['population_total'],8134274)
        self.assertEqual(sum(c['actual'] for c in diag['cells']),65000)
        self.assertTrue(all(c['actual']==c['adjusted_target']<=c['donor_capacity'] for c in diag['cells']))
        self.assertEqual(len(diag['age_reallocations']),7)
        self.assertEqual(sum(len(g['district_marriage']) for g in diag['groups']),659)
        self.assertTrue(all(s['status']=='optimal' for g in diag['groups'] for s in g['stages']))

if __name__ == '__main__':unittest.main()
