// ecoflower-home – Weboberfläche (ohne Build-Schritt, reines JavaScript)
"use strict";

const $app = document.getElementById("app");
let cache = { devices: [], commands: [], status: {} };

// ---------- Hilfsfunktionen ----------

async function api(path, opts = {}) {
  const res = await fetch("/api" + path, {
    headers: { "Content-Type": "application/json" },
    ...opts,
    body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || res.statusText);
  return data;
}

function esc(s) {
  return String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}

function toast(msg, isErr = false) {
  const t = document.getElementById("toast");
  t.textContent = msg;
  t.className = isErr ? "err" : "";
  t.style.display = "block";
  clearTimeout(toast._t);
  toast._t = setTimeout(() => (t.style.display = "none"), 4500);
}

const fmtTime = ts => (ts ? new Date(ts * 1000).toLocaleString("de-DE") : "–");
const fmtNum = (v, unit = "") => (v === null || v === undefined ? "–" : `${Math.round(v * 10) / 10}${unit}`);
const devName = d => d.alias || d.name || d.sn;

async function guarded(fn) {
  try { await fn(); } catch (e) { toast(e.message, true); }
}

async function loadBase() {
  [cache.status, cache.devices, cache.commands] = await Promise.all([api("/status"), api("/devices"), api("/commands")]);
  document.getElementById("version").textContent = "v" + cache.status.version;
  const m = cache.status.mqtt || {};
  const mqttBadge = m.connected
    ? `<span class="badge ok" title="Live-Daten pro Gerät, ${m.messages} Nachrichten">MQTT verbunden</span>`
    : `<span class="badge err" title="${esc(m.error || "")}">MQTT getrennt</span>`;
  document.getElementById("mode-badge").innerHTML = mqttBadge + " " + (cache.status.dry_run
    ? '<span class="badge warn" title="Regeln senden keine Befehle, sie protokollieren nur">Testmodus</span>'
    : '<span class="badge ok">Regeln aktiv</span>');
}

// ---------- Routing ----------

const routes = {
  "": renderOverview,
  device: renderDevice,
  rules: renderRules,
  commands: renderCommands,
  events: renderEvents,
  settings: renderSettings,
};

async function route() {
  const [name, arg] = location.hash.replace(/^#\/?/, "").split("/");
  document.querySelectorAll("nav a").forEach(a => {
    const tab = a.dataset.tab;
    a.classList.toggle("active", (tab === "overview" && (name === "" || name === "device")) || tab === name);
  });
  clearInterval(route._timer);
  await guarded(async () => {
    await loadBase();
    await (routes[name] || renderOverview)(decodeURIComponent(arg || ""));
  });
}
window.addEventListener("hashchange", route);

// ---------- Übersicht ----------

async function renderOverview() {
  const devs = cache.devices;
  let html = `<div class="toolbar"><h1>Übersicht</h1><span class="spacer"></span>
    <button class="secondary" id="sync">Geräte von EcoFlow abrufen</button></div>`;
  if (!cache.status.configured) {
    html += `<div class="notice">Noch keine API-Zugangsdaten hinterlegt. Bitte zuerst unter <a href="#/settings">Einstellungen</a> Access Key und Secret Key eintragen.</div>`;
  }
  if (!devs.length) {
    html += `<p class="muted">Noch keine Geräte. Klicken Sie auf „Geräte von EcoFlow abrufen“.</p>`;
  }
  html += `<div id="system">${systemCard(devs)}</div>`;
  html += `<h2>Speicher</h2><div class="grid">` + devs.map(deviceCard).join("") + `</div>`;
  $app.innerHTML = html;
  document.getElementById("sync").onclick = () => guarded(async () => {
    const r = await api("/devices/sync", { method: "POST" });
    toast(`${r.found} Geräte gefunden, ${r.added} neu`);
    setTimeout(route, 1500);
  });
  route._timer = setInterval(() => guarded(async () => {
    cache.devices = await api("/devices");
    const grid = $app.querySelector(".grid");
    if (grid) grid.innerHTML = cache.devices.map(deviceCard).join("");
    const sys = document.getElementById("system");
    if (sys) sys.innerHTML = systemCard(cache.devices);
  }), 15000);
}

function systemCard(devs) {
  const m = devs.find(d => d.role === "MASTER") || devs.find(d => (d.sys_soc ?? 0) > 0);
  if (!m) return "";
  const soc = m.sys_soc ?? 0;
  return `<div class="card system">
    <h3><span>System</span><span class="muted">Werte vom Hauptgerät ${esc(devName(m))}</span></h3>
    <div class="row" style="align-items:center">
      <div><div class="soc">${fmtNum(m.sys_soc, " %")}</div>
        <div class="bar"><div style="width:${soc}%"></div>${m.min_dsg != null ? `<div class="mark" style="left:${m.min_dsg}%"></div>` : ""}</div></div>
      <dl class="kv">
        <dt>Solar</dt><dd>${fmtNum(m.pv_w, " W")}</dd>
        <dt>Netz</dt><dd>${fmtNum(m.grid_w, " W")}</dd>
        <dt>Hausverbrauch</dt><dd>${fmtNum(m.load_w, " W")}</dd>
        <dt>Batterien gesamt</dt><dd>${fmtNum(m.battery_w, " W")}</dd>
      </dl>
      <dl class="kv">
        <dt>Entladegrenze</dt><dd>${fmtNum(m.min_dsg, " %")}</dd>
        <dt>Ladegrenze</dt><dd>${fmtNum(m.max_chg, " %")}</dd>
        <dt>Backup-Reserve</dt><dd>${fmtNum(m.backup, " %")}</dd>
      </dl>
    </div></div>`;
}

const onOff = v => (v === null || v === undefined ? "–" : v ? "an" : "aus");

function deviceCard(d) {
  const soc = d.soc ?? 0;
  const role = d.role ? `<span class="badge">${esc(d.role === "MASTER" ? "Haupt" : d.role === "SLAVE" ? "Neben" : d.role)}</span>` : "";
  const state = !d.enabled ? '<span class="badge">pausiert</span>'
    : d.last_error ? '<span class="badge err" title="' + esc(d.last_error) + '">Fehler</span>'
    : '<span class="badge ok">ok</span>';
  const socHint = d.soc == null ? `<div class="muted">Ladestand kommt nur über MQTT</div>` : "";
  return `<div class="card"><a href="#/device/${encodeURIComponent(d.sn)}">
    <h3><span>${esc(devName(d))}</span><span>${role} ${state}</span></h3>
    <div class="muted">${esc(d.product)} · ${esc(d.sn)}</div>
    <div class="soc">${fmtNum(d.soc, " %")}</div>
    <div class="bar"><div style="width:${soc}%"></div></div>${socHint}
    <dl class="kv">
      <dt>AC-Leistung (Gerät)</dt><dd>${fmtNum(d.ac_w, " W")}</dd>
      <dt>AC-Ausgang 1 / 2</dt><dd>${onOff(d.ac1_on)} / ${onOff(d.ac2_on)}</dd>
      <dt>Live-Daten (MQTT)</dt><dd>${fmtTime(d.mqtt_ts)}</dd>
      <dt>Letzte Abfrage</dt><dd>${fmtTime(d.last_poll)}</dd>
    </dl></a></div>`;
}

// ---------- Gerät ----------

async function renderDevice(sn) {
  const d = cache.devices.find(x => x.sn === sn);
  if (!d) { $app.innerHTML = "<p>Gerät nicht gefunden.</p>"; return; }
  const [quotas, history, events] = await Promise.all([
    api(`/devices/${encodeURIComponent(sn)}/quotas`),
    api(`/devices/${encodeURIComponent(sn)}/history?hours=24`),
    api(`/events?limit=30&sn=${encodeURIComponent(sn)}`),
  ]);
  const cmdOptions = cache.commands.map(c => `<option value="${c.id}">${esc(c.name)}</option>`).join("");
  $app.innerHTML = `
    <div class="toolbar"><a href="#/" class="muted">← Übersicht</a></div>
    <h1>${esc(devName(d))} <span class="muted">${esc(d.product)} · ${esc(d.sn)} ${d.role ? "· " + esc(d.role) : ""}</span></h1>

    <div class="grid">
      <div class="card">
        <h3>Gerät</h3>
        <label>Anzeigename</label><input id="alias" value="${esc(d.alias)}" placeholder="${esc(d.name)}">
        <label>Quota für den Ladestand <span class="muted">(leer = automatisch)</span></label>
        <input id="soc_key" value="${esc(d.soc_key)}" placeholder="bmsBattSoc, soc, f32ShowSoc …">
        <label class="checkbox"><input type="checkbox" id="enabled" ${d.enabled ? "checked" : ""}> regelmäßig abfragen</label>
        <div class="row" style="margin-top:12px"><button id="save-dev">Speichern</button>
          <button class="secondary" id="refresh">Jetzt abfragen</button></div>
      </div>
      <div class="card">
        <h3>Befehl senden</h3>
        <div class="muted">Wird sofort gesendet – auch im Testmodus.</div>
        <label>Befehl</label><select id="cmd">${cmdOptions}</select>
        <label>Wert</label><input id="cmd-value" placeholder="z. B. 20 oder true">
        <div class="row" style="margin-top:12px"><button id="send">Senden</button></div>
        <label>…oder Rohbefehl (JSON, „sn“ wird ergänzt)</label>
        <textarea id="raw" placeholder='{"cmdId":17,"cmdFunc":254,"dirDest":1,"dirSrc":1,"dest":2,"needAck":true,"params":{"cfgMinDsgSoc":20}}'></textarea>
        <div class="row" style="margin-top:8px"><button class="secondary" id="send-raw">Rohbefehl senden</button></div>
        <pre id="send-result" style="display:none"></pre>
      </div>
    </div>

    <h2>Ladestand der letzten 24 Stunden</h2>
    ${chart(history)}

    <h2>Letzte Ereignisse</h2>
    ${eventsTable(events)}

    <h2>Alle Messwerte (Quotas) <span class="muted">${Object.keys(quotas).length} Werte</span></h2>
    <input id="filter" placeholder="Filtern, z. B. Soc oder relay">
    <table id="quotas"></table>
  `;

  const drawQuotas = () => {
    const f = document.getElementById("filter").value.toLowerCase();
    const rows = Object.keys(quotas).sort().filter(k => !f || k.toLowerCase().includes(f) || String(quotas[k]).toLowerCase().includes(f));
    document.getElementById("quotas").innerHTML = "<tr><th>Schlüssel</th><th>Wert</th></tr>" +
      rows.map(k => `<tr><td class="mono">${esc(k)}</td><td class="mono">${esc(typeof quotas[k] === "object" ? JSON.stringify(quotas[k]) : quotas[k])}</td></tr>`).join("");
  };
  document.getElementById("filter").oninput = drawQuotas;
  drawQuotas();

  document.getElementById("save-dev").onclick = () => guarded(async () => {
    await api(`/devices/${encodeURIComponent(sn)}`, { method: "PUT", body: {
      alias: document.getElementById("alias").value,
      soc_key: document.getElementById("soc_key").value,
      enabled: document.getElementById("enabled").checked,
    }});
    toast("Gespeichert");
    route();
  });
  document.getElementById("refresh").onclick = () => guarded(async () => {
    await api(`/devices/${encodeURIComponent(sn)}/refresh`, { method: "POST" });
    toast("Aktualisiert");
    route();
  });
  const showResult = r => {
    const pre = document.getElementById("send-result");
    pre.style.display = "block";
    pre.textContent = JSON.stringify(r, null, 2);
  };
  document.getElementById("send").onclick = () => guarded(async () => {
    const r = await api(`/devices/${encodeURIComponent(sn)}/send`, { method: "POST", body: {
      command_id: Number(document.getElementById("cmd").value),
      value: document.getElementById("cmd-value").value,
    }});
    showResult(r);
    toast("Gesendet – Werte in ca. 30 s mit „Jetzt abfragen“ prüfen");
  });
  document.getElementById("send-raw").onclick = () => guarded(async () => {
    let raw;
    try { raw = JSON.parse(document.getElementById("raw").value); } catch { throw new Error("Rohbefehl ist kein gültiges JSON"); }
    showResult(await api(`/devices/${encodeURIComponent(sn)}/send`, { method: "POST", body: { raw } }));
    toast("Gesendet");
  });
}

function chart(samples) {
  const pts = samples.filter(s => s.soc != null);
  if (pts.length < 2) return `<div class="card muted">Noch nicht genug Messwerte.</div>`;
  const W = 1000, H = 200, pad = 30;
  const t0 = pts[0].ts, t1 = pts[pts.length - 1].ts || t0 + 1;
  const x = t => pad + ((t - t0) / Math.max(1, t1 - t0)) * (W - 2 * pad);
  const y = v => H - pad + 10 - (v / 100) * (H - 2 * pad);
  const line = pts.map((p, i) => `${i ? "L" : "M"}${x(p.ts).toFixed(1)},${y(p.soc).toFixed(1)}`).join(" ");
  const lim = samples.filter(s => s.min_dsg != null);
  const limLine = lim.length > 1 ? `<path d="${lim.map((p, i) => `${i ? "L" : "M"}${x(p.ts).toFixed(1)},${y(p.min_dsg).toFixed(1)}`).join(" ")}" stroke="var(--warn)" stroke-dasharray="4 4" fill="none"/>` : "";
  const grid = [0, 25, 50, 75, 100].map(v => `<line x1="${pad}" x2="${W - pad}" y1="${y(v)}" y2="${y(v)}" stroke="var(--line)"/><text x="4" y="${y(v) + 4}" fill="var(--muted)" font-size="11">${v}%</text>`).join("");
  const lbl = ts => new Date(ts * 1000).toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
  return `<svg class="chart" viewBox="0 0 ${W} ${H}" preserveAspectRatio="none">${grid}${limLine}
    <path d="${line}" stroke="var(--accent)" stroke-width="2" fill="none"/>
    <text x="${pad}" y="${H - 4}" fill="var(--muted)" font-size="11">${lbl(t0)}</text>
    <text x="${W - pad}" y="${H - 4}" fill="var(--muted)" font-size="11" text-anchor="end">${lbl(t1)}</text></svg>
    <div class="muted">Türkis: Ladestand · gestrichelt: vom Gerät gemeldete Entladegrenze</div>`;
}

function eventsTable(events) {
  if (!events.length) return `<p class="muted">Keine Ereignisse.</p>`;
  const name = sn => { const d = cache.devices.find(x => x.sn === sn); return d ? devName(d) : sn; };
  return `<table><tr><th>Zeit</th><th>Gerät</th><th>Meldung</th></tr>` +
    events.map(e => `<tr class="level-${esc(e.level)}"><td style="white-space:nowrap">${fmtTime(e.ts)}</td><td>${esc(name(e.sn))}</td><td>${esc(e.message)}</td></tr>`).join("") + `</table>`;
}

// ---------- Regeln ----------

async function renderRules() {
  const rules = await api("/rules");
  const cmdName = id => (cache.commands.find(c => c.id === id) || {}).name || "–";
  const devLabel = sn => { const d = cache.devices.find(x => x.sn === sn); return d ? devName(d) : sn; };
  const describe = r => r.kind === "keep"
    ? `„${esc(cmdName(r.command_id))}“ = ${esc(r.value)} halten${r.read_key ? ` (prüft ${esc(r.read_key)})` : ""}`
    : `≤ ${r.below ?? "–"} %: „${esc(cmdName(r.below_command_id))}“ = ${esc(r.below_value)} · ≥ ${r.above ?? "–"} %: „${esc(cmdName(r.above_command_id))}“ = ${esc(r.above_value)}`;
  const stateLabel = { ok: "übernommen", sent: "gesendet", rejected: "nicht übernommen", below: "unterhalb", above: "oberhalb", outside: "außerhalb Zeitfenster" };

  $app.innerHTML = `
    <div class="toolbar"><h1>Regeln</h1><span class="spacer"></span><button id="new-rule">Neue Regel</button></div>
    ${cache.status.dry_run ? `<div class="notice">Testmodus ist aktiv: Regeln protokollieren nur, was sie tun würden. Umschalten unter <a href="#/settings">Einstellungen</a>.</div>` : ""}
    <table><tr><th>Name</th><th>Gerät</th><th>Regel</th><th>Zeitfenster</th><th>Status</th><th></th></tr>
    ${rules.map(r => `<tr>
      <td>${esc(r.name)} ${r.enabled ? "" : '<span class="badge">aus</span>'}</td>
      <td>${esc(devLabel(r.sn))}</td>
      <td>${describe(r)}</td>
      <td>${r.window_start && r.window_end ? esc(r.window_start + "–" + r.window_end) : "immer"}</td>
      <td><span class="badge ${r.last_state === "rejected" ? "err" : ""}">${esc(stateLabel[r.last_state] || r.last_state || "–")}</span></td>
      <td style="white-space:nowrap"><button class="secondary" data-edit="${r.id}">Bearbeiten</button> <button class="danger" data-del="${r.id}">✕</button></td>
    </tr>`).join("")}</table>
    <div id="editor"></div>`;

  document.getElementById("new-rule").onclick = () => ruleEditor({ kind: "keep", enabled: true });
  $app.querySelectorAll("[data-edit]").forEach(b => b.onclick = () => ruleEditor(rules.find(r => r.id == b.dataset.edit)));
  $app.querySelectorAll("[data-del]").forEach(b => b.onclick = () => guarded(async () => {
    if (!confirm("Regel löschen?")) return;
    await api("/rules/" + b.dataset.del, { method: "DELETE" });
    route();
  }));
}

function ruleEditor(r) {
  const opts = (sel) => `<option value="">–</option>` + cache.commands.map(c => `<option value="${c.id}" ${c.id === sel ? "selected" : ""}>${esc(c.name)}</option>`).join("");
  const devOpts = cache.devices.map(d => `<option value="${esc(d.sn)}" ${d.sn === r.sn ? "selected" : ""}>${esc(devName(d))}</option>`).join("");
  const el = document.getElementById("editor");
  el.innerHTML = `<div class="card" style="margin-top:20px">
    <h3>${r.id ? "Regel bearbeiten" : "Neue Regel"}</h3>
    <div class="row">
      <div><label>Name</label><input id="r-name" value="${esc(r.name)}"></div>
      <div><label>Gerät</label><select id="r-sn">${devOpts}</select></div>
      <div><label>Typ</label><select id="r-kind">
        <option value="keep" ${r.kind === "keep" ? "selected" : ""}>Wert halten</option>
        <option value="threshold" ${r.kind === "threshold" ? "selected" : ""}>Schwellwert auf Ladestand</option></select></div>
    </div>
    <div class="row">
      <div><label>Zeitfenster von (HH:MM, leer = immer)</label><input id="r-ws" value="${esc(r.window_start)}" placeholder="22:00"></div>
      <div><label>bis</label><input id="r-we" value="${esc(r.window_end)}" placeholder="06:00"></div>
      <div><label class="checkbox" style="margin-top:30px"><input type="checkbox" id="r-en" ${r.enabled !== false ? "checked" : ""}> aktiv</label></div>
    </div>
    <div id="r-keep">
      <div class="muted" style="margin-top:12px">Sendet den Befehl, bis der zurückgelesene Wert dem Sollwert entspricht. Meldet, wenn das Gerät den Wert nicht übernimmt.</div>
      <div class="row">
        <div><label>Befehl</label><select id="r-cmd">${opts(r.command_id)}</select></div>
        <div><label>Sollwert</label><input id="r-val" value="${esc(r.value)}" placeholder="20"></div>
        <div><label>Zum Prüfen zurücklesen (Quota)</label><input id="r-read" value="${esc(r.read_key)}" placeholder="cmsMinDsgSoc"></div>
      </div>
    </div>
    <div id="r-thr">
      <div class="muted" style="margin-top:12px">Schaltet einmal, wenn der Ladestand die untere Schwelle erreicht, und erst wieder, wenn er die obere erreicht. Außerhalb des Zeitfensters wird der „oberhalb“-Befehl wiederhergestellt.</div>
      <div class="row">
        <div><label>Ladestand ≤ (%)</label><input id="r-below" type="number" value="${r.below ?? ""}"></div>
        <div><label>dann Befehl</label><select id="r-bcmd">${opts(r.below_command_id)}</select></div>
        <div><label>mit Wert</label><input id="r-bval" value="${esc(r.below_value)}" placeholder="false"></div>
      </div>
      <div class="row">
        <div><label>Ladestand ≥ (%)</label><input id="r-above" type="number" value="${r.above ?? ""}"></div>
        <div><label>dann Befehl</label><select id="r-acmd">${opts(r.above_command_id)}</select></div>
        <div><label>mit Wert</label><input id="r-aval" value="${esc(r.above_value)}" placeholder="true"></div>
      </div>
    </div>
    <div class="row" style="margin-top:16px"><button id="r-save">Speichern</button><button class="secondary" id="r-cancel">Abbrechen</button></div>
  </div>`;
  const toggle = () => {
    const k = document.getElementById("r-kind").value;
    document.getElementById("r-keep").style.display = k === "keep" ? "" : "none";
    document.getElementById("r-thr").style.display = k === "threshold" ? "" : "none";
  };
  document.getElementById("r-kind").onchange = toggle;
  toggle();
  const v = id => document.getElementById(id).value;
  const num = id => (v(id) === "" ? null : Number(v(id)));
  document.getElementById("r-cancel").onclick = () => (el.innerHTML = "");
  document.getElementById("r-save").onclick = () => guarded(async () => {
    if (!v("r-name")) throw new Error("Bitte einen Namen eingeben");
    await api("/rules", { method: "POST", body: {
      id: r.id || 0, sn: v("r-sn"), name: v("r-name"), enabled: document.getElementById("r-en").checked,
      kind: v("r-kind"), window_start: v("r-ws"), window_end: v("r-we"),
      command_id: num("r-cmd"), value: v("r-val"), read_key: v("r-read"),
      below: num("r-below"), below_command_id: num("r-bcmd"), below_value: v("r-bval"),
      above: num("r-above"), above_command_id: num("r-acmd"), above_value: v("r-aval"),
    }});
    toast("Regel gespeichert");
    route();
  });
  el.scrollIntoView({ behavior: "smooth" });
}

// ---------- Befehle ----------

async function renderCommands() {
  const cmds = cache.commands;
  $app.innerHTML = `
    <div class="toolbar"><h1>Befehlsvorlagen</h1><span class="spacer"></span><button id="new-cmd">Neue Vorlage</button></div>
    <p class="muted">Platzhalter: <span class="mono">{{sn}}</span> = Seriennummer, <span class="mono">{{value}}</span> = Wert (Zahl, true/false oder Text).</p>
    <table><tr><th>Name</th><th>Vorlage</th><th></th></tr>
    ${cmds.map(c => `<tr><td>${esc(c.name)}<div class="muted">${esc(c.description)}</div></td><td class="mono">${esc(c.template)}</td>
      <td style="white-space:nowrap"><button class="secondary" data-edit="${c.id}">Bearbeiten</button> <button class="danger" data-del="${c.id}">✕</button></td></tr>`).join("")}
    </table><div id="editor"></div>`;
  const edit = c => {
    document.getElementById("editor").innerHTML = `<div class="card" style="margin-top:20px">
      <label>Name</label><input id="c-name" value="${esc(c.name)}">
      <label>Beschreibung</label><input id="c-desc" value="${esc(c.description)}">
      <label>Vorlage (JSON)</label><textarea id="c-tpl">${esc(c.template)}</textarea>
      <div class="row" style="margin-top:12px"><button id="c-save">Speichern</button></div></div>`;
    document.getElementById("c-save").onclick = () => guarded(async () => {
      await api("/commands", { method: "POST", body: { id: c.id || 0, name: document.getElementById("c-name").value,
        description: document.getElementById("c-desc").value, template: document.getElementById("c-tpl").value } });
      toast("Gespeichert");
      route();
    });
  };
  document.getElementById("new-cmd").onclick = () => edit({ name: "", description: "",
    template: '{"sn":"{{sn}}","cmdId":17,"cmdFunc":254,"dirDest":1,"dirSrc":1,"dest":2,"needAck":true,"params":{"cfgXyz":{{value}}}}' });
  $app.querySelectorAll("[data-edit]").forEach(b => b.onclick = () => edit(cmds.find(c => c.id == b.dataset.edit)));
  $app.querySelectorAll("[data-del]").forEach(b => b.onclick = () => guarded(async () => {
    if (!confirm("Vorlage löschen?")) return;
    await api("/commands/" + b.dataset.del, { method: "DELETE" });
    route();
  }));
}

// ---------- Ereignisse ----------

async function renderEvents() {
  const events = await api("/events?limit=500");
  $app.innerHTML = `<h1>Ereignisse</h1>${eventsTable(events)}`;
}

// ---------- Einstellungen ----------

async function renderSettings() {
  const s = await api("/settings");
  $app.innerHTML = `<h1>Einstellungen</h1>
    <div class="card">
      <h3>EcoFlow-API</h3>
      <div class="muted">Zugangsdaten von <a href="https://developer-eu.ecoflow.com/" target="_blank" style="color:var(--accent)">developer-eu.ecoflow.com</a> (Access Key / Secret Key).</div>
      <label>Access Key</label><input id="s-ak" value="${esc(s.access_key)}">
      <label>Secret Key ${s.secret_key_set ? '<span class="badge ok">gespeichert</span>' : ""}</label>
      <input id="s-sk" type="password" placeholder="${s.secret_key_set ? "unverändert lassen" : ""}">
      <label>API-Adresse</label><select id="s-url">
        ${["https://api-e.ecoflow.com", "https://api.ecoflow.com", "https://api-a.ecoflow.com"].map(u => `<option ${u === s.api_url ? "selected" : ""}>${u}</option>`).join("")}
      </select>
      <div class="row">
        <div><label>Abfrage-Intervall (Sekunden, min. 10)</label><input id="s-poll" type="number" value="${s.poll_seconds}"></div>
        <div><label>Verlauf aufbewahren (Tage)</label><input id="s-ret" type="number" value="${s.retention_days}"></div>
      </div>
      <label class="checkbox" style="margin-top:14px"><input type="checkbox" id="s-mqtt" ${s.mqtt_enabled ? "checked" : ""}>
        Live-Daten über MQTT empfangen (nötig für den Ladestand der einzelnen Speicher)</label>
      <label class="checkbox" style="margin-top:8px"><input type="checkbox" id="s-dry" ${s.dry_run ? "checked" : ""}>
        Testmodus – Regeln protokollieren nur und senden keine Befehle</label>
      <div class="row" style="margin-top:16px"><button id="s-save">Speichern</button></div>
    </div>`;
  document.getElementById("s-save").onclick = () => guarded(async () => {
    await api("/settings", { method: "POST", body: {
      access_key: document.getElementById("s-ak").value,
      secret_key: document.getElementById("s-sk").value,
      api_url: document.getElementById("s-url").value,
      poll_seconds: Number(document.getElementById("s-poll").value),
      retention_days: Number(document.getElementById("s-ret").value),
      dry_run: document.getElementById("s-dry").checked,
      mqtt_enabled: document.getElementById("s-mqtt").checked,
    }});
    toast("Gespeichert");
    route();
  });
}

route();
