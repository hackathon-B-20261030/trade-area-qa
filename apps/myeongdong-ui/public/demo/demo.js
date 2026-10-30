const $ = (tag, className, value) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (value !== undefined) node.textContent = value;
  return node;
};
const meta = {
  building: {title:'한 건물의 업종을 바꾸면', setting:'밀레오레 명동의 대리 업종 · 옷가게 → 카페', note:'업종을 바꾸면 가격·제공 상품·수용량 등의 대리값도 함께 바뀝니다. 입장은 사람 수가 아닌 입장 사건 횟수입니다.', secondary:[['building_purchases','해당 장소 구매','건'],['building_revenue_krw','해당 장소 구매액','원']]},
  discount: {title:'전체 할인율을 높이면', setting:'대리 상점·가정 노점 할인 · 0% → 20%', note:'두 시드에서 구매 건수는 늘었지만 구매액의 방향은 달랐습니다. 할인 효과를 매출 증가로 단정할 수 없습니다.', secondary:[['revenue_krw','전체 구매액','원']]},
  income: {title:'방문객의 소득 가정을 높이면', setting:'방문객 소득 가정 · 기준 → 110%', note:'소득은 인물 프로필의 모형 가정입니다. 경제활동을 하지 않는 일부 인물의 지출 한도는 별도 기본값을 사용합니다.', secondary:[['revenue_krw','전체 구매액','원']]},
  pedestrian: {title:'보행 전용 시간을 늘리면', setting:'보행 전용 · 0시간 → 8시간 (12:00–20:00)', note:'면적은 3시간 동안 전체 간선 유효 면적의 평균입니다. 시나리오의 공간 가정이 직접 반영된 값이며, 실제 안전이나 이동 개선을 뜻하지 않습니다.', secondary:[]},
};
const fmt = (n, unit) => n.toLocaleString('ko-KR', {maximumFractionDigits:unit==='㎡'?1:0}) + unit;
function pairText(value, unit) {
  return `${fmt(value.before,unit)} → ${fmt(value.after,unit)}`;
}
function deltaText(value, unit) {
  const sign=value.delta>0?'+':'';
  const pct=value.pct===null?'':` · ${value.pct>0?'+':''}${value.pct.toFixed(1)}%`;
  return `${sign}${fmt(value.delta,unit)}${pct}`;
}
function replayLink(id, label, focus, primary) {
  if (!/^ui-[a-f0-9]{32}$/.test(id)) throw new Error('실행 ID가 올바르지 않습니다');
  const query=new URLSearchParams({run:`/runs/${id}`});
  if (focus) query.set('building','way/273025568');
  const a=$('a',primary?'primary':'',label);
  a.href=`/?${query}`; a.target='_blank'; a.rel='noopener';
  return a;
}
function metricRow(label, pairs, unit, valueFor) {
  const tr=$('tr'); tr.append($('td','',label));
  for (const pair of pairs) {
    const value=valueFor(pair); const td=$('td');
    if (!value) { td.textContent='기록 없음'; tr.append(td); continue; }
    td.append(document.createTextNode(pairText(value,unit)), $('span','delta',deltaText(value,unit))); tr.append(td);
  }
  return tr;
}
function makeCase(candidate, number) {
  const info=meta[candidate.id]; const pairs=[0,1].map(i=>candidate.pairs.find(p=>p.seed_index===i));
  if (!info || pairs.some(p=>!p)) throw new Error('검증된 두 실행을 찾을 수 없습니다');
  const article=$('article','case'); const head=$('div','case-header');
  head.append($('span','case-number',String(number).padStart(2,'0')));
  const heading=$('div'); heading.append($('h3','',info.title),$('p','context',info.setting)); head.append(heading);article.append(head);
  const wrap=$('div','table-wrap'),table=$('table');
  const thead=$('thead'),tr=$('tr'); ['지표','첫 번째 시드','두 번째 시드'].forEach(t=>tr.append($('th','',t)));thead.append(tr);table.append(thead);
  const tbody=$('tbody'); tbody.append(metricRow(candidate.metric_label,pairs,candidate.unit,p=>p));
  info.secondary.forEach(([key,label,unit])=>tbody.append(metricRow(label,pairs,unit,p=>p.secondary?.[key])));
  table.append(tbody);wrap.append(table);article.append(wrap,$('p','case-note',info.note));
  const actions=$('div','actions');actions.append(replayLink(pairs[0].baseline_run_id,'기준 기록 재생',candidate.id==='building',false),replayLink(pairs[0].run_id,'변경 기록 재생',candidate.id==='building',true));article.append(actions);
  const details=$('details','second-seed');details.append($('summary','','두 번째 시드 기록 열기'));const links=$('div');links.append(replayLink(pairs[1].baseline_run_id,'기준 기록 재생',candidate.id==='building',false),replayLink(pairs[1].run_id,'변경 기록 재생',candidate.id==='building',false));details.append(links);article.append(details);
  return article;
}
async function render() {
  const response=await fetch('/demo/results.json',{cache:'no-store'});
  if (!response.ok) throw new Error('비교 결과 파일을 불러오지 못했습니다');
  const data=await response.json();
  if (data.schema_version!==1 || !Array.isArray(data.candidates) || !Array.isArray(data.selected) || !data.reproducibility?.passed) throw new Error('비교 결과 검증 기록이 올바르지 않습니다');
  const root=document.getElementById('selected');root.replaceChildren();
  if (!data.selected.length) root.append($('p','loading','두 시드에서 시연 기준을 만족한 사례가 없습니다.'));
  data.selected.forEach((id,index)=>{const item=data.candidates.find(c=>c.id===id && c.status==='candidate');if(!item) throw new Error('선정된 사례가 유효하지 않습니다');root.append(makeCase(item,index+1));});
  const all=document.getElementById('all-results');all.replaceChildren();
  for (const item of data.candidates) {
    const row=$('tr'); const seed0=item.pairs.find(p=>p.seed_index===0), seed1=item.pairs.find(p=>p.seed_index===1);
    row.append($('td','',item.label),$('td','',item.metric_label),$('td','',seed0?pairText(seed0,item.unit):'미실행'),$('td','',seed1?pairText(seed1,item.unit):'미실행'),$('td','status',item.reason));all.append(row);
  }
  const limits=document.getElementById('limitations');
  ['표시된 수치는 시뮬레이션 결과이며 실제 방문자나 매출 예측과 다릅니다.',
   '두 시드의 결과를 비교한 탐색적 검증이며 추가 반복이 필요합니다.',
   '고정 엔진 버전의 기록입니다. 현재 엔진으로 다시 계산하면 결과가 달라질 수 있습니다.',
   '건물 업종은 가격·제공·수용 등 대리값이 함께 달라집니다. 물리 검증은 미완료입니다.']
    .forEach(text=>limits.append($('li','',text)));
  limits.append($('li','','노점·벤치·보행 폭의 변경은 엔진 입력과 기록에 반영되지만 3D 지도에 시설물이나 도로 폭을 별도 모델로 그리지는 않습니다.'));
  document.getElementById('engine-version').textContent=`고정 엔진 SHA-256: ${data.engine.binary_sha256}`;
}
render().catch(error=>{document.getElementById('selected').replaceChildren($('p','loading',error.message));});
