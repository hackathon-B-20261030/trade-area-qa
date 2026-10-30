"""Prepare an explicitly assumed, fixed-person replay on the source Myeongdong map.

This is a direct-arrival experiment, not a calibrated prediction or the complete
02c persona/space preparation pipeline. It never overwrites an existing run.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
import shutil
import sys
from collections import Counter
from pathlib import Path

import networkx as nx
import numpy as np
import pyarrow as pa
import pyarrow.parquet as pq
import yaml

import audit_myeongdong_ui_coordinates as coordinate_audit

ROOT = Path(__file__).resolve().parents[1]
MAP = ROOT / 'data/myeongdong-3d-map/map_data.json'
OSM = ROOT / 'data/myeongdong-3d-map/source/osm_raw.json'
BOUNDARY = ROOT / 'data/myeongdong-terrain/boundary.geojson'
PERSONAS = ROOT / 'data/myeongdong-augmented/personas.parquet'
BUSINESS = ROOT / 'apps/myeongdong-ui/src/data/building-business-data.json'
FIXTURE = ROOT / 'sim/runs/fixture'
SPEC = ROOT / 'docs/maps/02c_reaction_impl_map.md'
DISTRICT = {'종로구':'11110','중구':'11140','용산구':'11170','성동구':'11200','광진구':'11215','동대문구':'11230','중랑구':'11260','성북구':'11290','강북구':'11305','도봉구':'11320','노원구':'11350','은평구':'11380','서대문구':'11410','마포구':'11440','양천구':'11470','강서구':'11500','구로구':'11530','금천구':'11545','영등포구':'11560','동작구':'11590','관악구':'11620','서초구':'11650','강남구':'11680','송파구':'11710','강동구':'11740'}
ALLOWED = {'pedestrian','footway','steps','path','corridor','residential','service','living_street','primary','secondary','tertiary','primary_link','secondary_link','tertiary_link'}
FULL_WIDTH = {'pedestrian':8.,'footway':3.,'steps':2.5,'path':2.,'corridor':3.,'residential':6.5,'service':4.,'living_street':6.}
SIDE_WIDTH = {'primary':4.,'secondary':3.,'tertiary':2.5,'primary_link':2.,'secondary_link':2.,'tertiary_link':2.}
LANES = {'primary':6,'secondary':4,'tertiary':2,'primary_link':1,'secondary_link':1,'tertiary_link':1}
OFFER = {'restaurant':('store','restaurant','food',.8,15000.,.5,40,660,1320),'cafe':('cafe','cafe','food',.6,7000.,1.,30,480,1320),'fashion':('store','clothes','fashion',.8,50000.,.5,30,630,1320),'convenience':('store','convenience','food',.2,5000.,2.,15,0,1440)}
AGE_KNOTS = {
 'climb':([[19,8],[30,8],[60,6.2],[75,5.3],[90,4.4]],[[19,18],[30,18],[60,13.5],[75,11.25],[90,9]]),
 'fall':([[19,.30],[40,.30],[70,.21],[90,.15]],[[19,.70],[40,.70],[70,.52],[90,.40]]),
 'walk':([[19,2500],[40,2500],[70,2050],[90,1750]],[[19,6000],[40,6000],[70,4800],[90,4000]]),
 'step':([[19,18],[60,18],[80,16],[90,15]],[[19,30],[60,30],[80,25],[90,22.5]]),
}


def sha(path: Path) -> str:
    h = hashlib.sha256()
    with path.open('rb') as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def income_spec() -> dict:
    section = SPEC.read_text().split('### I-215-a',1)[1]
    block = section.split('```yaml\n',1)[1].split('\n```',1)[0]
    spec = yaml.safe_load(block)
    if spec.get('version') != '1.2.0':
        raise ValueError('Income spec version is not the approved 1.2.0')
    return spec


def select_profiles(table: pa.Table, n: int = 5044) -> list[dict]:
    if n <= 0 or n > table.num_rows:
        raise ValueError('Profile count is outside available range')
    rows = table.to_pylist()
    ids = [r.get('synthetic_id') for r in rows]
    if any(not isinstance(i, str) or not i for i in ids) or len(ids) != len(set(ids)):
        raise ValueError('Missing or duplicate synthetic_id')
    return sorted(rows, key=lambda r: (hashlib.sha256(r['synthetic_id'].encode()).digest(), r['synthetic_id']))[:n]


def estimate_income(row: dict, spec: dict) -> tuple[int, str, str | None]:
    age = int(row['age'])
    occupation = str(row.get('occupation') or '')
    if not occupation:
        raise ValueError('Empty occupation')
    cls = next((x['class'] for x in spec['occupation_classes'] if re.search(x['pattern'], occupation)), None)
    if cls in ('unemployed', 'student'):
        group = 'student' if cls == 'student' or age <= 24 else ('homemaker' if row.get('sex') == '여자' and age <= 64 else 'other')
        key = group if group != 'other' else ('other_65_plus' if age >= 65 else 'other_under_65')
        return int(spec['non_earner_income_krw'][key]), 'assumption', group
    earner = spec['earner']
    index = next(i for i,(lo,hi) in enumerate(earner['age_bands']) if lo <= age <= hi)
    sex = {'남자':'male','여자':'female'}.get(row.get('sex'))
    ratio = earner['sex_ratio'][cls].get(sex, 1.0)
    value = earner['base_month_krw'][cls] * earner['age_profile'][cls][index] * ratio
    return int(round(value / 1000.0) * 1000), earner.get('base_label',{}).get(cls,'statistics'), None


def profile_from_row(row: dict, spec: dict) -> tuple[dict, dict]:
    age = int(row['age'])
    if not 19 <= age <= 120:
        raise ValueError(f'Invalid age: {age}')
    pid = 'P:' + row['synthetic_id']
    district = str(row.get('district') or '').replace('서울-', '')
    if district not in DISTRICT:
        raise ValueError(f'Unknown Seoul district: {district}')
    p = {'persona_id':pid,'group':'domestic','origin_id':'G:'+DISTRICT[district]}
    interp = lambda knots: float(np.interp(age, [k[0] for k in knots], [k[1] for k in knots]))
    caps = {k:(interp(v[0]),interp(v[1]),'structured') for k,v in AGE_KNOTS.items()}
    caps.update({'crowd':(.7,2.,'assumption'),'sense':(.4,.9,'assumption'),'safe':(.3,.8,'assumption'),'way':(.8 if district=='중구' else .6,1.,'structured')})
    income, income_label, group = estimate_income(row, spec)
    ref = spec['price_sensitivity']['ref_income_krw']
    budget = spec['budget']
    if group:
        money = budget['non_earner'][group]
        comfort, limit = float(money['comfort']), float(money['limit'])
    else:
        comfort = max(budget['floor_krw']['comfort'],budget['comfort_ref_krw']*(income/ref)**budget['income_exponent']['comfort'])
        limit = max(budget['floor_krw']['limit'],budget['limit_ref_krw']*(income/ref)**budget['income_exponent']['limit'])
    caps['money'] = (comfort,limit,'statistics')
    for key,(comfort,limit,label) in caps.items():
        if not (0 <= comfort < limit):
            raise ValueError(f'Invalid {key} capability')
        p.update({f'cap_{key}_comfort':comfort,f'cap_{key}_limit':limit,f'cap_{key}_uncertainty':.25 if label=='assumption' else .10,f'cap_{key}_label':label})
    for dim in ('climb','fall','step','walk','time','money','way','sense','crowd','safe'):
        p['sens_'+dim] = 1.0
    ss=spec['price_sensitivity']
    p['sens_money'] = max(ss['min'],min(ss['max'],(ref/income)**ss['exponent']))
    for interest in ('food','beauty','fashion','souvenir','culture','faith','nature'):
        p['interest_'+interest] = .2
    p.update({'risk':.5,'punctuality':.5,'exploration':.5,'attention':.5,'tau':.5,
      'base_speed_mps':1.34*interp([[19,1.],[60,1.],[75,.85],[90,.75]]),
      'child_prob':.25 if age<=55 and any(s in str(row.get('family_type','')) for s in ('자녀','손자녀')) else 0.,
      'stroller_prob':0.,'luggage_prob':0.,
      'focal_range_m':interp([[19,25],[60,25],[75,20],[90,15]]),
      'focal_angle_deg':interp([[19,60],[60,50],[75,40],[90,30]]),
      'peripheral_range_m':10.,'peripheral_angle_deg':150.,'head_turn_deg':90.,'min_dwell_s':1.,
      'purposes':[{'purpose':'other','interest':'','value':1.,'target_place_id':None}],
      'availability':[{'day_type':'weekend','start_min':600,'end_min':1350}],
      'evidence':[],'profile_version':'direct-structured-v1'})
    attrs={'persona_id':pid,'group':'domestic','age':age,'age_band':'60대이상' if age>=60 else f'{age//10*10}대','sex':row['sex'],'nationality':'KR','income_krw':income,'income_label':income_label,'source_ref':row['synthetic_id']}
    return p,attrs


def to_local(lon: float, lat: float) -> tuple[float,float]:
    return (lon-126.98525)*111320.0*math.cos(math.radians(37.563)), -(lat-37.563)*111132.0


def inside(point: tuple[float,float], rings: list) -> bool:
    x,z=point; result=False
    for ring in rings:
        hit=False
        for (x0,z0),(x1,z1) in zip(ring,ring[1:]+ring[:1]):
            if (z0>z)!=(z1>z) and x < (x1-x0)*(z-z0)/(z1-z0)+x0:
                hit=not hit
        result ^= hit
    return result


def terrain_at(lon: float, lat: float, terrain: dict) -> float:
    a,_,c,_,e,f = terrain['transform']
    col=max(0.,min(terrain['width']-1.000001,(lon-c)/a-.5))
    row=max(0.,min(terrain['height']-1.000001,(lat-f)/e-.5))
    x,y=int(col),int(row); dx,dy=col-x,row-y; vals=terrain['values']
    value=(1-dy)*((1-dx)*vals[y][x]+dx*vals[y][x+1])+dy*((1-dx)*vals[y+1][x]+dx*vals[y+1][x+1])
    if not math.isfinite(value): raise ValueError('Invalid terrain')
    return float(value)


def project(point, edge):
    (x0,z0),(x1,z1)=edge['coords'];x,z=point; dx=x1-x0;dz=z1-z0
    length2=dx*dx+dz*dz;t=max(0.,min(1.,((x-x0)*dx+(z-z0)*dz)/length2)) if length2 else 0.
    ax,az=x0+t*dx,z0+t*dz
    return math.hypot(x-ax,z-az),t,ax,az,dx*(z-az)-dz*(x-ax)


def build_space(osm: dict, map_data: dict, businesses: dict, boundary: dict) -> dict:
    coordinates=boundary['features'][0]['geometry']['coordinates']
    rings=[[to_local(lon,lat) for lon,lat in ring] for polygon in coordinates for ring in polygon]
    all_nodes={e['id']:e for e in osm['elements'] if e.get('type')=='node' and 'lat' in e and 'lon' in e}
    points={i:to_local(n['lon'],n['lat']) for i,n in all_nodes.items()}
    flags={i:inside(p,rings) for i,p in points.items()}
    ways=[w for w in osm['elements'] if w.get('type')=='way']
    graph=nx.Graph(); candidates=[]; excluded=Counter()
    for way in ways:
        tags=way.get('tags',{}); kind=tags.get('highway');
        if kind not in ALLOWED: continue
        if tags.get('area')=='yes' or (tags.get('foot') in ('no','private')) or (tags.get('access') in ('no','private') and tags.get('foot')!='yes'):
            excluded['access_or_area']+=1;continue
        if str(tags.get('layer','0')).startswith('-') or tags.get('tunnel') in ('yes','building_passage'):
            excluded['underground_way']+=1;continue
        for index,(a,b) in enumerate(zip(way.get('nodes',[]),way.get('nodes',[])[1:])):
            if a not in points or b not in points or a==b or not (flags[a] or flags[b]):continue
            pa0,pb0=points[a],points[b];length=math.dist(pa0,pb0)
            if length<.1:continue
            graph.add_edge(a,b); candidates.append((way,index,a,b,pa0,pb0,length))
    if not graph: raise ValueError('No walkable OSM edges')
    main=max(nx.connected_components(graph),key=len)
    selected=[e for e in candidates if e[2] in main and e[3] in main]
    excluded['disconnected_edges']=len(candidates)-len(selected)
    used=sorted({n for e in selected for n in (e[2],e[3])})
    node_rows=[]
    for n in used:
        raw=all_nodes[n];tags=raw.get('tags',{});cross=tags.get('highway') in ('crossing','traffic_signals') or tags.get('crossing')=='traffic_signals'
        node_rows.append({'node_id':f'N:{n}','x':points[n][0],'z':points[n][1],'elev_m':terrain_at(raw['lon'],raw['lat'],map_data['terrain']),'elev_source':'extended','layer':0,'kind':'crossing' if cross else 'junction','signal':tags.get('highway')=='traffic_signals' or tags.get('crossing')=='traffic_signals','cross_safe_demand':.1 if cross else 0.,'inside_boundary':flags[n]})
    elevations={int(n['node_id'][2:]):n['elev_m'] for n in node_rows}
    edges=[]
    for way,index,a,b,pa0,pb0,length in selected:
        tags=way.get('tags',{});kind=tags['highway'];side=kind in SIDE_WIDTH;sw=SIDE_WIDTH.get(kind,0.)
        try: lanes=sum(int(x) for x in str(tags.get('lanes',LANES.get(kind,1))).split(';'))
        except ValueError: lanes=LANES.get(kind,1)
        try: osm_width=float(str(tags.get('width','')).replace(' m',''))
        except ValueError: osm_width=0.
        width=(max(1,lanes)*3.25+2*sw) if side else (osm_width if .5<=osm_width<=80 else FULL_WIDTH[kind])
        mixed=kind in ('residential','service','living_street');cross=tags.get('footway')=='crossing';is_steps=kind=='steps'
        grade=max(-99.,min(99.,(elevations[b]-elevations[a])/length*100.))
        row={'edge_id':f'E:w{way["id"]}:{index}','a':f'N:{a}','b':f'N:{b}','class':'steps' if is_steps else 'walk_segment','polyline':[{'x':pa0[0],'z':pa0[1]},{'x':pb0[0],'z':pb0[1]}], 'length_m':length,'road_type':kind,'name':tags.get('name',''),'osm_way_id':way['id'],'layer':0,'width_m':width,'label_width':'osm' if not side and osm_width else 'assumption','walk_mode':'sidewalks' if side else 'full','sidewalk_width_m':sw,'vehicle_mixed':mixed,'ped_zone':mixed and bool(re.fullmatch(r'명동(\d+[가-힣]?)?길',tags.get('name',''))),'is_crossing':cross,'carriage_share':.5 if mixed else 0.,'grade_pct':grade,'grade_pct_orig':None,'grade_pct_smooth':grade,'grade_source':'smooth','grade_baseline_m':length,'is_steps':is_steps,'riser_cm':15. if is_steps else 0.,'steps_count':max(1,round(abs(elevations[b]-elevations[a])*100/15)) if is_steps else 0,'conveying':tags.get('conveying') in ('yes','forward','backward','reversible'),'surface':'dry','ad_intensity':{'pedestrian':.6,'primary':.5,'secondary':.5,'residential':.4,'footway':.3}.get(kind,.2),'way_difficulty':{'pedestrian':.2,'primary':.3,'secondary':.3,'footway':.4,'steps':.4,'residential':.5,'service':.5,'corridor':.7}.get(kind,.4),'safe_demand':.3 if cross else (.4 if mixed else 0.),'component':0,'inside_boundary':flags[a] and flags[b],'assumed':['width_m','ad_intensity','way_difficulty','safe_demand']}
        edges.append({'row':row,'coords':(pa0,pb0)})
    if not edges:raise ValueError('Empty connected network')
    nodes_by_id={r['node_id']:r for r in node_rows}
    # Portals are snapped to a nearby node touching a full-width surface path.
    full_ids={e['row'][k] for e in edges if e['row']['walk_mode']=='full' for k in ('a','b')}
    full_nodes=[nodes_by_id[n] for n in sorted(full_ids)]
    portals=[]; portal_dist=[]
    for n in all_nodes.values():
        tags=n.get('tags',{});kind='subway' if tags.get('railway')=='subway_entrance' else ('bus' if tags.get('highway')=='bus_stop' else None)
        if not kind or not flags[n['id']]:continue
        x,z=points[n['id']];target=min(full_nodes,key=lambda a:math.hypot(a['x']-x,a['z']-z));distance=math.hypot(target['x']-x,target['z']-z)
        if distance>30.:excluded['portal_over_30m']+=1;continue
        portals.append({'portal_id':f'Q:{kind}:{n["id"]}','kind':kind,'node_id':target['node_id'],'x':target['x'],'z':target['z'],'name':tags.get('name') or tags.get('description') or kind,'headway_min':6. if kind=='subway' else 0.,'groups':['domestic']});portal_dist.append(distance)
    if not portals:raise ValueError('No real surface portals within 30m of connected paths')
    buildings=[];places=[]; common=0
    for building in map_data['buildings']:
        fp=building['footprint'];ident=building['id'];
        buildings.append({'building_id':'B:'+ident,'footprint':[{'x':x,'z':z} for x,z in fp],'height_m':float(building['height']),'base_m':float(building['base']),'min_height_m':float(building.get('minHeight',0)),'name':building.get('name'),'source_ids':building.get('source_ids',[])})
        business=businesses.get(ident)
        if not business:continue
        common+=1;kind=business.get('type')
        if kind not in OFFER:continue
        x=sum(p[0] for p in fp)/len(fp);z=sum(p[1] for p in fp)/len(fp)
        if not inside((x,z),rings):continue
        edge=min(edges,key=lambda e:project((x,z),e)[0]);dist,t,ax,az,cross=project((x,z),edge);er=edge['row']
        if dist>80:excluded['place_over_80m']+=1;continue
        if er['walk_mode']=='sidewalks':
            (x0,z0),(x1,z1)=edge['coords'];length=er['length_m'];offset=er['width_m']/2-er['sidewalk_width_m']/2;sgn=1 if cross>=0 else -1
            ax+=sgn*(-(z1-z0)/length)*offset;az+=sgn*((x1-x0)/length)*offset
        cls,category,dim,strength,price,rate,capacity,op,cl=OFFER[kind]
        offer={f'offer_{d}':strength if d==dim else 0. for d in ('food','beauty','fashion','souvenir','culture','faith','nature')}
        places.append({'place_id':'S:'+ident,'class':cls,'category':category,'name':business.get('name') or building.get('name') or '이름 없는 건물','x':x,'z':z,'edge_id':er['edge_id'],'offset_m':t*er['length_m'],'anchor_x':ax,'anchor_z':az,'side':'left' if cross>=0 else 'right','layer':0,'floor':1,**offer,'price_level_krw':price,'discount_pct':0.,'open_min':op,'close_min':cl,'day_types':['weekday','weekend','event_day'],'visibility':1.,'fame':.2,'foreign_label':False,'service_rate_per_min':rate,'capacity':capacity,'seats':None,'footprint_m2':None,'sense':None,'rent_krw_month':None,'staff_count':None,'wage_krw_month':None,'revenue_krw_month':None,'source':'user','label':'assumption','assumed':['category_proxy','price_level_krw','open_min','close_min','service_rate_per_min','capacity','visibility'],'store_ref':ident})
    if not places:raise ValueError('No matching business places')
    return {'nodes':node_rows,'edges':[e['row'] for e in edges],'buildings':buildings,'places':places,'portals':portals,'stats':{'osm_nodes':len(node_rows),'osm_edges':len(edges),'buildings':len(buildings),'common_business_buildings':common,'proxy_places':len(places),'portals':len(portals),'portal_max_snap_m':max(portal_dist),'excluded':dict(excluded)}}


def write_bundle(out: Path, tables: dict, fixture_root: Path) -> Path:
    if out.exists() and any(out.iterdir()):raise FileExistsError(f'Output directory is not empty: {out}')
    out.mkdir(parents=True,exist_ok=True); inputs=out/'input';inputs.mkdir(); defs=out/'defs';shutil.copytree(fixture_root/'ontology/v1',defs)
    schema_root=fixture_root/'data/reaction-inputs/fixture';files={}
    for name,rows in tables.items():
        if name=='stats':continue
        template=pq.read_schema(schema_root/(name+'.parquet'))
        table=pa.Table.from_pylist(rows,schema=template)
        path=inputs/(name+'.parquet');pq.write_table(table,path,compression='snappy',row_group_size=5000)
        files[name]=[{'path':str(path.relative_to(out)),'sha256':sha(path),'rows':table.num_rows}]
    scenario={'id':'myeongdong_direct_5044','schema_version':1,'date':'2026-11-07','day_type':'weekend','window':{'start':'14:00','end':'17:00'},'dt_s':.5,'render_every_ticks':2,'seed':9599481731617930288,'target_peak_concurrency':2000,'representation_ratio':1.,'pedestrian_only':[],'rent_support_pct':0,'events':[],'headway_multiplier':1.,'demand_multiplier':1.,'foreign_inflow_multiplier':1.,'labor_cost_index':1.,'interactions':True,'trace_bodies':30,'configuration':'static_stores','months':0,'event_days_per_month':0,'static_applied':{}}
    bundle=out/'bundle';bundle.mkdir();sp=bundle/'scenario.yaml';sp.write_text(yaml.safe_dump(scenario,allow_unicode=True,sort_keys=False))
    definition_sha=hashlib.sha256(b''.join(p.read_bytes() for p in sorted(defs.glob('*.yaml')))).hexdigest()
    shas={k:[x['sha256'] for x in v] for k,v in sorted(files.items())};canon={'ontology_sha256':definition_sha,'files':shas,'scenario_sha256':[sha(sp)]}
    bid=hashlib.sha256(json.dumps(canon,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()).hexdigest()[:16]
    manifest={'schema_version':1,'bundle_id':bid,'ontology_sha256':definition_sha,'ontology_validation':{'status':'synthetic','validator':'mdsim fixture definitions, not full ontology','dimensions_version':'1.0.0','checks':0},'space_id':'realmap_direct_v1','files':files,'scenarios':[{'day_type':'weekend','rep':0,'seed':scenario['seed'],'path':'bundle/scenario.yaml','sha256':sha(sp)}],'pending_inputs':['full_persona_extraction:DR-260','south_osm:DR-202','full_store_mapping:DR-226'],'assumption_flags':['development_fixture_definitions','structured_profile_defaults','forced_arrivals','surface_only','building_proxy_places'],'producer':{'package':'myeongdong-direct-replay','version':'0.1.0'}}
    (bundle/'manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
    return bundle


def main() -> int:
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--out',required=True,type=Path);parser.add_argument('--count',type=int,default=5044);parser.add_argument('--fixture-root',type=Path,default=FIXTURE);args=parser.parse_args()
    try:
        if args.out.exists() and any(args.out.iterdir()):raise FileExistsError(f'Output directory is not empty: {args.out}')
        osm=json.loads(OSM.read_text(encoding='utf-8-sig'));map_data=json.loads(MAP.read_text());businesses=json.loads(BUSINESS.read_text());boundary=json.loads(BOUNDARY.read_text());
        space=build_space(osm,map_data,businesses,boundary)
        cols=['synthetic_id','age','sex','occupation','district','family_type']
        selected=select_profiles(pq.read_table(PERSONAS,columns=cols),args.count);spec=income_spec();pairs=sorted((profile_from_row(r,spec) for r in selected),key=lambda pair:pair[0]['persona_id']);profiles=[p[0] for p in pairs];attrs=[p[1] for p in pairs]
        origins=sorted({p['origin_id'] for p in profiles});access=[{'origin_id':o,'destination':'myeongdong','mode':'walk','base_min':30.,'fare_krw':0.,'hops':None,'access_mode':'uniform','label':'assumption'} for o in origins]
        tables={**space,'profiles':profiles,'profile_attrs':attrs,'access':access,'competitors':[]}
        write_bundle(args.out,tables,args.fixture_root)
        alignment=coordinate_audit.audit(coordinate_audit.DEFAULT_GLB,MAP)
        source={'profile_count':len(profiles),'source_sha256':{str(p.relative_to(ROOT)):sha(p) for p in [PERSONAS,OSM,MAP,BOUNDARY,BUSINESS,SPEC]},'space':space['stats'],'alignment':alignment,'limitations':['합성 서울 페르소나, 실제 방문자 표본 아님','원문 행동 특성 미추출, 구조화 기본값','5,044명 강제 입장, 방문수요 예측 아님','지상 연결망과 건물 업종 대리값만 사용','GLB 지면 평면, 엔진 물리 검증 미완료']}
        (args.out/'source.json').write_text(json.dumps(source,ensure_ascii=False,indent=2)+'\n')
        print(json.dumps({'root':str(args.out),'bundle':str(args.out/'bundle'),'defs':str(args.out/'defs'),'scenario':str(args.out/'bundle/scenario.yaml'),'profile_count':len(profiles),'space':space['stats']},ensure_ascii=False,indent=2))
        return 0
    except (OSError,ValueError,KeyError,IndexError,TypeError) as e:
        print(f'Input preparation failed: {e}',file=sys.stderr);return 1


if __name__=='__main__':raise SystemExit(main())
