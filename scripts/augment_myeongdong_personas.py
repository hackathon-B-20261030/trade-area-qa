#!/usr/bin/env python3
"""Build a statistically calibrated, lineage-preserving synthetic persona sample."""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import csv
import hashlib
import json
import math
import platform
from pathlib import Path
import random
import shutil
import tempfile
import uuid

import numpy as np
import pyarrow as pa
import pyarrow.parquet as pq
import scipy
from scipy.optimize import Bounds, LinearConstraint, milp

from scripts.prepare_myeongdong_statistics import sha256
from scripts.extract_myeongdong_personas import TOKEN, PLACE_TOKENS, BUSINESS_TOKENS

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / 'data/nemotron-personas-korea/seoul.parquet'
PARENTS = ROOT / 'data/myeongdong-personas/myeongdong.parquet'
EVIDENCE = ROOT / 'data/myeongdong-personas/match_evidence.csv'
STATS = ROOT / 'data/myeongdong-augmentation-inputs/statistics.json'
SELECTION = ROOT / 'data/myeongdong-augmentation-inputs/selection.json'
ONTOLOGY = ROOT / 'docs/maps/01a_persona_ontology.md'
OUTPUT = ROOT / 'data/myeongdong-augmented'
SEED = 20261031
NAMESPACE = uuid.UUID('342bcc2a-5fc4-46c9-9546-f1b8219113e1')
EVIDENCE_ROLE = 'unverified_parent_context_visit_candidate'


def cell_key(row: dict) -> tuple[str, str, int]:
    age = row['age']
    if not isinstance(age, int) or not 19 <= age <= 99:
        raise ValueError(f'Invalid donor age: {age}')
    district = row['district']
    if not isinstance(district, str) or not district.startswith('서울-'):
        raise ValueError(f'Invalid donor district: {district}')
    if row['sex'] not in ('남자', '여자'):
        raise ValueError(f"Invalid donor sex: {row['sex']}")
    return (district.removeprefix('서울-'), row['sex'], 19 if age == 19 else age // 5 * 5)


def marriage_group(value: str) -> str:
    if value == '미혼': return 'never'
    if value == '배우자있음': return 'married'
    if value in ('사별', '이혼'): return 'divorced_widowed'
    raise ValueError(f'Unrecognized marital status: {value!r}')


def living_alone(value: str) -> bool:
    if not isinstance(value, str) or not value:
        raise ValueError('Missing family type')
    return value.startswith('혼자 거주')


def largest_remainder(weights: dict, n: int) -> dict:
    if n < 0 or not weights or any(not isinstance(v, int) or v < 0 for v in weights.values()):
        raise ValueError('Invalid weights or sample size')
    total = sum(weights.values())
    if not total:
        raise ValueError('Zero total weight')
    quotas = {k: n * v // total for k, v in weights.items()}
    ordering = sorted(weights, key=lambda k: (-(n * weights[k] % total), k))
    for k in ordering[:n - sum(quotas.values())]: quotas[k] += 1
    return quotas


def population_quotas(cells: list[dict], capacity: dict, n: int) -> tuple[dict, list[dict]]:
    weights = {(c['district'],c['sex'],c['age_lower']): c['population'] for c in cells}
    if len(weights) != len(cells): raise ValueError('Duplicate population cells')
    original = largest_remainder(weights,n)
    result = dict(original)
    moves = []
    for key in sorted(original):
        shortage = max(0, result[key] - capacity.get(key,0))
        for _ in range(shortage):
            eligible = [k for k in result if k[:2] == key[:2] and k != key and result[k] < capacity.get(k,0)]
            if not eligible: raise ValueError(f'Insufficient same-district/sex donor supply for {key}')
            dest = min(eligible, key=lambda k: (abs(k[2]-key[2]),k[2]))
            result[key] -= 1
            result[dest] += 1
            moves.append({'from':list(key),'to':list(dest),'count':1})
    if sum(result.values()) != n or any(q > capacity.get(k,0) for k,q in result.items()):
        raise ValueError('Quota reconciliation failed')
    return result,moves


def _group_age(lower: int) -> int:
    return min(lower,85)


def _optimize_group(keys: list[tuple], quota: dict, cell_stats: dict, combos: dict,
                    marriage: dict | None, district_marriage: dict) -> tuple[dict,dict]:
    # Each combination corresponds to a set of interchangeable actual source rows.
    variants = sorted((k,alone,mar) for k in keys for alone in (False,True)
                      for mar in ('divorced_widowed','married','never')
                      if combos.get((k,alone,mar)))
    count = len(variants)
    if not count and sum(quota[k] for k in keys): raise ValueError('No feasible candidates')
    total = sum(quota[k] for k in keys)
    marriage_keys = ['never','married','divorced_widowed'] if marriage else []
    marriage_target = largest_remainder({m:marriage[m] for m in marriage_keys},total) if marriage else {}
    district_names=sorted({k[0] for k in keys if k[0] in district_marriage}) if marriage else []
    district_targets={gu:largest_remainder({m:district_marriage[gu][m] for m in marriage_keys},
                                          sum(quota[k] for k in keys if k[0]==gu))
                      for gu in district_names}
    expected_single = {k: quota[k]*cell_stats[k]['single_registered']/cell_stats[k]['population']
                       if cell_stats[k]['population'] else 0.0 for k in keys}
    single_target = int(math.floor(sum(expected_single.values()) + .5))
    # x, two slacks per city marital category, group single, district marital and cell single.
    mstart = count
    sstart = mstart + 2*len(marriage_keys)
    dstart = sstart + 2
    cstart = dstart + 2*len(district_names)*len(marriage_keys)
    nv = cstart + 2*len(keys)
    lb = np.zeros(nv); ub = np.full(nv,np.inf)
    ub[:count] = [len(combos[v]) for v in variants]
    integer = np.zeros(nv, dtype=int); integer[:count] = 1
    rows=[]; values=[]
    for k in keys:
        row=np.zeros(nv)
        for i,(cell,alone,mar) in enumerate(variants):
            if cell == k: row[i]=1
        rows.append(row); values.append(quota[k])
    for j,mar in enumerate(marriage_keys):
        row=np.zeros(nv)
        for i,(cell,alone,m) in enumerate(variants):
            if m == mar: row[i]=1
        row[mstart+2*j]=1;row[mstart+2*j+1]=-1
        rows.append(row);values.append(marriage_target[mar])
    row=np.zeros(nv)
    for i,(_,alone,_) in enumerate(variants):
        if alone:row[i]=1
    row[sstart]=1;row[sstart+1]=-1
    rows.append(row);values.append(single_target)
    for gi,gu in enumerate(district_names):
        for mi,mar in enumerate(marriage_keys):
            row=np.zeros(nv)
            for i,(cell,_,m) in enumerate(variants):
                if cell[0]==gu and m==mar:row[i]=1
            j=gi*len(marriage_keys)+mi
            row[dstart+2*j]=1;row[dstart+2*j+1]=-1
            rows.append(row);values.append(district_targets[gu][mar])
    for j,k in enumerate(keys):
        row=np.zeros(nv)
        for i,(cell,alone,_) in enumerate(variants):
            if cell==k and alone:row[i]=1
        row[cstart+2*j]=1;row[cstart+2*j+1]=-1
        rows.append(row);values.append(expected_single[k])
    matrix=np.array(rows); values=np.array(values,dtype=float)
    locked=[]; solution=None; stages=[]
    objectives=[]
    if marriage_keys:
        obj=np.zeros(nv);obj[mstart:sstart]=1;objectives.append(('marriage',obj))
    obj=np.zeros(nv);obj[sstart:dstart]=1;objectives.append(('single_group',obj))
    if district_names:
        obj=np.zeros(nv);obj[dstart:cstart]=1;objectives.append(('marriage_district',obj))
    obj=np.zeros(nv);obj[cstart:]=1;objectives.append(('single_cell',obj))
    for stage,obj in objectives:
        constraints=[LinearConstraint(matrix,values,values)]
        for old_obj,optimum in locked:
            constraints.append(LinearConstraint(old_obj,optimum,optimum))
        solved=milp(obj,integrality=integer,bounds=Bounds(lb,ub),constraints=constraints,options={'mip_rel_gap':0.0})
        if not solved.success or solved.x is None or solved.status != 0:
            raise ValueError(f'Optimization failed at {stage}: {solved.message}')
        solution=solved.x
        objective=float(obj @ solution)
        if stage in ('marriage','single_group','marriage_district'):
            nearest=round(objective)
            if abs(objective-nearest)>1e-5: raise ValueError('Integer margin error is fractional')
            locked.append((obj,float(nearest)))
        stages.append({'stage':stage,'objective':objective,'status':'optimal'})
    selected={v:int(round(solution[i])) for i,v in enumerate(variants)}
    if any(abs(solution[i]-selected[v])>1e-5 or selected[v]>len(combos[v]) for i,v in enumerate(variants)):
        raise ValueError('Non-integer or out-of-bounds optimization result')
    for k in keys:
        if sum(n for (cell,_,_),n in selected.items() if cell==k)!=quota[k]: raise ValueError('Cell quota violated')
    actual_marriage={m:sum(n for (_,_,mr),n in selected.items() if mr==m) for m in marriage_keys}
    district_report=[]
    for gu in district_names:
        actual={m:sum(n for (cell,_,mr),n in selected.items() if cell[0]==gu and mr==m) for m in marriage_keys}
        target=district_targets[gu]
        district_report.append({'district':gu,'target':target,'actual':actual,
                                'error':{m:actual[m]-target[m] for m in marriage_keys}})
    actual_single=sum(n for (_,alone,_),n in selected.items() if alone)
    per_cell_single={k:sum(n for (cell,alone,_),n in selected.items() if cell==k and alone) for k in keys}
    return selected, {'sex':keys[0][1],'marriage_age_lower':_group_age(keys[0][2]),'n':total,
                      'marriage_target':marriage_target,'marriage_actual':actual_marriage,
                      'marriage_error':{m:actual_marriage[m]-marriage_target[m] for m in marriage_keys},
                      'district_marriage':district_report,
                      'single_proxy_target':single_target,'single_proxy_actual':actual_single,
                      'single_proxy_error':actual_single-single_target,
                      'expected_single_by_cell':expected_single,'actual_single_by_cell':per_cell_single,
                      'stages':stages}


def allocate_donors(candidates: list[dict], stats: dict, n: int, seed: int) -> tuple[list[str],dict]:
    cells=stats['population']; statmap={(c['district'],c['sex'],c['age_lower']):c for c in cells}
    combos=defaultdict(list); cap=Counter(); seen=set()
    for p in candidates:
        uid=p['uuid']
        if not isinstance(uid,str) or not uid or uid in seen: raise ValueError('Missing or duplicate donor UUID')
        seen.add(uid); key=cell_key(p)
        if key not in statmap: raise ValueError(f'Donor outside population cells: {key}')
        flag=living_alone(p['family_type']); mar=marriage_group(p['marital_status'])
        combos[(key,flag,mar)].append(uid);cap[key]+=1
    adjusted,moves=population_quotas(cells,cap,n)
    original=largest_remainder({k:v['population'] for k,v in statmap.items()},n)
    marriage={(m['sex'],m['age_lower']):m for m in stats['marriage']}
    district_marr={(m['district'],m['sex'],m['age_lower']):m for m in stats.get('district_marriage',[])}
    bygroup=defaultdict(list)
    for key in sorted(statmap):
        if adjusted[key]:bygroup[(key[1],_group_age(key[2]))].append(key)
    chosen=[]; group_reports=[]; cell_single={}; cell_expected={}
    for group,keys in sorted(bygroup.items()):
        reference=None if group[1]==19 else marriage.get(group)
        if group[1]!=19 and reference is None: raise ValueError(f'Missing marriage reference: {group}')
        local_refs={k[0]:district_marr[(k[0],group[0],group[1])] for k in keys
                    if (k[0],group[0],group[1]) in district_marr}
        allocated,report=_optimize_group(keys,adjusted,statmap,combos,reference,local_refs)
        cell_single.update(report.pop('actual_single_by_cell'))
        cell_expected.update(report.pop('expected_single_by_cell'))
        group_reports.append(report)
        for variant,num in sorted(allocated.items()):
            ids=sorted(combos[variant],key=lambda uid:(hashlib.sha256(f'{seed}|{uid}'.encode()).digest(),uid))
            chosen.extend(ids[:num])
    if len(chosen)!=n or len(set(chosen))!=n: raise ValueError('Selected donor count or uniqueness failed')
    cell_reports=[]
    for key in sorted(statmap):
        c=statmap[key]
        cell_reports.append({'district':key[0],'sex':key[1],'age_lower':key[2],
            'official_population':c['population'],'official_single_registered':c['single_registered'],
            'original_target':original[key],'adjusted_target':adjusted[key],
            'donor_capacity':cap[key],'actual':adjusted[key],
            'single_proxy_expected':cell_expected.get(key,0.0),'single_proxy_actual':cell_single.get(key,0)})
    return chosen,{'requested':n,'population_total':sum(c['population'] for c in cells),
                  'cells':cell_reports,'groups':group_reports,'age_reallocations':moves}


def read_candidates(path: Path) -> list[dict]:
    pf=pq.ParquetFile(path)
    out=[]
    for batch in pf.iter_batches(batch_size=4096,columns=['uuid','district','sex','age','family_type','marital_status']):
        out.extend(batch.to_pylist())
    return out


def extract_parent_evidence(parent: dict, evidence: dict) -> list[dict]:
    fields = evidence['matched_fields'].split('|')
    expected_tokens = set(evidence['matched_tokens'].split('|'))
    if not fields or not expected_tokens or evidence['match_type'] not in ('place_mention','business_only'):
        raise ValueError('Invalid parent evidence record')
    results=[]; found=set()
    accepted = PLACE_TOKENS | BUSINESS_TOKENS
    for field in fields:
        value=parent.get(field)
        if not isinstance(value,str): raise ValueError(f'Missing evidence field {field}')
        matches=[m for m in TOKEN.finditer(value) if m.group() in accepted]
        if not matches: raise ValueError(f'No accepted token in field {field}')
        for match in matches:
            token=match.group();found.add(token)
            start,end=match.span()
            left=max(value.rfind(ch,0,start) for ch in '.!?\n')+1
            boundary=[value.find(ch,end) for ch in '.!?\n']
            boundary=[x for x in boundary if x>=0]
            right=(min(boundary)+1) if boundary else len(value)
            while left<start and value[left].isspace():left+=1
            while right>end and value[right-1].isspace():right-=1
            results.append({'field':field,'token':token,'start':start,'end':end,
                            'sentence_start':left,'sentence_end':right,'sentence':value[left:right]})
    if found != expected_tokens:
        raise ValueError(f'Parent evidence tokens disagree: {found} versus {expected_tokens}')
    if evidence['match_type']=='business_only' and found & PLACE_TOKENS:
        raise ValueError('Business-only parent has a place token')
    return results


def assign_parents(parent_ids: list[str], donor_ids: list[str], seed: int) -> list[tuple[str,int,str]]:
    if not parent_ids or len(parent_ids)!=len(set(parent_ids)) or len(donor_ids)!=len(set(donor_ids)):
        raise ValueError('Empty/duplicate parents or duplicate donors')
    rng=random.Random(seed)
    parents=sorted(parent_ids); extra_order=parents.copy();rng.shuffle(extra_order)
    base,extra=divmod(len(donor_ids),len(parents)); extra_set=set(extra_order[:extra])
    slots=[(parent,index) for parent in parents for index in range(1,base+1+(parent in extra_set))]
    donors=sorted(donor_ids);rng.shuffle(donors)
    for i,(parent,_) in enumerate(slots):
        if donors[i]==parent:
            swap=next((j for j in range(i+1,len(slots))
                       if donors[j]!=parent and donors[i]!=slots[j][0]),None)
            if swap is None:
                swap=next((j for j in range(0,i)
                           if donors[j]!=parent and donors[i]!=slots[j][0]),None)
            if swap is None: raise ValueError('Cannot avoid donor-parent self connection')
            donors[i],donors[swap]=donors[swap],donors[i]
    return [(parent,index,donor) for (parent,index),donor in zip(slots,donors)]


def synthetic_id(parent: str, index: int, donor: str) -> str:
    return str(uuid.uuid5(NAMESPACE,f'{parent}|{index}|{donor}'))


def load_parent_data(parent_path: Path, evidence_path: Path) -> tuple[dict,dict]:
    people={p['uuid']:p for p in pq.read_table(parent_path).to_pylist()}
    with evidence_path.open(encoding='utf-8-sig',newline='') as f:
        rows=list(csv.DictReader(f))
    evidence={r['uuid']:r for r in rows}
    if len(people)!=len(evidence) or set(people)!=set(evidence) or len(rows)!=len(evidence):
        raise ValueError('Parent/evidence IDs differ or are duplicated')
    return people,evidence


def write_personas(source: Path, parents: Path, evidence: Path,
                   assignments: list[tuple[str,int,str]], output: Path) -> None:
    people,evidence_rows=load_parent_data(parents,evidence)
    lookup={d:(p,i) for p,i,d in assignments}
    if len(lookup)!=len(assignments) or any(p not in people for p,_,_ in assignments):
        raise ValueError('Invalid assignment mapping')
    evidence_json={p:json.dumps(extract_parent_evidence(row,evidence_rows[p]),ensure_ascii=False,separators=(',',':'))
                   for p,row in people.items()}
    source_file=pq.ParquetFile(source)
    original_schema=source_file.schema_arrow
    added=[pa.field('synthetic_id',pa.string(),nullable=False),pa.field('donor_uuid',pa.string(),nullable=False),
           pa.field('parent_uuid',pa.string(),nullable=False),pa.field('variation_index',pa.int32(),nullable=False),
           pa.field('parent_match_type',pa.string(),nullable=False),pa.field('parent_evidence_json',pa.string(),nullable=False),
           pa.field('parent_evidence_role',pa.string(),nullable=False)]
    schema=original_schema
    for field in added:schema=schema.append(field)
    written=0
    with pq.ParquetWriter(output,schema,compression='snappy') as writer:
        for batch in source_file.iter_batches(batch_size=2048):
            rows=[]
            for row in batch.to_pylist():
                donor=row['uuid']
                if donor not in lookup:continue
                parent,index=lookup[donor]
                row.update(synthetic_id=synthetic_id(parent,index,donor),donor_uuid=donor,parent_uuid=parent,
                           variation_index=index,parent_match_type=evidence_rows[parent]['match_type'],
                           parent_evidence_json=evidence_json[parent],parent_evidence_role=EVIDENCE_ROLE)
                rows.append(row)
            if rows:
                writer.write_table(pa.Table.from_pylist(rows,schema=schema));written+=len(rows)
    if written!=len(assignments):raise ValueError(f'Not all assigned donors found: {written}/{len(assignments)}')


def verify_output(output: Path, source: Path, parents: Path, evidence: Path,
                  stats: dict, diagnostics: dict, selected_ids: list[str]) -> dict:
    people,evidence_rows=load_parent_data(parents,evidence)
    source_file=pq.ParquetFile(source); output_file=pq.ParquetFile(output)
    original_schema=source_file.schema_arrow
    if output_file.schema_arrow.names[:len(original_schema)] != original_schema.names:
        raise ValueError('Original columns missing or reordered')
    for field in original_schema:
        if output_file.schema_arrow.field(field.name).type != field.type:
            raise ValueError(f'Original field type changed: {field.name}')
    added=['synthetic_id','donor_uuid','parent_uuid','variation_index','parent_match_type',
           'parent_evidence_json','parent_evidence_role']
    if output_file.schema_arrow.names[len(original_schema):] != added:
        raise ValueError('Unexpected added columns')
    if output_file.metadata.num_rows != len(selected_ids) or len(selected_ids)!=len(set(selected_ids)):
        raise ValueError('Output/selection count mismatch')
    selected=set(selected_ids)
    actual_rows=(r for batch in output_file.iter_batches(batch_size=2048) for r in batch.to_pylist())
    parents_seen=Counter(); parent_indices=defaultdict(set); new_ids=set(); donors=set()
    cells=Counter(); single=Counter(); marriage=Counter(); district_marriage=Counter(); evidence_cache={}; business=0
    count=0
    for batch in source_file.iter_batches(batch_size=2048):
        for expected in batch.to_pylist():
            donor=expected['uuid']
            if donor not in selected:continue
            try:row=next(actual_rows)
            except StopIteration:raise ValueError('Output ended before source') from None
            if any(row[name] != expected[name] for name in original_schema.names):
                raise ValueError(f'Original values changed for donor {donor}')
            if row['donor_uuid']!=donor or donor in donors:raise ValueError('Invalid/duplicate donor')
            donors.add(donor)
            parent=row['parent_uuid']; index=row['variation_index']; uid=row['synthetic_id']
            if parent not in people or parent==donor or not isinstance(index,int) or index<1:
                raise ValueError('Invalid parent or variation index')
            if uid != synthetic_id(parent,index,donor) or uid in new_ids:
                raise ValueError('Invalid or duplicate generated ID')
            new_ids.add(uid); parents_seen[parent]+=1
            if index in parent_indices[parent]:raise ValueError('Duplicate parent variation index')
            parent_indices[parent].add(index)
            if row['parent_match_type']!=evidence_rows[parent]['match_type'] or row['parent_evidence_role']!=EVIDENCE_ROLE:
                raise ValueError('Parent evidence type/role changed')
            raw=row['parent_evidence_json']
            if parent in evidence_cache and evidence_cache[parent]!=raw:
                raise ValueError('Inconsistent evidence for parent')
            if parent not in evidence_cache:
                entries=json.loads(raw)
                expected_hits=set()
                for field in evidence_rows[parent]['matched_fields'].split('|'):
                    text=people[parent][field]
                    expected_hits.update((field,m.group(),m.start(),m.end()) for m in TOKEN.finditer(text)
                                         if m.group() in PLACE_TOKENS|BUSINESS_TOKENS)
                observed=set()
                for item in entries:
                    field=item['field']; value=people[parent][field]
                    start,end=item['start'],item['end'];a,b=item['sentence_start'],item['sentence_end']
                    if not (0<=a<=start<end<=b<=len(value)) or value[start:end]!=item['token'] or value[a:b]!=item['sentence']:
                        raise ValueError('Invalid original evidence offsets or sentence')
                    observed.add((field,item['token'],start,end))
                if observed!=expected_hits or len(observed)!=len(entries) or not entries:
                    raise ValueError('Missing, duplicate, or spurious evidence match')
                evidence_cache[parent]=raw
            business += int(row['parent_match_type']=='business_only')
            key=cell_key(row);cells[key]+=1
            single[key]+=int(living_alone(row['family_type']))
            marriage[(row['sex'],_group_age(key[2]),marriage_group(row['marital_status']))]+=1
            district_marriage[(key[0],row['sex'],_group_age(key[2]),marriage_group(row['marital_status']))]+=1
            count+=1
    try:next(actual_rows);raise ValueError('Extra output rows')
    except StopIteration:pass
    if donors!=selected or count!=len(selected_ids) or set(parents_seen)!=set(people):
        raise ValueError('Output donor or parent coverage mismatch')
    base,extra=divmod(count,len(people))
    if sum(n==base+1 for n in parents_seen.values())!=extra or any(n not in (base,base+1) for n in parents_seen.values()):
        raise ValueError('Parent variation count mismatch')
    if any(indices!=set(range(1,parents_seen[p]+1)) for p,indices in parent_indices.items()):
        raise ValueError('Parent variation indices are not contiguous')
    ref={(c['district'],c['sex'],c['age_lower']):c for c in stats['population']}
    original=largest_remainder({k:c['population'] for k,c in ref.items()},count)
    adjusted=dict(original)
    for move in diagnostics['age_reallocations']:
        a=tuple(move['from']);b=tuple(move['to']);n=move['count']
        if a not in adjusted or b not in adjusted or a[:2]!=b[:2] or not isinstance(n,int) or n<=0:
            raise ValueError('Invalid age reallocation')
        adjusted[a]-=n;adjusted[b]+=n
    for c in diagnostics['cells']:
        k=(c['district'],c['sex'],c['age_lower'])
        if k not in ref or c['official_population']!=ref[k]['population'] or c['official_single_registered']!=ref[k]['single_registered']:
            raise ValueError('Population reference mismatch')
        if (c['original_target']!=original[k] or c['adjusted_target']!=adjusted[k] or
            c['actual']!=cells[k] or c['adjusted_target']!=cells[k] or c['single_proxy_actual']!=single[k]):
            raise ValueError(f'Population/single result mismatch: {k}')
    if len(diagnostics['cells'])!=len(ref) or set(cells)-set(ref):raise ValueError('Missing population diagnostics')
    for g in diagnostics['groups']:
        sex,lo=g['sex'],g['marriage_age_lower']
        keys=[k for k in ref if k[1]==sex and _group_age(k[2])==lo]
        actual_single=sum(single[k] for k in keys)
        expected_single=int(math.floor(sum(adjusted[k]*ref[k]['single_registered']/ref[k]['population']
                                          if ref[k]['population'] else 0 for k in keys)+.5))
        if g['n']!=sum(cells[k] for k in keys) or g['single_proxy_actual']!=actual_single or g['single_proxy_target']!=expected_single or g['single_proxy_error']!=actual_single-expected_single:
            raise ValueError(f'Single group diagnostic mismatch: {sex} {lo}')
        if lo!=19:
            marr=next((m for m in stats['marriage'] if m['sex']==sex and m['age_lower']==lo),None)
            if marr is None:raise ValueError('Missing marriage reference')
            target=largest_remainder({m:marr[m] for m in ('never','married','divorced_widowed')},g['n'])
            actual={m:marriage[(sex,lo,m)] for m in target}
            if target!=g['marriage_target'] or actual!=g['marriage_actual'] or {m:actual[m]-target[m] for m in target}!=g['marriage_error']:
                raise ValueError(f'Marriage diagnostic mismatch: {sex} {lo}')
            refs={m['district']:m for m in stats.get('district_marriage',[]) if m['sex']==sex and m['age_lower']==lo}
            expected_districts={k[0] for k in keys if adjusted[k] and k[0] in refs}
            if {d['district'] for d in g['district_marriage']}!=expected_districts:
                raise ValueError('District marriage diagnostics missing or unexpected')
            for d in g['district_marriage']:
                gu=d['district'];source_m=refs[gu]
                n=sum(cells[k] for k in keys if k[0]==gu)
                target=largest_remainder({m:source_m[m] for m in ('never','married','divorced_widowed')},n)
                actual={m:district_marriage[(gu,sex,lo,m)] for m in target}
                if target!=d['target'] or actual!=d['actual'] or {m:actual[m]-target[m] for m in target}!=d['error']:
                    raise ValueError(f'District marriage diagnostic mismatch: {gu} {sex} {lo}')
    return {'rows':count,'unique_synthetic_ids':len(new_ids),'unique_donors':len(donors),
            'parent_count':len(parents_seen),'parent_variations_min':min(parents_seen.values()),
            'parent_variations_max':max(parents_seen.values()),'parents_with_extra':extra,
            'business_only_variations':business,'original_columns_verified':len(original_schema),
            'evidence_parents_verified':len(evidence_cache),'verified':True}


def build_dataset(source: Path, parents: Path, evidence: Path, statistics: Path, selection: Path,
                  ontology: Path, output: Path, expected_n: int, expected_parent_count: int) -> dict:
    if output.exists():raise FileExistsError(f'Refusing to overwrite {output}')
    stats=json.loads(statistics.read_text(encoding='utf8'))
    choice=json.loads(selection.read_text(encoding='utf8'))
    if sha256(source)!=choice['source_sha256'] or sha256(statistics)!=choice['statistics_sha256']:
        raise ValueError('Selection input hashes do not match')
    ids=choice['selected_donor_uuids']
    people,_=load_parent_data(parents,evidence)
    if len(ids)!=expected_n or len(people)!=expected_parent_count:
        raise ValueError('Unexpected donor or parent count')
    assignments=assign_parents(list(people),ids,choice['seed'])
    output.parent.mkdir(parents=True,exist_ok=True)
    staged=Path(tempfile.mkdtemp(prefix='.myeongdong-augmentation-',dir=output.parent))
    try:
        data_path=staged/'personas.parquet'
        write_personas(source,parents,evidence,assignments,data_path)
        check=verify_output(data_path,source,parents,evidence,stats,choice['diagnostics'],ids)
        shutil.copyfile(statistics,staged/'statistics.json')
        shutil.copyfile(ontology,staged/'ontology_contract_snapshot.md')
        report={'schema_version':'1.0','status':'verified','seed':choice['seed'],
                'dataset':'nvidia/Nemotron-Personas-Korea','source_revision':'ada0f5b53a38bb5a30cce09358adde883c1ab63a',
                'source_license':'CC BY 4.0','source_url':'https://huggingface.co/datasets/nvidia/Nemotron-Personas-Korea',
                'hashes':{'source_parquet':sha256(source),'parent_parquet':sha256(parents),
                          'parent_evidence_csv':sha256(evidence),'statistics':sha256(statistics),
                          'selection':sha256(selection),'ontology_contract_snapshot':sha256(staged/'ontology_contract_snapshot.md'),
                          'generator_script':sha256(Path(__file__)),
                          'statistics_preparer_script':sha256(ROOT/'scripts/prepare_myeongdong_statistics.py'),
                          'personas_parquet':sha256(data_path)},
                'runtime':{'python':platform.python_version(),'pyarrow':pa.__version__,'numpy':np.__version__,'scipy':scipy.__version__},
                'ontology_contract':'01a_persona_ontology.md section 7.1; snapshot is draft, only minimum contract applied',
                'validation':check,'calibration':choice['diagnostics'],'limitations':stats['limitations']}
        (staged/'diagnostics.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n',encoding='utf8')
        cells=choice['diagnostics']['cells'];groups=choice['diagnostics']['groups']
        single_target=sum(g['single_proxy_target'] for g in groups)
        single_actual=sum(g['single_proxy_actual'] for g in groups)
        marital_error=sum(abs(v) for g in groups for v in g['marriage_error'].values())
        local_marital_error=sum(abs(v) for g in groups for d in g['district_marriage'] for v in d['error'].values())
        local_marital_groups=sum(len(g['district_marriage']) for g in groups)
        readme=f'''# 명동 맥락 서울 합성 페르소나 {check['rows']:,}건

이 자료는 NVIDIA [Nemotron-Personas-Korea](https://huggingface.co/datasets/nvidia/Nemotron-Personas-Korea) (CC BY 4.0, revision `ada0f5b53a38bb5a30cce09358adde883c1ab63a`)의 **합성** 서울 페르소나를 통계에 맞춰 고르고, 명동 관련 원본 {check['parent_count']}개의 단서를 연결한 것이다. 실존 인물, 행동 관측치 또는 명동 방문자 대표 표본이 아니다. 서울에 거주하는 내국인 19~99세를 기준으로 하며 외국인·거주불명자·재외국민·100세 이상을 제외한 주민등록인구에 보정했다.

## 파일

- `personas.parquet`: {check['rows']:,}행, 원본 26개 필드와 아래의 추적 필드. 모든 출처 서울 인물은 한 번만 사용된다.
- `diagnostics.json`: 850개 인구 셀과 성·연령 혼인 집단의 원목표·보정목표·실현·오차, 표본 부족 이동, 입력·출력 해시와 검증 결과.
- `statistics.json`: 실제 사용한 공식 원표 숫자와 조회 조건, 원본 응답 해시 및 자료별 한계.
- `ontology_contract_snapshot.md`: 생성 당시 병행 온톨로지 초안의 스냅샷. 확정된 최소 연결 계약 §7.1을 적용했다.

## 통계 반영 결과

| 기준 | 적용 및 결과 |
|---|---|
| [행정안전부 2026년 9월 주민등록 연령별 인구](https://jumin.mois.go.kr/ageStatMonth.do) | 서울 25구 × 성별 × 19세 또는 5세 연령대 850개 셀. 원본이 부족한 셀에서 {len(choice['diagnostics']['age_reallocations'])}명은 같은 구·성의 가까운 연령대로 이동. |
| [행정안전부 성·연령별 1인세대](https://jumin.mois.go.kr/sexdAge1Hshd.do?sttsGbn=admm) | 원본의 `family_type`이 '혼자 거주'로 시작하는 유형을 대리 지표로 사용. 집단별 합산 목표 {single_target:,}명, 결과 {single_actual:,}명. 주민등록상 1인세대와 실제 1인가구의 정의는 다르다. |
| [국가데이터처 인구총조사 2025 혼인상태](https://kosis.kr/statisticsList/mass/mass_list.jsp?list_id=&org_id=101&process=statHtml&tbl_id=DT_1MR2060&vw_cd=) | 서울 성·연령 및 공개된 구별 미혼·유배우·사별/이혼 목표에 맞췄다. 서울 단위 범주별 절대오차의 합은 {marital_error}명, 표본이 있는 공개 구별 집단 {local_marital_groups}개의 합은 {local_marital_error}명이다(부족과 초과를 각각 합산). 구별 목표에서 공개되지 않은 41개 셀은 제외했다. 19세는 혼인상태를 보정하지 않았고, 85세 이상 기준은 100세 이상을 포함한 대리 비율이다. |
| 행정동 맥락 | 명동 행정동 주민은 2026년 9월 전체 연령 {stats['myeongdong_context']['all_ages_total']:,}명이다. 이는 방문객 수가 아니며 전체 6.5만 명을 명동 거주자로 간주하지 않았다. |

통계의 조사 시점·모집단은 일치하지 않는다. 주거 유형·교육·직업·소득·건강·개별 방문·행동은 해당 통계에 맞춰 보정하지 않았으며 원본 합성 페르소나의 편향과 내부 서술 오류가 남을 수 있다. 각 5세 구간 내부의 단일 연령 분포와 혼인·가구의 모든 교차분포까지 일치한다는 뜻도 아니다.

## 연결 필드와 온톨로지

- `uuid`: 출처 서울 페르소나의 UUID다. 원본 필드 값을 보존하기 때문에 바꾸지 않았다.
- `synthetic_id`: 이 증강 인물의 고유 ID. 새로운 인물 식별에 사용한다.
- `donor_uuid`: `uuid`와 같은 출처 서울 인물. 온톨로지 R-02의 26개 열 입력은 이 인물의 데이터다.
- `parent_uuid`, `variation_index`: 명동 관련 원본과 그 원본에서 만든 변형 번호. 각 부모는 126개 또는 127개다.
- `parent_match_type`: `place_mention` 또는 위치가 미확정인 `business_only`.
- `parent_evidence_json`: 부모 원문의 필드명·토큰·문자 시작/끝·문장 시작/끝·문장 원문을 담은 JSON 배열. 문자열 오프셋은 유니코드 코드포인트 기준 반개구간이다.
- `parent_evidence_role`: `unverified_parent_context_visit_candidate`. 이는 부모의 단서이지 새 인물이 실제로 그 일을 했다는 뜻이 아니다. 현재·과거·부정·희망 여부와 새 인물의 직업·가족 등과의 충돌은 이후 온톨로지 단계에서 확인해야 한다. 상호 전용 부모 4개의 파생 {check['business_only_variations']}행은 매장의 실제 명동 위치가 확인되지 않았다.

능력·민감도·환경·반응은 이번 증강에서 만들지 않았다. 온톨로지의 최종 특성 스키마와 반응 계산은 초안이므로, 이 결과의 연결 필드와 원문을 입력으로 삼아 후속 일괄 파생해야 한다.

## 재현과 읽기

저장된 서울·명동 원본과 `data/myeongdong-augmentation-inputs/statistics.json`이 있을 때 아래 명령으로 새 경로에 배분과 검증된 결과를 재생성할 수 있다. 지정한 경로는 미리 존재하지 않아야 한다. 시드, 라이브러리 버전 및 해시는 `diagnostics.json`에 있다. 입력 통계는 공개 사이트 조회 응답을 `python3 scripts/prepare_myeongdong_statistics.py --help`에 맞게 제공하면 재정규화할 수 있다.

```sh
python3 -m scripts.augment_myeongdong_personas allocate --selection /tmp/myeongdong-selection-replay.json
python3 -m scripts.augment_myeongdong_personas build --selection /tmp/myeongdong-selection-replay.json --output /tmp/myeongdong-replay
```

```python
import pyarrow.parquet as pq
table = pq.read_table('data/myeongdong-augmented/personas.parquet')
print(table.select(['synthetic_id', 'parent_uuid', 'age', 'district']).slice(0, 5))
```
'''
        (staged/'README.md').write_text(readme,encoding='utf8')
        if output.exists():raise FileExistsError(f'Output appeared during build: {output}')
        staged.rename(output)
        return report
    except BaseException:
        shutil.rmtree(staged,ignore_errors=True)
        raise


def main() -> None:
    ap=argparse.ArgumentParser()
    ap.add_argument('stage',choices=['allocate','build'])
    ap.add_argument('--source',type=Path,default=SOURCE)
    ap.add_argument('--statistics',type=Path,default=STATS)
    ap.add_argument('--selection',type=Path,default=SELECTION)
    ap.add_argument('--parents',type=Path,default=PARENTS)
    ap.add_argument('--evidence',type=Path,default=EVIDENCE)
    ap.add_argument('--ontology',type=Path,default=ONTOLOGY)
    ap.add_argument('--output',type=Path,default=OUTPUT)
    ap.add_argument('--count',type=int,default=65000)
    ap.add_argument('--parent-count',type=int,default=514)
    ap.add_argument('--seed',type=int,default=SEED)
    args=ap.parse_args()
    if args.stage=='build':
        result=build_dataset(args.source,args.parents,args.evidence,args.statistics,args.selection,
                             args.ontology,args.output,args.count,args.parent_count)
        print(f"Verified and saved {result['validation']['rows']} personas to {args.output}")
        return
    if args.selection.exists():raise FileExistsError(f'Refusing to overwrite {args.selection}')
    stats=json.loads(args.statistics.read_text(encoding='utf8'))
    ids,diagnostics=allocate_donors(read_candidates(args.source),stats,args.count,args.seed)
    output={'source_sha256':sha256(args.source),'statistics_sha256':sha256(args.statistics),
            'seed':args.seed,'selected_donor_uuids':ids,'diagnostics':diagnostics}
    args.selection.parent.mkdir(parents=True,exist_ok=True)
    with args.selection.open('x',encoding='utf8') as f:
        json.dump(output,f,ensure_ascii=False,indent=2);f.write('\n')
    print(f'Selected {len(ids)} unique donors, {len(diagnostics["age_reallocations"])} sparse-cell moves')

if __name__=='__main__':main()
