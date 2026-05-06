// VARIATION 1 — TERMINAL / CRT
// All-monochrome with a single accent. ASCII frames, scanlines optional,
// monospace everything. Reads like an old VT100 system console.
//
// E-ink palette: BLACK on WHITE, plus one accent (RED by default).
// 800×480, render at 1:1.

const D = window.DASH_DATA;

// ── tiny helpers ────────────────────────────────────────────────
const ascii = {
  // box-drawing using ASCII so it stays crisp on e-ink
  hr: (w, ch = "─") => ch.repeat(w),
};

function Bar({ pct, width = 12, fill = "█", empty = "░" }) {
  const f = Math.round((pct / 100) * width);
  return <span>{fill.repeat(f)}{empty.repeat(width - f)}</span>;
}

function Cell({ children, style, label }) {
  return (
    <div style={{
      border: "1px solid #000",
      padding: "6px 8px",
      ...style,
    }}>
      {label && (
        <div style={{
          fontSize: 9, letterSpacing: 1.5, marginBottom: 4,
          display: "flex", justifyContent: "space-between",
          borderBottom: "1px dashed #000", paddingBottom: 2,
        }}>
          <span>┤ {label} ├</span>
          <span style={{ opacity: 0.6 }}>v1</span>
        </div>
      )}
      {children}
    </div>
  );
}

function TerminalDashboard({ accent = "#c1121f" }) {
  return (
    <div style={{
      width: 800, height: 480, background: "#fff", color: "#000",
      fontFamily: '"IBM Plex Mono", "JetBrains Mono", ui-monospace, monospace',
      fontSize: 12, lineHeight: 1.25,
      padding: 10, boxSizing: "border-box",
      display: "grid",
      gridTemplateRows: "26px 1fr 22px",
      rowGap: 6,
      position: "relative",
    }}>
      {/* faint scanline overlay — printed as alternating 1px rows */}
      <div style={{
        position: "absolute", inset: 0, pointerEvents: "none",
        backgroundImage: "repeating-linear-gradient(0deg,#000 0 1px,transparent 1px 3px)",
        opacity: 0.04, mixBlendMode: "multiply",
      }} />

      {/* ── HEADER ───────────────────────────────────────── */}
      <div style={{
        display: "grid",
        gridTemplateColumns: "auto 1fr auto",
        alignItems: "center", gap: 12,
        borderBottom: "2px solid #000", paddingBottom: 4,
      }}>
        <div style={{ fontWeight: 700, letterSpacing: 1 }}>
          <span style={{ background: "#000", color: "#fff", padding: "1px 6px" }}>{D.device}</span>
          <span style={{ marginLeft: 8 }}>{D.hostname}</span>
        </div>
        <div style={{
          textAlign: "center", fontSize: 11, letterSpacing: 2,
          color: accent, fontWeight: 700,
        }}>
          ▌ {D.motto} ▐
        </div>
        <div style={{ display: "flex", gap: 10, fontVariantNumeric: "tabular-nums" }}>
          <span>{D.date.dow} {D.date.day} {D.date.month}</span>
          <span>BAT <Bar pct={D.battery} width={6} /> {D.battery}%</span>
          <span>SIG {"▮".repeat(D.signal)}{"▯".repeat(4 - D.signal)}</span>
        </div>
      </div>

      {/* ── BODY GRID ────────────────────────────────────── */}
      <div style={{
        display: "grid",
        gridTemplateColumns: "1.1fr 1fr 1fr",
        gridTemplateRows: "1fr 1fr",
        gap: 6,
      }}>
        {/* HOST */}
        <Cell label="SYS / pve.lab" style={{ gridRow: "span 1" }}>
          <div style={{ display: "grid", gridTemplateColumns: "44px 1fr auto", rowGap: 2, columnGap: 6, fontSize: 11 }}>
            <span>CPU</span><Bar pct={D.host.cpu} width={22} /><span>{D.host.cpu}%</span>
            <span>TMP</span><Bar pct={D.host.cpuTemp} width={22} /><span>{D.host.cpuTemp}°C</span>
            <span>RAM</span><Bar pct={D.host.ram} width={22} /><span>{D.host.ramUsed}/{D.host.ramTotal}G</span>
            <span>DSK</span><Bar pct={D.host.disk} width={22} /><span>{D.host.disk}%</span>
          </div>
          <div style={{ marginTop: 6, fontSize: 10, opacity: 0.8, display: "flex", justifyContent: "space-between" }}>
            <span>UP {D.host.uptimeDays}d</span>
            <span>LOAD {D.host.load.join(" ")}</span>
          </div>
        </Cell>

        {/* WEATHER */}
        <Cell label="WX / local">
          <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
            <span style={{ fontSize: 32, fontWeight: 700, lineHeight: 1, color: accent }}>{D.weather.nowTempC}°</span>
            <span style={{ fontSize: 10, letterSpacing: 1 }}>{D.weather.nowCond}</span>
            <span style={{ marginLeft: "auto", fontSize: 10 }}>↑{D.weather.hi}° ↓{D.weather.lo}°</span>
          </div>
          <div style={{ marginTop: 4, fontSize: 10, display: "grid", gridTemplateColumns: "repeat(4,1fr)", gap: 4 }}>
            {D.weather.forecast.map((f) => (
              <div key={f.d} style={{ borderTop: "1px dashed #000", paddingTop: 2 }}>
                <div style={{ fontWeight: 700 }}>{f.d}</div>
                <div>{f.hi}/{f.lo}</div>
                <div style={{ opacity: 0.7 }}>{f.cond}</div>
              </div>
            ))}
          </div>
        </Cell>

        {/* BUDGET */}
        <Cell label={`$ / ${D.budget.monthLabel}`}>
          <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
            <span style={{ fontSize: 22, fontWeight: 700, color: accent }}>${D.budget.spent}</span>
            <span style={{ fontSize: 10 }}>of ${D.budget.cap}</span>
          </div>
          <div style={{ fontSize: 10, marginTop: 2 }}>
            <Bar pct={(D.budget.spent / D.budget.cap) * 100} width={36} />
          </div>
          <div style={{ marginTop: 4, fontSize: 10, display: "grid", gridTemplateColumns: "auto 1fr auto", columnGap: 6, rowGap: 1 }}>
            {D.budget.cats.map((c) => (
              <React.Fragment key={c.k}>
                <span>{c.k}</span>
                <Bar pct={(c.v / c.cap) * 100} width={14} />
                <span>${c.v}</span>
              </React.Fragment>
            ))}
          </div>
        </Cell>

        {/* AGENDA */}
        <Cell label="AGENDA / today">
          <div style={{ fontSize: 11 }}>
            {D.agenda.map((a, i) => (
              <div key={i} style={{ display: "grid", gridTemplateColumns: "42px 1fr 36px", columnGap: 6, padding: "1px 0" }}>
                <span style={{ color: accent, fontWeight: 700 }}>{a.time}</span>
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{a.title}</span>
                <span style={{ fontSize: 9, opacity: 0.7, textAlign: "right" }}>[{a.tag}]</span>
              </div>
            ))}
          </div>
        </Cell>

        {/* TASKS */}
        <Cell label="TASKS / open">
          <div style={{ fontSize: 11 }}>
            {D.tasks.map((t, i) => (
              <div key={i} style={{ display: "grid", gridTemplateColumns: "14px 28px 1fr", columnGap: 4 }}>
                <span>{t.done ? "[x]" : "[ ]"}</span>
                <span style={{ fontSize: 9, color: t.prio === "HI" ? accent : "#000", opacity: t.prio === "LOW" ? 0.5 : 1 }}>
                  {t.prio}
                </span>
                <span style={{ textDecoration: t.done ? "line-through" : "none", opacity: t.done ? 0.5 : 1 }}>
                  {t.t}
                </span>
              </div>
            ))}
          </div>
        </Cell>

        {/* ALERTS */}
        <Cell label="LOG / alerts">
          <div style={{ fontSize: 10, fontFamily: "inherit" }}>
            {D.alerts.map((a, i) => (
              <div key={i} style={{ display: "grid", gridTemplateColumns: "32px 36px 1fr", columnGap: 4, padding: "1px 0" }}>
                <span style={{
                  color: a.lvl === "ERR" ? accent : "#000",
                  fontWeight: a.lvl === "ERR" ? 700 : 400,
                }}>
                  {a.lvl === "ERR" ? "▶" : a.lvl === "WRN" ? "!" : "·"} {a.lvl}
                </span>
                <span>{a.t}</span>
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{a.msg}</span>
              </div>
            ))}
          </div>
        </Cell>
      </div>

      {/* ── FOOTER ──────────────────────────────────────── */}
      <div style={{
        display: "flex", justifyContent: "space-between",
        borderTop: "1px solid #000", paddingTop: 3,
        fontSize: 10, letterSpacing: 1,
      }}>
        <span>$ tail -f /var/log/dash &nbsp; <span style={{ background: "#000", color: "#fff", padding: "0 4px" }}>SYNC OK</span></span>
        <span>last {D.lastSync} · next {D.nextSync} · refresh 60m</span>
        <span>{D.date.iso}</span>
      </div>
    </div>
  );
}

window.TerminalDashboard = TerminalDashboard;
