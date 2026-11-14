#!/bin/sh
# A PRIVATE minRender farm on this machine, for the parts of the conformance work that write (canon row 391): N nodes of
# a 0.5.x build, each with its own home, temporary folder, port and node id, one scratch share, loopback addresses, UDP
# off. Nothing here can reach a real farm: another share, other ports, 127.0.0.1 only. The real installation's data
# folder is never used (each node's HOME is a scratch folder).
#   sh tools/mr-conform/farm.sh start [N]     (default 2; needs MR_HEADLESS = the path of a `minrender-headless` build)
#   sh tools/mr-conform/farm.sh stop
#   sh tools/mr-conform/farm.sh config <n>    prints the path of node n's config.json (for `mr-conform record --config`)
# Everything lives under target/mr-farm (ignored).
set -e
cd "$(dirname "$0")/../.."
W="$PWD/target/mr-farm"
data() { echo "$W/n$1/Library/Application Support/MinRender"; }
case "$1" in
  start)
    N="${2:-2}"; [ -x "${MR_HEADLESS:-}" ] || { echo "set MR_HEADLESS to a minrender-headless build"; exit 1; }
    [ -f "$W/pids" ] && { echo "a farm is running: stop it first"; exit 1; }
    rm -rf "$W"; mkdir -p "$W/share"
    i=1; while [ "$i" -le "$N" ]; do
      d="$(data $i)"; mkdir -p "$d" "$W/n$i/t"
      printf '{"sync_root":"%s","http_port":%d,"ip_override":"127.0.0.1","udp_enabled":false,"udp_port":%d,"node_stopped":false,"tags":[],"staging_enabled":false,"show_notifications":false,"auto_start_agent":true,"path_mappings":[]}\n' "$W/share" $((8430 + i)) $((4330 + i)) > "$d/config.json"
      HOME="$W/n$i" TMPDIR="$W/n$i/t" "$MR_HEADLESS" > "$W/n$i/out.log" 2>&1 &
      echo $! >> "$W/pids"; i=$((i + 1)); sleep 1
    done
    echo "started $N nodes; ports 8431..$((8430 + N)); share $W/share" ;;
  stop)
    [ -f "$W/pids" ] || { echo "no farm is running"; exit 0; }
    while read -r p; do kill "$p" 2>/dev/null || true; done < "$W/pids"; sleep 2
    while read -r p; do kill -9 "$p" 2>/dev/null || true; done < "$W/pids"; rm -f "$W/pids"; echo stopped ;;
  config) echo "$(data "$2")/config.json" ;;
  *) echo "farm.sh start [N] | stop | config <n>"; exit 1 ;;
esac
