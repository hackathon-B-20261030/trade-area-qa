"""DR-227: 내려받은 공식 통계 원자료에서 필요한 표를 추출해 근거 JSON으로 쓴다.
사용: python3 -I build_stats_json.py <data/reaction-stats 폴더>
값이 기대와 다르면 AssertionError로 멈춘다(추출 어긋남 방지)."""
import sys, os, re, json, hashlib, csv, glob, collections
import openpyxl

ROOT = sys.argv[1]
RAW = os.path.join(ROOT, 'raw')
TXT = os.path.join(ROOT, 'text')


def sha(p):
    return hashlib.sha256(open(p, 'rb').read()).hexdigest()


def num(s):
    return int(s.replace(',', ''))


def fnum(s):
    return float(s.replace(',', ''))


# ---------------------------------------------------------------- 02: 고용형태별근로실태조사 2025
L = open(os.path.join(TXT, '02_datago_wage_survey.txt'), encoding='utf-8').read().split('\n')


def first_int(line):
    m = re.match(r'\s*([\d,]+)', line)
    return num(m.group(1)) if m else None


def all_workers(a, b):
    return [first_int(l) for l in L[a - 1:b] if 'All workers' in l and first_int(l) is not None]


occ_all = all_workers(3912, 4363)
assert occ_all == [3834, 12545, 4761, 4516, 1877, 3108, 2546, 3488, 4019, 2095], occ_all
sex_all = all_workers(2524, 2640)
assert sex_all == [3834, 4503, 2959], sex_all
edu_all = all_workers(2641, 2878)
assert edu_all == [3834, 2016, 2850, 3792, 4669, 6686], edu_all
age_all = all_workers(2879, 3110)
assert age_all == [3834, 2691, 4093, 4706, 4311, 2734], age_all
OCC = ['관리자', '전문가 및 관련 종사자', '사무 종사자', '서비스 종사자', '판매 종사자', '농림어업 숙련 종사자',
       '기능원 및 관련 기능 종사자', '장치·기계조작 및 조립 종사자', '단순노무 종사자']

# 상용근로자 성별·연령
sa = {}
pat = re.compile(r'^\s*(전\s*체|19세 이하|\d\d~\d\d세|6 0 세 이 상)\s+([\d,]+)\s+\(([\d.]+)\)\s+([\d,]+)\s+\(([\d.]+)\)\s+([\d,]+)\s+\(([\d.]+)\)\s+\[([\d.]+)\]')
for l in L[2000:2050]:
    m = pat.match(l)
    if m:
        lab = re.sub(r'\s+', '', m.group(1)).replace('60세이상', '60세이상')
        sa[lab] = {'total': num(m.group(2)), 'male': num(m.group(4)), 'female': num(m.group(6)), 'female_to_male_pct': float(m.group(8))}
assert len(sa) == 11, sa.keys()
assert sa['전체'] == {'total': 4482, 'male': 5182, 'female': 3486, 'female_to_male_pct': 67.3}

# 상용근로자 학력
edu_reg = None
for i, l in enumerate(L[2070:2100], 2070):
    m = re.search(r'4,482\s+2,641\s+3,444\s+4,121\s+5,388', l)
    if m:
        edu_reg = {'중졸 이하': 2641, '고졸': 3444, '전문대졸': 4121, '대졸 이상': 5388}
assert edu_reg

# 상용근로자 직종 × 연령(10개 구간)
age_bands = ['19세 이하', '20~24', '25~29', '30~34', '35~39', '40~44', '45~49', '50~54', '55~59', '60세 이상']
rows = []
for l in L[2272:2326]:
    toks = l.split()
    pre = []
    for t in toks:
        if re.fullmatch(r'-|[\d,]+', t):
            pre.append(t)
        else:
            break
    if len(pre) == 11 and not l.strip().startswith('('):
        rows.append([None if t == '-' else num(t) for t in pre])
assert len(rows) == 10, len(rows)
occ_age = {}
for name, r in zip(['전체'] + OCC, rows):
    occ_age[name] = {'all_ages': r[0], 'by_age': dict(zip(age_bands, r[1:]))}
assert occ_age['사무 종사자']['all_ages'] == 4942 and occ_age['사무 종사자']['by_age']['60세 이상'] == 5237
assert occ_age['관리자']['by_age']['19세 이하'] is None

# 상용근로자 직종 × 성별 (임금구조통계표 12)
rowre = re.compile(r'^\s*([\d,]+)\s+([\d,]+)\s+([\d,]+)\s+([\d,]+)\s+([\d,]+)\s+([\d,]+)\s+([\d,]+)\s+(Total|Men|Women)\b\s*(.*)$')
keys = [('관리자', 'Managers'), ('전문가 및 관련 종사자', 'Professionals and Related Workers'), ('사무 종사자', 'Clerks'),
        ('서비스 종사자', 'Service Workers'), ('판매 종사자', 'Sales Workers'), ('농림어업 숙련 종사자', 'Skilled Agricultural'),
        ('기능원 및 관련 기능 종사자', 'Craft and Related'), ('장치·기계조작 및 조립 종사자', 'Equipment, Machine'), ('단순노무 종사자', 'Elementary')]
occ_sex = {}
for i in range(5170, 7100):
    m = rowre.match(L[i])
    if not m or m.group(8) != 'Total':
        continue
    for ko, en in keys:
        if ko in occ_sex:
            continue
        if m.group(9).strip().startswith(en):
            trio = [m, rowre.match(L[i + 1]), rowre.match(L[i + 2])]
            if all(trio) and trio[1].group(8) == 'Men' and trio[2].group(8) == 'Women':
                occ_sex[ko] = {'total': num(trio[0].group(1)), 'male': num(trio[1].group(1)), 'female': num(trio[2].group(1)),
                               'workers_total': num(trio[0].group(7)), 'workers_male': num(trio[1].group(7)), 'workers_female': num(trio[2].group(7))}
assert len(occ_sex) == 9, occ_sex.keys()
assert occ_sex['관리자']['total'] == 12563 and occ_sex['단순노무 종사자']['female'] == 2315
# 직종 대분류 상용 월 임금총액은 '직종별 월 임금총액' 표와 같아야 한다
reg_occ_total = {k: v['total'] for k, v in occ_sex.items()}
assert reg_occ_total == {'관리자': 12563, '전문가 및 관련 종사자': 5149, '사무 종사자': 4942, '서비스 종사자': 2285, '판매 종사자': 4104,
                         '농림어업 숙련 종사자': 3111, '기능원 및 관련 기능 종사자': 4147, '장치·기계조작 및 조립 종사자': 4215, '단순노무 종사자': 2631}

wage = {
    'meta': {
        'source_id': 'S02', 'title': '2025년 고용형태별근로실태조사 보고서', 'publisher': '고용노동부',
        'url': 'https://laborstat.moel.go.kr/lsm/bbs/selectBbsList.do?bbsId=LSS108 (첨부 FILE_000000000058663)',
        'reference_period': '2025년 6월 급여계산기간', 'file': 'raw/02_datago_wage_survey/report.pdf',
        'sha256': sha(os.path.join(RAW, '02_datago_wage_survey', 'report.pdf')),
        'unit': '천원/월 세전. 월 임금총액 = 정액급여 + 초과급여 + 전년도 연간특별급여/12',
        'scope_all_workers': '근로자 1인 이상 민간 사업체의 임금근로자 전체(정규직·비정규직, 특수형태근로종사자 제외). 통계표 2·3·4·6',
        'scope_regular': '상용근로자 5인 이상 사업체의 상용근로자(임금구조분석). 통계표 12 및 본문 Ⅱ-2',
        'excluded': '자영업주·고용주·무급가족종사자, 공무원·군인·경찰·국공립교육기관, 가사서비스업, 개인 운영 농림어업체',
        'extraction': 'pdftotext -layout 후 정규식(build_stats_json.py). 값이 기대와 다르면 중단',
    },
    'all_workers': {
        'overall': 3834,
        'by_occupation': dict(zip(OCC, occ_all[1:])),
        'by_sex': {'남': sex_all[1], '여': sex_all[2]},
        'by_education': {'중졸 이하': edu_all[1], '고졸': edu_all[2], '전문대졸': edu_all[3], '대졸': edu_all[4], '대학원졸': edu_all[5]},
        'by_age': {'29세 이하': age_all[1], '30~39세': age_all[2], '40~49세': age_all[3], '50~59세': age_all[4], '60세 이상': age_all[5]},
    },
    'regular_5plus': {
        'overall': 4482,
        'by_sex_age': sa,
        'by_education': edu_reg,
        'by_occupation_sex': occ_sex,
        'by_occupation_age': occ_age,
    },
}

# ---------------------------------------------------------------- 06: 서울관광 실태조사(외국인) 통계표 xlsx
XL = os.path.join(RAW, '06_sto_2025_foreign_tables', 'tables.xlsx')
wb = openpyxl.load_workbook(XL, read_only=True, data_only=True)
ws = wb['Sheet1']
allrows = {i: r for i, r in enumerate(ws.iter_rows(min_row=1, max_row=8140, max_col=17, values_only=True), 1)}


def block(t, nrows=57):
    """제목 행 t의 표를 {(그룹, 범주, 연도): (헤더 행 이후 값 목록)}로 읽는다."""
    hdr = allrows[t + 1]
    grp = cat = None
    out = {}
    for i in range(t + 3, t + nrows):
        r = allrows.get(i)
        if r is None:
            continue
        if r[0] and str(r[0]).startswith('☞'):
            break
        if r[0]:
            grp = str(r[0]).strip()
        if r[1]:
            cat = str(r[1]).strip()
        if r[4] is None:
            continue
        out[(re.sub(r'\s+', '', grp), cat, str(r[3]) if r[3] else '')] = r[4:]
    return hdr, out


REG = ['일본', '중국', '대만', '홍콩', '기타 아시아(무슬림)', '기타아시아(비무슬림)', '미주', '구주', '대양주', '중동/아프리카']
REGK = {r: ('거주국(권역)', r, '2025') for r in REG}
REGK = {r: ('거주국(권역)', r, '2025') for r in REG}


def reg_get(out, r):
    k = ('거주국(권역)', r, '2025')
    return out[k]


assert allrows[4485][0] == '[MEAN] B6_1.항목별 지출 금액_1)항공료/선박료'
items = {'airfare': 4485, 'lodging': 4543, 'home_agency': 4601, 'shopping': 4659, 'food': 4717, 'seoul_transport': 4775,
         'seoul_agency': 4833, 'culture': 4891, 'medical_beauty': 5007, 'other': 5123, 'total_incl_airfare': 5181}
spend = {}
for name, t in items.items():
    hdr, out = block(t)
    assert any('MEAN' in str(h) for h in allrows[t + 2] if h), (name, allrows[t + 2])
    spend[name] = {r: reg_get(out, r)[11] for r in REG}                    # 사례수 다음 11번째 칸이 평균(원)
    spend[name]['__2025'] = out[('조사연도', '2025년', '')][11]
    spend[name]['__all_years'] = out[('전체', None, '')][11] if ('전체', None, '') in out else None
nights_h, nights = block(303)
assert '서울 체류 기간_일' in [str(h) for h in nights_h if h]
nidx = [str(h) for h in nights_h].index('서울 체류 기간_일') - 4
purpose_h, purpose = block(1220)
age_h, agetab = block(477)
comp_h, comp = block(834)
compn_h, compn = block(1162)
type_h, ttype = block(361)
mdong_h, mdong = block(8078)


def pct_row(hdr, out, r, skip_last=0):
    names = [str(h) for h in hdr[5:] if h]
    vals = list(reg_get(out, r)[1:1 + len(names)])
    return {n: v for n, v in zip(names, vals) if n not in ('계', '연령') and not n.endswith('_명')}


regions = {}
for r in REG:
    n = reg_get(nights, r)[0]
    nights_mean = reg_get(nights, r)[nidx]
    regions[r] = {
        'cases_2025': n,
        'seoul_nights_mean': nights_mean,
        'seoul_days_mean': round(nights_mean + 1, 2),
        'spend_mean_krw': {k: spend[k][r] for k in items},
        'purpose_pct': pct_row(purpose_h, purpose, r),
        'age_pct': pct_row(age_h, agetab, r),
        'age_mean': reg_get(agetab, r)[7],
        'companion_pct_multi': pct_row(comp_h, comp, r),
        'companion_count_pct': pct_row(compn_h, compn, r),
        'companion_count_mean': reg_get(compn, r)[6],
        'travel_type_pct': pct_row(type_h, ttype, r),
        'myeongdong_visit_cases': reg_get(mdong, r)[0],
    }
    s = regions[r]['spend_mean_krw']
    regions[r]['derived'] = {
        'spend_excl_airfare_krw': round(s['total_incl_airfare'] - s['airfare']),
        'daily_spend_excl_airfare_krw': round((s['total_incl_airfare'] - s['airfare']) / (nights_mean + 1)),
        'daily_discretionary_krw': round((s['shopping'] + s['food'] + s['medical_beauty'] + s['culture']) / (nights_mean + 1)),
        'myeongdong_visit_rate': round(regions[r]['myeongdong_visit_cases'] / n, 3),
    }
all2025 = {
    'cases_2025': allrows and block(303)[1][('조사연도', '2025년', '')][0],
    'spend_mean_krw': {k: spend[k]['__2025'] for k in items},
    'myeongdong_visit_cases': mdong[('조사연도', '2025년', '')][0],
}
all2025['myeongdong_visit_rate'] = round(all2025['myeongdong_visit_cases'] / all2025['cases_2025'], 3)
assert all2025['cases_2025'] == 2423 and all2025['myeongdong_visit_cases'] == 1778

# ---------------------------------------------------------------- 07/08: 보고서 본문 수치(존재 확인 후 기록)
T07 = open(os.path.join(TXT, '07_sto_2025_foreign_report.txt'), encoding='utf-8').read()
T08 = open(os.path.join(TXT, '08_kto_2025q4_inbound.txt'), encoding='utf-8').read()


def must(text, pat):
    m = re.search(pat, text)
    assert m, pat
    return m


m = must(T07, r'총 지출비용\*\s+286\.8\s+274\.1\s+275\.6\s+282\.1\s+315\.4\s+252\.8\s+284\.4')
sto_items = {'총 지출비용': 286.8, '여행사 지불비': 15.5, '숙박비': 48.8, '식음료비': 32.8, '국제 교통비(항공료/선박료)': 82.6,
             '서울 시내 교통비': 10.0, '문화/오락비': 5.4, '쇼핑비': 73.3, '통신비': 1.5, '의료/미용 시술비': 15.8, '기타': 1.1, '카지노(합계에서 제외)': 1.0}
must(T07, r'숙박비\s+48\.8\s+46\.8'); must(T07, r'식음료비\s+32\.8\s+32\.8'); must(T07, r'쇼핑비\s+73\.3\s+72\.3')
must(T07, r'서울 시내 교통비\s+10\.0\s+10\.2'); must(T07, r'문화/오락비[^\n]*\n\s+5\.4\s+5\.3'); must(T07, r'통신비\s+1\.5\s+1\.5')
must(T07, r'204\.2\s+199\.5\s+195\.5\s+194\.5\s+227\.3\s+168\.6\s+198\.0'); must(T07, r'6\.1\s+6\.7\s+5\.9\s+6\.1\s+5\.8\s+5\.4')
assert abs(sum(v for k, v in sto_items.items() if k not in ('총 지출비용', '카지노(합계에서 제외)', '국제 교통비(항공료/선박료)')) + 82.6 - 286.8) < 0.15

arr_line = must(T08, r'2025년 4분기\(B\)\s+((?:[\d,]+\s+){21}[\d,]+)').group(1).split()
arr_names = ['전체', '중국', '일본', '대만', '미국', '홍콩', '태국', '베트남', '말레이시아', '필리핀', '싱가포르', '러시아', '중동', '인도네시아',
             '캐나다', '호주', '영국', '몽골', '독일', '프랑스', '인도', '기타']
arrivals = dict(zip(arr_names, [num(x) for x in arr_line]))
assert arrivals['전체'] == 4588 and arrivals['중국'] == 1205 and arrivals['기타'] == 272
assert sum(v for k, v in arrivals.items() if k != '전체') == 4588 + 0 or abs(sum(v for k, v in arrivals.items() if k != '전체') - 4588) <= 2
for pat in [r'1,801\.9', r'1,327\.3', r'321\.9', r'238\.3', r'US\$1,821\.9', r'US\$1,342\.0', r'US\$326\.1', r'US\$244\.4']:
    must(T08, pat)

tour = {
    'meta': {
        'sources': {
            'S06': {'title': '2025 서울관광 실태조사(외국인) 통계표(통합 2019·2023~2025)', 'publisher': '서울관광재단',
                    'file': 'raw/06_sto_2025_foreign_tables/tables.xlsx', 'sha256': sha(XL),
                    'note': '사례수 기준(가중치 미적용) 평균. 권역 × 조사연도 2025 행. 표본 2025년 2,423명(5회 조사). 이용 조건 표기 없음(재단 확인 필요)'},
            'S07': {'title': '2025 서울관광 실태조사 결과 보고서(외국인) 잠정치', 'publisher': '서울관광재단',
                    'file': 'raw/07_sto_2025_foreign_report/report.pdf', 'sha256': sha(os.path.join(RAW, '07_sto_2025_foreign_report', 'report.pdf')),
                    'note': '가중치를 적용한 공식 잠정치(서울 1인당 지출 286.8만 원, 항공·선박료 제외 204.2만 원, 체류 6.1일)'},
            'S08': {'title': '2025 외래관광객조사 4분기 잠정치 보고서', 'publisher': '문화체육관광부·한국문화관광연구원',
                    'file': 'raw/08_kto_2025q4_inbound/inbound_q4.pdf', 'sha256': sha(os.path.join(RAW, '08_kto_2025q4_inbound', 'inbound_q4.pdf')),
                    'note': '가중치 적용 결과. 2025년 연간 확정치는 2026년 5월 말 별도 공표(이 파일에는 없음)'},
        },
        'unit': 'spend_mean_krw는 원/인(여행 전체, 항공·선박료 포함 total_incl_airfare). 응답자 전체 평균(지출 없음 포함)',
        'caveat': '서울 조사 지출 정의(서울 안 지출인지 한국 여행 전체 경비인지)는 보고서에 명시되지 않았다. 서울 2024 252.8만 ≈ 한국 2024 256.6만',
    },
    'seoul_survey_2025_by_region': regions,
    'seoul_survey_2025_all': all2025,
    'seoul_survey_2025_official_weighted': {
        'source': 'S07', 'unit': '만원/인',
        'total_incl_airfare': 286.8, 'total_excl_airfare': 204.2, 'stay_days': 6.1, 'revisit_pct': 50.2,
        'waves_total_incl_airfare': {'5월': 274.1, '6월': 275.6, '8월': 282.1, '10월': 315.4},
        'items_incl_zero_spenders': sto_items,
        'items_only_spenders': {'거주국 여행사 지불비(패키지 이용자만)': 110.3, '서울 체재 시 여행사 지불비(지출 경험자만)': 28.5,
                                '문화/오락비(지출 경험자만)': 18.4, '카지노(지출 응답자만)': 115.9, '의료/미용 시술비(지출 응답자만)': 111.1},
        'by_nationality_total_incl_airfare': {'중국': 285.0, '홍콩': 254.7, '대만': 241.9, '일본': 132.6, '기타아시아(무슬림)': 231.8,
                                              '기타아시아(비무슬림)': 278.4, '미주': 442.6, '구주': 383.8},
    },
    'inbound_2025q4_thousand_persons': {'source': 'S08', 'arrivals': arrivals,
                                        'note': '입국자 수는 교포·승무원 제외. 4분기 값이며 연간 비중이 아니다'},
    'inbound_per_person_usd': {'source': 'S08', 'year_2025': {'total_incl_intl_transport': 1801.9, 'total_excl_intl_transport': 1327.3,
                                                              'per_day_incl': 321.9, 'per_day_excl': 238.3},
                               'q4_2025': {'total_incl': 1821.9, 'total_excl': 1342.0, 'per_day_incl': 326.1, 'per_day_excl': 244.4,
                                           'stay_days': 6.3, 'seoul_visit_rate_pct': 75.7},
                               'note': '1인 지출은 사례수 기준 상하위 1%를 가장 가까운 값으로 대체. 1일 지출은 61일 이상 체류자 제외'},
}

# ---------------------------------------------------------------- 03/04/09: 국내 방문 예산 관련
T03 = open(os.path.join(TXT, '03_mcst_2025_travel_press.txt'), encoding='utf-8').read()
T04 = open(os.path.join(TXT, '04_kostat_2025q4_household.txt'), encoding='utf-8').read()
for pat in [r'97\.0%', r'300\.9', r'472\.5', r'39\.5', r'85\s*만\s*2\s*천\s*원', r'41\.3%']:
    must(T03, pat)
for pat in [r'542\.2', r'336\.9', r'112\.4', r'76\.6', r'107\.3', r'300\.8', r'434\.9', r'5\.59']:
    must(T04, pat)
trips_m, spend_tr = 300.9, 39.5
domestic = {
    'meta': {'sources': {
        'S03': {'title': '2025년 국민여행조사 결과 보도자료(문체부, 2026-06-30)', 'file': 'raw/03_mcst_2025_travel_press/press.pdf',
                'sha256': sha(os.path.join(RAW, '03_mcst_2025_travel_press', 'press.pdf')),
                'note': '본문 텍스트와 별첨 정보그림(이미지)을 읽음. 당일·숙박 1회 지출 분리값은 이 파일에 없다'},
        'S04': {'title': "'25년 4분기 가계동향조사 보도참고자료(국가데이터처, 2026-02-26)", 'file': 'raw/04_kostat_2025q4_household/household_press.pdf',
                'sha256': sha(os.path.join(RAW, '04_kostat_2025q4_household', 'household_press.pdf')),
                'note': '가구당 월평균. 분위별 금액표는 이 파일에 없다(첨부 xlsx 필요)'}}},
    'domestic_trip_2025': {
        'source': 'S03', 'trips_million': trips_m, 'days_million': 472.5, 'spend_trillion_krw': spend_tr,
        'per_person_year': {'trips': 6.5, 'days': 10.2, 'spend_krw': 852000},
        'overnight_trip_share_pct': 41.3, 'day_trip_share_pct': 58.8,
        'derived_mean_spend_per_trip_krw': round(spend_tr * 1e12 / (trips_m * 1e6)),
        'purchase_item_share_pct': {'음식점비': 36.7, '교통비': 21.3, '숙박비': 15.8, '식료품비': 9.2, '여행 활동비': 5.9},
        'day_trip_mean_krw_secondary': {'value': 69000, 'overnight_mean_krw': 221000, 'source': '세계일보 2026-07-05(2차)',
                                        'check': '0.588×69,000 + 0.412×221,000 = %d (1차 합계 평균 %d원/회와 정합)' % (
                                            round(0.588 * 69000 + 0.412 * 221000), round(spend_tr * 1e12 / (trips_m * 1e6)))},
        'trip_definition_note': '국민여행조사의 여행은 일상생활권을 벗어난 이동이라 서울 거주자의 명동 방문과 정의가 다를 수 있다',
    },
    'household_2025q4_per_household_month_manwon': {
        'source': 'S04', 'income_total': 542.2, 'earned': 336.9, 'business': 112.4, 'transfer': 76.6, 'non_consumption': 107.3,
        'consumption': 300.8, 'disposable': 434.9, 'surplus': 134.0, 'quintile_ratio_equivalized': 5.59,
        'note': '4분기는 특별급여 시기 영향이 있다(보도자료 설명)'},
}

# 서울 상권분석서비스 추정매출(명동 상권, 2025 1~4분기)
ZIPDIR = os.path.join(RAW, '09_seoul_trade_area_sales_extracted')
csvp = glob.glob(os.path.join(ZIPDIR, '*.csv'))[0]
sel = []
with open(csvp, encoding='cp949', newline='') as f:
    for row in csv.DictReader(f):
        if row['상권_코드'] in ('3120028', '3120027', '3001492'):
            sel.append(row)


def fl(x):
    try:
        return float(x)
    except Exception:
        return 0.0


ages = ['10', '20', '30', '40', '50', '60_이상']
card = {}
for code in ('3120028', '3120027', '3001492'):
    rs = [x for x in sel if x['상권_코드'] == code]
    amt = sum(fl(x['당월_매출_금액']) for x in rs); cnt = sum(fl(x['당월_매출_건수']) for x in rs)
    ag = {a: (sum(fl(x['연령대_%s_매출_금액' % a]) for x in rs), sum(fl(x['연령대_%s_매출_건수' % a]) for x in rs)) for a in ages}
    ind = collections.defaultdict(lambda: [0.0, 0.0])
    for x in rs:
        ind[x['서비스_업종_코드_명']][0] += fl(x['당월_매출_금액']); ind[x['서비스_업종_코드_명']][1] += fl(x['당월_매출_건수'])
    top = sorted(ind.items(), key=lambda kv: -kv[1][0])[:20]
    card[code] = {
        'name': rs[0]['상권_코드_명'], 'type': rs[0]['상권_구분_코드_명'], 'quarters': sorted(set(x['기준_년분기_코드'] for x in rs)),
        'sales_100m_krw': round(amt / 1e8), 'transactions_10k': round(cnt / 1e4), 'mean_per_transaction_krw': round(amt / cnt),
        'per_transaction_by_age_krw': {a: round(v[0] / v[1]) for a, v in ag.items() if v[1]},
        'sales_share_by_age_pct': {a: round(100 * v[0] / sum(t[0] for t in ag.values()), 1) for a, v in ag.items()},
        'top_industries': [{'industry': k, 'sales_100m_krw': round(v[0] / 1e8), 'per_transaction_krw': round(v[0] / v[1]) if v[1] else None} for k, v in top],
    }
card_doc = {'meta': {'source_id': 'S09', 'title': '서울시 상권분석서비스(추정매출-상권) 2025년', 'publisher': '서울특별시(열린데이터광장 OA-15572)',
                     'file': 'raw/09_seoul_trade_area_sales/sales_2025.zip', 'sha256': sha(os.path.join(RAW, '09_seoul_trade_area_sales', 'sales_2025.zip')),
                     'note': '건당(결제 1건) 평균이며 방문 1회 지출이 아니다. 카드 기반 추정 매출이고 외국인 포함 여부는 확인하지 못했다. 연령은 카드 이용자 연령대'},
            'trade_areas': card}


# ---------------------------------------------------------------- 10: 가계동향조사 2025 통계표(국가데이터처, xlsx)
import math

HQ4 = os.path.join(RAW, '10_kostat_2025q4_household_tables', 'stats_2025q4_tables.xlsx')
HQA = os.path.join(RAW, '10_kostat_2025q4_household_tables', 'stats_2025_annual_expenditure_tables.xlsx')
wbq = openpyxl.load_workbook(HQ4, read_only=True, data_only=True)
wsq = wbq['1.5_p.21']
qrows = list(wsq.iter_rows(min_row=1, max_row=40, max_col=13, values_only=True))
assert str(qrows[0][0]).startswith('1.5. 전국가구 소득 5분위별 가계수지') and str(qrows[3][2]).startswith('2025. 4/4')
quint = {}
for r in qrows[5:34]:
    if not r[0]:
        continue
    lab = re.sub(r'\s+', '', str(r[0]))
    vals = [r[i] for i in (2, 4, 6, 8, 10)]
    if all(isinstance(v, (int, float)) for v in vals):
        quint[lab] = vals
assert quint['소득'] == [1269, 2948, 4482, 6530, 11877], quint['소득']
assert quint['소비지출'] == [1464, 1970, 2780, 3716, 5110]
assert quint['음식·숙박'] == [198, 306, 452, 634, 798] and quint['오락·문화'] == [63, 90, 149, 215, 387]
assert quint['의류·신발'] == [61, 102, 147, 238, 341] and quint['기타상품·서비스'] == [89, 154, 215, 329, 509]
assert all(abs(x - y) < 0.06 for x, y in zip(quint['가구원수(명)'], [1.3, 1.6, 2.1, 2.7, 3.1]))
wba = openpyxl.load_workbook(HQA, read_only=True, data_only=True)
wsa = wba['1.1.~1.2.']
arow = None
for r in wsa.iter_rows(min_row=1, max_row=30, max_col=16, values_only=True):
    if r[0] == 2025:
        arow = r
        break
ANN_LAB = ['가계지출', '소비지출', '식료품·비주류음료', '주류·담배', '의류·신발', '주거·수도·광열', '가정용품·가사서비스', '보건', '교통·운송',
           '정보통신', '오락·문화', '교육', '음식·숙박', '기타상품·서비스', '비소비지출']
annual = dict(zip(ANN_LAB, arow[1:16]))
assert annual['소비지출'] == 2939 and annual['음식·숙박'] == 465 and annual['오락·문화'] == 175 and annual['의류·신발'] == 141 and annual['기타상품·서비스'] == 250


def ols_slope(xs, ys):
    n = len(xs); mx = sum(xs) / n; my = sum(ys) / n
    return sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / sum((x - mx) ** 2 for x in xs)


DISC = ['음식·숙박', '오락·문화', '의류·신발', '기타상품·서비스']
disc_hh = [sum(quint[k][i] for k in DISC) for i in range(5)]
nper = quint['가구원수(명)']
eps_disc = ols_slope([math.log(quint['소득'][i] / nper[i]) for i in range(5)], [math.log(disc_hh[i] / nper[i]) for i in range(5)])

household = {
    'meta': {'sources': {
        'S10a': {'title': '(통계표)2025년 4분기 가계동향조사 결과 통계표.xlsx (시트 1.5 전국가구 소득 5분위별 가계수지)', 'publisher': '국가데이터처',
                 'url': 'https://mods.go.kr/boardDownload.es?bid=214&list_no=443727&seq=3', 'file': 'raw/10_kostat_2025q4_household_tables/stats_2025q4_tables.xlsx',
                 'sha256': sha(HQ4), 'unit': '천원/가구/월, 2025년 4/4분기'},
        'S10b': {'title': '(통계표)2025년 연간 지출 가계동향조사 결과 통계표.xlsx (시트 1.1)', 'publisher': '국가데이터처',
                 'url': 'https://mods.go.kr/boardDownload.es?bid=214&list_no=443727&seq=4', 'file': 'raw/10_kostat_2025q4_household_tables/stats_2025_annual_expenditure_tables.xlsx',
                 'sha256': sha(HQA), 'unit': '천원/가구/월, 2025년 연간, 전국가구'}}},
    'quarter_2025q4_by_income_quintile_thousand_krw': {'columns': ['1분위', '2분위', '3분위', '4분위', '5분위'], 'items': quint},
    'annual_2025_national_thousand_krw': annual,
    'derived': {
        'discretionary_items': DISC,
        'discretionary_per_household_2025q4': disc_hh,
        'discretionary_per_capita_2025q4': [round(disc_hh[i] / nper[i], 1) for i in range(5)],
        'income_per_capita_2025q4': [round(quint['소득'][i] / nper[i], 1) for i in range(5)],
        'elasticity_discretionary_per_capita_vs_income_per_capita': round(eps_disc, 3),
        'food_lodging_share_of_consumption_2025_annual_pct': round(100 * annual['음식·숙박'] / annual['소비지출'], 1),
        'method': '5개 분위의 로그-로그 최소제곱 기울기',
    },
}

# ---------------------------------------------------------------- 11: 국민여행조사 2025 통계편(이미지 쪽을 읽어 옮김)
# 통계편 PDF는 텍스트 층이 없다. 인쇄쪽 478(PDF 236)과 492(PDF 250)을 그림으로 바꿔 사람이 읽어 옮겼다(img/11/h-236.png, h-250.png).
TRIP_COLS = ['국내전체', '국내숙박', '국내당일', '관광전체', '관광숙박', '관광당일', '기타전체', '기타숙박', '기타당일']
TRIP = {
    '전체': [133, 221, 69, 140, 235, 71, 81, 120, 57],
    '성별:남자': [135, 228, 70, 143, 242, 72, 83, 128, 59], '성별:여자': [131, 216, 68, 137, 228, 69, 79, 112, 56],
    '연령:15-19세': [108, 182, 65, 115, 207, 68, 63, 86, 41], '연령:20대': [142, 239, 75, 148, 249, 77, 79, 118, 56],
    '연령:30대': [142, 229, 70, 148, 240, 71, 94, 141, 61], '연령:40대': [129, 206, 67, 137, 221, 68, 79, 109, 58],
    '연령:50대': [140, 228, 72, 148, 243, 74, 84, 118, 62], '연령:60대': [129, 226, 68, 136, 238, 69, 79, 125, 58],
    '연령:70세이상': [106, 197, 61, 112, 211, 63, 75, 122, 49],
    '직업:임금봉급근로자': [135, 225, 70, 142, 237, 72, 83, 123, 59], '직업:고용원있는사업주': [185, 286, 82, 200, 307, 88, 91, 141, 56],
    '직업:고용원없는자영자': [126, 210, 69, 133, 223, 70, 79, 109, 64], '직업:무급가족종사자': [117, 191, 65, 119, 200, 67, 102, 143, 55],
    '직업:전업주부': [132, 218, 64, 139, 231, 66, 81, 126, 50], '직업:학생': [113, 184, 68, 121, 203, 71, 63, 85, 44],
    '직업:기타': [121, 229, 60, 128, 247, 61, 78, 128, 50],
    '학력:초졸이하': [89, 153, 56, 84, 151, 54, 111, 159, 64], '학력:중학교': [99, 181, 60, 106, 201, 63, 68, 107, 45],
    '학력:고등학교': [123, 210, 67, 130, 224, 69, 76, 113, 54], '학력:대학교이상': [142, 231, 71, 149, 243, 73, 85, 123, 61],
    '가구원수:1인': [136, 223, 73, 141, 237, 74, 103, 143, 68], '가구원수:2인': [130, 230, 69, 137, 242, 71, 83, 136, 57],
    '가구원수:3인이상': [134, 218, 68, 141, 232, 70, 76, 108, 56],
    '가구소득:100만원미만': [95, 154, 53, 96, 163, 55, 86, 126, 41], '가구소득:100~200만원': [98, 181, 58, 100, 183, 60, 86, 170, 49],
    '가구소득:200~300만원': [115, 200, 65, 120, 213, 66, 79, 115, 55], '가구소득:300~400만원': [120, 205, 67, 126, 219, 68, 81, 116, 59],
    '가구소득:400~500만원': [123, 206, 68, 129, 217, 69, 85, 130, 61], '가구소득:500~600만원': [130, 215, 68, 137, 228, 69, 79, 115, 58],
    '가구소득:600만원이상': [155, 246, 75, 164, 261, 77, 81, 116, 57],
}
SEOUL_DAY = {  # 4-8-3 '1회 평균 여행지별 국내 당일여행 지출액'(천원)의 서울 열
    '전체': 69, '성별:남자': 66, '성별:여자': 71, '연령:15-19세': 71, '연령:20대': 83, '연령:30대': 68, '연령:40대': 55, '연령:50대': 65,
    '연령:60대': 72, '연령:70세이상': 60, '직업:임금봉급근로자': 69, '직업:고용원있는사업주': 98, '직업:고용원없는자영자': 72,
    '직업:무급가족종사자': 61, '직업:전업주부': 55, '직업:학생': 73, '직업:기타': 61, '학력:초졸이하': 89, '학력:중학교': 95,
    '학력:고등학교': 61, '학력:대학교이상': 71, '가구원수:1인': 79, '가구원수:2인': 69, '가구원수:3인이상': 67,
    '가구소득:100만원미만': 42, '가구소득:100~200만원': 55, '가구소득:200~300만원': 63, '가구소득:300~400만원': 68,
    '가구소득:400~500만원': 65, '가구소득:500~600만원': 65, '가구소득:600만원이상': 76,
}
assert set(TRIP) == set(SEOUL_DAY) and all(len(v) == 9 for v in TRIP.values())
# 옮겨 적은 값의 일관성: 각 행에서 국내전체 = w × 당일 + (1 − w) × 숙박의 w(당일 몫)가 0.45~0.75여야 한다(전체 행은 약 0.58, 보도자료의 당일 비중 58.8%)
w_all = {}
for key, v in TRIP.items():
    for (a, d_, o_) in ((0, 2, 1), (3, 5, 4)):
        if v[o_] == v[d_]:
            continue
        w = (v[o_] - v[a]) / (v[o_] - v[d_])
        if a == 0:
            w_all[key] = round(w, 3)
        assert 0.40 <= w <= 0.80, (key, a, w)
assert abs(w_all['전체'] - 0.579) < 0.01 and SEOUL_DAY['전체'] == TRIP['전체'][2]
inc_mid = [150, 250, 350, 450, 550]
inc_keys = ['가구소득:100~200만원', '가구소득:200~300만원', '가구소득:300~400만원', '가구소득:400~500만원', '가구소득:500~600만원']
eps_day = ols_slope([math.log(m) for m in inc_mid], [math.log(TRIP[k][2]) for k in inc_keys])
eps_over = ols_slope([math.log(m) for m in inc_mid], [math.log(TRIP[k][1]) for k in inc_keys])
trip_doc = {
    'meta': {'source_id': 'S11', 'title': '2025년 국민여행조사 보고서 -통계편-', 'publisher': '문화체육관광부·한국문화관광연구원',
             'url': 'https://know.tour.go.kr/stat/nReportsOfForeignerDis19Re.do (등록 2026.06.30, 파일 2025년_국민여행조사_보고서_-_통계편-)',
             'file': 'raw/11_tour_2025_domestic_tables/domestic_stat_report_2025.pdf', 'sha256': sha(os.path.join(RAW, '11_tour_2025_domestic_tables', 'domestic_stat_report_2025.pdf')),
             'unit': '천원/1회, 여행 경험자 기준, 2025년', 'population': '만 15세 이상 전국민(조사 대상 기간 2025-01-01~12-31)',
             'transcription': 'manual(이미지)', 'pages': {'4-6 1회 평균 국내여행 지출액': {'printed': 478, 'pdf': 236, 'image': 'img/11/h-236.png'},
                                                     '4-8-3 1회 평균 여행지별 국내 당일여행 지출액': {'printed': 492, 'pdf': 250, 'image': 'img/11/h-250.png'}},
             'note': '통계편 PDF는 텍스트 층이 없어 쪽 그림을 읽어 옮겼다. 일관성 검사: 모든 행에서 국내전체 = w×당일 + (1−w)×숙박의 w가 0.40~0.80이고 전체 행은 0.579(보도자료 당일 비중 58.8%와 정합)'},
    'columns': TRIP_COLS,
    'per_trip_mean_thousand_krw': TRIP,
    'implied_day_trip_weight': w_all,
    'seoul_day_trip_mean_thousand_krw': SEOUL_DAY,
    'calibration': {
        'income_classes_used_man_won': inc_mid, 'method': '가구소득 닫힌 5개 구간(100~600만 원) 중앙값의 로그에 대한 1회 평균 지출 로그의 최소제곱 기울기',
        'elasticity_day_trip_vs_household_income': round(eps_day, 3), 'elasticity_overnight_trip_vs_household_income': round(eps_over, 3),
    },
}

os.makedirs(os.path.join(ROOT, 'income'), exist_ok=True)
os.makedirs(os.path.join(ROOT, 'tourists'), exist_ok=True)
json.dump(wage, open(os.path.join(ROOT, 'income', 'wage_2025.json'), 'w', encoding='utf-8'), ensure_ascii=False, indent=1)
json.dump(domestic, open(os.path.join(ROOT, 'income', 'domestic_trip_household_2025.json'), 'w', encoding='utf-8'), ensure_ascii=False, indent=1)
json.dump(card_doc, open(os.path.join(ROOT, 'income', 'myeongdong_card_sales_2025.json'), 'w', encoding='utf-8'), ensure_ascii=False, indent=1)
json.dump(tour, open(os.path.join(ROOT, 'tourists', 'foreign_visitors_2025.json'), 'w', encoding='utf-8'), ensure_ascii=False, indent=1)
json.dump(household, open(os.path.join(ROOT, 'income', 'household_2025.json'), 'w', encoding='utf-8'), ensure_ascii=False, indent=1)
json.dump(trip_doc, open(os.path.join(ROOT, 'income', 'domestic_trip_spend_2025.json'), 'w', encoding='utf-8'), ensure_ascii=False, indent=1)
print('elasticities', household['derived']['elasticity_discretionary_per_capita_vs_income_per_capita'], trip_doc['calibration'])
print('OK')
print({r: regions[r]['derived'] for r in REG})
