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
  [cache.status, cache.devices, cache.commands, cache.systems] = await Promise.all([api("/status"), api("/devices"), api("/commands"), api("/systems")]);
  document.getElementById("version").textContent = "v" + cache.status.version;
  const m = cache.status.mqtt || {};
  const mqttBadge = !cache.status.configured ? "" : m.connected
    ? `<span class="badge ok" title="Live-Daten pro Gerät, ${m.messages} Nachrichten">MQTT verbunden</span>`
    : `<span class="badge err" title="${esc(m.error || "")}">MQTT getrennt</span>`;
  const u = cache.status.update || {};
  const updBadge = u.available ? `<a href="#/settings" class="badge warn" style="text-decoration:none">Update ${esc(u.latest)} verfügbar</a> ` : "";
  document.getElementById("mode-badge").innerHTML = updBadge + mqttBadge + " " + (cache.status.dry_run
    ? '<span class="badge warn" title="Regeln senden keine Befehle, sie protokollieren nur">Testmodus</span>'
    : '<span class="badge ok">Regeln aktiv</span>');
}

// ---------- Routing ----------

const routes = {
  "": renderOverview,
  device: renderDevice,
  regulator: renderRegulator,
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
  clearInterval(route._meterTimer);
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
    <button class="secondary" id="new-system">Neues System</button>
    <button class="secondary" id="sync">Geräte von EcoFlow abrufen</button></div>`;
  if (!cache.status.configured) {
    html += `<div class="notice">Noch keine API-Zugangsdaten hinterlegt. Bitte zuerst unter <a href="#/settings">Einstellungen</a> Access Key und Secret Key eintragen.</div>`;
  }
  if (!devs.length) {
    html += `<p class="muted">Noch keine Geräte. Klicken Sie auf „Geräte von EcoFlow abrufen“.</p>`;
  }
  html += `<div id="meter">${meterCard(cache.status.meter)}</div>`;
  html += `<div id="systems">${systemsHtml()}</div>`;
  $app.innerHTML = html;
  bindSystemControls();
  // Zähler im Sekundentakt aktualisieren
  route._meterTimer = setInterval(async () => {
    try {
      const m = await api("/meter");
      const el = document.getElementById("meter");
      if (el) el.innerHTML = meterCard(m); else clearInterval(route._meterTimer);
    } catch { /* nächster Versuch */ }
  }, 3000);

  document.getElementById("sync").onclick = () => guarded(async () => {
    const r = await api("/devices/sync", { method: "POST" });
    toast(`${r.found} Geräte gefunden, ${r.added} neu`);
    setTimeout(route, 1500);
  });
  document.getElementById("new-system").onclick = () => guarded(async () => {
    const name = prompt("Name des neuen Systems (z. B. „Bad“):");
    if (!name) return;
    await api("/systems", { method: "POST", body: { name } });
    route();
  });
  route._timer = setInterval(() => guarded(async () => {
    // Nicht neu zeichnen, während eine Auswahlliste offen ist
    if (document.activeElement && ["SELECT", "INPUT"].includes(document.activeElement.tagName)) return;
    [cache.devices, cache.systems] = await Promise.all([api("/devices"), api("/systems")]);
    const el = document.getElementById("systems");
    if (el) { el.innerHTML = systemsHtml(); bindSystemControls(); }
  }), 15000);
}

function meterCard(m) {
  if (!m || !m.configured) return "";
  if (!m.ok && m.power_w == null) {
    return `<div class="card meter"><h3><span>Stromzähler</span><span class="badge err">nicht erreichbar</span></h3>
      <div class="muted">${esc(m.error || "")} · <a href="#/settings" style="color:var(--accent)">Einstellungen</a></div></div>`;
  }
  const p = m.power_w ?? 0;
  const dir = p > 20 ? { cls: "act-discharge", text: "Bezug aus dem Netz" } : p < -20 ? { cls: "act-charge", text: "Einspeisung ins Netz" } : { cls: "act-idle", text: "ausgeglichen" };
  const phases = (m.phases || []).map((v, i) => `<dt>L${i + 1}</dt><dd>${fmtNum(v, " W")}</dd>`).join("");
  return `<div class="card meter">
    <h3><span>Stromzähler ${m.model ? `<span class="muted">${esc(m.model)}</span>` : ""}</span>
      <span>${m.ok ? "" : `<span class="badge err" title="${esc(m.error || "")}">veraltet</span> `}<span class="muted">${fmtTime(m.ts)}</span></span></h3>
    <div class="row" style="align-items:center">
      <div><div class="soc">${fmtNum(Math.abs(p), " W")}</div><span class="act ${dir.cls}">${dir.text}</span></div>
      <dl class="kv">${phases}</dl>
    </div></div>`;
}

// Haupt- vor Nebengeräten, dann nach Name
const byRole = (a, b) => (a.role === "MASTER" ? 0 : 1) - (b.role === "MASTER" ? 0 : 1) || devName(a).localeCompare(devName(b));

function systemsHtml() {
  let html = "";
  const count = id => cache.devices.filter(d => d.system_id === id).length;
  const systems = [...cache.systems].sort((a, b) => count(b.id) - count(a.id) || a.name.localeCompare(b.name));
  for (const sys of systems) {
    const devs = cache.devices.filter(d => d.system_id === sys.id).sort(byRole);
    html += `<section class="system-section">
      <div class="toolbar system-head">
        <h2>${esc(sys.name)}</h2>
        <span class="muted">${devs.length} ${devs.length === 1 ? "Gerät" : "Geräte"}${sys.group_key ? " · von EcoFlow erkannt" : " · von Hand angelegt"}</span>
        <span class="spacer"></span>
        <button class="secondary small" data-rename="${sys.id}">Umbenennen</button>
        <button class="secondary small" data-delsys="${sys.id}" title="System entfernen (Geräte bleiben erhalten)">✕</button>
      </div>
      ${devs.length ? systemCard(devs) : `<p class="muted">Noch keine Geräte in diesem System. Ordnen Sie unten auf einer Gerätekarte ein Gerät zu.</p>`}
      <div class="grid">${devs.map(deviceCard).join("")}</div>
    </section>`;
  }
  const loose = cache.devices.filter(d => !cache.systems.some(s => s.id === d.system_id)).sort(byRole);
  if (loose.length) {
    html += `<section class="system-section">
      <div class="toolbar system-head"><h2>Ohne System</h2><span class="muted">${loose.length} ${loose.length === 1 ? "Gerät" : "Geräte"}</span></div>
      ${cache.systems.length ? "" : `<div class="notice">Die Zuordnung zu Systemen kommt automatisch über die MQTT-Live-Daten (<span class="mono">systemGroupId</span>). Alternativ legen Sie mit „Neues System“ ein System an und ordnen die Geräte unten auf den Karten zu.</div>`}
      ${systemCard(loose)}
      <div class="grid">${loose.map(deviceCard).join("")}</div>
    </section>`;
  }
  return html;
}

function systemSelect(d) {
  const opts = cache.systems.map(s => `<option value="${s.id}" ${s.id === d.system_id ? "selected" : ""}>${esc(s.name)}</option>`).join("");
  return `<select data-sysfor="${esc(d.sn)}" title="System, zu dem das Gerät gehört">
    <option value="auto">automatisch${d.system_manual ? "" : " ✓"}</option>
    <option value="0" ${d.system_manual && d.system_id == null ? "selected" : ""}>kein System</option>
    ${d.system_manual ? opts : opts.replace(/ selected/g, "")}</select>`;
}

function bindSystemControls() {
  $app.querySelectorAll("[data-outlet]").forEach(cb => {
    cb.onchange = () => guarded(async () => {
      const on = cb.checked, n = cb.dataset.outlet, d = cache.devices.find(x => x.sn === cb.dataset.sn);
      if (!on && !confirm(`AC-Ausgang ${n} an „${devName(d)}“ ausschalten? Angeschlossene Geräte verlieren den Strom.`)) {
        cb.checked = true;
        return;
      }
      cb.disabled = true;
      try {
        await api(`/devices/${encodeURIComponent(cb.dataset.sn)}/outlet`, { method: "POST", body: { outlet: Number(n), on } });
        toast(`AC-Ausgang ${n} ${on ? "eingeschaltet" : "ausgeschaltet"}`);
        cache.devices = await api("/devices");
        const el = document.getElementById("systems");
        if (el) { el.innerHTML = systemsHtml(); bindSystemControls(); }
      } catch (e) {
        cb.checked = !on;
        throw e;
      } finally {
        cb.disabled = false;
      }
    });
  });
  $app.querySelectorAll("[data-sysfor]").forEach(sel => {
    const d = cache.devices.find(x => x.sn === sel.dataset.sysfor);
    if (d && !d.system_manual) sel.value = "auto";
    sel.onchange = () => guarded(async () => {
      const body = sel.value === "auto" ? { system_auto: true } : { system_id: Number(sel.value) };
      await api(`/devices/${encodeURIComponent(sel.dataset.sysfor)}`, { method: "PUT", body });
      toast("Zuordnung gespeichert");
      route();
    });
  });
  $app.querySelectorAll("[data-rename]").forEach(b => b.onclick = () => guarded(async () => {
    const sys = cache.systems.find(x => x.id == b.dataset.rename);
    const name = prompt("Neuer Name:", sys.name);
    if (!name) return;
    await api("/systems", { method: "POST", body: { id: sys.id, name } });
    route();
  }));
  $app.querySelectorAll("[data-delsys]").forEach(b => b.onclick = () => guarded(async () => {
    if (!confirm("System entfernen? Die Geräte bleiben erhalten und werden ggf. automatisch neu zugeordnet.")) return;
    await api("/systems/" + b.dataset.delsys, { method: "DELETE" });
    route();
  }));
}

function systemCard(devs) {
  const m = devs.find(d => d.role === "MASTER") || devs.find(d => (d.sys_soc ?? 0) > 0);
  if (!m) return "";
  const soc = m.sys_soc ?? 0;
  return `<div class="card system">
    <h3><span>Systemwerte${m.mode ? ` <span class="badge">${esc(m.mode)}</span>` : ""}</span><span class="muted">vom Hauptgerät ${esc(devName(m))}</span></h3>
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

const ACT = {
  charging: { icon: "▲", text: "lädt", cls: "act-charge" },
  discharging: { icon: "▼", text: "entlädt", cls: "act-discharge" },
  idle: { icon: "●", text: "Standby", cls: "act-idle" },
};

function activityBadge(a) {
  if (!a) return `<span class="act act-unknown" title="Zustand erst mit MQTT-Live-Daten bekannt">? unbekannt</span>`;
  const x = ACT[a.state] || ACT.idle;
  const p = a.power_w != null && a.state !== "idle" ? ` · ${Math.round(a.power_w)} W` : "";
  return `<span class="act ${x.cls}" title="Quelle: ${esc(a.source)}">${x.icon} ${x.text}${p}</span>`;
}

function outletRow(d, n, on, w) {
  if (on == null && w == null) return "";
  return `<div class="outlet">
    <span>AC-Ausgang ${n}</span>
    <span class="outlet-w">${on === false ? "aus" : fmtNum(w, " W")}</span>
    <label class="switch" title="AC-Ausgang ${n} ${on ? "ausschalten" : "einschalten"}">
      <input type="checkbox" data-outlet="${n}" data-sn="${esc(d.sn)}" ${on ? "checked" : ""}><span></span>
    </label></div>`;
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
    <div class="soc-row"><div class="soc">${fmtNum(d.soc, " %")}</div>${activityBadge(d.activity)}</div>
    <div class="bar"><div style="width:${soc}%"></div></div>${socHint}
    <dl class="kv">
      <dt>AC-Leistung (Gerät)</dt><dd>${fmtNum(d.ac_w, " W")}</dd>
      <dt>Live-Daten (MQTT)</dt><dd>${fmtTime(d.mqtt_ts)}</dd>
      <dt>Letzte Abfrage</dt><dd>${fmtTime(d.last_poll)}</dd>
    </dl></a>
    ${outletRow(d, 1, d.ac1_on, d.ac1_w)}${outletRow(d, 2, d.ac2_on, d.ac2_w)}
    <div class="card-foot"><span class="muted">System</span>${systemSelect(d)}</div></div>`;
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
        <label>System</label>${systemSelect(d)}
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
  bindSystemControls();

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

// ---------- Regler ----------

async function renderRegulator() {
  const { config: c } = await api("/regulator");
  const devOpts = (sel, empty) => `<option value="">${empty}</option>` + cache.devices.map(d =>
    `<option value="${esc(d.sn)}" ${d.sn === sel ? "selected" : ""}>${esc(devName(d))}${d.role === "MASTER" ? " (Haupt)" : ""}</option>`).join("");
  const field = (id, label, val, unit, hint = "") =>
    `<div><label>${label} ${hint ? `<span class="muted">${hint}</span>` : ""}</label>
      <div class="unit-input"><input id="${id}" type="number" value="${val}"><span>${unit}</span></div></div>`;
  $app.innerHTML = `
    <h1>Regler für einen Einzelspeicher</h1>
    <div class="notice">Für einen Speicher in einem <b>eigenen EcoFlow-System ohne Zähler</b> (z. B. AC Pro Bad). Der Regler misst mit dem Shelly und
      <b>gibt</b> oberhalb der Mindestreserve so viel <b>ab</b>, wie das Haus braucht (über das Einspeiselimit), und <b>lädt</b> nur bei Überschuss
      (über die Backup-Reserve). Er stoppt das Laden, sobald dafür Netzstrom oder Strom aus dem Hauptsystem nötig wäre.
      ${cache.status.dry_run ? "<br><b>Testmodus ist an:</b> Der Regler zeigt nur, was er tun würde. Umschalten unter Einstellungen." : ""}
      ${cache.status.meter?.configured ? "" : `<br><b>Kein Stromzähler eingerichtet</b> – bitte zuerst unter <a href="#/settings" style="color:var(--accent)">Einstellungen</a> den Shelly eintragen.`}</div>

    <div class="grid" style="grid-template-columns: 1fr 1fr">
      <div class="card">
        <h3>Einstellungen</h3>
        <label class="checkbox"><input type="checkbox" id="r-en" ${c.enabled ? "checked" : ""}> Regler eingeschaltet</label>
        <label>Geregelter Speicher</label><select id="r-sn">${devOpts(c.sn, "– bitte wählen –")}</select>
        <label>Hauptgerät des anderen Systems <span class="muted">(optional: stoppt das Laden, wenn es dafür entlädt)</span></label>
        <select id="r-main">${devOpts(c.main_sn, "– keins –")}</select>
        <div class="row">
          ${field("r-res", "Mindestreserve", c.reserve, "%")}
          ${field("r-until", "Laden bis", c.charge_until, "%")}
        </div>
        <div class="row">
          ${field("r-max", "Max. Abgabe", c.max_out, "W")}
          ${field("r-target", "Ziel am Zähler", c.target_grid, "W", "(+ = etwas Bezug)")}
        </div>
        <div class="row">
          ${field("r-start", "Laden ab Einspeisung", c.start_export, "W")}
          ${field("r-stop", "Laden stoppen ab Bezug", c.stop_import, "W")}
        </div>
        <div class="row">
          ${field("r-delay", "Verzögerung", c.delay_secs, "s")}
          ${field("r-int", "Regeltakt", c.interval_secs, "s")}
        </div>
        <div class="row" style="margin-top:14px"><button id="r-save">Speichern</button></div>
      </div>
      <div class="card">
        <h3>Status</h3>
        <div id="reg-status" class="muted">Wird geladen …</div>
      </div>
    </div>`;

  const v = id => document.getElementById(id).value;
  document.getElementById("r-save").onclick = () => guarded(async () => {
    await api("/regulator", { method: "POST", body: {
      enabled: document.getElementById("r-en").checked, sn: v("r-sn"), main_sn: v("r-main"),
      reserve: Number(v("r-res")), charge_until: Number(v("r-until")), max_out: Number(v("r-max")),
      target_grid: Number(v("r-target")), start_export: Number(v("r-start")), stop_import: Number(v("r-stop")),
      delay_secs: Number(v("r-delay")), interval_secs: Number(v("r-int")),
    }});
    toast("Regler gespeichert");
    setTimeout(drawStatus, 1500);
  });

  const drawStatus = async () => {
    const el = document.getElementById("reg-status");
    if (!el) { clearInterval(route._timer); return; }
    const { status: s } = await api("/regulator");
    const i = s.inputs || {};
    const mode = !s.running ? `<span class="act act-unknown">aus</span>`
      : s.waiting ? `<span class="act act-unknown" title="${esc(s.waiting)}">wartet</span>`
      : s.mode === "charge" ? `<span class="act act-charge">▲ lädt aus Überschuss</span>`
      : (i.batt_w ?? 0) < -10 ? `<span class="act act-discharge">▼ versorgt das Haus</span>`
      : `<span class="act act-idle">● bereit</span>`;
    el.innerHTML = `<div style="margin-bottom:10px">${mode} ${s.waiting ? `<span class="muted">${esc(s.waiting)}</span>` : ""}</div>
      ${s.inputs ? `<dl class="kv">
        <dt>Ladestand</dt><dd>${fmtNum(i.soc, " %")}</dd>
        <dt>Akku (+ lädt / − gibt ab)</dt><dd>${fmtNum(i.batt_w, " W")}</dd>
        <dt>Zähler (+ Bezug / − Einspeisung)</dt><dd>${fmtNum(i.grid_w, " W")}</dd>
        <dt>Abgabe-Limit am Gerät</dt><dd>${fmtNum(i.limit_now, " W")}</dd>
        <dt>Backup-Reserve am Gerät</dt><dd>${fmtNum(i.reserve_now, " %")}</dd>
        <dt>Hauptsystem-Akku</dt><dd>${fmtNum(i.main_batt_w, " W")}</dd>
        <dt>Letzter Takt</dt><dd>${fmtTime(s.last_tick)}</dd></dl>` : ""}
      <h2 style="font-size:14px">Letzte Aktionen</h2>
      ${(s.history || []).length ? `<table>${s.history.map(([ts, t]) => `<tr><td style="white-space:nowrap">${fmtTime(ts)}</td><td>${esc(t)}</td></tr>`).join("")}</table>` : `<p class="muted">noch keine</p>`}`;
  };
  await drawStatus();
  route._timer = setInterval(() => guarded(drawStatus), 3000);
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

// ---------- Update ----------

function renderUpdate(u) {
  const el = document.getElementById("upd");
  if (!el) return;
  let html = `<dl class="kv"><dt>Installiert</dt><dd>${esc(u.current)}</dd>
    <dt>Neueste Version</dt><dd>${u.latest ? `<a href="${esc(u.url)}" target="_blank" style="color:var(--accent)">${esc(u.latest)}</a>` : "–"}</dd>
    <dt>Zuletzt geprüft</dt><dd>${fmtTime(u.checked_at)}</dd></dl>`;
  if (u.error) html += `<div class="notice">Prüfung fehlgeschlagen: ${esc(u.error)}</div>`;
  if (u.available && u.blocked) html += `<div class="notice">Update verfügbar, aber nicht automatisch installierbar: ${esc(u.blocked)}</div>`;
  if (u.available && u.notes) html += `<details style="margin-top:10px"><summary>Was ist neu?</summary><pre>${esc(u.notes)}</pre></details>`;
  html += `<div class="row" style="margin-top:12px">
    <button class="secondary" id="upd-check">Nach Updates suchen</button>
    ${u.available && !u.blocked ? `<button id="upd-install">Update auf ${esc(u.latest)} installieren</button>` : ""}</div>`;
  if (!u.available && u.latest && !u.error) html += `<div class="muted" style="margin-top:8px">Sie verwenden die neueste Version.</div>`;
  el.innerHTML = html;
  document.getElementById("upd-check").onclick = () => guarded(async () => {
    el.querySelector("#upd-check").disabled = true;
    renderUpdate(await api("/update/check", { method: "POST" }));
    await loadBase();
  });
  const inst = document.getElementById("upd-install");
  if (inst) inst.onclick = () => guarded(async () => {
    if (!confirm(`Update auf Version ${u.latest} installieren? Das Programm startet danach neu.`)) return;
    inst.disabled = true;
    inst.textContent = "Wird heruntergeladen …";
    const r = await api("/update/install", { method: "POST" });
    inst.textContent = "Neustart …";
    // Warten, bis die neue Version antwortet
    for (let i = 0; i < 60; i++) {
      await new Promise(res => setTimeout(res, 2000));
      try {
        const st = await api("/status");
        if (st.version === r.version) { toast(`Update auf ${r.version} abgeschlossen`); location.reload(); return; }
      } catch { /* startet noch */ }
    }
    toast("Neustart dauert ungewöhnlich lange – bitte Seite neu laden", true);
  });
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
    </div>

    <div class="card" style="margin-top:16px">
      <h3>Stromzähler (Shelly 3EM / Pro 3EM)</h3>
      <div class="muted">Wird direkt im Heimnetz abgefragt – schneller als über die EcoFlow-Cloud. Generation (Shelly 3EM bzw. Pro 3EM) wird automatisch erkannt. Benutzer ist „admin“; einen anderen Benutzer beim Shelly 3EM als <span class="mono">benutzer:passwort</span> eintragen.</div>
      <div class="row">
        <div><label>IP-Adresse oder Name (leer = aus)</label><input id="m-host" value="${esc(s.shelly_host)}" placeholder="192.168.1.50"></div>
        <div><label>Passwort ${s.shelly_password_set ? '<span class="badge ok">gespeichert</span>' : '<span class="muted">(nur falls im Shelly eingeschaltet)</span>'}</label>
          <input id="m-pw" type="password" placeholder="${s.shelly_password_set ? "unverändert lassen" : "kein Passwort"}"></div>
        <div><label>Abfrage alle (Sekunden)</label><input id="m-int" type="number" min="1" max="60" value="${s.shelly_interval}"></div>
      </div>
      ${s.shelly_password_set ? `<label class="checkbox" style="margin-top:8px"><input type="checkbox" id="m-pw-del"> gespeichertes Passwort löschen</label>` : ""}
      <div class="row" style="margin-top:12px">
        <button class="secondary" id="m-test">Verbindung testen</button>
        <button id="m-save">Speichern</button>
      </div>
      <div id="m-result" style="margin-top:10px"></div>
    </div>

    <div class="card" style="margin-top:16px">
      <h3>Update</h3>
      <div id="upd">Wird geladen …</div>
    </div>

    <div class="card" style="margin-top:16px">
      <h3>Daten-Download zur Fehlersuche</h3>
      <div class="muted">Eine JSON-Datei mit allen Geräten, Messwerten, den letzten MQTT-Nachrichten, Regeln, Befehlen, dem Verlauf der letzten 24 Stunden und den letzten 500 Ereignissen. Secret Key und Passwörter sind nicht enthalten, der Access Key nur gekürzt.</div>
      <div style="margin-top:12px"><a class="btn" href="/api/export" download style="display:inline-block;text-decoration:none">Daten herunterladen</a></div>
    </div>`;
  renderUpdate(await api("/update"));

  document.getElementById("m-test").onclick = () => guarded(async () => {
    const out = document.getElementById("m-result");
    out.innerHTML = `<span class="muted">Verbinde …</span>`;
    try {
      const r = await api("/meter/test", { method: "POST", body: { host: document.getElementById("m-host").value, password: document.getElementById("m-pw").value } });
      out.innerHTML = `<span class="badge ok">Verbindung ok</span> ${esc(r.model || "")} · aktuell <b>${fmtNum(r.power_w, " W")}</b>` +
        (r.phases?.length ? ` <span class="muted">(${r.phases.map((v, i) => `L${i + 1}: ${fmtNum(v, " W")}`).join(", ")})</span>` : "");
    } catch (e) {
      out.innerHTML = `<span class="badge err">Fehler</span> ${esc(e.message)}`;
    }
  });
  document.getElementById("m-save").onclick = () => guarded(async () => {
    const del = document.getElementById("m-pw-del");
    await api("/settings", { method: "POST", body: {
      shelly_host: document.getElementById("m-host").value,
      shelly_password: del && del.checked ? "-" : document.getElementById("m-pw").value,
      shelly_interval: Number(document.getElementById("m-int").value) || 2,
    }});
    toast("Stromzähler gespeichert");
    route();
  });
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
