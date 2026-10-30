#!/usr/bin/env python3
"""Connect Myeongdong commercial-area population marginals to synthetic personas."""
from __future__ import annotations

import argparse
import csv
from decimal import Decimal, InvalidOperation
import hashlib
import json
import math
from pathlib import Path
import shutil
import tempfile

import pyarrow as pa
import pyarrow.parquet as pq

ROOT=Path(__file__).resolve().parent.parent
SOURCE=Path('/Users/aiden/Downloads/myeongdong_street.csv')
IMAGE_AGE=Path('/Users/aiden/Downloads/KakaoTalk_Image_2026-10-31-02-35-11_001.png')
IMAGE_TIME=Path('/Users/aiden/Downloads/KakaoTalk_Image_2026-10-31-02-35-11_002.png')
PERSONAS=ROOT/'data/myeongdong-augmented/personas.parquet'
ONTOLOGY=ROOT/'docs/maps/01a_persona_ontology.md'
CONTRACT=ROOT/'docs/maps/05_myeongdong_traffic.md'
OUTPUT=ROOT/'data/myeongdong-traffic'
SEX=['남성','여성']
AGE=['10대','20대','30대','40대','50대','60대이상']
ADULT_AGE=AGE[1:]
BANDS=[('00~06시',0,360),('06~11시',360,660),('11~14시',660,840),
       ('14~17시',840,1020),('17~21시',1020,1260),('21~24시',1260,1440)]
TIME=[x[0] for x in BANDS]
DAYS=['월','화','수','목','금','토','일']
NUMBER_FIELDS=['총유동인구',*SEX,*AGE,*TIME,*DAYS]
COLUMNS=['기준년분기','TRDAR_SE_CD','상권구분','TRDAR_CD','상권명',*NUMBER_FIELDS,'연도','분기','기간']
EXPECTED_PERIODS=[f'{y}{q}' for y in range(2021,2027) for q in range(1,5) if (y,q)<=(2026,2)]
QUARTER_LABELS={1:'1~3월',2:'4~6월',3:'7~9월',4:'10~12월'}


def digest(path: Path) -> str:
    h=hashlib.sha256()
    with path.open('rb') as f:
        for b in iter(lambda:f.read(4*1024*1024),b''):h.update(b)
    return h.hexdigest()


def integer_count(raw: str,field: str) -> int:
    try:
        value=Decimal(raw)
        if not value.is_finite() or value!=value.to_integral_value() or value<0:
            raise ValueError('Expected a nonnegative integer')
        return int(value)
    except (InvalidOperation,TypeError,ValueError) as exc:
        raise ValueError(f'Invalid count {field}: {raw!r}') from exc


def load_quarters(path: Path) -> list[dict]:
    with path.open(encoding='utf-8-sig',newline='') as f:
        reader=csv.DictReader(f)
        if reader.fieldnames!=COLUMNS:raise ValueError('Unexpected or duplicate source columns')
        raw=list(reader)
    if len(raw)!=len(EXPECTED_PERIODS):raise ValueError('Expected 22 source quarters')
    seen=set();output=[]
    for row in raw:
        key=row['기준년분기']
        if key in seen or key not in EXPECTED_PERIODS:raise ValueError(f'Duplicate/unexpected quarter: {key}')
        seen.add(key)
        year=integer_count(row['연도'],'연도');quarter=integer_count(row['분기'],'분기')
        if f'{year}{quarter}'!=key or row['기간']!=QUARTER_LABELS.get(quarter):
            raise ValueError('Quarter code, year and label disagree')
        if (row['TRDAR_SE_CD'],row['상권구분'],row['TRDAR_CD'],row['상권명'])!=('D','발달상권','3120028','명동(명동거리)'):
            raise ValueError('Unexpected area or business district')
        values={field:integer_count(row[field],field) for field in NUMBER_FIELDS}
        if values['총유동인구']<=0:raise ValueError('Zero total population')
        differences={name:sum(values[k] for k in fields)-values['총유동인구']
                     for name,fields in [('sex',SEX),('age',AGE),('time',TIME),('weekday',DAYS)]}
        if any(abs(x)>10 for x in differences.values()):raise ValueError(f'Marginal total discrepancy: {key}, {differences}')
        output.append({'period':key,'year':year,'quarter':quarter,'quarter_label':row['기간'],
                       'area_code':row['TRDAR_CD'],'area_name':row['상권명'],
                       'counts':values,'source_total_difference':differences})
    if seen!=set(EXPECTED_PERIODS):raise ValueError('Source quarter coverage incomplete')
    return sorted(output,key=lambda x:x['period'])


def periods_from_quarters(rows: list[dict]) -> dict[str,dict]:
    periods={r['period']:r for r in rows}
    if len(periods)!=len(rows) or not all(f'2025{q}' in periods for q in range(1,5)):
        raise ValueError('2025 quarter missing or duplicate')
    annual_counts={key:sum(periods[f'2025{q}']['counts'][key] for q in range(1,5)) for key in NUMBER_FIELDS}
    diffs={name:sum(annual_counts[k] for k in fields)-annual_counts['총유동인구']
           for name,fields in [('sex',SEX),('age',AGE),('time',TIME),('weekday',DAYS)]}
    periods['2025_all']={'period':'2025_all','year':2025,'quarter':None,'quarter_label':'2025 네 분기 합산',
                         'area_code':'3120028','area_name':'명동(명동거리)','counts':annual_counts,
                         'source_total_difference':diffs,'component_periods':['20251','20252','20253','20254']}
    return periods


def _shares(values: dict, keys: list[str]) -> dict[str,float]:
    total=sum(values[k] for k in keys)
    if total<=0:raise ValueError(f'Zero marginal total: {keys}')
    return {k:values[k]/total for k in keys}


def build_profiles(periods: dict[str,dict]) -> tuple[dict,list[dict]]:
    reference={'schema_version':'1.0','zone':{'type':'commercial_area','code':'3120028',
               'name':'명동(명동거리)','scope':'상권; 명동 행정동과 동일하다고 가정하지 않음'},
               'source_url':'https://data.seoul.go.kr/dataList/OA-15568/S/1/datasetView.do',
               'default_period':'2025_all','periods':{},
               'semantics':{'population':'서울시·KT 내국인 생활인구 기반 길단위 추정치',
                'joint_distributions':'연령, 성별, 요일, 시간의 주변합만 제공; 교차 분포 미상',
                'time_10min':'각 원본 시간대 안에서 균등하게 나눈 상대 형태; 실제 10분 관측치 아님',
                'source_count_per_hour':'원본 분기 집계치를 해당 구간의 시간 수로 나눈 값; 일평균 고유 방문객·유입률로 해석하지 않음',
                'age19':'10대가 10~19세로 합쳐져 19세 전용 분포 미상',
                'age60plus':'원표 60세 이상은 100세 이상을 포함하고 인물 표본은 60~99세',
                'population_scope':'상권 길단위 생활인구와 서울 거주자 합성 인물은 다른 모집단'}}
    slots=[]
    for key,period in sorted(periods.items()):
        counts=period['counts']
        sex=_shares(counts,SEX);age=_shares(counts,AGE);adult=_shares(counts,ADULT_AGE)
        time=_shares(counts,TIME);days=_shares(counts,DAYS)
        hourly={label:counts[label]/((end-start)/60) for label,start,end in BANDS}
        reference['periods'][key]={**period,'sex_share':sex,'age_share':age,'age20plus_share':adult,
                                   'time_band_share':time,'weekday_share':days,
                                   'source_count_per_hour':hourly}
        for label,start,end in BANDS:
            per_slot=time[label]/((end-start)//10)
            for minute in range(start,end,10):
                slots.append({'period':key,'slot':minute//10,'start_minute':minute,'end_minute':minute+10,
                              'time_band':label,'slot_share':per_slot,'relative_intensity':per_slot*144,
                              'assumption':'uniform_within_time_band'})
    return reference,slots


def age_band(age: int) -> str:
    if not isinstance(age,int) or not 19<=age<=99:raise ValueError(f'Invalid persona age: {age}')
    return '10대' if age==19 else '60대이상' if age>=60 else f'{(age//10)*10}대'


def compute_age_factors(group_counts: dict[str,int], periods: dict[str,dict]) -> dict[str,dict]:
    if any(not isinstance(group_counts.get(k,0),int) or group_counts.get(k,0)<0 for k in ADULT_AGE):
        raise ValueError('Invalid adult age group counts')
    adult_count=sum(group_counts.get(k,0) for k in ADULT_AGE)
    if adult_count<=0:raise ValueError('No supported adult personas')
    result={}
    for period,info in sorted(periods.items()):
        target=_shares(info['counts'],ADULT_AGE)
        factors={}
        for band in ADULT_AGE:
            count=group_counts.get(band,0)
            if count==0 and target[band]>0:
                raise ValueError(f'No persona supply for positive age target: {period} {band}')
            factors[band]=target[band]/(count/adult_count) if count else 0.0
        result[period]={'target_share':target,'relative_factor':factors}
    return result


def persona_weights(personas: Path, periods: dict[str,dict]) -> tuple[pa.Table,dict]:
    pf=pq.ParquetFile(personas);ids=[];ages=[];bands=[]
    for batch in pf.iter_batches(batch_size=4096,columns=['synthetic_id','age']):
        data=batch.to_pydict()
        for uid,age in zip(data['synthetic_id'],data['age']):
            if not isinstance(uid,str) or not uid:raise ValueError('Invalid synthetic ID')
            ids.append(uid);ages.append(age);bands.append(age_band(age))
    if len(ids)!=65000 or len(set(ids))!=len(ids):raise ValueError('Expected 65,000 unique synthetic personas')
    counts={k:bands.count(k) for k in AGE}
    factors=compute_age_factors(counts,periods)
    fields=[pa.field('synthetic_id',pa.string(),nullable=False),pa.field('age',pa.int64(),nullable=False),
            pa.field('age_band',pa.string(),nullable=False),pa.field('weight_status',pa.string(),nullable=False)]
    columns=[pa.array(ids,type=pa.string()),pa.array(ages,type=pa.int64()),pa.array(bands,type=pa.string()),
             pa.array(['age19_unresolved' if b=='10대' else 'adult20plus_relative_only' for b in bands],type=pa.string())]
    for period in sorted(factors):
        fields.append(pa.field('weight_'+period,pa.float64(),nullable=True))
        columns.append(pa.array([None if b=='10대' else factors[period]['relative_factor'][b] for b in bands],type=pa.float64()))
    return pa.Table.from_arrays(columns,schema=pa.schema(fields)),{'age_group_counts':counts,'period_factors':factors}


def validate_delivery(output: Path, source: Path, personas: Path) -> dict:
    periods=periods_from_quarters(load_quarters(source))
    expected_ref,expected_slots=build_profiles(periods)
    saved_ref=json.loads((output/'traffic_reference.json').read_text(encoding='utf8'))
    if saved_ref!=expected_ref:raise ValueError('Reference numbers or assumptions changed on write')
    ts=pq.read_table(output/'time_profile_10min.parquet').to_pylist()
    if ts!=expected_slots or len(ts)!=len(periods)*144:raise ValueError('10-minute profile does not round-trip')
    for period in periods:
        data=[r for r in ts if r['period']==period]
        if len(data)!=144 or not math.isclose(math.fsum(r['slot_share'] for r in data),1,abs_tol=1e-12):
            raise ValueError(f'10-minute profile is incomplete: {period}')
        for band in TIME:
            expected=expected_ref['periods'][period]['time_band_share'][band]
            actual=math.fsum(r['slot_share'] for r in data if r['time_band']==band)
            if not math.isclose(actual,expected,abs_tol=1e-12):raise ValueError('Time-band shares do not reconcile')
    original=pq.read_table(personas,columns=['synthetic_id','age']).to_pydict()
    table=pq.read_table(output/'persona_traffic_weights.parquet'); d=table.to_pydict()
    if d['synthetic_id']!=original['synthetic_id'] or d['age']!=original['age'] or len(set(d['synthetic_id']))!=65000:
        raise ValueError('Persona ID or age does not match the original')
    bands=[age_band(a) for a in original['age']]
    if bands!=d['age_band']:raise ValueError('Persona age band mismatch')
    counts={b:bands.count(b) for b in AGE};adults=sum(counts[b] for b in ADULT_AGE)
    factors=compute_age_factors(counts,periods)
    stats={}
    for period in sorted(periods):
        values=d['weight_'+period]
        expected=factors[period]
        for i,band in enumerate(bands):
            if band=='10대':
                if values[i] is not None or d['weight_status'][i]!='age19_unresolved':
                    raise ValueError('19-year-old weight must be unknown')
            elif (d['weight_status'][i]!='adult20plus_relative_only' or values[i] is None or
                  not math.isclose(values[i],expected['relative_factor'][band],rel_tol=1e-12)):
                raise ValueError('Adult relative weight mismatch')
        sums={band:math.fsum(v for v,b in zip(values,bands) if b==band) for band in ADULT_AGE}
        allsum=math.fsum(sums.values())
        if not math.isclose(allsum,adults,abs_tol=1e-7):raise ValueError('Adult weights do not sum to supported population')
        actual={b:sums[b]/allsum for b in ADULT_AGE}
        if any(not math.isclose(actual[b],expected['target_share'][b],abs_tol=1e-12) for b in ADULT_AGE):
            raise ValueError('Adult weighted age distribution differs from reference')
        stats[period]={'target_age20plus_share':expected['target_share'],'weighted_age20plus_share':actual,
                       'relative_age_factors':expected['relative_factor'],'weighted_total':allsum}
    if digest(output/'sources'/'myeongdong_street.csv')!=digest(source):raise ValueError('Source CSV copy changed')
    return {'verified':True,'quarter_count':len(periods)-1,'period_count':len(periods),'slots_per_period':144,
            'slot_count':len(ts),'personas':len(original['synthetic_id']),'adult20plus':adults,
            'age19_unresolved':counts['10대'],'age_group_counts':counts,'period_checks':stats,
            'source_total_differences':{k:v['source_total_difference'] for k,v in periods.items()}}


def build_delivery(source: Path, image_age: Path, image_time: Path, personas: Path,
                   ontology: Path, contract: Path, output: Path) -> dict:
    if output.exists():raise FileExistsError(f'Refusing to overwrite {output}')
    quarters=load_quarters(source);periods=periods_from_quarters(quarters)
    reference,slots=build_profiles(periods)
    weights,_=persona_weights(personas,periods)
    original_hash=digest(personas)
    output.parent.mkdir(parents=True,exist_ok=True)
    stage=Path(tempfile.mkdtemp(prefix='.myeongdong-traffic-',dir=output.parent))
    try:
        source_dir=stage/'sources';source_dir.mkdir()
        for src,name in [(source,'myeongdong_street.csv'),(image_age,'age_chart_2025.png'),
                         (image_time,'time_chart_2025.png'),(ontology,'ontology_snapshot.md'),
                         (contract,'ontology_connection_snapshot.md')]:
            shutil.copyfile(src,source_dir/name)
            if digest(src)!=digest(source_dir/name):raise ValueError(f'Source copy changed: {src}')
        (stage/'traffic_reference.json').write_text(json.dumps(reference,ensure_ascii=False,indent=2)+'\n',encoding='utf8')
        schema=pa.schema([pa.field('period',pa.string(),nullable=False),pa.field('slot',pa.int16(),nullable=False),
                          pa.field('start_minute',pa.int16(),nullable=False),pa.field('end_minute',pa.int16(),nullable=False),
                          pa.field('time_band',pa.string(),nullable=False),pa.field('slot_share',pa.float64(),nullable=False),
                          pa.field('relative_intensity',pa.float64(),nullable=False),pa.field('assumption',pa.string(),nullable=False)])
        pq.write_table(pa.Table.from_pylist(slots,schema=schema),stage/'time_profile_10min.parquet',compression='snappy')
        pq.write_table(weights,stage/'persona_traffic_weights.parquet',compression='snappy')
        check=validate_delivery(stage,source,personas)
        if digest(personas)!=original_hash:raise ValueError('Persona input changed while building')
        inputs={'source_csv':digest(source),'age_image':digest(image_age),'time_image':digest(image_time),
                'personas_parquet':original_hash,'ontology_snapshot':digest(source_dir/'ontology_snapshot.md'),
                'ontology_link_contract':digest(source_dir/'ontology_connection_snapshot.md'),'generator_script':digest(Path(__file__))}
        outputs={name:digest(stage/name) for name in ('traffic_reference.json','time_profile_10min.parquet','persona_traffic_weights.parquet')}
        report={'status':'verified','input_sha256':inputs,'output_sha256':outputs,'validation':check,
                'limitations':list(reference['semantics'].values()),
                'methodology':{'time':'uniform within each reported time band; slot shares sum to 1',
                 'age_weight':'source 20+ age share divided by synthetic 20+ age share',
                 'weight_unit':'dimensionless relative factor; not a probability, arrival count, or density',
                 'cross_distributions':'not provided or inferred',
                 'source':'https://data.seoul.go.kr/dataList/OA-15568/S/1/datasetView.do'}}
        (stage/'validation.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n',encoding='utf8')
        ref=reference['periods']['2025_all']; percentages=', '.join(f"{k} {100*v:.1f}%" for k,v in ref['age_share'].items())
        readme=f'''# 명동 상권 유동인구의 시뮬레이션 참고 자료

사용자가 제공한 서울시 상권분석서비스 명동(명동거리) 상권(D/3120028)의 길단위 생활인구 CSV와 2025년 그래프를 반영했다. CSV는 2021년 1분기~2026년 2분기 22개 분기를 포함한다. `2025_all`은 첨부 그래프와 같이 2025년 네 분기의 같은 항목을 합산한 비교용 기간이다.

이 값은 서울시·KT의 내국인 생활인구를 기반으로 만든 길단위 **추정치**다. 상권 전체 유동인구, 서울 거주자, 고유 방문객, 새로 도착한 사람, 명동 행정동 인구를 같은 집단으로 취급하지 않는다. [서울시 원표 설명](https://data.seoul.go.kr/dataList/OA-15568/S/1/datasetView.do), [서울시 자료 정의](https://golmokxpert.seoul.go.kr/main.do). 2024년부터 공간 단위가 변경되었으므로 시계열 해석 시 주의한다.

## 파일

- `traffic_reference.json`: 원본 22개 분기와 2025 합산의 원표 수치, 성별·연령별·요일별·시간대별 **개별 주변분포** 및 원본 합계와의 차이.
- `time_profile_10min.parquet`: 23기간 × 144개 슬롯. 원본 시간대 안에 10분씩 균등 배분한 `slot_share`와 하루 평균이 1인 `relative_intensity`.
- `persona_traffic_weights.parquet`: 기존 합성 인물 {check['personas']:,}명의 `synthetic_id`와 연령 그룹, 기간별 연령 보조 상대 가중치 `weight_<기간>`.
- `validation.json`: 원본·코드·결과 해시, 그룹별 목표·가중합·오차, 10분 슬롯과 ID 검증 결과.
- `sources/`: 전달받은 CSV와 두 이미지, 적용 시점의 온톨로지 초안 및 연결 계약 스냅샷.

## 계산과 적용 범위

첨부 이미지의 2025 연령 비율은 원표의 네 분기를 합산하면 {percentages}다. 각 시간대 그래프 값은 해당 원표 값을 시간대 길이로 나눈 값이다. 이를 매일의 시간당 고유 방문자 수로 확정하지 않았다.

시간 가중치 `slot_share`는 각 기간의 시간대 수를 시간대 합으로 나눈 뒤 구간의 10분 슬롯 수로 다시 나눈 값이다. `relative_intensity = slot_share × 144`다. 시간대 안에서 균등하다는 가정이며 실제 10분별 측정값이나 도착률이 아니다. 요일별 값도 별도의 주변분포일 뿐 요일×시간 교차표는 없다.

연령 가중치는 `상권의 20대 이상 연령 비율 ÷ 합성 인물의 20대 이상 연령 비율`이다. 전체 20세 이상 표본에 적용한 가중합은 20대 이상 상권 연령 분포와 같아진다. 기존 거주 인구를 재생성하거나 바꾸지는 않았다. 19세 {check['age19_unresolved']:,}명은 원표의 10~19세를 더 나눌 수 없어 모든 기간 가중치를 null로 둔다. 60대 이상 원표는 100세 이상을 포함하지만 합성 인물은 99세까지여서 근사다.

이 가중치는 **개인의 방문확률이나 실제 대표 인원 수가 아니다**. 20세 이상 전체 표본에 대한 상대 보정이며, 인물이 실제로 방문을 선택하고 체류한 뒤의 연령 분포는 별도로 검증해야 한다. 상권에는 서울 외 거주자도 포함될 수 있으나 합성 인물은 서울 거주자이고 외국인은 포함하지 않는다. 성별×연령·연령×시각·요일×시각은 제공되지 않아 생성하지 않았다.

병행 온톨로지 R-09의 상대 관측 참고와 R-12의 주변분포 비교에 연결할 수 있다. R-09의 도착률은 체류시간·인구 집계 방식이 필요하고, R-10의 `D-CROWD`는 인/㎡이므로 절대 인원과 보행 유효 면적이 확보되기 전에는 이 자료의 상대 지수를 대입할 수 없다. [연결 계약](../../docs/maps/05_myeongdong_traffic.md)을 참조한다. 개인별 활동 스냅샷은 이 자료에서 생성하지 않았다.

## 기존 데이터에 결합하기

```python
import pyarrow.parquet as pq
people = pq.read_table('data/myeongdong-augmented/personas.parquet',
                       columns=['synthetic_id', 'age', 'district'])
weights = pq.read_table('data/myeongdong-traffic/persona_traffic_weights.parquet',
                        columns=['synthetic_id', 'weight_2025_all'])
print(people.join(weights, keys='synthetic_id', join_type='left outer').slice(0, 5))
```

다른 빈 경로에 다시 생성하려면 `python3 -m scripts.prepare_myeongdong_traffic --output /tmp/myeongdong-traffic-replay`를 실행한다. 기본 입력 경로는 명령의 `--help`에서 확인할 수 있다. 기존 산출물은 덮어쓰지 않는다.
'''
        (stage/'README.md').write_text(readme,encoding='utf8')
        if output.exists():raise FileExistsError(f'Output appeared during build: {output}')
        stage.rename(output)
        return report
    except BaseException:
        shutil.rmtree(stage,ignore_errors=True)
        raise


def main() -> None:
    ap=argparse.ArgumentParser()
    ap.add_argument('--source',type=Path,default=SOURCE)
    ap.add_argument('--image-age',type=Path,default=IMAGE_AGE)
    ap.add_argument('--image-time',type=Path,default=IMAGE_TIME)
    ap.add_argument('--personas',type=Path,default=PERSONAS)
    ap.add_argument('--ontology',type=Path,default=ONTOLOGY)
    ap.add_argument('--contract',type=Path,default=CONTRACT)
    ap.add_argument('--output',type=Path,default=OUTPUT)
    args=ap.parse_args()
    result=build_delivery(args.source,args.image_age,args.image_time,args.personas,args.ontology,args.contract,args.output)
    print(f"Verified {result['validation']['personas']} persona weights and {result['validation']['slot_count']} time slots: {args.output}")


if __name__=='__main__':main()
