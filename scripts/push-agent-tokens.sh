#!/usr/bin/env bash
# Push this machine's daily Claude Code and Codex token counts to the desk
# screen. Run it from cron or a systemd timer on every machine that runs the
# agents, e.g. every 15 minutes:
#
#   */15 * * * *  TRMNL_URL=http://trmnl.lan:8080 /path/to/push-agent-tokens.sh
#
# Needs node (npx) and curl. SOURCE names the machine; pushes from different
# machines are summed on the panel.
set -euo pipefail

: "${TRMNL_URL:?set TRMNL_URL to the server, e.g. http://trmnl.lan:8080}"
SOURCE="${SOURCE:-$(hostname -s)}"
SINCE="$(date -d '-8 days' +%Y%m%d 2>/dev/null || date -v-8d +%Y%m%d)"

for provider in claude codex; do
  if json="$(npx -y ccusage@latest "$provider" daily --json --since "$SINCE" 2>/dev/null)"; then
    curl -fsS -X PUT -H 'Content-Type: application/json' --data-binary "$json" \
      "$TRMNL_URL/api/agents/$provider/tokens?source=$SOURCE" >/dev/null \
      && echo "$provider: pushed" \
      || echo "$provider: push failed" >&2
  else
    echo "$provider: ccusage found no usage, skipped" >&2
  fi
done
