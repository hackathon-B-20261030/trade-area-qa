"""Edit recorded browser frames into a subtitled, silent H.264 demonstration."""
from __future__ import annotations
import hashlib
import json
from pathlib import Path
import subprocess
from PIL import Image, ImageDraw, ImageFont

ROOT=Path(__file__).resolve().parents[1]
BATCH=ROOT/'output/myeongdong-hypotheses/demo-20261031-a'
VIDEO=BATCH/'video'
FFMPEG=Path('/opt/homebrew/Cellar/ffmpeg/8.1.1/bin/ffmpeg')
FONT='/System/Library/Fonts/AppleSDGothicNeo.ttc'


def digest(path: Path) -> str:
    h=hashlib.sha256()
    with path.open('rb') as f:
        for chunk in iter(lambda:f.read(1024*1024),b''):h.update(chunk)
    return h.hexdigest()


def caption_image(lines: list[str], index: int, total: int, output: Path) -> None:
    if len(lines)!=2 or any('합성' in x or '실험' in x for x in lines): raise ValueError('자막 내용이 올바르지 않습니다')
    canvas=Image.new('RGBA',(1280,720),(0,0,0,0)); d=ImageDraw.Draw(canvas)
    d.rounded_rectangle((42,609,1238,705),radius=14,fill=(17,37,27,230))
    d.rounded_rectangle((57,623,116,648),radius=6,fill=(60,100,73,255))
    small=ImageFont.truetype(FONT,15,index=4)
    d.text((68,624),f'{index:02d} / {total:02d}',font=small,fill=(223,240,226,255),stroke_width=0)
    fonts=[]
    for text,size,face in ((lines[0],30,4),(lines[1],24,0)):
        f=ImageFont.truetype(FONT,size,index=face)
        while d.textbbox((0,0),text,font=f)[2]>1080 and size>18:
            size-=1; f=ImageFont.truetype(FONT,size,index=face)
        if d.textbbox((0,0),text,font=f)[2]>1080: raise ValueError('자막이 화면을 넘습니다')
        fonts.append(f)
    d.text((134,621),lines[0],font=fonts[0],fill=(250,253,249,255))
    d.text((134,663),lines[1],font=fonts[1],fill=(212,230,217,255))
    canvas.save(output)


def command(args: list[str]) -> None:
    completed=subprocess.run(args,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    if completed.returncode: raise RuntimeError(completed.stderr[-3500:])


def main() -> None:
    plan=json.loads((VIDEO/'plan.json').read_text()); manifest=json.loads((VIDEO/'capture_manifest.json').read_text())
    if plan['report_sha256']!=digest(BATCH/'report.json'): raise ValueError('자막 작성 후 보고서가 바뀌었습니다')
    if not FFMPEG.is_file() or plan['fps']!=8 or len(plan['scenes'])!=8: raise ValueError('편집 도구 또는 장면 구성이 다릅니다')
    raw=Path(manifest['raw_root']).resolve()
    if not raw.is_relative_to(VIDEO.resolve()): raise ValueError('캡처 경로가 다릅니다')
    by_id={x['id']:x for x in manifest['scenes']}; clips=VIDEO/'clips'; clips.mkdir(exist_ok=True)
    overlays=VIDEO/'overlays';overlays.mkdir(exist_ok=True)
    output=VIDEO.parent/'demo-video.mp4'; temp=VIDEO/'demo-video.partial.mp4'
    clip_paths=[]
    for idx,scene in enumerate(plan['scenes'],1):
        entry=by_id[scene['id']]; frames=sorted((raw/scene['id']).glob('frame_*.jpg'))
        if len(frames)!=entry['frame_count'] or len(frames)!=scene['duration']*plan['fps']: raise ValueError('캡처 프레임 수가 맞지 않습니다')
        if scene.get('run_id')!=entry.get('run_id'): raise ValueError('캡처 실행 ID가 일치하지 않습니다')
        overlay=overlays/(scene['id']+'.png');caption_image(scene['caption'],idx,len(plan['scenes']),overlay)
        clip=clips/(scene['id']+'.mp4');clip_paths.append(clip)
        filters='[0:v]scale=1280:720:force_original_aspect_ratio=decrease:force_divisible_by=2,pad=1280:720:(ow-iw)/2:(oh-ih)/2:color=0xf4f7f3,fps=24[base];[base][1:v]overlay=0:0:shortest=1,format=yuv420p[v]'
        print(f'편집 {idx}/{len(plan["scenes"])}: {scene["id"]}',flush=True)
        command([str(FFMPEG),'-hide_banner','-loglevel','error','-y','-framerate',str(plan['fps']),'-start_number','0',
                 '-i',str(raw/scene['id']/'frame_%04d.jpg'),'-loop','1','-framerate','24','-i',str(overlay),
                 '-filter_complex',filters,'-map','[v]','-t',str(scene['duration']),'-an','-c:v','libx264','-preset','fast',
                 '-crf','20','-threads','4','-pix_fmt','yuv420p','-movflags','+faststart',str(clip)])
    concat=VIDEO/'concat.txt';concat.write_text(''.join("file '"+str(p).replace("'","'\\''")+"'\n" for p in clip_paths))
    command([str(FFMPEG),'-hide_banner','-loglevel','error','-y','-f','concat','-safe','0','-i',str(concat),'-c','copy','-an','-movflags','+faststart',str(temp)])
    temp.replace(output)
    metadata={'output':str(output),'sha256':digest(output),'duration_s':sum(s['duration'] for s in plan['scenes']),
              'fps':24,'dimensions':[1280,720],'audio':False,'source':'browser_tab_capture','scenes':[s['id'] for s in plan['scenes']]}
    (VIDEO/'edited.json').write_text(json.dumps(metadata,ensure_ascii=False,indent=2)+'\n')
    print('완료:',output,flush=True)

if __name__=='__main__':main()
