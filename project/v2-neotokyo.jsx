// VARIATION 2 — NEO-TOKYO
// Brutalist grid, big numerals, katakana / kanji accents, neon stripes.
// Uses BLACK + RED + YELLOW from the e-ink palette as functional accents.
// Background is paper-white; bold "ink-stamp" panels.

const D2 = window.DASH_DATA;

function StampHeader({ jp, en, accent = "#c1121f" }) {
  return (
    <div style={{
      display: "flex", alignItems: "center", gap: 6,
      borderBottom: `2px solid #000`, padding: "0 0 2px",
      marginBottom: 4,
    }}>
      <span style={{
        background: "#000", color: "#fff",
        padding: "1px 5px", fontSize: 9, letterSpacing: 1, fontWeight: 700,
      }}>{en}</span>
      <span style={{ fontSize: 11, color: accent, fontWeight: 700, fontFamily: '"Noto Sans JP", system-ui' }}>{jp}</span>
      <span style={{ marginLeft: "auto", fontSize: 9, opacity: 0.6 }}>// 01</span>
    </div>
  );
}

function NeoTokyoDashboard({ accent = "#c1121f", warn = "#e5b800" }) {
  return (
    <div style={{
      width: 800, height: 480, background: "#fff", color: "#000",
      fontFamily: '"Space Grotesk", "Inter", system-ui, sans-serif',
      fontSize: 11, lineHeight: 1.2,
      padding: 0, boxSizing: "border-box",
      position: "relative", overflow: "hidden",
    }}>
      {/* corner registration marks */}
      {[
        { top: 4, left: 4 },
        { top: 4, right: 4 },
        { bottom: 4, left: 4 },
        { bottom: 4, right: 4 },
      ].map((p, i) => (
        <div key={i} style={{ position: "absolute", width: 8, height: 8, ...p, borderTop: "1px solid #000", borderLeft: "1px solid #000", transform: i === 1 ? "scaleX(-1)" : i === 2 ? "scaleY(-1)" : i === 3 ? "scale(-1)" : "" }} />
      ))}

      {/* ── HEADER ── */}
      <div style={{
        display: "grid",
        gridTemplateColumns: "auto 1fr auto",
        alignItems: "stretch",
        borderBottom: "3px solid #000",
        height: 44,
      }}>
        <div style={{
          background: "#000", color: "#fff",
          padding: "0 12px", display: "flex", alignItems: "center", gap: 8,
          clipPath: "polygon(0 0, 100% 0, calc(100% - 14px) 100%, 0 100%)",
          paddingRight: 22,
        }}>
          <div style={{ fontSize: 10, letterSpacing: 2, opacity: 0.7 }}>UNIT</div>
          <div style={{ fontSize: 22, fontWeight: 800, letterSpacing: -0.5 }}>{D2.device}</div>
          <div style={{ fontFamily: '"Noto Sans JP", system-ui', fontSize: 12, color: accent }}>端末</div>
        </div>
        <div style={{
          display: "flex", alignItems: "center", justifyContent: "center",
          fontSize: 13, fontWeight: 700, letterSpacing: 4,
          background: `repeating-linear-gradient(135deg,${accent} 0 6px,#000 6px 8px,${accent} 8px 14px,#fff 14px 20px)`,
          color: "#fff",
          textShadow: "0 1px 0 #000, 1px 0 0 #000, -1px 0 0 #000, 0 -1px 0 #000",
        }}>
          {D2.motto}
        </div>
        <div style={{ display: "flex", flexDirection: "column", justifyContent: "center", padding: "0 12px", textAlign: "right", fontFeatureSettings: '"tnum"' }}>
          <div style={{ fontSize: 10, letterSpacing: 1 }}>{D2.date.dow} · {D2.date.day}/{D2.date.month}/{D2.date.year}</div>
          <div style={{ fontSize: 10, display: "flex", gap: 8, justifyContent: "flex-end" }}>
            <span>BAT {D2.battery}%</span>
            <span>SIG {"●".repeat(D2.signal)}{"○".repeat(4 - D2.signal)}</span>
          </div>
        </div>
      </div>

      {/* ── BODY ── */}
      <div style={{
        display: "grid",
        gridTemplateColumns: "260px 1fr 220px",
        gridTemplateRows: "1fr 1fr",
        gap: 0,
        height: "calc(100% - 44px - 22px)",
      }}>
        {/* HERO: weather (spans 2 rows) */}
        <div style={{ gridRow: "span 2", borderRight: "2px solid #000", padding: "10px 12px", display: "flex", flexDirection: "column" }}>
          <StampHeader en="WX" jp="天気" accent={accent} />
          <div style={{ display: "flex", alignItems: "flex-start", gap: 6 }}>
            <div style={{ fontSize: 96, fontWeight: 800, lineHeight: 0.85, letterSpacing: -4 }}>
              {D2.weather.nowTempC}<span style={{ color: accent }}>°</span>
            </div>
            <div style={{ marginTop: 6 }}>
              <div style={{ fontSize: 11, fontWeight: 700, letterSpacing: 1 }}>{D2.weather.nowCond}</div>
              <div style={{ fontSize: 9 }}>↑{D2.weather.hi}° ↓{D2.weather.lo}°</div>
            </div>
          </div>
          <div style={{ marginTop: "auto", display: "grid", gridTemplateColumns: "repeat(4, 1fr)", borderTop: "1px solid #000" }}>
            {D2.weather.forecast.map((f, i) => (
              <div key={f.d} style={{
                padding: "4px 4px 6px",
                borderRight: i < 3 ? "1px dashed #000" : "none",
                textAlign: "center",
              }}>
                <div style={{ fontSize: 9, letterSpacing: 1, fontWeight: 700 }}>{f.d}</div>
                <div style={{ fontSize: 18, fontWeight: 800, lineHeight: 1 }}>{f.hi}°</div>
                <div style={{ fontSize: 8, opacity: 0.6 }}>{f.lo}° {f.cond}</div>
              </div>
            ))}
          </div>
        </div>

        {/* CENTER TOP: Agenda */}
        <div style={{ borderBottom: "2px solid #000", padding: "8px 12px", overflow: "hidden" }}>
          <StampHeader en="AGENDA" jp="予定" accent={accent} />
          {D2.agenda.map((a, i) => (
            <div key={i} style={{
              display: "grid", gridTemplateColumns: "56px 1fr 44px",
              alignItems: "center", padding: "2px 0",
              borderBottom: i < D2.agenda.length - 1 ? "1px dashed #000" : "none",
            }}>
              <div style={{
                fontSize: 14, fontWeight: 800, fontFeatureSettings: '"tnum"',
                color: i === 0 ? accent : "#000",
              }}>{a.time}</div>
              <div style={{ fontSize: 11, fontWeight: 600 }}>{a.title}</div>
              <div style={{
                fontSize: 8, fontWeight: 700, letterSpacing: 1, textAlign: "center",
                background: "#000", color: "#fff", padding: "1px 0",
              }}>{a.tag}</div>
            </div>
          ))}
        </div>

        {/* CENTER BOT: Host status */}
        <div style={{ padding: "8px 12px" }}>
          <StampHeader en="SYS" jp="状態" accent={accent} />
          <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 8 }}>
            {[
              { l: "CPU", v: D2.host.cpu, u: "%" },
              { l: "TMP", v: D2.host.cpuTemp, u: "°" },
              { l: "RAM", v: D2.host.ram, u: "%" },
              { l: "DSK", v: D2.host.disk, u: "%" },
            ].map((s) => (
              <div key={s.l} style={{ border: "1px solid #000", padding: "4px 6px" }}>
                <div style={{ fontSize: 8, letterSpacing: 1, fontWeight: 700 }}>{s.l}</div>
                <div style={{ fontSize: 22, fontWeight: 800, lineHeight: 1 }}>{s.v}<span style={{ fontSize: 11, color: accent }}>{s.u}</span></div>
                <div style={{ height: 4, background: "#eee", border: "1px solid #000", marginTop: 2 }}>
                  <div style={{ height: "100%", width: `${Math.min(s.v, 100)}%`, background: s.v > 70 ? accent : "#000" }} />
                </div>
              </div>
            ))}
          </div>
          <div style={{ marginTop: 6, fontSize: 9, display: "flex", justifyContent: "space-between" }}>
            <span>HOST {D2.host.name}</span>
            <span>UPTIME {D2.host.uptimeDays}d</span>
            <span>LOAD {D2.host.load.join(" / ")}</span>
          </div>
        </div>

        {/* RIGHT TOP: Budget */}
        <div style={{ borderLeft: "2px solid #000", borderBottom: "2px solid #000", padding: "8px 10px" }}>
          <StampHeader en="$" jp="予算" accent={accent} />
          <div style={{ fontSize: 26, fontWeight: 800, lineHeight: 1 }}>
            ${D2.budget.spent}
          </div>
          <div style={{ fontSize: 9, opacity: 0.7 }}>/ ${D2.budget.cap} · {D2.budget.monthLabel}</div>
          <div style={{ height: 8, border: "1px solid #000", marginTop: 4, position: "relative" }}>
            <div style={{ height: "100%", width: `${(D2.budget.spent / D2.budget.cap) * 100}%`, background: accent }} />
          </div>
          <div style={{ marginTop: 6, fontSize: 8, display: "grid", gridTemplateColumns: "auto 1fr auto", rowGap: 2, columnGap: 4 }}>
            {D2.budget.cats.map((c) => (
              <React.Fragment key={c.k}>
                <span style={{ fontWeight: 700 }}>{c.k}</span>
                <div style={{ height: 4, border: "1px solid #000", alignSelf: "center", margin: "0 4px" }}>
                  <div style={{ height: "100%", width: `${(c.v / c.cap) * 100}%`, background: c.v / c.cap > 0.85 ? accent : "#000" }} />
                </div>
                <span style={{ fontFeatureSettings: '"tnum"' }}>${c.v}</span>
              </React.Fragment>
            ))}
          </div>
        </div>

        {/* RIGHT BOT: Tasks + Alerts merged */}
        <div style={{ borderLeft: "2px solid #000", padding: "8px 10px", display: "flex", flexDirection: "column", gap: 4, overflow: "hidden" }}>
          <StampHeader en="OPS" jp="作業" accent={accent} />
          <div style={{ fontSize: 9, flex: 1, overflow: "hidden" }}>
            {D2.tasks.slice(0, 4).map((t, i) => (
              <div key={i} style={{ display: "grid", gridTemplateColumns: "12px 22px 1fr", columnGap: 3, padding: "1px 0" }}>
                <span>{t.done ? "■" : "□"}</span>
                <span style={{ fontWeight: 700, color: t.prio === "HI" ? accent : "#000" }}>{t.prio}</span>
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", textDecoration: t.done ? "line-through" : "none" }}>{t.t}</span>
              </div>
            ))}
          </div>
          <div style={{ borderTop: "1px dashed #000", paddingTop: 3, fontSize: 8 }}>
            {D2.alerts.slice(0, 2).map((a, i) => (
              <div key={i} style={{ display: "grid", gridTemplateColumns: "30px 1fr", columnGap: 4 }}>
                <span style={{ background: a.lvl === "ERR" ? accent : a.lvl === "WRN" ? warn : "#000", color: "#fff", textAlign: "center", fontWeight: 700, padding: "0 2px" }}>{a.lvl}</span>
                <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{a.msg}</span>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* ── FOOTER ── */}
      <div style={{
        position: "absolute", bottom: 0, left: 0, right: 0, height: 22,
        background: "#000", color: "#fff",
        display: "flex", alignItems: "center", gap: 12, padding: "0 10px",
        fontSize: 9, letterSpacing: 2, fontWeight: 700,
      }}>
        <span>SYNC {D2.lastSync}→{D2.nextSync}</span>
        <span style={{ color: accent }}>● ONLINE</span>
        <span style={{ marginLeft: "auto", fontFamily: '"Noto Sans JP", system-ui' }}>東京 ・ neo / TRMNL</span>
      </div>
    </div>
  );
}

window.NeoTokyoDashboard = NeoTokyoDashboard;
