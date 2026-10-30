"""Audit paired hypothesis runs and produce a transparent comparison report."""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import sys
from typing import Any

import yaml
import pyarrow.parquet as pq
from myeongdong_hypotheses import BATCH_ROOT, BASE, CASES, JOB_ROOT, OUTPUT_ROOT, SEEDS, changed_keys, difference, options_for
from myeongdong_scenarios import sha256
from myeongdong_ui_server import atomic_json

METRIC_LABELS={'purchases':('구매','건'),'notices':('장소 인지','건'),'rest_starts':('휴식 시작','건'),
               'effective_area_m2':('평균 유효 보행 면적','㎡'),'building_enters':('밀레오레 대리 장소 입장','회')}
ALLOWED={'discount':{'places'},'visibility':{'edges','places'},'pedestrian':set(),
         'bench':{'places'},'width':{'edges'},'building':{'places'},'income':{'profiles','profile_attrs'}}
ORDER=['building','discount','income','pedestrian','width','visibility','bench']

def combine_batches(primary: dict[str,Any], followup: dict[str,Any]) -> dict[str,Any]:
    for key in ('engine','base_source_sha256','reference_job','seeds','cases'):
        if primary[key]!=followup[key]: raise ValueError(f'재검증 배치의 {key}이 다릅니다')
    if set(primary['runs']) & set(followup['runs']): raise ValueError('재검증 배치에 중복 실행 키가 있습니다')
    return {**primary,'runs':{**primary['runs'],**followup['runs']}}

def exposure_minutes(run_id: str) -> float:
    rows=pq.read_table(OUTPUT_ROOT/run_id/'metrics_10min.parquet',columns=['metric','key','value']).to_pylist()
    active=[r['value'] for r in rows if r['metric']=='active_bodies_mean' and not r['key']]
    if len(active)!=18: raise ValueError('평균 재실 인원 구간이 18개가 아닙니다')
    minutes=sum(active)*10
    if minutes<=0: raise ValueError('재실 인·분이 0입니다')
    return minutes


def audit_pair(batch: dict[str,Any], seed_idx: int, case: str) -> None:
    baseline=batch['runs'][f'{seed_idx}:baseline']; changed=batch['runs'][f'{seed_idx}:{case}']
    if baseline['state']!='complete' or changed['state']!='complete': raise ValueError('완료되지 않은 쌍입니다')
    if baseline['seed']!=changed['seed'] or baseline['seed']!=SEEDS[seed_idx]: raise ValueError('시드가 다릅니다')
    if baseline['options']!=options_for('baseline') or changed['options']!=options_for(case):
        raise ValueError('설정 값이 사전 가설과 다릅니다')
    if changed_keys(baseline['options'],changed['options'])!=changed_keys(options_for('baseline'),options_for(case)):
        raise ValueError('설정 변화가 사전 가설과 다릅니다')
    manifests=[]; scenarios=[]; arrivals=[]
    for item in (baseline,changed):
        jobdir=JOB_ROOT/item['run_id']; inp=jobdir/'input'; out=OUTPUT_ROOT/item['run_id']
        job=json.loads((jobdir/'job.json').read_text()); source=json.loads((inp/'source.json').read_text())
        scn=yaml.safe_load((inp/'bundle/scenario.yaml').read_text()); status=json.loads((out/'run_status.json').read_text())
        if job['engine_build']!=batch['engine'] or source['engine_build']!=batch['engine'] or sha256(jobdir/'engine.bin')!=batch['engine']['binary_sha256']:
            raise ValueError('짝의 엔진 실행 파일이 다릅니다')
        if source['source_sha256']!=json.loads((BASE/'source.json').read_text())['source_sha256']:
            raise ValueError('짝의 원본 데이터 출처가 다릅니다')
        if (source['run_options']!=item['options'] or scn['seed']!=item['seed'] or status['spawned_bodies']!=5044
            or status['provenance']['run_options']!=item['options'] or status['provenance']['engine_build']!=batch['engine']):
            raise ValueError('실행 옵션·시드·인원이 다릅니다')
        manifest=json.loads((inp/'bundle/manifest.json').read_text())
        if sha256(inp/'bundle/scenario.yaml')!=manifest['scenarios'][0]['sha256'] or manifest['scenarios'][0]['seed']!=item['seed']:
            raise ValueError('시나리오 해시 또는 시드가 다릅니다')
        for entries in manifest['files'].values():
            for e in entries:
                if sha256(inp/e['path'])!=e['sha256']: raise ValueError('입력 파일 해시가 다릅니다')
        manifests.append(manifest)
        scenarios.append({k:v for k,v in scn.items() if k not in ('pedestrian_only','static_applied')})
        arrivals.append(pq.read_table(out/'agents.parquet',columns=['persona_id','arrival_tick','portal_in','mode']).to_pylist())
    if scenarios[0]!=scenarios[1] or manifests[0]['ontology_sha256']!=manifests[1]['ontology_sha256']:
        raise ValueError('기간·엔진 시나리오 또는 온톨로지 정의가 다릅니다')
    if arrivals[0]!=arrivals[1] or len(arrivals[0])!=5044:
        raise ValueError('두 실행의 인물·입장 계획이 다릅니다')
    before,after=({k:[e['sha256'] for e in v] for k,v in m['files'].items()} for m in manifests)
    changed_roles={k for k in before if before[k]!=after[k]}
    if not changed_roles <= ALLOWED[case]: raise ValueError(f'허용되지 않은 입력 차이: {sorted(changed_roles-ALLOWED[case])}')


def classification(case: str, pairs: list[dict[str,Any]]) -> tuple[str,str]:
    if len(pairs)<2: return 'not_replicated','다른 시드 재검증 전'
    delta=[p['delta'] for p in pairs]
    if any(d==0 for d in delta): return 'no_effect','재검증 중 변화가 0임'
    if (delta[0]>0)!=(delta[1]>0): return 'mixed','시드에 따라 변화 방향이 다름'
    if case!='building' and delta[0]<0: return 'opposite','예상과 반대 방향'
    metric=CASES[case]['metric']
    if metric!='effective_area_m2' and any(min(p['before'],p['after'])<10 or abs(p['delta'])<5 for p in pairs):
        return 'sparse','사건이 적거나 차이가 5건 미만'
    return 'candidate','두 시드에서 같은 방향. 탐색 후 재검증한 모형 결과'


def verify_repeat(batch: dict[str,Any], repeat: dict[str,Any]) -> dict[str,Any]:
    a=batch['runs']['0:baseline']; b=repeat['runs']['0:baseline']
    if a['state']!='complete' or b['state']!='complete' or a['options']!=b['options'] or a['seed']!=b['seed'] or batch['engine']!=repeat['engine']:
        raise ValueError('기준 재실행 조건이 다르거나 미완료입니다')
    dirs=[OUTPUT_ROOT/x['run_id'] for x in (a,b)]
    input_manifests=[json.loads((JOB_ROOT/x['run_id']/'input/bundle/manifest.json').read_text()) for x in (a,b)]
    inputs=[{k:[x['sha256'] for x in v] for k,v in m['files'].items()} for m in input_manifests]
    checks={name:sha256(dirs[0]/name)==sha256(dirs[1]/name) for name in ('agents.parquet','events.parquet','metrics_10min.parquet')}
    statuses=[json.loads((d/'run_status.json').read_text()) for d in dirs]
    checks['frames']=[x['sha256'] for x in statuses[0]['frames']]==[x['sha256'] for x in statuses[1]['frames']] and len(statuses[0]['frames'])==18
    checks['inputs']=inputs[0]==inputs[1]
    checks['scenario']=input_manifests[0]['scenarios'][0]['sha256']==input_manifests[1]['scenarios'][0]['sha256']
    return {'passed':all(checks.values()),'baseline_run_id':a['run_id'],'repeat_run_id':b['run_id'],'checks':checks}


def make_report(batch: dict[str,Any], repeat: dict[str,Any] | None = None) -> dict[str,Any]:
    results=[]
    for case in [x for x in CASES if x!='baseline']:
        data=CASES[case]; pairs=[]; errors=[]
        for idx in (0,1):
            base=batch['runs'].get(f'{idx}:baseline'); item=batch['runs'].get(f'{idx}:{case}')
            if not base or not item: continue
            if item['state']!='complete' or base['state']!='complete': errors.append(item.get('error','미완료')); continue
            try: audit_pair(batch,idx,case)
            except (OSError,ValueError,KeyError) as exc: errors.append(str(exc));continue
            metric=data['metric']; v=difference(base['summary'][metric],item['summary'][metric])
            pair={'seed_index':idx,'seed':SEEDS[idx],'baseline_run_id':base['run_id'],'run_id':item['run_id'],**v,
                  'baseline_summary':base['summary'],'changed_summary':item['summary']}
            if case=='visibility':
                try: person_before,person_after=exposure_minutes(base['run_id']),exposure_minutes(item['run_id'])
                except (OSError,ValueError,KeyError) as exc: errors.append(str(exc)); continue
                pair['secondary']={'person_minutes':difference(person_before,person_after),
                    'notices_per_1000_person_minutes':difference(
                        base['summary']['notices']/person_before*1000,item['summary']['notices']/person_after*1000)}
            elif case in ('discount','income'):
                pair['secondary']={'revenue_krw':difference(base['summary']['revenue_krw'],item['summary']['revenue_krw'])}
            elif case=='building':
                pair['secondary']={'building_purchases':difference(base['summary']['building_purchases'],item['summary']['building_purchases']),
                    'building_revenue_krw':difference(base['summary']['building_revenue_krw'],item['summary']['building_revenue_krw'])}
            pairs.append(pair)
        status,reason=('invalid','; '.join(errors)) if errors else classification(case,pairs)
        label,unit=METRIC_LABELS[data['metric']]
        results.append({'id':case,'label':data['label'],'hypothesis':data['hypothesis'],'change':data['change'],
                        'metric':data['metric'],'metric_label':label,'unit':unit,'pairs':pairs,'status':status,'reason':reason})
    selected=[x for name in ORDER for x in results if x['id']==name and x['status']=='candidate'][:4]
    reproducibility=verify_repeat(batch,repeat) if repeat else None
    if not reproducibility or not reproducibility['passed']: selected=[]
    return {'schema_version':1,'batch_id':batch['batch_id'],'engine':batch['engine'],'reference_job':batch['reference_job'],
            'seeds':list(SEEDS),'candidates':results,'selected':[x['id'] for x in selected],
            'reproducibility':reproducibility,
            'limitations':['합성 인물 5,044명을 같은 입장 계획으로 3시간에 걸쳐 직접 투입한 실험이며 실제 방문자·매출 예측이 아닙니다.',
             '사례를 1차 결과에서 고른 뒤 다른 시드 하나로 재확인한 탐색적 결과이며 통계적 확증이 아닙니다.',
             '동일한 과거 고정 엔진 실행 파일을 사용했습니다. 이후 수정된 현재 엔진으로 다시 계산하면 결과가 달라질 수 있습니다.',
             '건물 업종은 가격·제공·수용 등 대리값이 함께 달라집니다. 물리·실측 검증은 미완료입니다.']}


def markdown(report: dict[str,Any]) -> str:
    lines=['# 명동 가설 실험 결과','',f"실험 묶음: `{report['batch_id']}` · 기준 인물: 5,044명 · 14:00–17:00",
           f"고정 엔진 SHA-256: `{report['engine']['binary_sha256']}`",'',
           '동일 기준 재실행: '+('일치' if report['reproducibility'] and report['reproducibility']['passed'] else '미확인 또는 불일치'),'',
           '| 가설 | 지표 | 첫 시드 기준 → 변경 | 두 번째 시드 기준 → 변경 | 판정 |', '|---|---|---|---|---|']
    for c in report['candidates']:
        pairs={p['seed_index']:p for p in c['pairs']}
        def desc(index):
            p=pairs.get(index)
            return '미실행' if not p else f"{p['before']:,.1f} → {p['after']:,.1f} ({p['delta']:+,.1f}{c['unit']})"
        lines.append(f"| {c['label']} | {c['metric_label']} | {desc(0)} | {desc(1)} | {c['reason']} |")
    lines.extend(['','추천: '+(', '.join(report['selected']) if report['selected'] else '조건을 만족하는 항목 없음'),'','## 보조 지표와 재생 기록',''])
    for c in report['candidates']:
        for pair in c['pairs']:
            extra=[]
            for key, diff in pair.get('secondary',{}).items():
                extra.append(f"{key}: {diff['before']:,.2f} → {diff['after']:,.2f}")
            lines.append(f"- {c['id']} 시드 {pair['seed_index']+1}: 기준 `{pair['baseline_run_id']}`, 변경 `{pair['run_id']}`"+(' · '+', '.join(extra) if extra else ''))
    lines.extend(['','## 한계',''])
    lines.extend('- '+x for x in report['limitations'])
    return '\n'.join(lines)+'\n'


def main() -> int:
    p=argparse.ArgumentParser();p.add_argument('--batch',required=True);p.add_argument('--repeat-batch');p.add_argument('--followup-batch',action='append',default=[]);args=p.parse_args()
    folder=BATCH_ROOT/args.batch; batch=json.loads((folder/'batch.json').read_text())
    for followup in args.followup_batch: batch=combine_batches(batch,json.loads((BATCH_ROOT/followup/'batch.json').read_text()))
    repeat=json.loads((BATCH_ROOT/args.repeat_batch/'batch.json').read_text()) if args.repeat_batch else None
    report=make_report(batch,repeat)
    atomic_json(folder/'report.json',report);(folder/'report.md').write_text(markdown(report))
    print(markdown(report));return 0

if __name__=='__main__':sys.exit(main())
