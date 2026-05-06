// VARIATION 3 — CORPORATE DYSTOPIA
// Severance / Alien-terminal feel. Clinical labels, ALL-CAPS, condensed
// industrial type, rule lines, file-clerk numbering. Mostly black on
// white with cool blue as the only accent. Feels like it was printed
// on a thermal printer in a corp basement.

const D3 = window.DASH_DATA;

function Pill({ children, dark, style }) {
  return (
    <span style={{
      display: "inline-block",
      padding: "0 5px",
      background: dark ? "#000" : "transparent",
      color: dark ? "#fff" : "#000",
      border: dark ? "none" : "1px solid #000",
      fontSize: 8, letterSpacing: 2, fontWeight: 700,
      lineHeight: "13px",
      ...style,
    }}>{children}</span>
  );
}

function Field({ k, v, hint, accent }) {
  return (
    <div style={{
      display: "grid",
      gridTemplateColumns: "84px 1fr",
      borderBottom: "1px solid #000",
      padding: "3px 0",
      alignItems: "baseline",
    }}>
      <span style={{ fontSize: 8, letterSpacing: 2, opacity: 0.7 }}>{k}</span>
      <span style={{ fontSize: 13, fontWeight: 700, fontFeatureSettings: '"tnum"', color: accent || "#000" }}>{v}</span>
      {hint && <span style={{ gridColumn: "2", fontSize: 8, opacity: 0.6, marginTop: -2 }}>{hint}</span>}
    </div>
  );
}

function Section({ no, title, sub, children, style }) {
  return (
    <div style={{
      border: "1px solid #000",
      padding: "0",
      display: "flex", flexDirection: "column",
      ...style,
    }}>
      <div style={{
        display: "flex", alignItems: "stretch",
        borderBottom: "1px solid #000",
      }}>
        <div style={{
          width: 24, background: "#000", color: "#fff",
          display: "flex", alignItems: "center", justifyContent: "center",
          fontSize: 11, fontWeight: 700, fontFeatureSettings: '"tnum"',
          letterSpacing: 1,
        }}>{no}</div>
        <div style={{ padding: "3px 8px", flex: 1 }}>
          <div style={{ fontSize: 11, fontWeight: 700, letterSpacing: 3 }}>{title}</div>
          {sub && <div style={{ fontSize: 7, letterSpacing: 1.5, opacity: 0.7, marginTop: -1 }}>{sub}</div>}
        </div>
      </div>
      <div style={{ padding: "6px 8px", flex: 1, minHeight: 0 }}>{children}</div>
    </div>
  );
}

function CorpDashboard({ accent = "#1e3a8a" /* e-ink blue */ }) {
  return (
    <div style={{
      width: 800, height: 480, background: "#fff", color: "#000",
      fontFamily: '"Barlow Condensed", "Helvetica Neue", system-ui, sans-serif',
      fontSize: 11, lineHeight: 1.15,
      padding: 8, boxSizing: "border-box",
      display: "grid",
      gridTemplateRows: "auto 1fr 16px",
      rowGap: 6,
      position: "relative",
    }}>
      {/* ── Masthead: form-like ── */}
      <div style={{
        border: "2px solid #000",
        display: "grid",
        gridTemplateColumns: "1fr auto 1fr",
        alignItems: "stretch",
      }}>
        <div style={{ padding: "4px 8px", borderRight: "1px solid #000" }}>
          <div style={{ fontSize: 7, letterSpacing: 2, opacity: 0.7 }}>FORM TRMNL-1002 · INTERNAL USE</div>
          <div style={{ fontSize: 17, fontWeight: 700, letterSpacing: 4, lineHeight: 1 }}>DAILY · STATUS · BRIEF</div>
          <div style={{ fontSize: 8, letterSpacing: 1, marginTop: 2, color: accent, fontWeight: 700 }}>
            "{D3.motto}"
          </div>
        </div>
        <div style={{
          padding: "4px 14px", textAlign: "center",
          display: "flex", flexDirection: "column", justifyContent: "center",
          borderRight: "1px solid #000",
        }}>
          <div style={{ fontSize: 7, letterSpacing: 2, opacity: 0.7 }}>DATE OF RECORD</div>
          <div style={{ fontSize: 22, fontWeight: 700, letterSpacing: 2, lineHeight: 1, fontFeatureSettings: '"tnum"' }}>
            {D3.date.day}.{D3.date.month}.{D3.date.year}
          </div>
          <div style={{ fontSize: 8, letterSpacing: 4, marginTop: 2 }}>{D3.date.dow}</div>
        </div>
        <div style={{ padding: "4px 8px", textAlign: "right" }}>
          <div style={{ fontSize: 7, letterSpacing: 2, opacity: 0.7 }}>TERMINAL // OPERATOR</div>
          <div style={{ fontSize: 12, fontWeight: 700, letterSpacing: 2 }}>{D3.device}</div>
          <div style={{ display: "flex", gap: 4, justifyContent: "flex-end", marginTop: 2 }}>
            <Pill>BAT {D3.battery}%</Pill>
            <Pill>SIG {D3.signal}/4</Pill>
            <Pill dark>OK</Pill>
          </div>
        </div>
      </div>

      {/* ── Body ── */}
      <div style={{
        display: "grid",
        gridTemplateColumns: "1fr 1fr 1fr 1fr",
        gridTemplateRows: "1fr 1fr",
        gap: 6,
      }}>
        <Section no="01" title="WX REPORT" sub="LOCAL // 24H">
          <div style={{ display: "flex", alignItems: "baseline", gap: 4 }}>
            <span style={{ fontSize: 36, fontWeight: 700, letterSpacing: -2, color: accent, lineHeight: 0.9 }}>{D3.weather.nowTempC}°</span>
            <span style={{ fontSize: 9, letterSpacing: 2 }}>{D3.weather.nowCond}</span>
          </div>
          <div style={{ fontSize: 8, marginBottom: 4 }}>HI {D3.weather.hi}° / LO {D3.weather.lo}°</div>
          {D3.weather.forecast.slice(0, 3).map((f) => (
            <div key={f.d} style={{ display: "grid", gridTemplateColumns: "30px 1fr 50px", borderTop: "1px dotted #000", padding: "1px 0", fontSize: 9 }}>
              <span style={{ fontWeight: 700, letterSpacing: 1 }}>{f.d}</span>
              <span style={{ opacity: 0.7 }}>{f.cond}</span>
              <span style={{ textAlign: "right", fontFeatureSettings: '"tnum"' }}>{f.hi}/{f.lo}</span>
            </div>
          ))}
        </Section>

        <Section no="02" title="HOST METRICS" sub={D3.host.name.toUpperCase()} style={{ gridColumn: "span 2" }}>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", columnGap: 12 }}>
            <Field k="CPU LOAD" v={`${D3.host.cpu}%`} hint={`LOAD AVG ${D3.host.load.join(" · ")}`} accent={D3.host.cpu > 70 ? accent : null} />
            <Field k="THERMAL" v={`${D3.host.cpuTemp}°C`} hint="WITHIN NOM. RANGE" />
            <Field k="MEMORY" v={`${D3.host.ramUsed} / ${D3.host.ramTotal} GB`} hint={`${D3.host.ram}% UTILIZED`} />
            <Field k="STORAGE" v={`${D3.host.disk}%`} hint="POOL: TANK/MAIN" />
          </div>
          <div style={{ marginTop: 4, display: "flex", justifyContent: "space-between", fontSize: 8, opacity: 0.7, letterSpacing: 1 }}>
            <span>UPTIME · {D3.host.uptimeDays} DAYS</span>
            <span>NEXT MAINT · {String(7 - (D3.host.uptimeDays % 7)).padStart(2, "0")}D</span>
          </div>
        </Section>

        <Section no="03" title="BUDGET" sub={D3.budget.monthLabel}>
          <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
            <span style={{ fontSize: 22, fontWeight: 700, lineHeight: 1, color: accent }}>${D3.budget.spent}</span>
            <span style={{ fontSize: 8, letterSpacing: 1 }}>OF ${D3.budget.cap}</span>
          </div>
          <div style={{ fontSize: 8, marginBottom: 3 }}>
            REMAINING ${D3.budget.cap - D3.budget.spent} · {D3.budget.runway}D RUNWAY
          </div>
          {D3.budget.cats.slice(0, 4).map((c) => (
            <div key={c.k} style={{ display: "grid", gridTemplateColumns: "44px 1fr 36px", alignItems: "center", padding: "1px 0", fontSize: 9, borderTop: "1px dotted #000" }}>
              <span style={{ letterSpacing: 1, fontWeight: 700 }}>{c.k}</span>
              <div style={{ height: 4, border: "1px solid #000", margin: "0 6px" }}>
                <div style={{ height: "100%", width: `${(c.v / c.cap) * 100}%`, background: c.v / c.cap > 0.85 ? accent : "#000" }} />
              </div>
              <span style={{ textAlign: "right", fontFeatureSettings: '"tnum"' }}>${c.v}</span>
            </div>
          ))}
        </Section>

        <Section no="04" title="AGENDA" sub="SCHED · LOCAL TZ">
          {D3.agenda.map((a, i) => (
            <div key={i} style={{
              display: "grid", gridTemplateColumns: "44px 1fr",
              borderBottom: i < D3.agenda.length - 1 ? "1px dotted #000" : "none",
              padding: "2px 0", alignItems: "baseline",
            }}>
              <span style={{ fontSize: 13, fontWeight: 700, fontFeatureSettings: '"tnum"', color: i === 0 ? accent : "#000" }}>{a.time}</span>
              <div>
                <div style={{ fontSize: 10, fontWeight: 600, letterSpacing: 0.5 }}>{a.title}</div>
                <div style={{ fontSize: 7, letterSpacing: 2, opacity: 0.7 }}>CAT · {a.tag}</div>
              </div>
            </div>
          ))}
        </Section>

        <Section no="05" title="TASK QUEUE" sub="PRIORITIZED">
          {D3.tasks.map((t, i) => (
            <div key={i} style={{
              display: "grid", gridTemplateColumns: "16px 30px 1fr",
              padding: "2px 0", fontSize: 10,
              borderBottom: i < D3.tasks.length - 1 ? "1px dotted #000" : "none",
              alignItems: "center",
            }}>
              <span style={{ width: 10, height: 10, border: "1.5px solid #000", display: "inline-block", background: t.done ? "#000" : "transparent" }} />
              <Pill dark={t.prio === "HI"} style={{ background: t.prio === "HI" ? accent : t.prio === "MED" ? "#000" : "transparent", color: t.prio === "LOW" ? "#000" : "#fff", border: t.prio === "LOW" ? "1px solid #000" : "none", fontSize: 7, padding: "0 3px" }}>{t.prio}</Pill>
              <span style={{ marginLeft: 4, textDecoration: t.done ? "line-through" : "none", opacity: t.done ? 0.5 : 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{t.t}</span>
            </div>
          ))}
        </Section>

        <Section no="06" title="INCIDENT LOG" sub="LAST 24H" style={{ gridColumn: "span 2" }}>
          <div style={{ display: "grid", gridTemplateColumns: "44px 36px 1fr 60px", fontSize: 7, letterSpacing: 2, borderBottom: "1px solid #000", paddingBottom: 1 }}>
            <span>LEVEL</span><span>TIME</span><span>MESSAGE</span><span style={{ textAlign: "right" }}>HOST</span>
          </div>
          {D3.alerts.map((a, i) => (
            <div key={i} style={{ display: "grid", gridTemplateColumns: "44px 36px 1fr 60px", padding: "2px 0", fontSize: 9, borderBottom: i < D3.alerts.length - 1 ? "1px dotted #000" : "none", alignItems: "center" }}>
              <Pill dark={a.lvl !== "INF"} style={{ background: a.lvl === "ERR" ? accent : a.lvl === "WRN" ? "#000" : "transparent", color: a.lvl === "INF" ? "#000" : "#fff", border: a.lvl === "INF" ? "1px solid #000" : "none" }}>{a.lvl}</Pill>
              <span style={{ fontFeatureSettings: '"tnum"' }}>{a.t}</span>
              <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{a.msg}</span>
              <span style={{ textAlign: "right", fontSize: 8, opacity: 0.7 }}>node-0{(i % 2) + 1}</span>
            </div>
          ))}
        </Section>
      </div>

      {/* ── Footer ── */}
      <div style={{
        borderTop: "2px solid #000", paddingTop: 2,
        display: "flex", justifyContent: "space-between",
        fontSize: 7, letterSpacing: 2, fontWeight: 700,
      }}>
        <span>DOC ID · TRMNL-1002-{D3.date.iso.replace(/-/g, "")}</span>
        <span>SYNC {D3.lastSync} → NEXT {D3.nextSync}</span>
        <span>PAGE 1 / 1 · CLASSIFIED // INTERNAL</span>
      </div>
    </div>
  );
}

window.CorpDashboard = CorpDashboard;
