---
version: 1.0
last_modified: 2026-10-31
status: draft
---

# 규칙 데이터 v1 (구현체 Map 부록)

[구현체 Map](02_impl_map.md)의 활동 엔진 단위(I-04~I-08)가 읽는 **규칙 데이터의 정본**이다. 아래 코드 블록의 내용을 표시된 경로에 **그대로 저장**한다(수정하면 규칙 버전이 바뀌므로 `v2`로 올린다). 값은 2026-10-30에 서울 원본(185,228명)을 조사한 결과와, 앞 표본에서 돌린 검증용 프로토타입으로 확인한 것이며, 정규식은 Python 3.13 `re`에서 컴파일된다. 설계 상수(템플릿·확률·창)는 [DR-19](decisions.md#dr-19--기본-일과-템플릿)의 값이고 현실 분포가 아니다.

- 파일 인코딩은 UTF-8, JSON은 들여쓰기 2칸이다.
- `rule_set_sha256` = 경로 오름차순으로 `경로 + "\0" + 파일 바이트`를 이어 붙인 SHA-256. 모든 실행 메타데이터에 기록한다([I-08-a](02_impl_map.md#i-08-a--실행-메타데이터와-재생)).
- 정규식 문자열은 JSON에서 백슬래시가 이중으로 보인다. 파일에서 읽은 값을 그대로 `re.compile`한다(플래그 없음).
- 지리 상수 3종 중 구 인접표(`district_adjacency.csv`)는 공식 경계 자료로 만드는 데이터라 여기에 값을 싣지 않고 형식과 불변식만 정한다([I-06-a](02_impl_map.md#i-06-a--지리-상수-3종)).

## 파일 해시 (추출 검증용)

| 경로 | 바이트 | SHA-256 |
|---|---:|---|
| `rules/v1/activities.json` | 6,999 | `716460f75106f1988fbd4b37d05c646af7d7204a2f75815aed77c521cb77c009` |
| `rules/v1/geo/outside_places.csv` | 3,258 | `931f77afac9a43f2bf65e34c4f646c958738b05c5beb5266fa29f786e85f3f7b` |
| `rules/v1/geo/seoul_places.csv` | 6,931 | `f96319b4a7738c42750205ac7a0296c0a8d7edb3daee592a77133773320523e0` |
| `rules/v1/location.json` | 734 | `8063a2de7d2401953df3c3a695fc0865aa4f73accc01fb905c081041bce6d855` |
| `rules/v1/modality.json` | 977 | `0db1f8f7bf0213d9bbeeb2315a81899eb540bb687046f5f48dd6e49e3d1be9d3` |
| `rules/v1/templates.json` | 7,343 | `5ffd11e274931a855f6573f1c223fa55ac7967a4a07abc07a7f9f402ab46bd8d` |
| `rules/v1/text.json` | 786 | `af739738f4477bc4cdd90313d4ec94c7d3c1ee22a760e7450290af5e9a3d16d9` |
| `rules/v1/time.json` | 3,972 | `0833afe2a7c35d5da4eed40366584f137344e990e4122b628a3ce6056499baf4` |

위 파일 8개만으로 계산한 `rule_set_sha256` = `8570b260068f7bc6575a682147c6f126dfe9a801438f36fe8635dc2be69e9ce2`. 구 인접표 `district_adjacency.csv`가 만들어지면 그 파일을 포함해 다시 계산하고, 그때 규칙 버전의 최종 해시를 이 문서와 `run.json`에 기록한다([I-06-a](02_impl_map.md#i-06-a--지리-상수-3종)).

## `rules/v1/text.json`

```json
{
  "rule_version": "v1",
  "field_order": [
    ["hobbies_and_interests", "A"],
    ["sports_persona", "A"],
    ["arts_persona", "A"],
    ["family_persona", "A"],
    ["professional_persona", "B"],
    ["career_goals_and_ambitions", "B"],
    ["culinary_persona", "B"],
    ["cultural_background", "B"],
    ["persona", "B"],
    ["skills_and_expertise", "B"],
    ["travel_persona", "B"]
  ],
  "list_field": "hobbies_and_interests_list",
  "sentence_break": "(?<=[.!?])\\s+|\\n+",
  "clause_break": "\\s*[,;]\\s*",
  "mask_char": "□",
  "nontime_titles": ["6\\s*시\\s*내고향|뉴스\\s*9(?!\\d)|톱\\s*10|(?<!\\d)\\d{1,2}\\s*시청(?!소)"],
  "name_mask": "[가-힣]{2,4}(?=\\s*씨)",
  "min_clause_chars": 2,
  "max_mentions_per_clause": 2,
  "max_clause_text_chars": 300
}
```

## `rules/v1/time.json`

```json
{
  "weekend": "주말|토요일|일요일|휴일|공휴일",
  "weekday": "평일|주중|월요일|화요일|수요일|목요일|금요일",
  "weekday_named": "([월화수목금토일])요일",
  "weekday_index": {"월": 0, "화": 1, "수": 2, "목": 3, "금": 4, "토": 5, "일": 6},
  "weekend_freq": "주말마다|주말이면|주말에는|주말엔",
  "weekday_freq": "평일마다|평일이면|평일에는|평일엔|평일\\s*(?:아침|저녁|밤|낮)",
  "weekday_freq_default": {"kind": "PER_WEEK", "min": 3, "max": 5},
  "freq_rules": [
    {"kind": "ALMOST_DAILY", "pattern": "거의\\s*(?:매일|날마다)", "min": 5, "max": 6},
    {
      "kind": "PER_WEEK",
      "pattern": "(?<![\\d두세네])(?:주|일주일에?)\\s*(?P<a>\\d+|한|두|세|네|다섯|여섯|일곱)\\s*(?:[~\\-]\\s*(?P<b>\\d+|한|두|세|네|다섯|여섯|일곱))?\\s*(?:회|번|차례)",
      "min_group": "a",
      "max_group": "b"
    },
    {"kind": "PER_WEEK", "pattern": "(?:주|일주일에?)\\s*(?P<c>한두|두세|서너|너덧|대여섯|예닐곱|두어)\\s*(?:회|번|차례)", "approx_group": "c"},
    {"kind": "DAILY", "pattern": "하루도\\s*(?:거르지|빠짐)|매일|날마다|하루마다|어김없이|빠짐없이|거르지\\s*않고|꼬박꼬박", "min": 7, "max": 7},
    {"kind": "WEEKLY", "pattern": "매주|주마다|매\\s*주", "min": 1, "max": 1},
    {"kind": "PROB_WEEK", "pattern": "(?P<w>\\d+|두|세|네)\\s*주에\\s*(?P<n>\\d+|한|두|세|네|다섯|여섯|일곱)\\s*(?:회|번)", "ratio_n": "n", "ratio_w": "w"},
    {"kind": "PROB_WEEK", "pattern": "격주", "p": 0.5},
    {"kind": "PROB_WEEK", "pattern": "(?:한\\s*달에|월)\\s*(?P<m>\\d+|한|두|세|네|다섯|여섯|일곱)\\s*(?:회|번)", "per_month_group": "m"},
    {"kind": "PROB_WEEK", "pattern": "매달|달마다", "p": 0.25},
    {"kind": "OCCASIONAL", "pattern": "가끔|종종|때때로|이따금|간혹|틈틈이", "min": 0, "max": 1}
  ],
  "numerals": {"한": 1, "두": 2, "세": 3, "네": 4, "다섯": 5, "여섯": 6, "일곱": 7, "여덟": 8, "아홉": 9, "열": 10},
  "approx_numerals": {"한두": [1, 2], "두세": [2, 3], "서너": [3, 4], "너덧": [4, 5], "대여섯": [5, 6], "예닐곱": [6, 7], "두어": [2, 2]},
  "dayparts": [
    {"code": "DAWN", "pattern": "새벽", "window": [240, 360]},
    {"code": "MORNING", "pattern": "아침", "window": [360, 540]},
    {"code": "FORENOON", "pattern": "오전", "window": [540, 720]},
    {"code": "NOON", "pattern": "한낮|(?<![가-힣])낮(?=에|엔|시간|동안|\\s|$)", "window": [720, 900]},
    {"code": "AFTERNOON", "pattern": "오후", "window": [720, 1080]},
    {"code": "EVENING", "pattern": "저녁|퇴근\\s*후", "window": [1080, 1260]},
    {"code": "NIGHT", "pattern": "한밤|(?<![가-힣])밤(?=에|엔|이면|마다|늦|새|\\s|$)", "window": [1260, 1440]}
  ],
  "daypart_word_to_code": {"새벽": "DAWN", "아침": "MORNING", "오전": "FORENOON", "낮": "NOON", "오후": "AFTERNOON", "저녁": "EVENING", "밤": "NIGHT"},
  "clock_a": "(?P<dp>새벽|아침|오전|(?<![가-힣])낮|오후|저녁|(?<![가-힣])밤)\\s*(?P<h>\\d{1,2})\\s*시(?!간|대|장|즌|중|절|청)(?:\\s*(?P<m>\\d{1,2})\\s*분|\\s*(?P<half>반))?",
  "clock_24": "(?<![\\d:])(?P<h>[01]?\\d|2[0-3])\\s*:\\s*(?P<m>[0-5]\\d)(?![\\d:])",
  "clock_b": "(?<![\\d가-힣])(?P<h>\\d{1,2})\\s*시(?!간|대|장|즌|중|절|청|작)(?:\\s*(?P<m>\\d{1,2})\\s*분|\\s*(?P<half>반))?",
  "clock_korean_detect_only": "(?:[한두세네]|다섯|여섯|일곱|여덟|아홉|열한|열두|열)\\s*시(?=[에쯤경면부까가는를이께,\\.\\s]|정각|무렵|반|전|$)",
  "roles": [
    {"role": "WAKE", "pattern": "일어나|일어납|기상|눈을\\s*뜨|잠에서\\s*깨"},
    {"role": "BED", "pattern": "잠자리에\\s*들|취침|잠들|자러\\s*가|잠을\\s*청|불을\\s*끄"},
    {"role": "WORK_START", "pattern": "출근(?!\\s*길|\\s*전)"},
    {"role": "WORK_END", "pattern": "퇴근(?!\\s*후|\\s*길)"}
  ]
}
```

## `rules/v1/modality.json`

```json
{
  "order": ["NEGATION", "HYPOTHETICAL", "ASPIRATION", "PAST", "PRESENT_HABIT|UNKNOWN"],
  "neg": "지\\s*않(?:습니다|는다|아요|아|아서|으며|고|는\\s*편)|하지\\s*못|못\\s*합|못\\s*한다|안\\s*합|싫어|꺼립|꺼려|피합니다",
  "hyp": "상상|만약|만일|다면|이라면",
  "asp_strong": "꿈꾸|꿈을|꿈이|꿈입|목표|희망|바라는|바랍|바란|바라며|바라고|싶|할\\s*계획|할\\s*예정|계획입니다|계획하고\\s*있|이루고자|이루기|이루려|이루고\\s*싶|이루어내|되고자|하고자|기대하|기대합|기대감|그리워|지향|다짐",
  "asp_value_tier_b_only": "집중하|집중합|가치를|가치\\s*있|추구|만족|우선순위|중요하게|소중히|노력하|위해\\s*노력|되찾는|지켜내는|유지하는\\s*것",
  "past": "예전|과거|젊은\\s*시절|젊었을\\s*때|한때|옛날|어린\\s*시절|지난날|퇴직\\s*전|은퇴\\s*전|하던\\s*시절|했었|곤\\s*했"
}
```

## `rules/v1/activities.json`

```json
{
  "codes": ["SLEEP", "REST", "MEAL", "PERSONAL", "WORK", "STUDY", "HOUSEHOLD", "CARE", "MOVE", "EXERCISE", "HOBBY", "SOCIAL", "CULTURE", "ERRAND", "OTHER"],
  "clause_rules": [
    {"code": "WORK", "pattern": "야간\\s*근무|교대\\s*근무|야근|밤샘"},
    {"code": "STUDY", "pattern": "공부|자격증|수강|강의|독서실|어학|코딩|필사|기출|시험"},
    {"code": "CULTURE", "pattern": "전시|박물관|미술관|갤러리|공연|콘서트|뮤지컬|연극|고궁|유적|영화관|축제|관람"},
    {
      "code": "EXERCISE",
      "pattern": "산책|등산|둘레길|트레킹|조깅|러닝|달리기|걷기|걸으며|걷고|걸어|거닐|수영|요가|필라테스|헬스|배드민턴|탁구|테니스|골프|볼링|자전거|라이딩|축구|농구|스트레칭|체조|게이트볼|파크골프|클라이밍|맨발|에어로빅|운동"
    },
    {"code": "MEAL", "pattern": "식사|점심|외식|식당|맛집|카페|커피|브런치|국밥|술집|한잔|막걸리|주점|밥상|도시락|디저트"},
    {"code": "HOUSEHOLD", "pattern": "청소|빨래|설거지|요리|가계부|정리\\s*정돈|살림|반찬|김치|세탁|다림질"},
    {"code": "CARE", "pattern": "손주|손자|손녀|육아|아이를|아이와|자녀를|돌봄|돌보|간병|반려견|반려묘|강아지|고양이|등하교"},
    {"code": "PERSONAL", "pattern": "목욕|사우나|찜질|반신욕|미용실|이발|피부\\s*관리|마사지|네일|에스테틱|스파"},
    {
      "code": "ERRAND",
      "pattern": "장보기|장을\\s*(?:보|봅)|(?:재래|전통|동네|수산|농수산)시장|대형\\s*마트|동네\\s*마트|마트(?=에|를|로|에서)|병원(?=에|을|으로)|은행(?=에|을)|관공서|주민센터|쇼핑|백화점|아울렛"
    },
    {"code": "SOCIAL", "pattern": "모임|동호회|경로당|수다|전화\\s*통화|통화(?=를|하|로)|동창|계모임|친목|교회|성당|봉사"},
    {"code": "REST", "pattern": "낮잠|쪽잠|명상|멍때리|티타임|휴식"},
    {
      "code": "HOBBY",
      "pattern": "게임|독서|책을|웹툰|영상|유튜브|드라마|영화|노래|음악|라디오|뜨개|그림(?!자)|사진|촬영|텃밭|화분|원예|바둑|낚시|당구|공예|서예|캘리|악기|피아노|블로그|글쓰기|일기(?!\\s*예보)|퍼즐|수집|시청(?!역|\\s*앞|\\s*근처)|감상|뉴스|신문"
    }
  ],
  "label_rules": [
    {"code": "HOBBY", "pattern": "유튜브|영상|시청|감상|웹툰|드라마|영화|예능|라디오|팟캐스트|뉴스|넷플릭스|트로트|듣|음악|청취|OTT"},
    {"code": "TRIP", "pattern": "여행|캠핑|드라이브|당일치기|펜션|리조트|글램핑|템플스테이|온천|스키|휴양"},
    {"code": "CULTURE", "pattern": "전시|박물관|미술관|공연|콘서트|뮤지컬|연극|고궁|경복궁|창덕궁|창경궁|덕수궁|경희궁|유적|영화관|갤러리|문화센터|축제|오페라|클래식|직관|관람|에버랜드|롯데월드"},
    {
      "code": "EXERCISE",
      "pattern": "산책|둘레길|등산|트레킹|조깅|러닝|달리기|걷기|수영|요가|필라테스|헬스|운동|배드민턴|탁구|테니스|골프|볼링|자전거|사이클|축구|농구|야구|스트레칭|체조|게이트볼|파크골프|클라이밍|맨발|에어로빅|줌바|복싱|검도|태권도|승마|근력|웨이트|라이딩|족구|트랙"
    },
    {"code": "CARE", "pattern": "육아|손주|손자|손녀|아이|자녀|아들|딸|돌봄|간병|부모님|반려|강아지|고양이|애견"},
    {"code": "HOUSEHOLD", "pattern": "요리|청소|정리|가계부|빨래|설거지|반찬|김치|살림|수리|DIY|인테리어|세탁|밑반찬|조리"},
    {"code": "PERSONAL", "pattern": "목욕|사우나|찜질|반신욕|미용|피부|네일|마사지|에스테틱|스파|이발|염색|한의원|외모|그루밍|바버"},
    {"code": "ERRAND", "pattern": "쇼핑|시장|마트|병원|은행|관공서|주민센터|장보기|백화점|아울렛|구매|중고|당근"},
    {"code": "MEAL", "pattern": "맛집|식당|카페|커피|빵|디저트|외식|식사|술|한잔|포장마차|막걸리|노포|미식|브런치|맥주|와인|먹|국밥|술집|치킨|회식|밥|일식집|주점|분식|시식|순례"},
    {"code": "STUDY", "pattern": "공부|자격증|학습|강좌|수강|강의|어학|영어|코딩|독서실|스터디|시험|기출|배우기|교육|외국어|한자|필사|논문|연구"},
    {"code": "WORK", "pattern": "근무|업무|부업|알바|아르바이트|영업|장사|가게|사업|투자|재테크|주식|창업|프리랜서|컨설팅|자원봉사|봉사|일자리|구직"},
    {"code": "SOCIAL", "pattern": "모임|동호회|친구|수다|동창|계모임|경로당|커뮤니티|클럽|소모임|통화|교회|성당|기도|예배|이웃|활동|친목"},
    {"code": "REST", "pattern": "낮잠|휴식|멍|명상|산림욕|힐링|쉬기|쉼|여유|티타임|차 ?마시"},
    {
      "code": "HOBBY",
      "pattern": "게임|독서|책|그림|사진|촬영|출사|카메라|뜨개|공예|도예|서예|캘리|악기|기타|피아노|노래|텃밭|화분|원예|가드닝|화초|다육|상추|채소|식물|수집|프라모델|레고|바둑|장기|당구|퍼즐|보드|블로그|글쓰기|일기|자수|퀼트|베이킹|만들기|조립|모형|낚시|꽃|분재|스크랩|신문|읽기|취미|컬렉션|퀴즈|카드|댄스|춤|레전드|동물의 숲|닌텐도|서적|탐독|만화"
    }
  ],
  "trip_clause": "여행|캠핑|드라이브|나들이|당일치기|펜션|리조트|글램핑|템플스테이|휴양",
  "trip_context": "여행|캠핑|드라이브|나들이|당일치기|펜션|리조트|글램핑|템플스테이|휴양|떠나|떠난|떠납|다녀|방문|찾아가|찾아다니|돌아다니|출국|탐방|출사|명소|관광|투어|여정|순례",
  "shift": "야간\\s*근무|교대\\s*근무",
  "place_like": "[가-힣0-9]{2,10}(?:공원|산|천|호수|시장|궁|역|도서관|미술관|박물관|체육센터|복지관|둘레길|숲길|해변|계곡|섬)",
  "district_names": ["강남", "강동", "강북", "강서", "관악", "광진", "구로", "금천", "노원", "도봉", "동대문", "동작", "마포", "서대문", "서초", "성동", "성북", "송파", "양천", "영등포", "용산", "은평", "종로", "중랑", "중"],
  "district_pattern": "(?:서울\\s*)?(동대문|서대문|영등포|강남|강동|강북|강서|관악|광진|구로|금천|노원|도봉|동작|마포|서초|성동|성북|송파|양천|용산|은평|종로|중랑|중)구(?!민체육)",
  "district_sport_pattern": "(동대문|서대문|영등포|강남|강동|강북|강서|관악|광진|구로|금천|노원|도봉|동작|마포|서초|성동|성북|송파|양천|용산|은평|종로|중랑|중)구민체육센터",
  "other_as_pool_code": "HOBBY",
  "merge_stoplist": ["가볍게", "가족과", "관리", "시간", "정기적인", "즐기기", "참여", "천천히", "친구들과", "친구와", "함께", "활동"]
}
```

## `rules/v1/location.json`

```json
{
  "home_hint": "통화|전화|온라인|집에서|거실|베란다|홈트|화상|침대|소파",
  "venue_hint": "도서관|독서실|스터디|공방|서점|노래방|당구장|낚시터|미용실|사우나|찜질|스파|마사지|피부|네일|이발|목욕탕|카페|식당|맛집|술집|주점",
  "home_codes": ["CARE", "HOUSEHOLD", "REST", "SLEEP"],
  "near_codes": ["CARE", "ERRAND", "EXERCISE", "HOUSEHOLD", "MEAL", "PERSONAL", "REST"],
  "late_home_after_min": 1380,
  "move_len_by_hops": {"0": 10, "1": 20, "2": 30, "3+": 40},
  "workplace_probs": {"same": 0.3, "adjacent": 0.4, "other": 0.3},
  "leisure_probs": {"same": 0.5, "adjacent": 0.3, "other": 0.2},
  "near_probs": {"same": 1.0, "adjacent": 0.0, "other": 0.0}
}
```

## `rules/v1/templates.json`

```json
{
  "slot_minutes": 10,
  "templates": {
    "WORK_DAY": {
      "WEEKDAY": {
        "wake": [360, 450],
        "bed": [1350, 1440],
        "work": {"start": [510, 570], "net": 480},
        "fixed_work": null,
        "flex": [
          {"code": "EXERCISE", "days": "WEEKDAY", "per_week": [2, 3], "dur": [30, 60], "window": [1110, 1350]},
          {"code": "OUTING", "days": "WEEKDAY", "per_week": [1, 1], "dur": [60, 120], "window": [1110, 1350]},
          {"code": "HOUSEHOLD", "days": "WEEKDAY", "per_week": "EVERY", "dur": [30, 60], "window": [1110, 1380]},
          {"code": "HOBBY", "days": "WEEKDAY", "per_week": "EVERY", "dur": [60, 120], "window": [1170, 1410]}
        ]
      },
      "WEEKEND": {
        "wake": [420, 540],
        "bed": [1380, 1470],
        "work": null,
        "fixed_work": null,
        "flex": [
          {"code": "NAP", "days": "WEEKEND", "per_week": [1, 1], "dur": [30, 30], "window": [780, 960]},
          {"code": "EXERCISE", "days": "WEEKEND", "per_week": [1, 1], "dur": [60, 120], "window": [480, 720]},
          {"code": "OUTING", "days": "WEEKEND", "per_week": [1, 2], "dur": [120, 240], "window": [600, 1140]},
          {"code": "HOUSEHOLD", "days": "WEEKEND", "per_week": "EVERY", "dur": [60, 120], "window": [540, 900]},
          {"code": "HOBBY", "days": "WEEKEND", "per_week": "EVERY", "dur": [120, 240], "window": [780, 1200]}
        ]
      }
    },
    "HOME_U65": {
      "WEEKDAY": {
        "wake": [390, 510],
        "bed": [1380, 1470],
        "work": null,
        "fixed_work": null,
        "flex": [
          {"code": "NAP", "days": "WEEKDAY", "per_week": [2, 2], "dur": [30, 30], "window": [780, 960]},
          {"code": "OUTING", "days": "WEEKDAY", "per_week": [2, 3], "dur": [60, 120], "window": [600, 1020]},
          {"code": "EXERCISE", "days": "WEEKDAY", "per_week": [3, 4], "dur": [30, 90], "window": [480, 690]},
          {"code": "HOUSEHOLD", "days": "WEEKDAY", "per_week": "EVERY", "dur": [120, 180], "window": [540, 960]},
          {"code": "HOBBY", "days": "WEEKDAY", "per_week": "EVERY", "dur": [60, 120], "window": [840, 1080]},
          {"code": "HOBBY", "days": "WEEKDAY", "per_week": "EVERY", "dur": [60, 120], "window": [1170, 1380]}
        ]
      },
      "WEEKEND": {
        "wake": [420, 540],
        "bed": [1380, 1470],
        "work": null,
        "fixed_work": null,
        "flex": [
          {"code": "NAP", "days": "WEEKEND", "per_week": [1, 1], "dur": [30, 30], "window": [780, 960]},
          {"code": "EXERCISE", "days": "WEEKEND", "per_week": [1, 1], "dur": [60, 120], "window": [480, 720]},
          {"code": "OUTING", "days": "WEEKEND", "per_week": [1, 2], "dur": [120, 240], "window": [600, 1140]},
          {"code": "HOUSEHOLD", "days": "WEEKEND", "per_week": "EVERY", "dur": [60, 120], "window": [540, 900]},
          {"code": "HOBBY", "days": "WEEKEND", "per_week": "EVERY", "dur": [120, 240], "window": [780, 1200]}
        ]
      }
    },
    "HOME_65P": {
      "WEEKDAY": {
        "wake": [300, 390],
        "bed": [1260, 1350],
        "work": null,
        "fixed_work": null,
        "flex": [
          {"code": "EXERCISE", "days": "WEEKDAY", "per_week": "EVERY", "dur": [60, 90], "window": [330, 600]},
          {"code": "OUTING", "days": "WEEKDAY", "per_week": [3, 4], "dur": [60, 120], "window": [570, 960]},
          {"code": "NAP", "days": "WEEKDAY", "per_week": "EVERY", "dur": [30, 60], "window": [840, 990]},
          {"code": "HOUSEHOLD", "days": "WEEKDAY", "per_week": "EVERY", "dur": [60, 120], "window": [540, 900]},
          {"code": "HOBBY", "days": "WEEKDAY", "per_week": "EVERY", "dur": [120, 180], "window": [900, 1140]}
        ]
      },
      "WEEKEND": {
        "wake": [420, 540],
        "bed": [1380, 1470],
        "work": null,
        "fixed_work": null,
        "flex": [
          {"code": "NAP", "days": "WEEKEND", "per_week": [1, 1], "dur": [30, 30], "window": [780, 960]},
          {"code": "EXERCISE", "days": "WEEKEND", "per_week": [1, 1], "dur": [60, 120], "window": [480, 720]},
          {"code": "OUTING", "days": "WEEKEND", "per_week": [1, 2], "dur": [120, 240], "window": [600, 1140]},
          {"code": "HOUSEHOLD", "days": "WEEKEND", "per_week": "EVERY", "dur": [60, 120], "window": [540, 900]},
          {"code": "HOBBY", "days": "WEEKEND", "per_week": "EVERY", "dur": [120, 240], "window": [780, 1200]}
        ]
      }
    },
    "SHIFT": {
      "WEEKDAY": {
        "wake": [870, 960],
        "bed": [420, 510],
        "work": null,
        "fixed_work": [1320, 1800],
        "flex": [
          {"code": "EXERCISE", "days": "WEEKDAY", "per_week": [2, 2], "dur": [30, 60], "window": [1000, 1200]},
          {"code": "OUTING", "days": "WEEKDAY", "per_week": [1, 1], "dur": [60, 120], "window": [960, 1200]},
          {"code": "HOUSEHOLD", "days": "WEEKDAY", "per_week": "EVERY", "dur": [30, 60], "window": [960, 1140]},
          {"code": "HOBBY", "days": "WEEKDAY", "per_week": "EVERY", "dur": [60, 120], "window": [930, 1200]}
        ]
      },
      "WEEKEND": {
        "wake": [870, 960],
        "bed": [420, 510],
        "work": null,
        "fixed_work": null,
        "flex": [
          {"code": "OUTING", "days": "WEEKEND", "per_week": [1, 2], "dur": [120, 240], "window": [960, 1260]},
          {"code": "HOUSEHOLD", "days": "WEEKEND", "per_week": "EVERY", "dur": [30, 60], "window": [960, 1140]},
          {"code": "HOBBY", "days": "WEEKEND", "per_week": "EVERY", "dur": [120, 240], "window": [930, 1260]}
        ]
      }
    }
  },
  "default_duration": {
    "EXERCISE": [30, 90],
    "MEAL": [30, 60],
    "HOBBY": [60, 180],
    "SOCIAL": [60, 180],
    "CULTURE": [120, 240],
    "ERRAND": [30, 90],
    "PERSONAL": [30, 90],
    "HOUSEHOLD": [30, 120],
    "CARE": [60, 180],
    "STUDY": [60, 180],
    "REST": [30, 60],
    "OTHER": [60, 120]
  },
  "default_label": {
    "HOUSEHOLD": "가사",
    "EXERCISE": "운동·산책",
    "HOBBY": "여가",
    "REST": "휴식",
    "SOCIAL": "모임",
    "CULTURE": "문화 활동",
    "ERRAND": "용무",
    "PERSONAL": "개인 정비",
    "MEAL": "식사",
    "CARE": "돌봄",
    "WORK": "근무",
    "SLEEP": "수면",
    "STUDY": "학습"
  },
  "meal": {
    "breakfast_after_wake": [30, 60],
    "breakfast_minutes": 30,
    "lunch_start": [720, 780],
    "lunch_minutes_workday": 60,
    "lunch_minutes_other": [30, 60],
    "dinner_start": [1080, 1170],
    "dinner_minutes": 60,
    "shift_dinner_before_work": [0, 240]
  },
  "sleep": {"min_minutes": 300, "max_minutes": 600, "wake_clamp": [240, 720], "bed_clamp": [1200, 1560], "carve_keep_minutes": 240},
  "outing_codes": ["ERRAND", "SOCIAL", "CULTURE"],
  "outing_default_weight": 1,
  "pool_daypart_weight": 3.0,
  "child_care": {"code": "CARE", "dur": [60, 120], "window": [1110, 1290]},
  "filler": {
    "personal_after_wake": 30,
    "personal_before_bed": 30,
    "absorb_max_minutes": 20,
    "rest_exact_minutes": 30,
    "long_label": "자유 시간(기본)",
    "rest_label": "휴식",
    "after_wake_label": "기상 후 준비",
    "before_bed_label": "취침 준비",
    "late_label": "취침 전 여가"
  },
  "night_meal": {"start_after_midnight": [60, 180], "minutes": 30, "label": "식사(근무 중)"},
  "work": {"label": "근무", "lunch_label": "식사(점심)"}
}
```

## `rules/v1/geo/seoul_places.csv`

서울 장소 사전 시드 177행. 열: `token`(사전 표기), `scope`(`SEOUL`), `districts`(서울 구 후보, `|` 구분, 비면 일반어), `note`, `guard`(뒤에 이 정규식이 오면 일치로 보지 않음), `ctx`(`TRIP`이면 같은 절에 여행 문맥이 있을 때만 유효). 상위 빈도 토큰(북한산·관악산·중랑천·석촌호수·안양천·한강공원·경복궁 등)을 우선 담았고, 구 후보는 행정구역 상식으로 적은 **초안**이라 Phase 7에서 공식 경계 자료와 대조해 `note`에 출처를 남긴다.

```csv
token,scope,districts,note,guard,ctx
북한산,SEOUL,종로구|성북구|강북구|도봉구|은평구,국립공원 서울 구간,,
북한산둘레길,SEOUL,종로구|성북구|강북구|도봉구|은평구,북한산 별칭,,
관악산,SEOUL,관악구|금천구,경기 안양·과천에도 걸침,,
중랑천,SEOUL,도봉구|노원구|중랑구|동대문구|성동구,하천 통과 구,,
안양천,SEOUL,금천구|구로구|영등포구|양천구|강서구,하천 통과 구,,
양재천,SEOUL,강남구|서초구,하천 통과 구,,
성북천,SEOUL,성북구|동대문구,하천 통과 구,,
정릉천,SEOUL,성북구|동대문구,하천 통과 구,,
불광천,SEOUL,은평구|서대문구|마포구,하천 통과 구,,
홍제천,SEOUL,서대문구|마포구,하천 통과 구,,
도림천,SEOUL,관악구|구로구|영등포구,하천 통과 구,,
반포천,SEOUL,서초구|동작구,하천 통과 구,,
성내천,SEOUL,송파구|강동구,하천 통과 구,,
청계천,SEOUL,종로구|중구|성동구|동대문구,하천 통과 구,,
한강공원,SEOUL,강동구|송파구|광진구|성동구|강남구|서초구|동작구|용산구|마포구|영등포구|양천구|강서구,한강 접한 12개 구,,
한강,SEOUL,강동구|송파구|광진구|성동구|강남구|서초구|동작구|용산구|마포구|영등포구|양천구|강서구,한강 접한 12개 구,,
망원한강공원,SEOUL,마포구,,,
뚝섬한강공원,SEOUL,광진구,,,
반포한강공원,SEOUL,서초구,,,
강서한강공원,SEOUL,강서구,,,
여의도한강공원,SEOUL,영등포구,,,
잠실한강공원,SEOUL,송파구,,,
광나루한강공원,SEOUL,광진구,,,
이촌한강공원,SEOUL,용산구,,,
난지한강공원,SEOUL,마포구,,,
양화한강공원,SEOUL,영등포구,,,
잠원한강공원,SEOUL,서초구,,,
석촌호수,SEOUL,송파구,,,
올림픽공원,SEOUL,송파구,,,
롯데월드,SEOUL,송파구,,,
가락시장,SEOUL,송파구,,,
수락산,SEOUL,노원구,의정부·남양주에도 걸침,,
불암산,SEOUL,노원구,남양주에도 걸침,,
도봉산,SEOUL,도봉구,의정부·양주에도 걸침,,
아차산,SEOUL,광진구,구리에도 걸침,,
용마산,SEOUL,중랑구|광진구,,,
망우산,SEOUL,중랑구,구리에도 걸침,,
봉화산,SEOUL,중랑구,,,
청계산,SEOUL,서초구,경기 과천·성남·의왕에도 걸침,,
대모산,SEOUL,강남구,성남에도 걸침,,
일자산,SEOUL,강동구,하남에도 걸침,,
개화산,SEOUL,강서구,,,
인왕산,SEOUL,종로구|서대문구,,,
남산,SEOUL,중구|용산구,,,
서울숲,SEOUL,성동구,,,
뚝섬,SEOUL,성동구|광진구,,,
어린이대공원,SEOUL,광진구,,,
보라매공원,SEOUL,동작구,,,
도산공원,SEOUL,강남구,,,
서리풀공원,SEOUL,서초구,,,
용산가족공원,SEOUL,용산구,,,
허브천문공원,SEOUL,강서구,,,
월드컵공원,SEOUL,마포구,,,
하늘공원,SEOUL,마포구,,,
경복궁,SEOUL,종로구,,,
창덕궁,SEOUL,종로구,,,
창경궁,SEOUL,종로구,,,
덕수궁,SEOUL,중구,,,
광화문,SEOUL,종로구,,,
국립중앙박물관,SEOUL,용산구,,,
국립현대미술관,SEOUL,종로구|중구,과천관은 서울 밖,,
리움미술관,SEOUL,용산구,,,
서울시립미술관,SEOUL,중구,,,
예술의전당,SEOUL,서초구,,,
코엑스,SEOUL,강남구,,,
동대문디자인플라자,SEOUL,중구,,,
N서울타워,SEOUL,용산구,,,
광장시장,SEOUL,종로구,,,
경동시장,SEOUL,동대문구,,,
망원시장,SEOUL,마포구,,,
구로시장,SEOUL,구로구,,,
남대문시장,SEOUL,중구,,,
노량진수산시장,SEOUL,동작구,,,
강남역,SEOUL,강남구|서초구,,,
수유역,SEOUL,강북구,,,
마곡나루역,SEOUL,강서구,,,
마곡역,SEOUL,강서구,,,
발산역,SEOUL,강서구,,,
신림역,SEOUL,관악구,,,
낙성대역,SEOUL,관악구,,,
청량리역,SEOUL,동대문구,,,
상봉역,SEOUL,중랑구,,,
망우역,SEOUL,중랑구,,,
건대입구역,SEOUL,광진구,,,
신도림역,SEOUL,구로구|영등포구,,,
노원역,SEOUL,노원구,,,
천호역,SEOUL,강동구,,,
용산역,SEOUL,용산구,,,
서울역,SEOUL,용산구|중구,,,
불광역,SEOUL,은평구,,,
합정역,SEOUL,마포구,,,
창동역,SEOUL,도봉구,,,
금천구청역,SEOUL,금천구,,,
한성대입구역,SEOUL,성북구,,,
영등포역,SEOUL,영등포구,,,
성수동,SEOUL,성동구,,,
왕십리,SEOUL,성동구,,,
망원동,SEOUL,마포구,,,
연남동,SEOUL,마포구,,,
상수동,SEOUL,마포구,,,
합정,SEOUL,마포구,,,
홍대,SEOUL,마포구,,,
상암,SEOUL,마포구,,,
한남동,SEOUL,용산구,,,
이태원,SEOUL,용산구,,,
이촌동,SEOUL,용산구,,,
용산,SEOUL,용산구,,,
성북동,SEOUL,성북구,,,
상계동,SEOUL,노원구,,,
서초동,SEOUL,서초구,,,
반포,SEOUL,서초구,,,
서래마을,SEOUL,서초구,,,
고속터미널,SEOUL,서초구,,,
신림동,SEOUL,관악구,,,
망우동,SEOUL,중랑구,,,
을지로,SEOUL,중구,,,
명동,SEOUL,중구,,,
충무로,SEOUL,중구,,,
남대문,SEOUL,중구,,,
여의도,SEOUL,영등포구,,,
잠실,SEOUL,송파구,,,
압구정,SEOUL,강남구,,,
가로수길,SEOUL,강남구,,,
삼성동,SEOUL,강남구,,,
역삼동,SEOUL,강남구,,,
대치동,SEOUL,강남구,,,
삼청동,SEOUL,종로구,,,
인사동,SEOUL,종로구,,,
북촌,SEOUL,종로구,,,
서촌,SEOUL,종로구,,,
익선동,SEOUL,종로구,,,
종로,SEOUL,종로구,,,
신촌,SEOUL,서대문구,,,
목동,SEOUL,양천구,,,
노량진,SEOUL,동작구,,,
전통시장,SEOUL,,일반어,,
재래시장,SEOUL,,일반어,,
근린공원,SEOUL,,일반어,,
시민공원,SEOUL,,일반어,,
체육공원,SEOUL,,일반어,,
사회복지관,SEOUL,,일반어,,
노인복지관,SEOUL,,일반어,,
복지관,SEOUL,,일반어,,
주민센터,SEOUL,,일반어,,
경로당,SEOUL,,일반어,,
동네,SEOUL,,일반어,,
집 근처,SEOUL,,일반어,,
카페거리,SEOUL,,일반어,,
먹자골목,SEOUL,,일반어,,
서울식물원,SEOUL,강서구,,,
서울,SEOUL,,일반어(서울 안),,
강남,SEOUL,강남구|서초구,구 약칭,,
서초,SEOUL,서초구,구 약칭,,
송파,SEOUL,송파구,구 약칭,,
마포,SEOUL,마포구,구 약칭,,
영등포,SEOUL,영등포구,구 약칭,,
성북,SEOUL,성북구,구 약칭,,
성동,SEOUL,성동구,구 약칭,,
노원,SEOUL,노원구,구 약칭,,
도봉,SEOUL,도봉구,구 약칭,,
은평,SEOUL,은평구,구 약칭,,
서대문,SEOUL,서대문구,구 약칭,,
동작,SEOUL,동작구,구 약칭,,
양천,SEOUL,양천구,구 약칭,,
구로,SEOUL,구로구,구 약칭,,
금천,SEOUL,금천구,구 약칭,,
강서,SEOUL,강서구,구 약칭,,
강동,SEOUL,강동구,구 약칭,,
강북,SEOUL,강북구,구 약칭,,
광진,SEOUL,광진구,구 약칭,,
중랑,SEOUL,중랑구,구 약칭,,
동대문,SEOUL,동대문구|중구,구 약칭,,
식물원,SEOUL,,일반어,,
도서관,SEOUL,,일반어,,
구민체육센터,SEOUL,,일반어,,
고궁,SEOUL,종로구|중구,궁궐 일반어(서울의 고궁은 종로·중구),,
궁궐,SEOUL,종로구|중구,궁궐 일반어,,
```

## `rules/v1/geo/outside_places.csv`

서울 밖 지명 사전 시드 115행. 같은 열을 쓰며 `scope`는 `OUTSIDE`, `districts`는 비어 있다. 동음이의어(청주·고양·공주·대구)는 `guard`로, 국가·해외·일반어는 `ctx=TRIP`로 오탐을 막는다.

```csv
token,scope,districts,note,guard,ctx
강원도,OUTSIDE,,도,,
제주도,OUTSIDE,,도,,
제주,OUTSIDE,,도,,
경기도,OUTSIDE,,도,,
충청도,OUTSIDE,,도,,
전라도,OUTSIDE,,도,,
경상도,OUTSIDE,,도,,
평창,OUTSIDE,,강원,,
인제,OUTSIDE,,강원,,
양양,OUTSIDE,,강원,,
정선,OUTSIDE,,강원,,
강릉,OUTSIDE,,강원,,
속초,OUTSIDE,,강원,,
춘천,OUTSIDE,,강원,,
삼척,OUTSIDE,,강원,,
영월,OUTSIDE,,강원,,
태백,OUTSIDE,,강원,,
동해시,OUTSIDE,,강원,,
가평,OUTSIDE,,경기,,
양평,OUTSIDE,,경기,,
포천,OUTSIDE,,경기,,
파주,OUTSIDE,,경기,,
일산,OUTSIDE,,경기,,
판교,OUTSIDE,,경기,,
분당,OUTSIDE,,경기,,
수원,OUTSIDE,,경기,,
성남,OUTSIDE,,경기,,
용인,OUTSIDE,,경기,,
고양,OUTSIDE,,경기,이,
과천,OUTSIDE,,경기,,
남양주,OUTSIDE,,경기,,
의정부,OUTSIDE,,경기,,
하남,OUTSIDE,,경기,,
구리시,OUTSIDE,,경기,,
부천,OUTSIDE,,경기,,
안양,OUTSIDE,,경기,,
광명,OUTSIDE,,경기,,
김포,OUTSIDE,,경기,,
남한산성,OUTSIDE,,경기 광주·성남·하남,,
남한산,OUTSIDE,,경기 광주·성남·하남,,
북한산성,OUTSIDE,,경기 고양,,
서울대공원,OUTSIDE,,경기 과천,,
에버랜드,OUTSIDE,,경기 용인,,
인천,OUTSIDE,,,,
송도,OUTSIDE,,인천,,
경주,OUTSIDE,,경북,,
안동,OUTSIDE,,경북,,
포항,OUTSIDE,,경북,,
영주,OUTSIDE,,경북,,
문경,OUTSIDE,,경북,,
울릉도,OUTSIDE,,경북,,
독도,OUTSIDE,,경북,,
부여,OUTSIDE,,충남,,
공주,OUTSIDE,,충남,님|처럼|같은|풍,
태안,OUTSIDE,,충남,,
보령,OUTSIDE,,충남,,
천안,OUTSIDE,,충남,,
온양온천,OUTSIDE,,충남,,
충주,OUTSIDE,,충북,,
제천,OUTSIDE,,충북,,
단양,OUTSIDE,,충북,,
청주,OUTSIDE,,충북,[을를]\s*(?:한\s*잔|마시|곁들),
대전,OUTSIDE,,,,
전주,OUTSIDE,,전북,,
군산,OUTSIDE,,전북,,
순천,OUTSIDE,,전남,,
여수,OUTSIDE,,전남,,
담양,OUTSIDE,,전남,,
보성,OUTSIDE,,전남,,
목포,OUTSIDE,,전남,,
남해,OUTSIDE,,경남,,
통영,OUTSIDE,,경남,,
거제,OUTSIDE,,경남,,
진주,OUTSIDE,,경남,,
김해,OUTSIDE,,경남,,
부산,OUTSIDE,,,,
울산,OUTSIDE,,,,
대구,OUTSIDE,,,탕|전|조림,
광주,OUTSIDE,,,,
세종,OUTSIDE,,,,
설악산,OUTSIDE,,강원,,
지리산,OUTSIDE,,,,
내장산,OUTSIDE,,전북,,
용문산,OUTSIDE,,경기 양평,,
오대산,OUTSIDE,,강원,,
사려니숲길,OUTSIDE,,제주,,
한라산,OUTSIDE,,제주,,
해외,OUTSIDE,,일반어,,TRIP
국내외,OUTSIDE,,일반어,,TRIP
전국,OUTSIDE,,일반어,,TRIP
지방,OUTSIDE,,일반어,,TRIP
외국,OUTSIDE,,일반어,,TRIP
일본,OUTSIDE,,국가,,TRIP
도쿄,OUTSIDE,,해외,,TRIP
오사카,OUTSIDE,,해외,,TRIP
후쿠오카,OUTSIDE,,해외,,TRIP
교토,OUTSIDE,,해외,,TRIP
베트남,OUTSIDE,,국가,,TRIP
다낭,OUTSIDE,,해외,,TRIP
태국,OUTSIDE,,국가,,TRIP
방콕,OUTSIDE,,해외,,TRIP
대만,OUTSIDE,,국가,,TRIP
타이베이,OUTSIDE,,해외,,TRIP
싱가포르,OUTSIDE,,국가,,TRIP
유럽,OUTSIDE,,대륙,,TRIP
파리,OUTSIDE,,해외,,TRIP
런던,OUTSIDE,,해외,,TRIP
이탈리아,OUTSIDE,,국가,,TRIP
스페인,OUTSIDE,,국가,,TRIP
미국,OUTSIDE,,국가,,TRIP
뉴욕,OUTSIDE,,해외,,TRIP
하와이,OUTSIDE,,해외,,TRIP
캐나다,OUTSIDE,,국가,,TRIP
호주,OUTSIDE,,국가,,TRIP
홍콩,OUTSIDE,,해외,,TRIP
```

## 난수 정답 벡터 (KeyedRng)

[I-05-a](02_impl_map.md#i-05-a--시간-눈금-키드-난수-스키마)의 `KeyedRng`가 아래 값을 **정확히** 내야 한다(SHA-256 시드 + SplitMix64). 플랫폼·실행 순서와 무관해야 한다.

```json
{
  "KeyedRng(1,'a').u64() x3": [
    12308909679053736194,
    2681302019881911891,
    1185002552124725586
  ],
  "KeyedRng(20261030,'0000000000000000000000000000abcd',2026,45,1,'anchors').randint(0,9) x5": [
    3,
    8,
    1,
    6,
    5
  ],
  "...then .step(360,450) x3": [
    360,
    420,
    410
  ],
  "KeyedRng(7).u64() x2": [
    15299818559002601059,
    15839335979461772782
  ],
  "KeyedRng(20261030,'u',2026,45,'wk',3).sample(range(7),3)": [
    6,
    2,
    1
  ],
  "KeyedRng(1,'w').random() x2": [
    0.84150627504,
    0.948453268594
  ]
}
```

## Changelog

- 2026-10-31: 최초 작성. 프로토타입에서 검증한 규칙 데이터 v1을 구현체 Map 부록으로 고정했다.
