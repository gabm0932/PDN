#!/data/data/com.termux/files/usr/bin/bash
# SIGINT variant: forward to guest group, reap, clean session, exit 130.
cd /data/data/com.termux/files/home/workspace/PDN || exit 1
BIN="${BIN:-target/debug/podnext}"
SESS="$HOME/.podnext/sessions"

"$BIN" run alpine -- sleep 120 >/dev/null 2>&1 &
PN=$!
sleep 4
echo "== podnext pid $PN =="
echo "-- child (ppid=$PN) --"
ps -A -o pid,ppid,pgid,comm | awk -v p="$PN" 'NR==1 || $2==p'
echo "-- session file before --"
ls "$SESS/$PN.json" 2>&1

kill -INT "$PN"
sleep 3
echo "-- after: proot survivors --"
ps -A -o pid,comm | grep -i "[p]root" || echo "(none: clean)"
echo "-- after: session file --"
ls "$SESS/$PN.json" 2>&1
wait "$PN"
echo "exit=$?"
