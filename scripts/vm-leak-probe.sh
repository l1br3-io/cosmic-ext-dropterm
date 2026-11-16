#!/bin/sh
# vm-leak-probe.sh <toggles> — drive N show/hide cycles against the live
# dropterm service and print RSS samples for dropterm and cosmic-comp.
# Runs as the desktop user inside the guest session (via
# `just vm-exec-user 'sh /tmp/vm-leak-probe.sh N'`), so `dropterm toggle`
# rides the same session bus F12 would.
#
# One line per sample:  <label> <dropterm-rss-kB> <dropterm-hwm-kB>
# <comp-rss-kB> <extra-dropterm-procs>
set -u

TOGGLES="${1:-0}"
SETTLE=1.0          # seconds between toggles (slide is ~130 ms)

pid_of() { pidof "$1" 2>/dev/null | tr -s ' ' '\n' | sort -n | head -n1; }

sample() {
    label="$1"
    dp="$(pid_of dropterm)"
    cp="$(pid_of cosmic-comp)"
    d_rss=0; d_hwm=0; c_rss=0
    [ -n "$dp" ] && d_rss="$(awk '/^VmRSS/ {print $2}' /proc/$dp/status)"
    [ -n "$dp" ] && d_hwm="$(awk '/^VmHWM/ {print $2}' /proc/$dp/status)"
    [ -n "$cp" ] && c_rss="$(awk '/^VmRSS/ {print $2}' /proc/$cp/status)"
    n_proc="$(pgrep -c -x dropterm 2>/dev/null || echo '?')"
    echo "$label dropterm_rss=${d_rss:-0}kB hwm=${d_hwm:-0} comp_rss=${c_rss:-0}kB procs=$n_proc"
}

sample "t0-baseline"
i=1
while [ "$i" -le "$TOGGLES" ]; do
    dropterm toggle >/dev/null 2>&1 || echo "toggle $i failed"
    sleep "$SETTLE"
    if [ $((i % 10)) -eq 0 ]; then
        sample "after-$i"
    fi
    i=$((i + 1))
done
sleep 3
sample "after-$TOGGLES-settled"
