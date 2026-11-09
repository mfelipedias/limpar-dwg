# ---------- 1. compila o motor (Rust + acadrust, sem AutoCAD) ----------
FROM rust:1-slim-bookworm AS engine
WORKDIR /src
COPY engine/ .
RUN cargo build --release --locked && strip target/release/nanoclean

# ---------- 2. imagem final: site + fila + motor ----------
FROM python:3.12-slim-bookworm

ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1 DATA_DIR=/data NANOCLEAN_BIN=/usr/local/bin/nanoclean
WORKDIR /app
COPY server/requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
COPY server/ .
COPY --from=engine /src/target/release/nanoclean /usr/local/bin/nanoclean

RUN useradd -r -u 10001 app && mkdir -p /data && chown app /data
USER app
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s CMD python -c "import urllib.request;urllib.request.urlopen('http://127.0.0.1:8080/healthz')"
# 1 processo: a fila fica em memoria (e em /data/jobs/*/job.json)
CMD ["uvicorn", "app:app", "--host", "0.0.0.0", "--port", "8080", "--proxy-headers", "--forwarded-allow-ips", "*", "--timeout-keep-alive", "75"]
