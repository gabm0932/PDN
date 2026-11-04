#!/data/data/com.termux/files/usr/bin/bash
# Repro harness: SIGTERM to a foreground `podnext run` must kill the guest,
# remove the session file, and exit 143.
cd /data/data/com.termux/files/home/workspace/PDN || exit 1
BIN="${BIN:-target/debug/podnext}"
SESS="$HOME/.podnext/sessions"

"$BIN" run alpine -- sleep 120 >/dev/null 2>&1 &
PN=$!
sleep 4

echo "== podnext pid $PN =="
echo "-- podnext child (ppid=$PN) --"
ps -A -o pid,ppid,pgid,stat,comm | awk -v p="$PN" 'NR==1 || $2==p'
echo "-- session file before kill --"
ls -l "$SESS/$PN.json" 2>&1
echo "-- proot procs before kill --"
ps -A -o pid,ppid,pgid,comm | grep -i "[p]root" || echo "(none)"

kill -TERM "$PN"
sleep 3

echo "-- after kill: proot survivors --"
ps -A -o pid,ppid,pgid,comm | grep -i "[p]root" || echo "(none: clean)"
echo "-- after kill: session file --"
ls -l "$SESS/$PN.json" 2>&1
wait "$PN"
echo "exit=$?"

# cleanup any survivor so the next repro starts clean
pkill -TERM -f "proot.*sleep 120" 2>/dev/null
