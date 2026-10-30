"""Run traceable, paired 5,044-person experiments on one frozen Rust binary."""
from __future__ import annotations
import argparse
from collections import Counter, defaultdict
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import uuid
from typing import Any

import pyarrow.parquet as pq
import yaml

from myeongdong_scenarios import DEFAULTS, prepare_input, sha256, validate_options
from myeongdong_ui_server import atomic_json, summarize_run
from prepare_myeongdong_5044 import SPEC

ROOT = Path(__file__).resolve().parent.parent
BASE = ROOT / 'sim/runs/myeongdong_5044_input.HRjAMr'
REFERENCE_ID = 'ui-7cde261c09d74bf684d740730fedcf5f'
REFERENCE = ROOT / 'sim/runs/ui_jobs' / REFERENCE_ID
JOB_ROOT = ROOT / 'sim/runs/ui_jobs'
OUTPUT_ROOT = ROOT / 'apps/myeongdong-ui/public/runs'
BATCH_ROOT = ROOT / 'output/myeongdong-hypotheses'
GUARD = Path('/Users/aiden/memguard/bin/memguard')
CLI = ROOT / 'sim/target/release/mdsim'
FOCUS_BUILDING = 'way/273025568'
SEEDS = (9599481731617930288, 9599481731617930289)
CASES: dict[str, dict[str, Any]] = {
    'baseline': {'label':'기준', 'change':{}, 'metric':None},
    'discount': {'label':'할인 20%', 'change':{'discount_pct':20}, 'metric':'purchases', 'hypothesis':'할인하면 구매가 늘어난다'},
    'visibility': {'label':'시계성·옥외 광고 높음', 'change':{'visibility_level':'high'}, 'metric':'notices', 'hypothesis':'시계성을 높이면 장소 인지가 늘어난다'},
    'pedestrian': {'label':'보행 전용 8시간', 'change':{'pedestrian_hours':8}, 'metric':'effective_area_m2', 'hypothesis':'보행 전용으로 유효 보행 면적이 늘어난다'},
    'bench': {'label':'벤치 20개', 'change':{'bench_count':20}, 'metric':'rest_starts', 'hypothesis':'벤치를 설치하면 휴식 시작이 늘어난다'},
    'width': {'label':'보행 폭 +1m', 'change':{'width_delta_m':1}, 'metric':'effective_area_m2', 'hypothesis':'보행 폭을 늘리면 유효 보행 면적이 늘어난다'},
    'building': {'label':'밀레오레 명동 대리 업종을 카페로', 'change':{'building_overrides':{FOCUS_BUILDING:{'type':'cafe'}}}, 'metric':'building_enters', 'hypothesis':'건물 대리 업종 변경 시 해당 장소 입장이 달라진다'},
    'income': {'label':'방문객 소득 110%', 'change':{'income_multiplier':1.1}, 'metric':'purchases', 'hypothesis':'소득을 높이면 구매가 늘어난다'},
}


def options_for(case: str) -> dict[str, Any]:
    if case not in CASES: raise ValueError('알 수 없는 실험 항목')
    return validate_options({**copy.deepcopy(DEFAULTS), **copy.deepcopy(CASES[case]['change'])})


def changed_keys(a: dict[str, Any], b: dict[str, Any]) -> list[str]:
    return sorted(k for k in set(a) | set(b) if a.get(k) != b.get(k))


def difference(a: float, b: float) -> dict[str, float | None]:
    delta = b-a
    return {'before':a, 'after':b, 'delta':delta, 'pct':None if a == 0 else 100.0*delta/a}


def update_seed(prepared: Path, seed: int) -> None:
    if not 0 <= seed < 2**64: raise ValueError('시드가 u64 범위를 벗어났습니다')
    sp=prepared/'bundle/scenario.yaml'; mp=prepared/'bundle/manifest.json'
    scenario=yaml.safe_load(sp.read_text()); scenario['seed']=seed
    sp.write_text(yaml.safe_dump(scenario, allow_unicode=True, sort_keys=False))
    manifest=json.loads(mp.read_text())
    if len(manifest['scenarios']) != 1: raise ValueError('시나리오 수가 하나가 아닙니다')
    manifest['scenarios'][0]['seed']=seed
    manifest['scenarios'][0]['sha256']=sha256(sp)
    manifest['bundle_id']=hashlib.sha256((manifest['bundle_id']+':seed:'+str(seed)+':'+sha256(sp)).encode()).hexdigest()[:16]
    atomic_json(mp,manifest)


def restore_income_110(base: Path, prepared: Path, reference: Path = REFERENCE) -> None:
    """Reuse the verified 110% profile when the original specification file changed."""
    prior=reference/'input'; old=json.loads((prior/'source.json').read_text()); original=json.loads((base/'source.json').read_text())
    key=str(SPEC.relative_to(ROOT))
    if old['run_options']['income_multiplier'] != 1.1 or old['source_sha256'] != original['source_sha256']:
        raise ValueError('기존 소득 입력의 출처 또는 110% 설정이 일치하지 않습니다')
    manifest=json.loads((prepared/'bundle/manifest.json').read_text()); old_manifest=json.loads((prior/'bundle/manifest.json').read_text())
    baseline_ids=set(pq.read_table(base/'input/profiles.parquet',columns=['persona_id']).column(0).to_pylist())
    old_ids=set(pq.read_table(prior/'input/profiles.parquet',columns=['persona_id']).column(0).to_pylist())
    if len(baseline_ids)!=5044 or old_ids!=baseline_ids: raise ValueError('기존 소득 입력의 페르소나가 다릅니다')
    for role in ('profiles','profile_attrs'):
        old_entry=old_manifest['files'][role][0]; new_entry=manifest['files'][role][0]
        old_path=prior/old_entry['path']; new_path=prepared/new_entry['path']
        if sha256(old_path)!=old_entry['sha256']: raise ValueError(f'기존 {role} 해시가 다릅니다')
        shutil.copyfile(old_path,new_path)
        new_entry['sha256']=sha256(new_path); new_entry['rows']=pq.read_metadata(new_path).num_rows
    sp=prepared/'bundle/scenario.yaml'; scenario=yaml.safe_load(sp.read_text())
    scenario['static_applied']['v12_income_multiplier']=1.1
    sp.write_text(yaml.safe_dump(scenario,allow_unicode=True,sort_keys=False))
    manifest['scenarios'][0]['sha256']=sha256(sp)
    manifest['bundle_id']=hashlib.sha256((manifest['bundle_id']+':income110:'+manifest['files']['profiles'][0]['sha256']).encode()).hexdigest()[:16]
    atomic_json(prepared/'bundle/manifest.json',manifest)
    source=json.loads((prepared/'source.json').read_text()); source['run_options']['income_multiplier']=1.1
    source['income_profile_source']={'run_id':reference.name,'spec_sha256':old['source_sha256'][key],
        'profiles_sha256':manifest['files']['profiles'][0]['sha256'],'profile_attrs_sha256':manifest['files']['profile_attrs'][0]['sha256']}
    atomic_json(prepared/'source.json',source)


def run_summary(out: Path, prepared: Path) -> dict[str, Any]:
    result=summarize_run(out,prepared)
    events=pq.read_table(out/'events.parquet',columns=['kind']).column(0).to_pylist()
    event_counts=Counter(events)
    rows=pq.read_table(out/'metrics_10min.parquet',columns=['frame_start_min','metric','key','value']).to_pylist()
    areas=defaultdict(float)
    for r in rows:
        if r['metric']=='edge_effective_area_m2' and r['key']: areas[r['frame_start_min']]+=r['value']
    if len(areas)!=18: raise ValueError('유효 면적 10분 구간 수가 18개가 아닙니다')
    for metric in ('notices',):
        recorded=sum(r['value'] for r in rows if r['metric']==metric and not r['key'])
        if recorded != event_counts['notice']: raise ValueError('인지 사건과 10분 지표가 다릅니다')
    focused=result['buildings'].get(FOCUS_BUILDING)
    if focused is None: raise ValueError('비교할 건물 장소가 없습니다')
    people=pq.read_table(out/'agents.parquet',columns=['body_id','persona_id']).to_pylist()
    expected=set(pq.read_table(prepared/'input/profiles.parquet',columns=['persona_id']).column(0).to_pylist())
    if len(people)!=5044 or len({x['body_id'] for x in people})!=5044 or {x['persona_id'] for x in people}!=expected:
        raise ValueError('엔진 인원 5,044명의 고유 ID가 입력과 일치하지 않습니다')
    return {**result['totals'], 'notices':event_counts['notice'], 'rest_starts':event_counts['rest_start'],
            'effective_area_m2':sum(areas.values())/len(areas), 'building_enters':focused['enters'],
            'building_purchases':focused['purchases'], 'building_revenue_krw':focused['revenue_krw']}


def execute(batch: dict[str, Any], batch_path: Path, case: str, seed_idx: int) -> None:
    key=f'{seed_idx}:{case}'
    if batch['runs'].get(key,{}).get('state')=='complete':
        print(f'SKIP {key}: already complete',flush=True); return
    options=options_for(case); initial=options_for('baseline')
    if case!='baseline' and len(changed_keys(initial,options))!=1: raise ValueError('기준과 하나의 UI 설정만 달라야 합니다')
    ref_job=json.loads((REFERENCE/'job.json').read_text()); engine=ref_job['engine_build']
    runner=REFERENCE/'engine.bin'
    if sha256(runner)!=engine['binary_sha256'] or engine!=batch['engine']:
        raise ValueError('고정 엔진 바이너리가 변경됐습니다')
    ident='ui-'+uuid.uuid4().hex; jobdir=JOB_ROOT/ident; out=OUTPUT_ROOT/ident
    jobdir.mkdir(parents=True); out.mkdir(parents=True)
    job={'run_id':ident,'state':'preparing','replay_base':f'/runs/{ident}','options':options,
         'planned_bodies':5044,'error':None,'engine_build':engine,'experiment':{'batch_id':batch['batch_id'],'case':case,'seed_index':seed_idx}}
    atomic_json(jobdir/'job.json',job)
    batch['runs'][key]={'run_id':ident,'state':'preparing','options':options,'seed':SEEDS[seed_idx]}
    atomic_json(batch_path,batch)
    try:
        fallback=case=='income' and sha256(SPEC)!=json.loads((BASE/'source.json').read_text())['source_sha256'][str(SPEC.relative_to(ROOT))]
        prepare_opts={**options,'income_multiplier':1.0} if fallback else options
        prepared=prepare_input(BASE,jobdir/'input',prepare_opts)
        if fallback: restore_income_110(BASE,prepared)
        update_seed(prepared,SEEDS[seed_idx])
        source=json.loads((prepared/'source.json').read_text()); source['engine_build']=engine
        source['experiment']={'batch_id':batch['batch_id'],'case':case,'seed':SEEDS[seed_idx],'reference_binary_job':REFERENCE_ID}
        atomic_json(prepared/'source.json',source)
        isolated=jobdir/'engine.bin'; shutil.copyfile(runner,isolated); isolated.chmod(0o755)
        if sha256(isolated)!=engine['binary_sha256']: raise ValueError('실행 파일 복사 해시가 다릅니다')
        atomic_json(jobdir/'engine_build.json',engine)
        job['state']='queued'; atomic_json(jobdir/'job.json',job)
        print(f'START {key} {ident}',flush=True)
        command=[str(GUARD),'run','--label',f'myeongdong-exp:{batch["batch_id"]}:{key}', '--priority','low','--mem-gb','3','--',str(isolated),str(prepared),str(prepared/'defs'),str(prepared/'bundle'),str(prepared/'bundle/scenario.yaml'),str(out),'5044']
        env=os.environ.copy();env['RAYON_NUM_THREADS']='4'
        with (jobdir/'engine.log').open('wb') as log:
            process=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,env=env)
        if process.returncode: raise RuntimeError((jobdir/'engine.log').read_text(errors='replace')[-1800:])
        status=json.loads((out/'run_status.json').read_text())
        if status['status']!='complete' or status['spawned_bodies']!=5044 or len(status['frames'])!=18: raise ValueError('엔진 완료 기록이 올바르지 않습니다')
        verified=subprocess.run([str(CLI),'verify','--run',str(out)],text=True,capture_output=True)
        if verified.returncode: raise ValueError('실행 해시 검증 실패: '+verified.stderr[-800:])
        summary=run_summary(out,prepared)
        job['state']='complete';atomic_json(jobdir/'job.json',job)
        batch['runs'][key].update(state='complete',summary=summary,scenario_sha256=sha256(prepared/'bundle/scenario.yaml'))
        print(f'DONE {key} {json.dumps(summary,ensure_ascii=False)}',flush=True)
    except Exception as exc:
        job['state']='failed';job['error']=str(exc);atomic_json(jobdir/'job.json',job)
        batch['runs'][key].update(state='failed',error=str(exc))
        print(f'FAILED {key}: {exc}',file=sys.stderr,flush=True)
    finally:
        atomic_json(batch_path,batch)


def main() -> int:
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--batch',required=True)
    parser.add_argument('--cases',required=True,help='comma-separated case IDs')
    parser.add_argument('--seed-index',type=int,choices=(0,1),default=0)
    args=parser.parse_args()
    if not re.fullmatch(r'[a-z0-9][a-z0-9_-]{0,55}',args.batch): parser.error('invalid batch ID')
    cases=args.cases.split(',')
    if len(cases)!=len(set(cases)) or any(c not in CASES for c in cases): parser.error('invalid or duplicate cases')
    folder=BATCH_ROOT/args.batch;folder.mkdir(parents=True,exist_ok=True);path=folder/'batch.json'
    if path.exists():
        batch=json.loads(path.read_text())
    else:
        reference=json.loads((REFERENCE/'job.json').read_text())
        batch={'schema_version':1,'batch_id':args.batch,'reference_job':REFERENCE_ID,'engine':reference['engine_build'],
               'base_source_sha256':sha256(BASE/'source.json'),'seeds':list(SEEDS),'cases':CASES,'runs':{}}
        atomic_json(path,batch)
    if batch['base_source_sha256']!=sha256(BASE/'source.json') or batch['seeds']!=list(SEEDS): raise ValueError('배치 기준 입력이 바뀌었습니다')
    for case in cases: execute(batch,path,case,args.seed_index)
    return 0 if all(batch['runs'].get(f'{args.seed_index}:{c}',{}).get('state')=='complete' for c in cases) else 1

if __name__=='__main__': sys.exit(main())
