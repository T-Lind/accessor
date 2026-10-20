import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import tempfile
import time
import wave

ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / 'docs/benchmarks/2026-10-02'
ASSETS = Path(os.environ.get('ACC_ASSETS', Path.home() / '.config/accessor/assets'))
BIN = ROOT / 'target/release/acc'

def save(name, data):
    (OUT / name).write_text(json.dumps(data, indent=2) + '\n')

def run(args, env):
    result = subprocess.run([str(BIN), *map(str, args)], env=env, text=True,
                            capture_output=True, timeout=600)
    if result.returncode:
        return {'error': result.stderr, 'exit_code': result.returncode}
    return json.loads(result.stdout)

with tempfile.TemporaryDirectory(prefix='acc-review-') as temporary:
    home = Path(temporary)
    (home/'config.json').write_text(json.dumps({'tts': {'provider':'piper'}, 'stt': {'threads':2,'spin':False}}))
    env = dict(os.environ, ACC_HOME=str(home), ACC_ASSETS=str(ASSETS))
    for engine in ['parakeet','whisper-tiny','whisper-base']:
        report = run(['stt','benchmark',ROOT/'tests/fixtures/voice-benchmark.wav','--engine',engine,'--runs','5'],env)
        save('voice-'+engine+'.json', report)
        print(engine, [(r['median_ms'],r['first_text']) for r in report.get('files',[])],report.get('error',''),flush=True)
    report=run(['tts','benchmark','--provider','piper','--runs','9'],env)
    save('tts-piper.json',report)
    print('Piper', [round(r['total_ms']) for r in report.get('runs',[])],flush=True)
    spec=importlib.util.spec_from_file_location('wake_test',ROOT/'scripts/test_wake.py')
    wake=importlib.util.module_from_spec(spec); spec.loader.exec_module(wake)
    corpus=OUT/'generated-clips'; corpus.mkdir(exist_ok=True)
    sentences = {
        'wake-only':'Twenty nine.',
        'wake-music':'Twenty nine. Play some music.',
        'wake-timer':'Hey twenty nine. Set a timer for five minutes.',
        'near-28':'Twenty eight.',
        'near-39':'Thirty nine.',
        'negative':'The weather is nice today. Please pass the salt.',
    }
    manifest=[]
    for name,text in sentences.items():
        raw=home/(name+'.wav')
        result=subprocess.run([str(ASSETS/'piper/bin/piper'),'-m',str(ASSETS/'piper/voices/en_GB-alan-medium.onnx'),'-f',str(raw),'-q'],input=text+'\n',text=True,capture_output=True,timeout=30)
        result.check_returncode()
        samples=wake.read_wav(raw); path=corpus/(name+'.wav'); wake.write_wav(path,samples)
        manifest.append({'file':str(path.relative_to(ROOT)),'transcript':text,'seconds':len(samples)/16000,'sha256':hashlib.sha256(path.read_bytes()).hexdigest()})
    save('generated-fixtures.json', {'source':'Local Piper en_GB-alan-medium; synthetic, one voice; not a room test','clips':manifest})
    reports=[]
    for name in ('wake-only','wake-music','wake-timer'):
        variants=OUT/'wake-variants'; variants.mkdir(exist_ok=True)
        paths=wake.variants(corpus/(name+'.wav'),variants,10.0,0.1)
        reports.append(run(['stt','wake',*paths,'--noise-gate','compare','--json'],env))
    reports.append(run(['stt','wake',*[corpus/(name+'.wav') for name in ('near-28','near-39','negative')],'--noise-gate','compare','--json'],env))
    save('wake-corpus.json',{'reports':reports,'note':'Offline whole-file recognition, not the live rolling wake-window path; synthetic Gaussian noise at 10 dB SNR.'})
    print('wake corpus done',flush=True)
    stat=subprocess.run(['lscpu','-J'],capture_output=True,text=True,check=True)
    save('machine.json',{'platform':platform.platform(),'logical_cpus':os.cpu_count(),'cpu':json.loads(stat.stdout),'commit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'binary_sha256':hashlib.sha256(BIN.read_bytes()).hexdigest(),'default_endpoint_ms':600,'note':'User desktop and existing Accessor process remained running; no mic, cloud, or agent benchmarking.'})
