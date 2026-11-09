"use strict";

const $ = (s, el = document) => el.querySelector(s);
const STORE_KEY = "limparNanoCAD.jobs";
const TERMINAL = new Set(["done", "failed", "canceled"]);
const ACTIVE = new Set(["waiting", "uploading", "queued", "processing"]);
const APP_TITLE = "Limpar DWG";

const state = {
  status: null,      // /api/status
  jobs: new Map(),   // key -> {key, id?, file?, upload?, data?, el?}
  uploadQueue: [],
  uploading: false,
};

// ---------------------------------------------------------------- formatação
const nf = new Intl.NumberFormat("pt-BR");
const fmtInt = (n) => (n == null ? "–" : nf.format(n));
function fmtMB(bytes) {
  if (bytes == null) return "–";
  const kb = bytes / 1024;
  if (kb < 1000) return Math.max(1, Math.round(kb)) + " KB";
  const mb = kb / 1024;
  return (mb >= 100 ? mb.toFixed(0) : mb >= 10 ? mb.toFixed(1) : mb.toFixed(2)).replace(".", ",") + " MB";
}
function fmtLimit(mb) {
  if (mb >= 1024 && mb % 1024 === 0) return `${mb / 1024} GB`;
  return `${mb} MB`;
}
function fmtDur(sec) {
  if (sec == null || !isFinite(sec)) return "";
  sec = Math.max(0, Math.round(sec));
  const h = Math.floor(sec / 3600), m = Math.floor((sec % 3600) / 60), s = sec % 60;
  if (h) return `${h} h ${String(m).padStart(2, "0")} min`;
  if (m) return s ? `${m} min ${String(s).padStart(2, "0")} s` : `${m} min`;
  return `${s} s`;
}
function fmtDurLong(sec) {
  sec = Math.max(1, Math.round(sec || 0));
  if (sec < 60) return sec === 1 ? "1 segundo" : `${sec} segundos`;
  const m = Math.floor(sec / 60), s = sec % 60;
  const mins = m === 1 ? "1 minuto" : `${m} minutos`;
  return s ? `${mins} e ${s} s` : mins;
}
const plural = (n, one, many) => `${fmtInt(n)} ${n === 1 ? one : many}`;

const DWG_VERSIONS = {
  AC1032: "AutoCAD 2018 a 2026", AC1027: "AutoCAD 2013 a 2017", AC1024: "AutoCAD 2010 a 2012",
  AC1021: "AutoCAD 2007 a 2009", AC1018: "AutoCAD 2004 a 2006", AC1015: "AutoCAD 2000 a 2002",
};
const fmtVersion = (v) => (v ? (DWG_VERSIONS[v] ? `${DWG_VERSIONS[v]} (${v})` : v) : "–");

// ---------------------------------------------------------------- persistência local
function saveIds() {
  const ids = [...state.jobs.values()].filter((j) => j.id).map((j) => j.id);
  try { localStorage.setItem(STORE_KEY, JSON.stringify(ids)); } catch {}
}
function loadIds() {
  try { return JSON.parse(localStorage.getItem(STORE_KEY) || "[]"); } catch { return []; }
}

// ---------------------------------------------------------------- API
async function api(path, opts) {
  const r = await fetch(path, opts);
  if (!r.ok) {
    let msg = `Erro ${r.status}`;
    try { const j = await r.json(); if (j.detail) msg = j.detail; } catch {}
    throw new Error(msg);
  }
  return r.status === 204 ? null : r.json();
}

async function refreshStatus() {
  try { state.status = await api("/api/status"); } catch { state.status = null; }
  renderStatus();
}

async function refreshJobs() {
  const active = [...state.jobs.values()].filter((j) => j.id && (!j.data || !TERMINAL.has(j.data.state)));
  if (!active.length) return;
  try {
    const list = await api("/api/jobs?ids=" + active.map((j) => j.id).join(","));
    const seen = new Set();
    for (const d of list) {
      seen.add(d.id);
      const j = state.jobs.get(d.id);
      if (!j) continue;
      const finished = TERMINAL.has(d.state) && !TERMINAL.has(j.data?.state);
      j.data = d;
      if (finished) { j.justFinished = true; onFinished(j); }
      render(j);
      if (finished) refreshStatus();
    }
    for (const j of active) {
      if (!seen.has(j.id)) {
        j.data = { ...(j.data || {}), state: "failed", progress: null,
          error: "Este arquivo já foi apagado do servidor. Envie de novo para limpar." };
        render(j);
      }
    }
  } catch {
    /* servidor fora do ar: tenta de novo no próximo ciclo */
  }
}

function onFinished(j) {
  const d = j.data;
  const name = d.filename || "arquivo";
  if (d.state === "done") {
    const r = d.result || {};
    const red = r.size_before ? Math.round((1 - r.size_after / r.size_before) * 100) : 0;
    announce(`${name} está pronto${red > 0 ? `, ficou ${red}% mais leve` : ""}. Já pode baixar a cópia limpa.`);
  } else if (d.state === "failed") {
    announce(`Não foi possível limpar ${name}.`);
  }
}

function announce(msg) {
  const a = $("#announcer");
  a.textContent = "";
  setTimeout(() => { a.textContent = msg; }, 50);
}

// ---------------------------------------------------------------- envio
function addFiles(files) {
  const maxMB = state.status?.max_upload_mb || 1024;
  let newest = null;
  for (const file of files) {
    const key = "u" + Math.random().toString(36).slice(2);
    const j = { key, file, upload: { loaded: 0, total: file.size, start: 0 } };
    if (!/\.dwg$/i.test(file.name)) {
      j.data = { state: "failed", filename: file.name, size: file.size, local: true,
        error: "Este arquivo não é um .dwg. Envie o desenho salvo em formato DWG.", noRetry: true };
    } else if (file.size > maxMB * 1048576) {
      j.data = { state: "failed", filename: file.name, size: file.size, local: true,
        error: `O arquivo passa do limite de ${fmtLimit(maxMB)} por envio.`, noRetry: true };
    } else {
      state.uploadQueue.push(j);
    }
    state.jobs.set(key, j);
    render(j);
    newest = j;
  }
  updateList();
  if (newest) revealJob(newest);
  pumpUploads();
}

// Leva a pessoa até o arquivo recém-enviado: a lista fica abaixo da área de soltar,
// fora da tela, e sem isso parece que o envio não aconteceu.
function revealJob(j) {
  const el = j.el;
  if (!el) return;
  requestAnimationFrame(() => {
    const r = el.getBoundingClientRect();
    if (r.top < 0 || r.top > window.innerHeight - 160) {
      const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
      $("#files").scrollIntoView({ behavior: reduce ? "auto" : "smooth", block: "start" });
    }
    el.classList.remove("is-new");
    void el.offsetWidth;
    el.classList.add("is-new");
    el.addEventListener("animationend", () => el.classList.remove("is-new"), { once: true });
  });
}

function pumpUploads() {
  if (state.uploading) return;
  const j = state.uploadQueue.shift();
  if (!j) return;
  state.uploading = true;

  const purge = $("#opt-purge").checked;
  const qs = new URLSearchParams({ filename: j.file.name, purge: String(purge) });
  const xhr = new XMLHttpRequest();
  j.xhr = xhr;
  j.upload.start = performance.now();
  xhr.open("POST", "/api/jobs?" + qs);
  xhr.setRequestHeader("Content-Type", "application/octet-stream");
  xhr.upload.onprogress = (e) => {
    j.upload.loaded = e.loaded;
    j.upload.total = e.total || j.file.size;
    render(j);
  };
  xhr.onload = () => {
    let body = null;
    try { body = JSON.parse(xhr.responseText); } catch {}
    if (xhr.status >= 200 && xhr.status < 300 && body) {
      state.jobs.delete(j.key);
      if (j.el) j.el.dataset.key = body.id;
      j.key = j.id = body.id;
      j.data = body;
      state.jobs.set(body.id, j);
      saveIds();
    } else {
      j.data = { state: "failed", filename: j.file.name, size: j.file.size, local: true,
        error: body?.detail || `O envio foi recusado pelo servidor (erro ${xhr.status}).` };
    }
    finish();
  };
  xhr.onerror = () => {
    j.data = { state: "failed", filename: j.file.name, size: j.file.size, local: true,
      error: "A conexão caiu durante o envio. Confira a internet e tente de novo." };
    finish();
  };
  xhr.onabort = () => {
    j.data = { state: "canceled", filename: j.file.name, size: j.file.size, local: true };
    finish();
  };
  xhr.send(j.file);
  render(j);

  function finish() {
    j.xhr = null;
    state.uploading = false;
    render(j);
    pumpUploads();
    refreshStatus();
  }
}

// ---------------------------------------------------------------- ações
async function cancelJob(j) {
  if (j.xhr) { j.xhr.abort(); return; }
  const qi = state.uploadQueue.indexOf(j);
  if (qi >= 0) {
    state.uploadQueue.splice(qi, 1);
    j.data = { state: "canceled", filename: j.file.name, size: j.file.size, local: true };
    render(j);
    return;
  }
  if (j.id) {
    try { j.data = await api(`/api/jobs/${j.id}`, { method: "DELETE" }); } catch {}
    render(j);
  }
}

async function removeJob(j) {
  if (j.id) { try { await api(`/api/jobs/${j.id}`, { method: "DELETE" }); } catch {} }
  state.jobs.delete(j.key);
  j.el?.remove();
  saveIds();
  updateList();
}

async function retryJob(j) {
  const file = j.file;
  await removeJob(j);
  if (file) addFiles([file]);
}

// ---------------------------------------------------------------- render: topo
function renderStatus() {
  const el = $("#server-status");
  const txt = $(".status-text", el);
  const s = state.status;
  el.className = "status";
  if (!s) {
    el.classList.add("is-err");
    txt.textContent = "Sem conexão com o servidor";
    return;
  }
  if (!s.engine) {
    el.classList.add("is-err");
    txt.textContent = "Servidor com problema";
    el.title = "O programa de limpeza não foi encontrado no servidor.";
    return;
  }
  el.title = "";
  if (s.queue) {
    el.classList.add("is-busy");
    txt.textContent = `Ocupado, ${plural(s.queue, "arquivo", "arquivos")} na fila`;
  } else if (s.processing) {
    el.classList.add("is-busy");
    txt.textContent = "Limpando um arquivo";
  } else {
    el.classList.add("is-ok");
    txt.textContent = "Pronto para receber";
  }
  $("#limit-hint").textContent = `Pode enviar vários de uma vez, até ${fmtLimit(s.max_upload_mb)} cada.`;
  const h = s.retention_hours;
  const prazo = h >= 24 && h % 24 === 0 ? (h === 24 ? "1 dia" : `${h / 24} dias`) : `${h} horas`;
  $("#retention").textContent =
    `O servidor apaga os arquivos enviados e as cópias limpas depois de ${prazo}. Baixe a sua antes disso.`;
}

// ---------------------------------------------------------------- render: arquivo
const STAGE_LABEL = {
  queued: "Começando",
  load: "Abrindo o desenho",
  erase: "Removendo o lixo invisível",
  purge: "Apagando blocos sem uso",
  save: "Salvando a cópia limpa",
};

function render(j) {
  if (!j.el) {
    j.el = $("#job-tpl").content.firstElementChild.cloneNode(true);
    j.el.dataset.key = j.key;
    $(".btn-cancel", j.el).onclick = () => cancelJob(j);
    $(".btn-remove", j.el).onclick = () => removeJob(j);
    $(".btn-retry", j.el).onclick = () => retryJob(j);
    $("#jobs").prepend(j.el);
  }
  const el = j.el;
  const d = j.data || {};
  const st = d.state || (j.xhr ? "uploading" : "waiting");
  const p = d.progress || {};
  const now = Date.now() / 1000;

  $(".job-name", el).textContent = d.filename || j.file?.name || "arquivo.dwg";
  const meta = [fmtMB(d.size ?? j.file?.size)];
  if (d.options?.purge) meta.push("apagando também os blocos sem uso");
  $(".job-meta", el).textContent = meta.join(", ");

  const steps = { upload: "", queue: "", clean: "", done: "" };
  let tag = "", tagCls = "", pct = 0, indeterminate = false, main = "", side = "";

  switch (st) {
    case "waiting":
      tag = "Aguardando"; tagCls = "t-wait";
      steps.upload = "active"; indeterminate = true;
      main = "Esperando o envio dos arquivos anteriores";
      break;
    case "uploading": {
      const u = j.upload;
      pct = u.total ? (u.loaded / u.total) * 100 : 0;
      const elapsed = (performance.now() - u.start) / 1000;
      const speed = elapsed > 0.5 ? u.loaded / elapsed : 0;
      tag = "Enviando"; tagCls = "t-run";
      steps.upload = "active";
      main = pct >= 100 ? "Terminando o envio" : `Enviando, ${Math.floor(pct)}%`;
      if (speed && pct < 100) side = `${fmtMB(u.loaded)} de ${fmtMB(u.total)}, falta ${fmtDur((u.total - u.loaded) / speed)}`;
      break;
    }
    case "queued":
      tag = "Na fila"; tagCls = "t-wait";
      steps.upload = "complete"; steps.queue = "active"; indeterminate = true;
      main = d.queue_position > 1
        ? `Esperando a vez, ${plural(d.queue_position - 1, "arquivo", "arquivos")} na frente`
        : "Esperando a vez, é o próximo";
      side = `há ${fmtDur(now - d.created)}`;
      break;
    case "processing":
      tag = "Limpando"; tagCls = "t-run";
      steps.upload = steps.queue = "complete"; steps.clean = "active";
      pct = p.pct || 0;
      main = STAGE_LABEL[p.stage] || "Limpando";
      if (p.stage === "erase" && p.total) main += `, ${fmtInt(p.done)} de ${fmtInt(p.total)}`;
      side = `${Math.floor(pct)}%, ${fmtDur(now - (d.started || now))}`;
      break;
    case "done":
      tag = "Pronto"; tagCls = "t-ok";
      break;
    case "failed":
      tag = "Não deu certo"; tagCls = "t-err";
      if (d.local && !j.id) steps.upload = "error";
      else { steps.upload = steps.queue = "complete"; steps.clean = "error"; }
      break;
    case "canceled":
      tag = "Cancelado";
      break;
  }

  const tagEl = $(".tag", el);
  tagEl.textContent = tag;
  tagEl.className = "tag " + tagCls;
  el.setAttribute("aria-busy", String(ACTIVE.has(st)));

  // andamento
  const live = $(".job-live", el);
  live.hidden = st === "done" || st === "canceled";
  for (const li of el.querySelectorAll(".steps li")) {
    li.className = steps[li.dataset.step] || "";
    li.toggleAttribute("aria-current", li.className === "active");
  }
  const bar = $(".bar", el);
  bar.hidden = !ACTIVE.has(st);
  bar.className = "bar" + (indeterminate ? " indeterminate" : "");
  $(".bar-fill", bar).style.width = pct + "%";
  if (indeterminate) bar.removeAttribute("aria-valuenow");
  else bar.setAttribute("aria-valuenow", String(Math.floor(pct)));
  $(".job-status", el).hidden = !ACTIVE.has(st);
  $(".status-main", el).textContent = main;
  $(".status-side", el).textContent = side;

  renderResult(j, st, d);
  renderNotes(j, st, d);

  // ações
  const dl = $(".btn-download", el);
  dl.hidden = st !== "done";
  if (st === "done") {
    dl.href = `/api/jobs/${d.id}/download`;
    dl.setAttribute("download", d.result?.download_name || "");
  }
  const retry = $(".btn-retry", el);
  retry.hidden = !(j.file && !d.noRetry && (st === "failed" || st === "canceled"));
  retry.textContent = st === "canceled" ? "Enviar de novo" : "Tentar de novo";
  $(".btn-cancel", el).hidden = !ACTIVE.has(st);
  $(".btn-remove", el).hidden = !TERMINAL.has(st);

  renderTech(j, st, d);
  updateList();
}

function renderResult(j, st, d) {
  const box = $(".job-result", j.el);
  if (st !== "done" || !d.result) { box.hidden = true; return; }
  box.hidden = false;
  const r = d.result;
  const before = r.size_before || 0, after = r.size_after || 0;
  const red = before ? Math.round((1 - after / before) * 100) : 0;

  $(".result-head", box).textContent =
    red >= 1 ? `Ficou ${red}% mais leve` :
    r.removed ? "Pronto, o arquivo foi limpo" :
    "Este arquivo já estava limpo";

  const [bEl, aEl] = box.querySelectorAll(".dim");
  setDimLabel(bEl, fmtMB(before), "antes");
  setDimLabel(aEl, fmtMB(after), "depois");
  const f = before ? Math.min(1, after / before) : 1;
  const line = $(".dim-line", aEl);
  if (j.justFinished) {
    // a cota do arquivo limpo encolhe a partir do tamanho original
    j.justFinished = false;
    line.style.transition = "none";
    line.style.setProperty("--f", "1");
    void line.offsetWidth;
    line.style.transition = "";
    requestAnimationFrame(() => line.style.setProperty("--f", String(f)));
  } else {
    line.style.setProperty("--f", String(f));
  }

  const parts = [`Limpo em ${fmtDurLong(r.seconds)}.`];
  if (r.removed) parts.push(`Foram removidos ${plural(r.removed, "item", "itens")} de lixo invisível`
    + (r.blocks_purged ? ` e ${plural(r.blocks_purged, "bloco sem uso", "blocos sem uso")}.` : "."));
  else if (r.blocks_purged) parts.push(`Foram apagados ${plural(r.blocks_purged, "bloco sem uso", "blocos sem uso")}.`);
  parts.push("Abra no AutoCAD e confira antes de substituir o original.");
  $(".result-sub", box).textContent = parts.join(" ");
}

function setDimLabel(dim, value, word) {
  const lbl = $(".dim-label", dim);
  lbl.textContent = "";
  const b = document.createElement("b");
  b.textContent = value;
  lbl.append(b, word);
}

function note(title, text, cls = "") {
  const n = document.createElement("div");
  n.className = "note " + cls;
  const s = document.createElement("strong");
  s.textContent = title;
  n.append(s, text);
  return n;
}

function renderNotes(j, st, d) {
  const box = $(".job-notes", j.el);
  const notes = [];
  if (st === "done" && d.result) {
    const r = d.result;
    if (r.skipped > 0) notes.push(note(
      "Confira o desenho com atenção",
      `${plural(r.skipped, "parte do arquivo original estava danificada", "partes do arquivo original estavam danificadas")} e ficou de fora da cópia limpa. Compare com o original antes de substituí-lo.`));
    if (r.datalinks_removed > 0) notes.push(note(
      r.datalinks_removed === 1 ? "Uma tabela não está mais ligada ao Excel" : `${fmtInt(r.datalinks_removed)} tabelas não estão mais ligadas ao Excel`,
      "O conteúdo atual continua no desenho, mas não atualiza mais sozinho a partir da planilha."));
  } else if (st === "failed") {
    const tip = d.local
      ? ""
      : " Se acontecer de novo, abra o arquivo no nanoCAD ou no AutoCAD, salve uma cópia com outro nome e envie essa cópia.";
    notes.push(note("Não foi possível limpar este arquivo", (d.error || "Ocorreu um erro inesperado.") + tip, "note-err"));
  }
  box.replaceChildren(...notes);
}

function renderTech(j, st, d) {
  const tech = $(".tech", j.el);
  const r = st === "done" ? d.result : null;
  const hasLog = d.log?.length > 0;
  tech.hidden = !r && !hasLog;
  if (tech.hidden) return;

  const dl = $(".tech-stats", tech);
  const rows = [];
  if (r) {
    rows.push(["Tabelas de ordem duplicadas (*A) removidas", fmtInt(r.removed)]);
    rows.push(["Blocos apagados", fmtInt(r.blocks_purged)]);
    if (r.invalid_hatches_removed) rows.push(["Hachuras sem contorno apagadas", fmtInt(r.invalid_hatches_removed)]);
    if (r.datalinks_removed) rows.push(["Vínculos com Excel (DATALINK) removidos", fmtInt(r.datalinks_removed)]);
    if (r.preserved_raw) rows.push(["Objetos copiados byte a byte, sem alteração", fmtInt(r.preserved_raw)]);
    if (r.skipped) rows.push(["Registros ilegíveis descartados", fmtInt(r.skipped)]);
    if (r.left) rows.push(["Entradas *A mantidas (não são lixo)", fmtInt(r.left)]);
    rows.push(["Versão do DWG", fmtVersion(r.version)]);
    rows.push(["Tamanho", `${fmtMB(r.size_before)} para ${fmtMB(r.size_after)}`]);
  }
  dl.replaceChildren(...rows.flatMap(([k, v]) => {
    const dt = document.createElement("dt"); dt.textContent = k;
    const dd = document.createElement("dd"); dd.textContent = v;
    return [dt, dd];
  }));

  const pre = $(".tech-log", tech);
  pre.hidden = !hasLog;
  if (hasLog) {
    const atBottom = pre.scrollTop + pre.clientHeight >= pre.scrollHeight - 4;
    pre.textContent = d.log.join("\n");
    if (atBottom) pre.scrollTop = pre.scrollHeight;
  }
}

function jobState(j) {
  return j.data?.state || (j.xhr ? "uploading" : "waiting");
}

function updateList() {
  const jobs = [...state.jobs.values()];
  $("#files").hidden = jobs.length === 0;
  $("#clear-done").hidden = jobs.filter((j) => TERMINAL.has(jobState(j))).length < 2;
  updateTitle(jobs);
}

function updateTitle(jobs) {
  const active = jobs.filter((j) => ACTIVE.has(jobState(j)));
  if (active.length) {
    const cur = active.find((j) => jobState(j) === "processing");
    document.title = cur
      ? `${Math.floor(cur.data.progress?.pct || 0)}%, limpando | ${APP_TITLE}`
      : `Enviando | ${APP_TITLE}`;
  } else if (document.hidden && jobs.some((j) => jobState(j) === "done")) {
    document.title = `Pronto | ${APP_TITLE}`;
  } else {
    document.title = APP_TITLE;
  }
}

// ---------------------------------------------------------------- arrastar e soltar
function setupDrop() {
  const drop = $("#drop");
  const input = $("#file-input");
  const title = $(".sheet-title", drop);
  const setOver = (on) => {
    drop.classList.toggle("is-over", on);
    title.textContent = on ? title.dataset.over : title.dataset.idle;
  };

  input.addEventListener("change", () => { addFiles([...input.files]); input.value = ""; });
  drop.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") { e.preventDefault(); input.click(); }
  });

  let depth = 0;
  const hasFiles = (e) => [...(e.dataTransfer?.types || [])].includes("Files");
  window.addEventListener("dragenter", (e) => { if (hasFiles(e)) { depth++; setOver(true); } });
  window.addEventListener("dragleave", () => { if (--depth <= 0) { depth = 0; setOver(false); } });
  window.addEventListener("dragover", (e) => { if (hasFiles(e)) e.preventDefault(); });
  window.addEventListener("drop", (e) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    depth = 0;
    setOver(false);
    addFiles([...e.dataTransfer.files]);
  });

  window.addEventListener("beforeunload", (e) => {
    if (state.uploading || state.uploadQueue.length) { e.preventDefault(); e.returnValue = ""; }
  });
  document.addEventListener("visibilitychange", () => { if (!document.hidden) updateList(); });
}

// ---------------------------------------------------------------- início
async function init() {
  setupDrop();
  $("#clear-done").onclick = () => {
    for (const j of [...state.jobs.values()]) if (TERMINAL.has(jobState(j))) removeJob(j);
  };
  await refreshStatus();

  // retoma os arquivos enviados por este navegador
  const ids = loadIds();
  if (ids.length) {
    try {
      const list = await api("/api/jobs?ids=" + ids.join(","));
      for (const d of list.sort((a, b) => a.created - b.created)) {
        const j = { key: d.id, id: d.id, data: d };
        state.jobs.set(d.id, j);
        render(j);
      }
    } catch {}
    saveIds();
  }
  updateList();

  setInterval(refreshJobs, 1500);
  setInterval(refreshStatus, 5000);
  // atualiza os cronômetros mesmo sem novidades do servidor
  setInterval(() => { for (const j of state.jobs.values()) if (!TERMINAL.has(jobState(j))) render(j); }, 1000);
}

init();
