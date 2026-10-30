import { useEffect, useMemo, useRef, useState } from 'react';
import type { Frame } from './frames.ts';
import { loadRun, ReplayReader, RequestGate, verifyMap, type RunStatus } from './replay.ts';

export function useReplay(base: string) {
  const reader = useMemo(() => new ReplayReader(base), [base]);
  const gate = useRef(new RequestGate());
  const lastBase = useRef(base);
  const [status, setStatus] = useState<RunStatus | null>(null);
  const [mapVerified, setMapVerified] = useState(false);
  const [error, setError] = useState('');
  const [frame, setFrame] = useState<Frame | null>(null);
  const [seconds, setSeconds] = useState(3600);
  const [playing, setPlaying] = useState(true);
  const [speed, setSpeed] = useState(60);

  useEffect(() => {
    if (lastBase.current !== base) {
      lastBase.current = base;
      gate.current.issue();
      setStatus(null);
      setFrame(null);
      setMapVerified(false);
      setError('');
      setSeconds(0);
      setPlaying(true);
    }
    let cancelled = false;
    let timer = 0;
    let verifiedHash = '';
    const poll = async () => {
      try {
        const next = await loadRun(base);
        if (cancelled) return;
        if (next.alignment.glb_sha256 !== verifiedHash) {
          setMapVerified(false);
          await verifyMap(next.alignment);
          if (cancelled) return;
          verifiedHash = next.alignment.glb_sha256;
          setMapVerified(true);
        }
        setStatus(next);
        setError(next.status === 'failed' ? `엔진 실행 실패: ${next.error ?? '원인 확인 필요'}` : '');
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : '실행을 불러오지 못했습니다');
      }
      if (!cancelled) timer = window.setTimeout(poll, 2500);
    };
    void poll();
    return () => { cancelled = true; window.clearTimeout(timer); gate.current.issue(); };
  }, [base]);

  const maxSeconds = status && status.completed_tick > 0
    ? Math.max(0, status.completed_tick * status.dt_s - status.render_every_ticks * status.dt_s) : 0;
  useEffect(() => {
    if (!playing || !status || !mapVerified || maxSeconds <= 0) return;
    let last = performance.now();
    const timer = window.setInterval(() => {
      const now = performance.now();
      const delta = Math.min((now - last) / 1000, 1);
      last = now;
      setSeconds((old) => Math.min(maxSeconds, old + delta * speed));
    }, 200);
    return () => window.clearInterval(timer);
  }, [playing, speed, status, mapVerified, maxSeconds]);

  useEffect(() => {
    if (status?.status === 'complete' && maxSeconds > 0 && seconds >= maxSeconds) setPlaying(false);
  }, [status?.status, maxSeconds, seconds]);

  useEffect(() => {
    if (!status || !mapVerified || status.frames.length === 0) return;
    const generation = gate.current.issue();
    const position = Math.max(0, Math.min(seconds, maxSeconds));
    void reader.frameAt(status, position).then((next) => {
      if (gate.current.current(generation)) setFrame(next);
    }).catch((e: unknown) => {
      if (gate.current.current(generation)) setError(e instanceof Error ? e.message : '프레임을 읽지 못했습니다');
    });
  }, [reader, status, seconds, maxSeconds, mapVerified]);

  const current = lastBase.current === base;
  return { status: current ? status : null, frame: current ? frame : null,
    seconds: current ? Math.min(seconds, maxSeconds) : 0, setSeconds, playing, setPlaying,
    speed, setSpeed, maxSeconds: current ? maxSeconds : 0, error: current ? error : '' };
}
