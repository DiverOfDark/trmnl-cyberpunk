/* =====================================================================
   V2 — NEO-TOKYO
   Heavy black bars, katakana accents, hard angles, neon-stripe accents.
   Accent: red as primary, yellow secondary.
   ===================================================================== */
const V2NeoTokyo = () => {
  const C = {
    bg: 'var(--eink-bg)', fg: 'var(--eink-fg)',
    red: 'var(--eink-red)', yellow: 'var(--eink-yellow)',
    blue: 'var(--eink-blue)', green: 'var(--eink-green)',
  };

  // Diagonal red/yellow stripe band
  const Stripes = ({ h = 10, color = 'var(--eink-red)' }) => (
    <div style={{
      height: h,
      backgroundImage: `repeating-linear-gradient(135deg, ${color} 0 8px, var(--eink-bg) 8px 14px)`,
    }} />
  );

  const Panel = ({ kanji, title, code, children, style }) => (
    <div style={{
      border: `2px solid ${C.fg}`,
      background: C.bg,
      position: 'relative',
      ...style,
    }}>
      <div style={{
        background: C.fg, color: C.bg,
        padding: '3px 8px',
        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
        fontSize: 10, letterSpacing: '0.1em', fontWeight: 700,
      }}>
        <span><span style={{ color: C.yellow, marginRight: 6 }}>{kanji}</span>{title}</span>
        <span style={{ opacity: 0.7 }}>{code}</span>
      </div>
      <div style={{ padding: '8px 10px' }}>{children}</div>
    </div>
  );

  return (
    <div className="eink" style={{ padding: 0, display: 'flex', flexDirection: 'column' }}>
      {/* Top stripe */}
      <Stripes h={8} color={C.red} />

      {/* Header */}
      <div style={{
        background: C.fg, color: C.bg,
        padding: '6px 14px',
        display: 'flex', justifyContent: 'space-between', alignItems: 'center',
      }}>
        <div style={{ display: 'flex', alignItems: 'baseline', gap: 10 }}>
          <span style={{ fontFamily: 'Space Grotesk', fontWeight: 800, fontSize: 22, letterSpacing: '-0.02em' }}>
            HOMELAB<span style={{ color: C.red }}>//</span>OS
          </span>
          <span style={{ fontSize: 10, color: C.yellow, letterSpacing: '0.2em' }}>ホームラボ</span>
        </div>
        <div style={{ display: 'flex', gap: 14, fontSize: 10, fontWeight: 700, letterSpacing: '0.1em' }}>
          <span>05.06.2026</span>
          <span>木曜日</span>
          <span style={{ color: C.yellow }}>BAT▮▮▮▮▯ 87</span>
        </div>
      </div>

      {/* Motto */}
      <div style={{
        background: C.bg, padding: '4px 14px',
        fontSize: 10, fontStyle: 'italic',
        borderBottom: `1px solid ${C.fg}`,
      }}>
        <span style={{ color: C.red, fontWeight: 700 }}>"</span>
        the sky above the port was the color of television, tuned to a dead channel
        <span style={{ color: C.red, fontWeight: 700 }}>"</span>
      </div>

      {/* Body grid */}
      <div style={{
        flex: 1, padding: 10,
        display: 'grid',
        gridTemplateColumns: '1.4fr 1fr 1fr',
        gridTemplateRows: '1fr 1fr',
        gap: 8,
      }}>
        {/* HOST -- big hero */}
        <Panel kanji="計算" title="HOST.METRICS" code="01">
          <div style={{ display: 'flex', gap: 14 }}>
            <div style={{ flex: 1 }}>
              {[
                ['CPU', '34', '%', C.green],
                ['MEM', '21.4', 'G', C.yellow],
                ['TMP', '52', '°C', C.green],
              ].map(([l, v, u, col]) => (
                <div key={l} style={{
                  display: 'flex', alignItems: 'baseline',
                  borderBottom: `1px solid ${C.fg}`, padding: '3px 0',
                }}>
                  <span style={{ fontSize: 10, fontWeight: 700, width: 38, letterSpacing: '0.1em' }}>{l}</span>
                  <span style={{ fontFamily: 'Space Grotesk', fontWeight: 800, fontSize: 22, color: col, letterSpacing: '-0.02em' }}>{v}</span>
                  <span style={{ fontSize: 10, marginLeft: 2, opacity: 0.7 }}>{u}</span>
                </div>
              ))}
            </div>
            <div style={{ width: 90, fontSize: 9 }}>
              <div style={{ background: C.fg, color: C.bg, padding: '2px 4px', fontWeight: 700, marginBottom: 4 }}>UPTIME</div>
              <div style={{ fontFamily: 'Space Grotesk', fontWeight: 800, fontSize: 18, lineHeight: 1 }}>47d</div>
              <div style={{ fontSize: 9, opacity: 0.7 }}>12:03:44</div>
              <div style={{ marginTop: 6, fontSize: 9 }}>
                LOAD <span style={{ color: C.green, fontWeight: 700 }}>0.42</span>
              </div>
              <div style={{ fontSize: 9 }}>
                NET <span style={{ color: C.blue, fontWeight: 700 }}>18Mb</span>
              </div>
            </div>
          </div>
        </Panel>

        {/* WEATHER */}
        <Panel kanji="気象" title="ATMOS" code="02">
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline' }}>
            <div style={{ fontFamily: 'Space Grotesk', fontWeight: 800, fontSize: 36, lineHeight: 1, letterSpacing: '-0.04em', color: C.blue }}>
              14°
            </div>
            <div style={{ fontSize: 9, textAlign: 'right' }}>
              <div style={{ fontWeight: 700 }}>OVERCAST</div>
              <div>78% RH</div>
              <div>1014 hPa</div>
            </div>
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 4, fontSize: 9 }}>
            {[['F', 16], ['S', 12], ['S', 9], ['M', 11]].map(([d, t], i) => (
              <div key={i} style={{ textAlign: 'center', borderTop: `2px solid ${C.fg}`, paddingTop: 2 }}>
                <div style={{ fontWeight: 700 }}>{d}</div>
                <div style={{ color: i === 2 ? C.blue : C.fg, fontWeight: 700 }}>{t}°</div>
              </div>
            ))}
          </div>
        </Panel>

        {/* BUDGET */}
        <Panel kanji="予算" title="FINANCE" code="03">
          <div style={{ fontSize: 9, opacity: 0.7 }}>MAY · SPENT</div>
          <div style={{ fontFamily: 'Space Grotesk', fontWeight: 800, fontSize: 24, lineHeight: 1, letterSpacing: '-0.03em' }}>
            $1,847
          </div>
          <div style={{ fontSize: 9 }}>of $3,200 · 58%</div>
          <div style={{
            height: 8, marginTop: 4, background: C.bg,
            border: `1px solid ${C.fg}`, position: 'relative',
          }}>
            <div style={{
              width: '58%', height: '100%', background: C.green,
              backgroundImage: `repeating-linear-gradient(135deg, ${C.green} 0 4px, ${C.fg} 4px 5px)`,
            }} />
          </div>
          <div style={{ fontSize: 9, marginTop: 4, color: C.red, fontWeight: 700 }}>
            ▲ subs +$12 vs apr
          </div>
        </Panel>

        {/* AGENDA */}
        <Panel kanji="予定" title="AGENDA.TODAY" code="04">
          {[
            ['09:30', 'STANDUP / DAILIES', C.fg],
            ['11:00', 'ARCH REVIEW · VAULT', C.fg],
            ['14:15', 'DENTIST', C.red],
            ['18:00', 'GYM CHEST+TRI', C.fg],
          ].map(([t, l, c], i) => (
            <div key={i} style={{
              display: 'flex', gap: 8, fontSize: 9.5,
              padding: '2px 0',
              borderBottom: i < 3 ? `1px dashed ${C.fg}` : 'none',
            }}>
              <span style={{ fontWeight: 800, color: c, width: 38, fontFamily: 'Space Grotesk' }}>{t}</span>
              <span style={{ fontWeight: 600, letterSpacing: '0.05em' }}>{l}</span>
            </div>
          ))}
        </Panel>

        {/* ALERTS */}
        <Panel kanji="警報" title="SYSLOG" code="05">
          {[
            ['WARN', 'disk sdb 78% full', C.yellow],
            [' OK ', 'backup complete', C.green],
            [' OK ', 'cert renewed (×4)', C.green],
            ['WARN', 'unifi 2 retries', C.yellow],
          ].map(([lv, m, c], i) => (
            <div key={i} style={{ display: 'flex', gap: 6, fontSize: 9, padding: '1px 0' }}>
              <span style={{ background: c, color: C.bg, padding: '0 3px', fontWeight: 800, letterSpacing: '0.05em' }}>{lv}</span>
              <span>{m}</span>
            </div>
          ))}
        </Panel>

        {/* TASKS */}
        <Panel kanji="任務" title="TASKQ" code="06">
          {[
            [true,  'flash sd / pi5'],
            [false, 'reroute vlan 30'],
            [false, 'submit Q2 expenses'],
            [false, 'patch grafana cve'],
          ].map(([done, txt], i) => (
            <div key={i} style={{
              display: 'flex', gap: 6, fontSize: 9.5, padding: '1px 0',
              opacity: done ? 0.5 : 1,
            }}>
              <span style={{
                width: 9, height: 9, border: `1px solid ${C.fg}`,
                background: done ? C.fg : C.bg, display: 'inline-block',
                marginTop: 2, position: 'relative',
              }}>
                {done && <span style={{ position: 'absolute', top: -3, left: 1, color: C.bg, fontSize: 11, fontWeight: 800 }}>×</span>}
              </span>
              <span style={{ textDecoration: done ? 'line-through' : 'none' }}>{txt}</span>
            </div>
          ))}
        </Panel>
      </div>

      {/* Bottom stripe */}
      <Stripes h={8} color={C.yellow} />
    </div>
  );
};
