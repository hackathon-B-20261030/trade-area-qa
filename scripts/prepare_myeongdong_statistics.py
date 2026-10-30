#!/usr/bin/env python3
"""Normalize the public Seoul statistics used by the Myeongdong sample."""
from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import zipfile
from decimal import Decimal, InvalidOperation
from pathlib import Path

from lxml import html


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open('rb') as f:
        for chunk in iter(lambda: f.read(4 * 1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def parse_html_rows(path: Path, width: int) -> list[list[str]]:
    root = html.parse(str(path))
    rows = []
    for tr in root.xpath('//tr[td]'):
        cells = tr.xpath('./td')
        if len(cells) == width:
            rows.append([x.text_content().strip().replace('\n', ' ').strip() for x in cells])
    if not rows:
        raise ValueError(f'{path}: no {width}-column data rows')
    return rows


def numbers(cells: list[str]) -> list[int]:
    try:
        decimals = [Decimal(c.replace(',', '')) for c in cells]
        if any(not d.is_finite() or d != d.to_integral_value() for d in decimals):
            raise ValueError('Non-integral count')
        out = [int(d) for d in decimals]
    except (InvalidOperation, ValueError) as exc:
        raise ValueError('Non-numeric or suppressed count in a required cell') from exc
    if min(out, default=0) < 0:
        raise ValueError('Negative count')
    return out


def parse_gu(rows: list[list[str]], prefix: bool) -> tuple[dict[str, list[int]], list[int]]:
    city = None
    districts = {}
    for row in rows:
        name = row[1]
        if name == '서울특별시':
            if city is not None:
                raise ValueError('Duplicate city total')
            city = numbers(row[2:])
        elif (prefix and name.startswith('서울특별시 ') and name.endswith('구')) or (not prefix and name.endswith('구')):
            name = name.removeprefix('서울특별시 ')
            if name in districts:
                raise ValueError(f'Duplicate district {name}')
            districts[name] = numbers(row[2:])
    if len(districts) != 25 or city is None:
        raise ValueError('Expected Seoul total and 25 districts')
    if len(city) % 3:
        raise ValueError('Expected total, male and female column blocks')
    width=len(city)//3
    for name,values in [('서울특별시',city),*districts.items()]:
        if any(values[j]!=values[width+j]+values[2*width+j] for j in range(width)):
            raise ValueError(f'Sex counts do not sum to total: {name}')
    for j, total in enumerate(city):
        if sum(v[j] for v in districts.values()) != total:
            raise ValueError(f'Districts do not sum to Seoul total: column {j}')
    return districts, city


def parse_marriage(path: Path) -> tuple[list[dict],list[dict],list[dict]]:
    with zipfile.ZipFile(path) as z:
        files = [x for x in z.namelist() if x.endswith('.csv')]
        if len(files) != 1:
            raise ValueError('Expected one CSV in marriage source')
        lines = z.read(files[0]).decode('cp949').splitlines()
    if len(lines) < 4 or '혼인상태별' not in lines[0]:
        raise ValueError('Unexpected marriage CSV title')
    reader = csv.DictReader(io.StringIO('\n'.join(lines[2:])))
    result = []; districts = []; suppressed = []
    for row in reader:
        name=row['행정구역별(시군구)'].strip()
        code=row['C행정구역별(시군구)'].lstrip("'")
        is_city=name == '서울특별시'
        is_district=code.startswith('11') and len(code)==5 and name.endswith('구')
        if (not is_city and not is_district) or row['연령별'] in ('합계', '20세미만'):
            continue
        label = row['연령별']
        lower = 85 if label == '85세이상' else int(label.split('~')[0])
        for sex in ('남자', '여자'):
            raw=[row[f'{sex}_계'], row[f'{sex}_미혼'], row[f'{sex}_유배우'], row[f'{sex}_사별·이혼']]
            if is_district and 'X' in raw:
                suppressed.append(dict(district=name,sex=sex,age_lower=lower))
                continue
            vals = numbers(raw)
            if sum(vals[1:]) != vals[0]:
                raise ValueError(f'Marriage counts do not sum: {name}, {sex}, {label}')
            entry=dict(sex=sex, age_lower=lower, total=vals[0], never=vals[1], married=vals[2], divorced_widowed=vals[3])
            if is_city:result.append(entry)
            else:districts.append(dict(district=name,**entry))
    if {(r['sex'], r['age_lower']) for r in result} != {(sex, age) for sex in ('남자','여자') for age in range(20,90,5)}:
        raise ValueError('Missing or duplicate Seoul marriage age groups')
    if len(result) != 28:
        raise ValueError('Duplicate marriage groups')
    def order(r):return (r['district'],r['sex'],r['age_lower'])
    if len(districts)+len(suppressed)!=700 or len({order(r) for r in districts+suppressed})!=700:
        raise ValueError('Missing or duplicate district marriage groups')
    return sorted(result, key=lambda r: (r['sex'], r['age_lower'])),sorted(districts,key=order),sorted(suppressed,key=order)


def make_statistics(age5: Path, age19: Path, single: Path, dong: Path, marriage_zip: Path) -> dict:
    p5, city5 = parse_gu(parse_html_rows(age5, 71), False)
    p19, city19 = parse_gu(parse_html_rows(age19, 11), False)
    ones, city1 = parse_gu(parse_html_rows(single, 311), True)
    if set(p5) != set(p19) or set(p5) != set(ones):
        raise ValueError('District lists disagree')
    population = []
    for gu in sorted(p5):
        for sex, g in (('남자', 1), ('여자', 2)):
            for lower in [19, *range(20, 100, 5)]:
                upper = 20 if lower == 19 else lower + 5
                count = p19[gu][g*3+1] if lower == 19 else p5[gu][g*23+2+lower//5]
                single_count = sum(ones[gu][g*103+2+a] for a in range(lower,upper))
                if single_count > count:
                    raise ValueError(f'Single registered households exceed population {gu} {sex} {lower}')
                population.append(dict(district=gu, sex=sex, age_lower=lower, population=count, single_registered=single_count))
    if len(population) != 850:
        raise ValueError('Unexpected number of cells')
    expected_pop = sum(city19[g*3+1] + sum(city5[g*23+2+a//5] for a in range(20,100,5)) for g in (1,2))
    expected_one = sum(sum(city1[g*103+2+a] for a in range(19,100)) for g in (1,2))
    if sum(r['population'] for r in population) != expected_pop or sum(r['single_registered'] for r in population) != expected_one:
        raise ValueError('Adult city totals do not match')
    dong_rows = [r for r in parse_html_rows(dong,71) if r[:2] == ['1114055000', '명동']]
    if len(dong_rows) != 1:
        raise ValueError('Myeong-dong district row missing')
    dong_values = numbers(dong_rows[0][2:])
    query = dict(tableChart='T', sltOrgType='2', sltOrgLvl1='1100000000', sltOrgLvl2='A', sltUndefType='Y', searchYearMonth='month', searchYearStart='2026', searchMonthStart='09', searchYearEnd='2026', searchMonthEnd='09', sum='sum', gender='gender', sltArgTypes='5', sltArgTypeA='0', sltArgTypeB='100', sltOrderType='1', sltOrderValue='ASC')
    q19 = dict(query, sltArgTypes='1', sltArgTypeA='19', sltArgTypeB='19')
    q1 = dict(query, sltArgTypes='1', sttsGbn='admm')
    qdong = dict(query, sltOrgLvl2='1114000000')
    marriage,district_marriage,suppressed=parse_marriage(marriage_zip)
    return {
        'schema_version':'1.0', 'reference_date':'2026-09-30',
        'population':population, 'marriage':marriage, 'district_marriage':district_marriage,
        'suppressed_district_marriage':suppressed,
        'myeongdong_context': {'code':'1114055000','name':'명동','all_ages_total':dong_values[0], 'all_ages_male':dong_values[23], 'all_ages_female':dong_values[46], 'age_5_year_raw_total_male_female':dong_values, 'interpretation':'2026-09 resident population in administrative dong; not Myeongdong visitors'},
        'sources':[
            {'id':'mois_population_age5','url':'https://jumin.mois.go.kr/ageStatMonth.do','period':'2026-09','query':query,'raw_response_sha256':sha256(age5),'meaning':'서울 구별 거주자, 외국인 제외, 5세 연령별'},
            {'id':'mois_population_age19','url':'https://jumin.mois.go.kr/ageStatMonth.do','period':'2026-09','query':q19,'raw_response_sha256':sha256(age19),'meaning':'서울 구별 거주자, 외국인 제외, 만 19세'},
            {'id':'mois_single_registered_households','url':'https://jumin.mois.go.kr/sexdAge1Hshd.do?sttsGbn=admm','period':'2026-09','query':q1,'raw_response_sha256':sha256(single),'meaning':'서울 구별 거주자의 성·1세별 주민등록 1인세대; 실제 1인가구의 대리 지표'},
            {'id':'mois_myeongdong','url':'https://jumin.mois.go.kr/ageStatMonth.do','period':'2026-09','query':qdong,'raw_response_sha256':sha256(dong),'meaning':'중구 행정동 명동 거주자의 5세별 인구; 방문자 통계 아님'},
            {'id':'kosis_marital_status','url':'https://kosis.kr/statisticsList/mass/mass_list.jsp?list_id=&org_id=101&process=statHtml&tbl_id=DT_1MR2060&vw_cd=','period':'2025','table':'DT_1MR2060','download_file':'101_DT_1MR2060_Y_2025.zip','raw_response_sha256':sha256(marriage_zip),'meaning':'인구총조사 내국인 성·연령·혼인상태; 85세 이상은 100세 이상을 포함'},
        ],
        'limitations':['MOIS 거주자 19~99세는 외국인, 거주불명자, 재외국민, 100세 이상을 제외한다.', '주민등록 1인세대는 인구총조사의 1인가구 또는 실제 혼자 사는 사람과 동일하지 않다.', '혼인 기준은 2025년 인구총조사 서울 내국인이며 2026년 주민등록인구와 시점·정의가 다르다.', '19세 혼인 상태는 18~19세 결합 및 소수 셀 비공개로 보정하지 않는다.', '구·성·연령 혼인 셀의 공개되지 않은 값은 사용하거나 추정하지 않는다.', '85세 이상 혼인 비율에는 100세 이상이 포함되므로 85~99세의 근사값이다.'],
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    for name in ('age5','age19','single','dong','marriage-zip','output'):
        ap.add_argument('--'+name, required=True, type=Path)
    args = ap.parse_args()
    if args.output.exists():
        raise FileExistsError(f'Refusing to overwrite {args.output}')
    stats = make_statistics(args.age5,args.age19,args.single,args.dong,args.marriage_zip)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('x',encoding='utf-8') as f:
        json.dump(stats,f,ensure_ascii=False,indent=2)
        f.write('\n')
    print(f"Saved {len(stats['population'])} population cells and {len(stats['marriage'])} marriage groups to {args.output}")

if __name__ == '__main__':
    main()
