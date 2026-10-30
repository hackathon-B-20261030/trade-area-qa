import { useCallback, useEffect, useRef, useState } from 'react';
import { getHealth, getJob, getMetrics, getBuildings, startRun, type BuildingCatalog, type Job, type Metrics, type RunOptions } from './client';
import { useReplay } from './useReplay';
import { createRunGuard } from './manualRun';

export function useEngineRun() {
  const [base, setBase] = useState(() => new URLSearchParams(window.location.search).get('run') ?? '');
  const baseRef = useRef(base);
  const replay = useReplay(base);
  const [health, setHealth] = useState<{ ready: boolean; error?: string } | null>(null);
  const [job, setJob] = useState<Job | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  const [metrics, setMetrics] = useState<Metrics | null>(null);
  const [buildings, setBuildings] = useState<BuildingCatalog | null>(null);
  const [buildingsError, setBuildingsError] = useState('');
  const [metricsError, setMetricsError] = useState('');
  const jobRef = useRef<Job | null>(null);
  jobRef.current = job;
  const runGuard = useRef(createRunGuard());

  useEffect(() => {
    let cancelled = false;
    let timer = 0;
    const check = async () => {
      try { const result = await getHealth(); if (!cancelled) setHealth(result); }
      catch (e) { if (!cancelled) setHealth({ ready: false, error: e instanceof Error ? e.message : '서버 연결 실패' }); }
      if (!cancelled) timer = window.setTimeout(check, 5000);
    };
    void check();
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, []);

  useEffect(() => {
    if (!health?.ready || buildings) return;
    let cancelled = false;
    void getBuildings().then((data) => { if (!cancelled) { setBuildings(data.buildings); setBuildingsError(''); } })
      .catch((e: unknown) => { if (!cancelled) setBuildingsError(e instanceof Error ? e.message : '건물 목록을 불러오지 못했습니다'); });
    return () => { cancelled = true; };
  }, [health?.ready, buildings]);

  useEffect(() => {
    const id = base.split('/').pop() ?? '';
    if (!/^ui-[a-f0-9]{32}$/.test(id)) return;
    let cancelled = false;
    void getJob(id).then((existing) => { if (!cancelled) setJob((current) => current ?? existing); }).catch(() => {});
    return () => { cancelled = true; };
  }, []);

  const jobId = job?.run_id;
  useEffect(() => {
    if (!jobId) return;
    let cancelled = false;
    let timer = 0;
    const poll = async () => {
      try {
        const next = await getJob(jobId);
        if (cancelled) return;
        setJob(next);
        setError(next.state === 'failed' ? next.error ?? '엔진 실행이 실패했습니다' : '');
        if (next.status_available && baseRef.current !== next.replay_base) {
          baseRef.current = next.replay_base;
          setBase(next.replay_base);
          const url = new URL(window.location.href);
          url.searchParams.set('run', next.replay_base);
          window.history.replaceState(null, '', url);
        }
        if (next.state === 'complete' || next.state === 'failed') return;
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : '진행 상태를 확인하지 못했습니다');
      }
      if (!cancelled) timer = window.setTimeout(poll, 1200);
    };
    void poll();
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [jobId]);

  useEffect(() => {
    setMetrics(null);
    setMetricsError('');
    if (replay.status?.status !== 'complete') return;
    const id = base.split('/').pop() ?? '';
    let cancelled = false;
    void getMetrics(id).then((result) => { if (!cancelled) setMetrics(result); }).catch((e: unknown) => {
      if (!cancelled) setMetricsError(e instanceof Error ? e.message : '결과를 불러오지 못했습니다');
    });
    return () => { cancelled = true; };
  }, [base, replay.status?.status]);

  const start = useCallback(async (options: RunOptions) => {
    const currentJob = jobRef.current;
    const busy = !!currentJob && !['complete', 'failed'].includes(currentJob.state);
    if (!runGuard.current.begin(busy)) return;
    setError('');
    setPending(true);
    try {
      const next = await startRun(options);
      jobRef.current = next;
      setJob(next);
    } catch (e) { setError(e instanceof Error ? e.message : '실행을 시작하지 못했습니다'); }
    finally { setPending(false); runGuard.current.end(); }
  }, []);

  const running = pending || !!job && !['complete', 'failed'].includes(job.state);
  const currentRunId = base.split('/').pop();
  return { base, replay, health, buildings, buildingsError, job, running, error,
    metrics: metrics?.run_id === currentRunId ? metrics : null, metricsError, start };
}
