"""
Limpar nanoCAD - servidor web autocontido.

Recebe os DWG pelo navegador, coloca numa fila e limpa com o motor `nanoclean`
(Rust + acadrust, sem AutoCAD), que roda dentro deste mesmo container.
O progresso do motor chega como linhas JSON no stdout e e repassado ao navegador.
"""

import asyncio
import base64
import json
import os
import re
import secrets
import shutil
import time
import uuid
from pathlib import Path
from urllib.parse import quote

from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import FileResponse, Response
from fastapi.staticfiles import StaticFiles
from starlette.requests import ClientDisconnect

DATA_DIR = Path(os.environ.get("DATA_DIR", "/data"))
JOBS_DIR = DATA_DIR / "jobs"
ENGINE = os.environ.get("NANOCLEAN_BIN", "/usr/local/bin/nanoclean")
WEB_USER = os.environ.get("WEB_USER", "")
WEB_PASSWORD = os.environ.get("WEB_PASSWORD", "")
MAX_UPLOAD_MB = int(os.environ.get("MAX_UPLOAD_MB", "1024"))
RETENTION_HOURS = float(os.environ.get("RETENTION_HOURS", "48"))
CONCURRENCY = max(1, int(os.environ.get("CONCURRENCY", "1")))
JOB_TIMEOUT_MIN = float(os.environ.get("JOB_TIMEOUT_MINUTES", "30"))
LOG_LINES = 300

STATIC_DIR = Path(__file__).parent / "static"

app = FastAPI(title="Limpar nanoCAD", docs_url=None, redoc_url=None)

jobs: dict[str, dict] = {}
procs: dict[str, asyncio.subprocess.Process] = {}
lock = asyncio.Lock()
wakeup = asyncio.Event()


# ---------------------------------------------------------------- utilidades
def now() -> float:
    return time.time()


def job_dir(jid: str) -> Path:
    return JOBS_DIR / jid


def save(job: dict) -> None:
    d = job_dir(job["id"])
    d.mkdir(parents=True, exist_ok=True)
    tmp = d / "job.json.tmp"
    tmp.write_text(json.dumps(job, ensure_ascii=False), encoding="utf-8")
    tmp.replace(d / "job.json")


def add_log(job: dict, line: str) -> None:
    job.setdefault("log", []).append(f"{time.strftime('%H:%M:%S')}  {line}")
    del job["log"][:-LOG_LINES]


def remove_job(jid: str) -> None:
    jobs.pop(jid, None)
    shutil.rmtree(job_dir(jid), ignore_errors=True)


def queued_ids() -> list[str]:
    q = [j for j in jobs.values() if j["state"] == "queued"]
    q.sort(key=lambda j: j["created"])
    return [j["id"] for j in q]


def public(job: dict) -> dict:
    v = {k: job.get(k) for k in (
        "id", "filename", "size", "options", "state", "created", "started",
        "finished", "updated", "progress", "result", "error", "log",
    )}
    if job["state"] == "queued":
        q = queued_ids()
        v["queue_position"] = q.index(job["id"]) + 1 if job["id"] in q else None
    return v


def content_disposition(name: str) -> str:
    ascii_name = re.sub(r"[^A-Za-z0-9._ -]", "_", name)
    return f"attachment; filename=\"{ascii_name}\"; filename*=UTF-8''{quote(name)}"


def result_name(job: dict) -> str:
    stem = re.sub(r"\.dwg$", "", job["filename"], flags=re.I)
    return f"{stem}_LIMPO.dwg"


# ---------------------------------------------------------------- autenticacao
@app.middleware("http")
async def basic_auth(request: Request, call_next):
    if WEB_PASSWORD and request.url.path != "/healthz":
        ok = False
        h = request.headers.get("authorization", "")
        if h.lower().startswith("basic "):
            try:
                user, _, pw = base64.b64decode(h[6:]).decode("utf-8").partition(":")
                ok = secrets.compare_digest(pw, WEB_PASSWORD) and (
                    not WEB_USER or secrets.compare_digest(user, WEB_USER))
            except Exception:
                ok = False
        if not ok:
            return Response(status_code=401, headers={"WWW-Authenticate": 'Basic realm="Limpar nanoCAD"'})
    return await call_next(request)


# ---------------------------------------------------------------- motor
# Faixas do progresso geral por etapa. "load" e "save" nao tem progresso
# interno no motor, entao avancam por tempo estimado (proporcional ao tamanho).
STAGES = {"load": (2, 70), "erase": (70, 78), "purge": (78, 82), "save": (82, 99)}


def estimate_secs(stage: str, size: int) -> float:
    mb = size / 1048576
    return {"load": 2 + mb * 0.45, "save": 1 + mb * 0.05}.get(stage, 1.0)


async def run_engine(job: dict) -> None:
    d = job_dir(job["id"])
    out = d / "result.part"
    out.unlink(missing_ok=True)
    args = [ENGINE, str(d / "input.dwg"), str(out)]
    if job["options"].get("purge"):
        args.append("--purge")

    proc = await asyncio.create_subprocess_exec(
        *args, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
    procs[job["id"]] = proc
    stage = {"name": "load", "since": time.monotonic()}
    final: dict = {}
    error: str | None = None

    def set_progress(name: str, extra: dict | None = None, frac: float = 0.0) -> None:
        lo, hi = STAGES.get(name, (0, 100))
        p = {"stage": name, "pct": round(lo + (hi - lo) * min(1.0, frac), 1)}
        p.update(extra or {})
        job["progress"] = p
        job["updated"] = now()

    async def ticker() -> None:
        # avanca a barra durante as etapas sem progresso proprio
        while True:
            await asyncio.sleep(0.7)
            name = stage["name"]
            if name in ("load", "save"):
                t = time.monotonic() - stage["since"]
                est = estimate_secs(name, job["size"])
                frac = 1 - pow(2.718281828, -t / est)  # aproxima 100% sem chegar
                set_progress(name, {"detail": f"{int(t)}s"}, frac)

    tick = asyncio.create_task(ticker())
    try:
        async def read_stdout() -> None:
            nonlocal error, final
            assert proc.stdout
            async for raw in proc.stdout:
                try:
                    ev = json.loads(raw.decode("utf-8", "replace"))
                except ValueError:
                    continue
                name = ev.get("stage")
                if name == "preserved":
                    final["preserved_raw"] = ev.get("count")
                    add_log(job, f"{ev.get('count')} entidades não interpretáveis mantidas byte a byte")
                    for m in (ev.get("messages") or [])[:3]:
                        add_log(job, f"  {m}")
                    continue
                if name != stage["name"]:
                    stage.update(name=name, since=time.monotonic())
                if name == "erase":
                    total = ev.get("total") or 0
                    done = ev.get("done") or 0
                    set_progress("erase", {"done": done, "total": total}, done / total if total else 1)
                    if done:
                        add_log(job, f"{done:,} tabelas *A removidas".replace(",", "."))
                elif name == "loaded":
                    add_log(job, f"DWG {ev.get('version')} lido em {ev.get('secs')}s: "
                                 f"{ev.get('entities')} entidades, {ev.get('objects')} objetos, {ev.get('blocks')} blocos")
                    stage.update(name="erase", since=time.monotonic())
                    set_progress("erase", {"done": 0, "total": None}, 0)
                elif name == "load" and ev.get("failsafe"):
                    add_log(job, f"leitura normal falhou ({ev.get('reason')}); tentando em modo de recuperação")
                elif name == "skipped":
                    final["skipped"] = ev.get("count")
                    final["skipped_messages"] = ev.get("messages")
                    add_log(job, f"{ev.get('count')} registros ilegíveis ignorados na leitura")
                    for m in (ev.get("messages") or [])[:5]:
                        add_log(job, f"  {m}")
                elif name in ("purge", "save", "load"):
                    set_progress(name)
                elif name == "done":
                    final.update(ev)
                elif name == "error":
                    error = ev.get("error") or "falha no motor"

        await asyncio.wait_for(asyncio.gather(read_stdout(), proc.wait()), JOB_TIMEOUT_MIN * 60)
    except asyncio.TimeoutError:
        proc.kill()
        error = f"tempo limite de {JOB_TIMEOUT_MIN:g} min excedido"
    finally:
        tick.cancel()
        procs.pop(job["id"], None)

    stderr = (await proc.stderr.read()).decode("utf-8", "replace").strip() if proc.stderr else ""
    if job["state"] == "canceled":
        out.unlink(missing_ok=True)
        return
    if not error and (proc.returncode != 0 or "removed" not in final or not out.exists()):
        error = "o motor terminou sem gerar o arquivo"
        if proc.returncode is not None and (proc.returncode < 0 or proc.returncode == 137):
            error = "o processo foi encerrado pelo sistema (provavelmente falta de memória no servidor)"
    if stderr:
        for line in stderr.splitlines()[-5:]:
            add_log(job, f"[motor] {line}")

    t = now()
    if error:
        out.unlink(missing_ok=True)
        job.update(state="failed", error=error)
        add_log(job, f"falhou: {error}")
    else:
        out.replace(d / "result.dwg")
        report = {
            "removed": final.get("removed"), "left": final.get("left"),
            "blocks_purged": final.get("blocks_purged"),
            "datalinks_removed": final.get("datalinks_removed"),
            "invalid_hatches_removed": final.get("invalid_hatches_removed"),
            "skipped": final.get("skipped", 0),
            "preserved_raw": final.get("preserved_raw", 0),
            "version": final.get("version"),
            "seconds": final.get("secs"),
            "size_before": job["size"], "size_after": (d / "result.dwg").stat().st_size,
            "download_name": result_name(job),
        }
        job.update(state="done", result=report, error=None, progress={"stage": "done", "pct": 100})
        add_log(job, "concluído")
        (d / "input.dwg").unlink(missing_ok=True)  # o original nao e mais necessario
    job["finished"] = job["updated"] = t


async def runner(n: int) -> None:
    while True:
        async with lock:
            q = queued_ids()
            job = jobs[q[0]] if q else None
            if job:
                t = now()
                job.update(state="processing", started=t, updated=t,
                           progress={"stage": "load", "pct": 2})
                add_log(job, "processamento iniciado")
                save(job)
        if not job:
            wakeup.clear()
            try:
                await asyncio.wait_for(wakeup.wait(), 5)
            except asyncio.TimeoutError:
                pass
            continue
        try:
            await run_engine(job)
        except Exception as e:  # nunca derruba o runner
            job.update(state="failed", error=f"erro interno: {e}", finished=now(), updated=now())
            add_log(job, f"erro interno: {e!r}")
        save(job)


async def persist_progress() -> None:
    # grava o progresso em disco de tempos em tempos (sobrevive a reinicios)
    while True:
        await asyncio.sleep(5)
        for j in list(jobs.values()):
            if j["state"] == "processing":
                save(j)


# ---------------------------------------------------------------- ciclo de vida
@app.on_event("startup")
async def startup() -> None:
    JOBS_DIR.mkdir(parents=True, exist_ok=True)
    for d in JOBS_DIR.iterdir():
        f = d / "job.json"
        if not f.exists():
            shutil.rmtree(d, ignore_errors=True)  # upload interrompido
            continue
        try:
            job = json.loads(f.read_text(encoding="utf-8"))
        except Exception:
            shutil.rmtree(d, ignore_errors=True)
            continue
        if job["state"] == "processing":  # servidor caiu no meio: refaz
            job["state"] = "queued"
            job["progress"] = {"stage": "queued", "pct": 0}
            add_log(job, "servidor reiniciado; trabalho voltou para a fila")
            save(job)
        jobs[job["id"]] = job
    for i in range(CONCURRENCY):
        asyncio.create_task(runner(i))
    asyncio.create_task(persist_progress())
    asyncio.create_task(housekeeping())


async def housekeeping() -> None:
    while True:
        await asyncio.sleep(300)
        limit = now() - RETENTION_HOURS * 3600
        async with lock:
            for jid in [j["id"] for j in jobs.values()
                        if j["state"] not in ("processing", "queued") and j["updated"] < limit]:
                remove_job(jid)


# ---------------------------------------------------------------- API
@app.get("/healthz")
async def healthz():
    return {"ok": True, "engine": Path(ENGINE).exists()}


@app.get("/api/status")
async def status():
    return {
        "engine": Path(ENGINE).exists(),
        "queue": len(queued_ids()),
        "processing": sum(1 for j in jobs.values() if j["state"] == "processing"),
        "concurrency": CONCURRENCY,
        "max_upload_mb": MAX_UPLOAD_MB,
        "retention_hours": RETENTION_HOURS,
    }


@app.post("/api/jobs")
async def create_job(request: Request, filename: str, purge: bool = False):
    filename = Path(filename.replace("\\", "/")).name.strip()
    if not filename.lower().endswith(".dwg"):
        raise HTTPException(400, "Envie um arquivo .dwg")
    limit = MAX_UPLOAD_MB * 1024 * 1024
    if int(request.headers.get("content-length") or 0) > limit:
        raise HTTPException(413, f"Arquivo maior que o limite de {MAX_UPLOAD_MB} MB")

    jid = uuid.uuid4().hex[:12]
    d = job_dir(jid)
    d.mkdir(parents=True)
    part = d / "input.part"
    size = 0
    head = b""
    try:
        with open(part, "wb") as f:
            async for chunk in request.stream():
                size += len(chunk)
                if size > limit:
                    raise HTTPException(413, f"Arquivo maior que o limite de {MAX_UPLOAD_MB} MB")
                if len(head) < 6:
                    head += chunk[: 6 - len(head)]
                f.write(chunk)
        if not head.startswith(b"AC1"):
            raise HTTPException(400, "O arquivo não parece ser um DWG válido")
        part.replace(d / "input.dwg")
    except ClientDisconnect:
        shutil.rmtree(d, ignore_errors=True)
        return Response(status_code=499)
    except BaseException:
        shutil.rmtree(d, ignore_errors=True)
        raise

    t = now()
    job = {
        "id": jid, "filename": filename, "size": size, "options": {"purge": purge},
        "state": "queued", "created": t, "updated": t, "started": None, "finished": None,
        "progress": {"stage": "queued", "pct": 0}, "result": None, "error": None, "log": [],
    }
    add_log(job, f"recebido {filename} ({size / 1048576:.1f} MB)")
    async with lock:
        jobs[jid] = job
        save(job)
    wakeup.set()
    return public(job)


@app.get("/api/jobs")
async def list_jobs(ids: str = ""):
    return [public(jobs[i]) for i in ids.split(",") if i in jobs]


@app.get("/api/jobs/{jid}")
async def get_job(jid: str):
    if jid not in jobs:
        raise HTTPException(404, "Trabalho não encontrado (pode ter expirado)")
    return public(jobs[jid])


@app.get("/api/jobs/{jid}/download")
async def download(jid: str):
    job = jobs.get(jid)
    f = job_dir(jid) / "result.dwg"
    if not job or job["state"] != "done" or not f.exists():
        raise HTTPException(404, "Arquivo limpo não disponível")
    return FileResponse(f, media_type="application/acad",
                        headers={"Content-Disposition": content_disposition(result_name(job))})


@app.delete("/api/jobs/{jid}")
async def delete_job(jid: str):
    """Cancela se estiver na fila/processando; remove se ja terminou."""
    async with lock:
        job = jobs.get(jid)
        if not job:
            return {"ok": True}
        if job["state"] in ("queued", "processing"):
            job["state"] = "canceled"
            job["finished"] = job["updated"] = now()
            add_log(job, "cancelado pelo usuário")
            proc = procs.get(jid)
            if proc and proc.returncode is None:
                proc.kill()
            (job_dir(jid) / "input.dwg").unlink(missing_ok=True)
            save(job)
            return public(job)
        remove_job(jid)
    return {"ok": True}


app.mount("/", StaticFiles(directory=STATIC_DIR, html=True), name="static")
