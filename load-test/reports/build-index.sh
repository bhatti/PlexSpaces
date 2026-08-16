#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build a compact run-index.json from all raw per-run data.
# Called automatically at the end of run.sh; also safe to run manually.
#
# USAGE:
#   bash load-test/reports/build-index.sh [<run-dir>]
#   bash load-test/reports/build-index.sh          # uses latest run

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DATA_DIR="$REPO_ROOT/load-test-data"

RUN_DIR="${1:-}"
if [ -z "$RUN_DIR" ]; then
  RUN_DIR=$(ls -1d "$DATA_DIR"/20* 2>/dev/null | sort | tail -1 || true)
  [ -z "$RUN_DIR" ] && { echo "No run dirs found."; exit 1; }
fi
[ -d "$RUN_DIR" ] || RUN_DIR="$DATA_DIR/$RUN_DIR"
[ -d "$RUN_DIR" ] || { echo "ERROR: not a directory: $RUN_DIR"; exit 1; }

INDEX="$RUN_DIR/run-index.json"

python3 - "$RUN_DIR" "$INDEX" <<'PYEOF'
import sys, os, json, glob, re
from datetime import datetime, timedelta

run_dir = sys.argv[1]
index_path = sys.argv[2]

LANG_ORDER = ["rust-embedded", "rust-wasm", "go", "typescript", "python"]

def test_type_from_name(name):
    if re.search(r'echo_http.*virtual', name): return "echo/virtual"
    if re.search(r'echo_http', name): return "echo/regular"
    if re.search(r'kv_http', name): return "kv/http"
    if re.search(r'lifecycle.*virtual', name): return "spawn/virtual"
    if re.search(r'lifecycle.*regular', name): return "spawn/regular"
    if re.search(r'lifecycle', name): return "spawn/lifecycle"
    if re.search(r'ws_connections|ws_capacity', name): return "ws/capacity"
    if re.search(r'actor_capacity.*virtual', name): return "actor/virtual"
    if re.search(r'actor_capacity', name): return "actor/capacity"
    return "unknown"

def get_json_times(json_file):
    """Read start/end times from k6 JSONL using only first/last 8KB."""
    t_start = t_end = None
    try:
        with open(json_file, 'rb') as f:
            head = f.read(8192).decode('utf-8', errors='replace')
            for line in head.splitlines():
                line = line.strip()
                if not line: continue
                try:
                    d = json.loads(line)
                    if d.get('type') == 'Point':
                        t_start = d['data']['time'][:19]
                        break
                except: pass
            f.seek(0, 2)
            size = f.tell()
            f.seek(max(0, size - 8192))
            tail = f.read().decode('utf-8', errors='replace')
            for line in reversed(tail.splitlines()):
                line = line.strip()
                if not line: continue
                try:
                    d = json.loads(line)
                    if d.get('type') == 'Point':
                        t_end = d['data']['time'][:19]
                        break
                except: pass
    except: pass
    return t_start, t_end

def parse_ts_to_utc(ts):
    """Convert timestamp string to naive UTC datetime."""
    if not ts: return None
    ts_clean = ts.replace('Z', '').split('+')[0]
    # Remove known offsets
    ts_clean = re.sub(r'-0[78]:00$', '', ts_clean)
    try:
        dt = datetime.fromisoformat(ts_clean)
        if dt.hour < 18:  # looks like PDT (UTC-7)
            dt = dt + timedelta(hours=7)
        return dt
    except: return None

def parse_log_box(log_file):
    """Parse handleSummary box format from echo_http logs."""
    result = {}
    try:
        with open(log_file) as f:
            content = f.read()
        # p50/p95/p99
        m = re.search(r'║\s+p50=([^\s]+)\s+p95=([^\s]+)\s+p99=([^\s]+)', content)
        if m:
            result['p50_raw'] = m.group(1)
            result['p95_raw'] = m.group(2)
            p99 = m.group(3)
            result['p99_raw'] = None if p99 == '–' else p99
        # RPS from VUs line
        m = re.search(r'║\s+VUs:\s+(\d+)\s+.*duration:\s+([^\s│║]+).*RPS:\s+([0-9.]+)', content)
        if m:
            result['max_vus'] = int(m.group(1))
            result['duration'] = m.group(2)
            result['rps'] = float(m.group(3))
        # error rate
        m = re.search(r'║\s+actor_error_rate:\s+([0-9.]+)', content)
        if m:
            result['errors'] = float(m.group(1))
        # total_requests
        m = re.search(r'║\s+total_requests:\s+(\d+)', content)
        if m:
            result['total_requests'] = int(m.group(1))
    except: pass
    return result

def parse_log_table(log_file):
    """Parse k6 native stats table (kv_http, lifecycle)."""
    result = {}
    try:
        with open(log_file) as f:
            content = f.read()
        # Find metric line (kv, echo, or spawn)
        for pattern in [r'perf_kv_latency_ms\.+', r'perf_echo_latency_ms\.+', r'perf_spawn_ms\.+']:
            m = re.search(pattern + r'.*?med=([^\s]+).*?p\(95\)=([^\s]+)(?:.*?p\(99\)=([^\s]+))?', content)
            if m:
                result['p50_raw'] = m.group(1)
                result['p95_raw'] = m.group(2)
                result['p99_raw'] = m.group(3) if m.group(3) else None
                break
        # http_reqs for RPS
        m = re.search(r'http_reqs\.*:\s+\d+\s+([0-9.]+)/s', content)
        if m: result['rps'] = float(m.group(1))
        m = re.search(r'http_reqs\.*:\s+(\d+)\s+', content)
        if m: result['total_requests'] = int(m.group(1))
        # error rate
        m = re.search(r'perf_error_rate\.*:\s+([0-9.]+)%', content)
        if m: result['errors'] = float(m.group(1))
        # VUs and duration from scenario line
        m = re.search(r'\[.*100%.*\]\s+(\d+)\s+VUs\s+([0-9.]+[smh](?:/[0-9.]+[smh])?)', content)
        if m:
            result['max_vus'] = int(m.group(1))
            # take the target duration (after /) if present, else the whole match
            d = m.group(2)
            result['duration'] = d.split('/')[-1] if '/' in d else d
    except: pass
    return result

def raw_to_ms(v):
    """Convert raw latency string to float ms."""
    if not v or v == '–': return None
    v = v.strip()
    if v.endswith('µs') or v.endswith('us'):
        n = re.sub(r'[µu]s$', '', v)
        try: return float(n) / 1000.0
        except: return None
    if v.endswith('ms'):
        try: return float(v[:-2])
        except: return None
    if v.endswith('s'):
        try: return float(v[:-1]) * 1000.0
        except: return None
    try: return float(v)
    except: return None

def get_prometheus_for_window(snap_dir, label=None, t_start_str=None, t_end_str=None):
    """Extract routing/actor_proc Prometheus metrics from the snapshot for this scenario.

    New runs: one snap per test named snap_<label>_<timestamp>.json — match by label.
    Old runs: fall back to UTC timestamp window correlation.
    """
    snaps = sorted(glob.glob(os.path.join(snap_dir, "snap_*.json")))

    # Primary: label-based match (new naming: snap_<label>_<timestamp>.json)
    window = []
    if label:
        label_snaps = [f for f in snaps if os.path.basename(f).startswith(f"snap_{label}_")]
        for f in label_snaps:
            try:
                with open(f) as fh: s = json.load(fh)
                window.append(s)
            except: pass

    # Fallback: timestamp window (old naming: snap_0001_<timestamp>.json)
    if not window:
        dt_start = parse_ts_to_utc(t_start_str)
        dt_end = parse_ts_to_utc(t_end_str)
        for f in snaps:
            try:
                with open(f) as fh: s = json.load(fh)
                ts = s.get('timestamp', '')
                try:
                    dt = datetime.fromisoformat(ts.replace('Z', '+00:00')).replace(tzinfo=None)
                except: continue
                if dt_start and dt_end:
                    if dt_start <= dt <= dt_end + timedelta(seconds=10):
                        window.append(s)
                else:
                    window.append(s)
            except: continue

    routing_p50 = routing_p95 = []
    actor_p50 = actor_p95 = []
    emb_actors = main_actors = 0

    for s in window:
        emb_actors = max(emb_actors, s.get('embedded_actors', 0) or 0)
        main_actors = max(main_actors, s.get('main_actors', 0) or 0)
        for key in ('main_metrics_table', 'embedded_metrics_table'):
            mt = s.get(key)
            if not mt: continue
            rows = mt if isinstance(mt, list) else mt.get('metrics', [])
            for row in (rows or []):
                n = row.get('name', '')
                labels = row.get('labels', {})
                try: v = float(row.get('value') or 0)
                except: continue
                q = labels.get('quantile', '')
                if 'message_routing_duration_seconds' in n:
                    if q == '0.5': routing_p50.append(v * 1000)
                    if q == '0.95': routing_p95.append(v * 1000)
                elif 'actor_message_processing_duration_seconds' in n:
                    if q == '0.5': actor_p50.append(v * 1000)
                    if q == '0.95': actor_p95.append(v * 1000)

    result = {'embedded_actors': emb_actors, 'main_actors': main_actors}
    if routing_p50:
        result['routing_p50_ms'] = sum(routing_p50) / len(routing_p50)
        result['routing_p95_ms'] = sum(routing_p95) / len(routing_p95) if routing_p95 else 0
    if actor_p50:
        result['actor_proc_p50_ms'] = sum(actor_p50) / len(actor_p50)
        result['actor_proc_p95_ms'] = sum(actor_p95) / len(actor_p95) if actor_p95 else 0

    # Collect spawn totals and WS thin-node counts
    spawn_total_vals = []
    ws_registered_vals = []
    ws_unregistered_vals = []
    for s in window:
        for key in ('main_metrics_table', 'embedded_metrics_table'):
            mt = s.get(key)
            if not mt: continue
            rows = mt if isinstance(mt, list) else mt.get('metrics', [])
            for row in (rows or []):
                n = row.get('name', '')
                try: v = float(row.get('value') or 0)
                except: continue
                if n == 'plexspaces_actor_spawn_total':
                    spawn_total_vals.append(v)
                elif n == 'plexspaces_ws_thin_node_registered_total':
                    ws_registered_vals.append(v)
                elif n == 'plexspaces_ws_thin_node_unregistered_total':
                    ws_unregistered_vals.append(v)
    # actor_spawn_total is cumulative since server start — store max across window snaps.
    # The label in analyze.sh makes clear this is a server-lifetime counter.
    if spawn_total_vals:
        result['actor_spawn_total'] = max(spawn_total_vals)
    # Derive peak concurrent thin nodes: max(registered - unregistered) is not directly
    # available since both are counters; store both so the display can show registered count.
    if ws_registered_vals:
        result['ws_thin_nodes_registered'] = max(ws_registered_vals)
        result['ws_thin_nodes_unregistered'] = max(ws_unregistered_vals) if ws_unregistered_vals else 0
    return result

# ── Build index ───────────────────────────────────────────────────────────────
index = {
    'run_id': os.path.basename(run_dir),
    'run_dir': run_dir,
    'scenarios': [],
    'languages': {},
}

for lang in LANG_ORDER:
    lang_dir = os.path.join(run_dir, f'lang-{lang}')
    if not os.path.isdir(lang_dir): continue

    snap_dir = os.path.join(lang_dir, 'snapshots')
    csv_file = os.path.join(lang_dir, 'metrics.csv')
    server_log = os.path.join(lang_dir, 'server.log')

    # Whole-run RSS/CPU from metrics.csv
    lang_stats = {'lang': lang}
    if os.path.isfile(csv_file):
        emb_rss, main_rss = [], []
        emb_cpu, main_cpu = 0.0, 0.0
        try:
            with open(csv_file) as f:
                next(f)  # header
                for line in f:
                    parts = line.strip().split(',')
                    if len(parts) < 7: continue
                    try:
                        e = float(parts[3]) if parts[3] else 0
                        m = float(parts[5]) if parts[5] else 0
                        if e > 0: emb_rss.append(e)
                        if m > 0: main_rss.append(m)
                        ec = float(parts[4]) if parts[4] else 0
                        mc = float(parts[6]) if parts[6] else 0
                        emb_cpu = max(emb_cpu, ec)
                        main_cpu = max(main_cpu, mc)
                    except: pass
        except: pass
        if emb_rss:
            lang_stats['emb_rss_min'] = min(emb_rss)
            lang_stats['emb_rss_max'] = max(emb_rss)
            lang_stats['emb_cpu_peak'] = emb_cpu
        if main_rss:
            lang_stats['main_rss_min'] = min(main_rss)
            lang_stats['main_rss_max'] = max(main_rss)
            lang_stats['main_cpu_peak'] = main_cpu

    # Server log errors
    if os.path.isfile(server_log):
        try:
            with open(server_log) as f:
                content = f.read()
            errs = len(re.findall(r'ERROR|error\[', content))
            warns = len(re.findall(r'WARN ', content))
            lang_stats['server_errors'] = errs
            lang_stats['server_warnings'] = warns
        except: pass

    # WASM reinstantiations (max across all snapshots)
    if os.path.isdir(snap_dir):
        snaps = sorted(glob.glob(os.path.join(snap_dir, "snap_*.json")))
        reins_vals, msgs_vals = [], []
        for f in snaps:
            try:
                with open(f) as fh: s = json.load(fh)
            except: continue
            for key in ("main_metrics_table", "embedded_metrics_table"):
                mt = s.get(key)
                if not mt: continue
                rows = mt if isinstance(mt, list) else mt.get("metrics", [])
                for row in (rows or []):
                    n = row.get("name", "")
                    try: v = float(row.get("value") or 0)
                    except: continue
                    if "wasm_reinstantiation_total" in n: reins_vals.append(v)
                    elif "wasm_component_message_success_total" in n: msgs_vals.append(v)
        if reins_vals:
            r = max(reins_vals)
            m_val = max(msgs_vals) if msgs_vals else 0
            lang_stats['wasm_reins'] = r
            lang_stats['wasm_msgs'] = m_val
            lang_stats['wasm_reins_ratio'] = r / m_val if m_val > 0 else None

    index['languages'][lang] = lang_stats

    # Per-scenario metrics
    for log_f in sorted(glob.glob(os.path.join(lang_dir, "*.log"))):
        if os.path.basename(log_f) == "server.log": continue
        scenario = os.path.basename(log_f)[:-4]
        test_type = test_type_from_name(scenario)
        json_f = log_f[:-4] + ".json"

        # Parse k6 metrics
        parsed = {}
        # ws/capacity: special box with peak connections
        if test_type == 'ws/capacity':
            try:
                with open(log_f) as lf: content = lf.read()
                m = re.search(r'max_concurrent_thin_nodes:\s+(\d+)', content)
                if m: parsed['peak_ws_conns'] = int(m.group(1))
                # upgrade p95 (first p95= occurrence)
                all_p95 = re.findall(r'p95=([^\s║]+)', content)
                if all_p95: parsed['upgrade_p95_raw'] = all_p95[0]
                if len(all_p95) > 1: parsed['reg_p95_raw'] = all_p95[1]
                m = re.search(r'upgrade_error_rate[^:]*:\s+([0-9.]+)', content)
                if m: parsed['errors'] = float(m.group(1))
                m = re.search(r'Total run:\s+([0-9]+s)', content)
                if not m: m = re.search(r'duration:\s+([0-9]+s)', content)
                if m: parsed['duration'] = m.group(1)
                parsed['rps'] = 0.0
                parsed['max_vus'] = parsed.get('peak_ws_conns', 0)
                # Store peak_conns in p50 slot for comparison table
                parsed['p50_raw'] = str(parsed.get('peak_ws_conns', 0))
                parsed['p95_raw'] = parsed.get('upgrade_p95_raw', '0ms')
                p99_ws = parsed.get('reg_p95_raw')
                parsed['p99_raw'] = p99_ws
            except: pass
        # actor/capacity and actor/virtual: parse peak actors, RSS, spawn_rps from box
        elif test_type in ('actor/capacity', 'actor/virtual'):
            try:
                with open(log_f) as lf: content = lf.read()
                m = re.search(r'max_live_actors[^:]*:\s+(\d+)', content)
                if not m: m = re.search(r'peak_live_actors[^:]*:\s+(\d+)', content)
                if not m: m = re.search(r'live_at_peak[^:]*:\s+(\d+)', content)
                if m: parsed['peak_actors'] = int(m.group(1))
                m = re.search(r'peak_rss[^:]*:\s+([0-9.]+)', content)
                if m: parsed['peak_rss_mb'] = float(m.group(1))
                m = re.search(r'spawn_rps[^:]*:\s+([0-9.]+)', content)
                if m: parsed['spawn_rps'] = float(m.group(1))
                m = re.search(r'overhead_per_actor[^:]*:\s+([0-9.]+)', content)
                if m: parsed['overhead_per_actor_kb'] = float(m.group(1))
                m = re.search(r'error_rate[^:]*:\s+([0-9.]+)', content)
                if m: parsed['errors'] = float(m.group(1))
                m = re.search(r'duration:\s+([0-9]+s)', content)
                if m: parsed['duration'] = m.group(1)
                parsed['rps'] = parsed.get('spawn_rps', 0.0)
                parsed['max_vus'] = 1
                # Store peak_actors in p50 slot, peak_rss_mb in p95 slot,
                # overhead_per_actor_kb in p99 slot for comparison table
                parsed['p50_raw'] = str(parsed.get('peak_actors', 0))
                parsed['p95_raw'] = str(parsed.get('peak_rss_mb', 0)) + 'ms'  # abusing ms slot for MB number
                oh = parsed.get('overhead_per_actor_kb')
                parsed['p99_raw'] = (str(oh) + 'ms') if oh is not None else None
            except: pass
        else:
            # Try native table format first
            table = parse_log_table(log_f)
            if table.get('p50_raw'):
                parsed = table
            else:
                # Fall back to box format
                box = parse_log_box(log_f)
                if box.get('p50_raw'):
                    parsed = box

        if not parsed:
            continue

        p50 = raw_to_ms(parsed.get('p50_raw'))
        p95 = raw_to_ms(parsed.get('p95_raw'))
        p99 = raw_to_ms(parsed.get('p99_raw'))
        # ws/capacity: p50 = peak connections (integer, not ms)
        if test_type == 'ws/capacity' and parsed.get('peak_ws_conns') is not None:
            p50 = float(parsed['peak_ws_conns'])
        # actor/capacity: p50 = peak actors (integer), p95 = peak_rss_mb (float, not ms)
        if test_type in ('actor/capacity', 'actor/virtual'):
            p50 = float(parsed.get('peak_actors', 0))
            p95 = float(parsed.get('peak_rss_mb', 0))

        # Extract start/end/elapsed from run.sh log lines
        log_start = log_end = log_elapsed = None
        try:
            with open(log_f) as lf:
                for line in lf:
                    if line.startswith('  Start: ') and log_start is None:
                        log_start = line.strip().split('Start: ', 1)[1]
                    elif line.startswith('  End: ') and log_end is None:
                        raw_end = line.strip().split('End: ', 1)[1]
                        # Format: "2026-01-01T00:00:00Z  (123s)"
                        parts = raw_end.split('  (')
                        log_end = parts[0].strip()
                        if len(parts) > 1:
                            log_elapsed = int(parts[1].rstrip('s)'))
        except: pass

        entry = {
            'lang': lang,
            'scenario': scenario,
            'test_type': test_type,
            'p50_ms': p50,
            'p95_ms': p95,
            'p99_ms': p99,
            'rps': parsed.get('rps', 0),
            'total_requests': parsed.get('total_requests', 0),
            'errors_pct': parsed.get('errors', 0),
            'max_vus': parsed.get('max_vus', 0),
            'duration': parsed.get('duration', '?'),
            'start_ts': log_start,
            'end_ts': log_end,
            'elapsed_s': log_elapsed,
        }

        # Prometheus metrics — label-based snap lookup first, timestamp fallback
        if os.path.isdir(snap_dir):
            t_start, t_end = (None, None)
            if os.path.isfile(json_f):
                t_start, t_end = get_json_times(json_f)
            prom = get_prometheus_for_window(snap_dir, label=scenario, t_start_str=t_start, t_end_str=t_end)
            entry['prometheus'] = prom

        index['scenarios'].append(entry)

with open(index_path, 'w') as f:
    json.dump(index, f, indent=2)
print(f"  Index written: {index_path}")
PYEOF
