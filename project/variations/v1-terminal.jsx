/* =====================================================================
   V1 — TERMINAL / CRT
   ASCII chrome, monospace everywhere, command-line widget headers.
   Accent: green (status/ok), red for alerts.
   ===================================================================== */
const V1Terminal = () => {
  const C = {
    bg: 'var(--eink-bg)', fg: 'var(--eink-fg)',
    accent: 'var(--eink-green)', alert: 'var(--eink-red)',
    blue: 'var(--eink-blue)', yellow: 'var(--eink-yellow)',
  };

  const Box = ({ title, children, w, h, accent = C.fg, style = {} }) => (
    <div style={{
      border: `1px solid ${C.fg}`,
      width: w, height: h,
      position: 'relative',
      background: C.bg,
      ...style,
    }}>
      <div style={{
        position: 'absolute', top: -7, left: 8,
        background: C.bg, padding: '0 4px',
        fontSize: 10, letterSpacing: '0.05em',
        color: accent, fontWeight: 700,
      }}>
        {title}
      </div>
      <div style={{ padding: '12px 10px 8px', height: '100%' }}>{children}</div>
    </div>
  );

  const Bar = ({ pct, color = C.fg, w = 100 }) => {
    const filled = Math.round((pct / 100) * 20);
    return (
      <span style={{ color, fontFamily: 'monospace', letterSpacing: '-1px' }}>
        [{'█'.repeat(filled)}{'░'.repeat(20 - filled)}]
      </span>
    );
  };

  return (
    <div className="eink" style={{ padding: 10, fontSize: 11 }}>
      {/* Header strip */}
      <div style={{
        display: 'flex', justifyContent: 'space-between',
        borderBottom: `1px solid ${C.fg}`, paddingBottom: 4,
        marginBottom: 8, fontSize: 10,
      }}>
        <div>
          <span style={{ fontWeight: 700 }}>root@homelab</span>
          <span style={{ color: C.accent }}>:~$</span>
          <span style={{ marginLeft: 6 }}>./status --watch</span>
          <span style={{
            display: 'inline-block', width: 7, height: 10,
            background: C.fg, marginLeft: 2, verticalAlign: '-1px'
          }}></span>
        </div>
        <div style={{ display: 'flex', gap: 14 }}>
          <span>2026.05.06 / THU</span>
          <span style={{ color: C.accent }}>● UPLINK</span>
          <span>BAT 87%</span>
        </div>
      </div>

      {/* Motto line */}
      <div style={{
        fontSize: 9, marginBottom: 8, color: C.fg, opacity: 0.75,
        borderBottom: `1px dashed ${C.fg}`, paddingBottom: 4,
      }}>
        {'// '}"the future is already here — it's just not very evenly distributed"
      </div>

      {/* Grid */}
      <div style={{
        display: 'grid',
        gridTemplateColumns: '1.2fr 1fr 1fr',
        gridTemplateRows: '128px 138px',
        gap: 14,
      }}>
        {/* HOST STATUS */}
        <Box title="┤ HOST.SYS ├" accent={C.accent}>
          <table style={{ width: '100%', fontSize: 11, borderCollapse: 'collapse' }}>
            <tbody>
              {[
                ['CPU', 34, C.accent, '34%'],
                ['MEM', 67, C.yellow, '21.4 / 32G'],
                ['TMP', 52, C.accent, '52°C'],
                ['NET', 18, C.accent, '18 Mb/s'],
              ].map(([k, v, col, label]) => (
                <tr key={k}>
                  <td style={{ width: 32, fontWeight: 700 }}>{k}</td>
                  <td style={{ width: 130 }}><Bar pct={v} color={col} /></td>
                  <td style={{ textAlign: 'right', fontSize: 10 }}>{label}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div style={{ marginTop: 6, fontSize: 9, color: C.fg, opacity: 0.7 }}>
            UPTIME 47d 12h 03m · LOAD 0.42 0.51 0.48
          </div>
        </Box>

        {/* WEATHER */}
        <Box title="┤ ATMOS.LOG ├" accent={C.blue}>
          <div style={{ fontSize: 32, fontWeight: 700, lineHeight: 1, color: C.blue }}>
            14°<span style={{ fontSize: 14 }}>C</span>
          </div>
          <div style={{ fontSize: 10, marginTop: 2 }}>OVERCAST · 78% RH</div>
          <div style={{ marginTop: 8, fontSize: 9, display: 'flex', gap: 8 }}>
            {['FRI 16°', 'SAT 12°', 'SUN 9°', 'MON 11°'].map(d => (
              <div key={d} style={{ borderLeft: `1px solid ${C.fg}`, paddingLeft: 4 }}>
                {d}
              </div>
            ))}
          </div>
          <div style={{ marginTop: 6, fontSize: 9, opacity: 0.7 }}>
            WIND NNW 12km/h · 1014 hPa
          </div>
        </Box>

        {/* AGENDA */}
        <Box title="┤ AGENDA.TXT ├" accent={C.yellow}>
          {[
            ['09:30', 'standup / dailies', C.fg],
            ['11:00', 'arch review > vault', C.fg],
            ['14:15', 'dentist appt', C.alert],
            ['18:00', 'gym // chest+tri', C.fg],
          ].map(([t, l, c], i) => (
            <div key={i} style={{ display: 'flex', gap: 8, marginBottom: 3, color: c, fontSize: 10 }}>
              <span style={{ fontWeight: 700, width: 36 }}>{t}</span>
              <span>{l}</span>
            </div>
          ))}
        </Box>

        {/* ALERTS / LOG */}
        <Box title="┤ DMESG.OUT ├" accent={C.alert}>
          {[
            ['12:04', 'WARN', 'disk /dev/sdb 78% full', C.yellow],
            ['11:47', ' OK ', 'backup-nightly completed', C.accent],
            ['11:12', ' OK ', 'cert renewal: 4 domains', C.accent],
            ['09:33', 'WARN', 'unifi-controller: 2 retries', C.yellow],
            ['08:01', 'INFO', 'kernel: 6.8.4-200.fc40', C.fg],
          ].map(([t, lv, m, c], i) => (
            <div key={i} style={{ display: 'flex', gap: 6, fontSize: 9.5, lineHeight: 1.5 }}>
              <span style={{ opacity: 0.6 }}>{t}</span>
              <span style={{ color: c, fontWeight: 700 }}>[{lv}]</span>
              <span style={{ flex: 1, overflow: 'hidden', whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{m}</span>
            </div>
          ))}
        </Box>

        {/* BUDGET */}
        <Box title="┤ FINANCE.DB ├" accent={C.accent}>
          <div style={{ fontSize: 9, opacity: 0.7 }}>SPENT / BUDGET · MAY</div>
          <div style={{ fontSize: 22, fontWeight: 700, lineHeight: 1.1, marginTop: 2 }}>
            $1,847<span style={{ fontSize: 11, opacity: 0.6 }}> / $3,200</span>
          </div>
          <div style={{ marginTop: 4 }}><Bar pct={58} color={C.accent} /></div>
          <div style={{ marginTop: 6, fontSize: 9 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between' }}>
              <span>groceries</span><span>$412</span>
            </div>
            <div style={{ display: 'flex', justifyContent: 'space-between' }}>
              <span>transport</span><span>$188</span>
            </div>
            <div style={{ display: 'flex', justifyContent: 'space-between' }}>
              <span>subs</span><span style={{ color: C.alert }}>$94 ↑</span>
            </div>
          </div>
        </Box>

        {/* TASKS */}
        <Box title="┤ TODO.QUEUE ├" accent={C.fg}>
          {[
            [true,  'flash sd card for pi5'],
            [true,  'order ssd for nas'],
            [false, 'reroute vlan 30 to ap-3'],
            [false, 'submit Q2 expenses'],
            [false, 'patch grafana cve-2026-1141'],
          ].map(([done, txt], i) => (
            <div key={i} style={{
              fontSize: 10, marginBottom: 2,
              textDecoration: done ? 'line-through' : 'none',
              opacity: done ? 0.5 : 1,
            }}>
              [{done ? '×' : ' '}] {txt}
            </div>
          ))}
        </Box>
      </div>
    </div>
  );
};
