#!/bin/sh
# vm-leak-probe.sh <toggles> [regime] — drive N show/hide cycles
# against the live dropterm service and print attributed memory
# samples for dropterm (plus cosmic-comp's total, the upstream creep
# watch). Runs as the desktop user inside the guest session (via
# `just vm-exec-user 'sh /tmp/vm-leak-probe.sh 150 spam100'`), so
# `dropterm toggle` rides the same session bus F12 would.
#
# Regimes (toggle-ram tr-1 — the spam pair drives toggles under
# TOGGLE_DEBOUNCE, 150 ms, core/surface_lifecycle.rs, and inside the
# ~130 ms slide, the overlap territory no probe had touched):
#   paced   — 1.0 s settle per toggle (74e6d85's regime; default)
#   spam100 — 0.1 s spacing
#   spam50  — 0.05 s burst
#
# One `#` header carries the regime and the build identity (the
# running exe's path, sha256, size, mtime — every run answers
# "which build" by construction; 74e6d85's mallopt pin rides the
# build it was measured on). One data line per sample (t0, every 10
# toggles, settled): total rss/hwm from Status; per-mapping RSS from
# smaps — [heap], anonymous, /dev/dri, memfd, and named files summed
# separately; fd count; thread count; the DROPTERM_TOGGLE_DEBUG
# trace's own footprint (/tmp/dropterm-toggle.log bytes — its growth
# class reads straight off the series: bounded per dump or runaway
# append); extra dropterm procs. The buckets sum to the kernel's own
# smaps_rollup Rss, which sits below status VmRSS by a stable gap on
# processes with shmem-class pages (verified against cosmic-comp:
# rollup = walk exactly) — attribution reads the bucket deltas, the
# status total is the cross-check. A mid-run pid change is announced
# loudly — that is the crash-and-respawn shape; the signature itself
# (journalctl -k oom/kill rows, coredumpctl, stderr) is the owner's
# documented pass alongside the run. The tail summary prints the
# settled-minus-halfway deltas the growth verdict reads from.
set -u

TOGGLES="${1:-0}"
REGIME="${2:-paced}"
case "$REGIME" in
    paced)   SETTLE=1.0 ;;
    spam100) SETTLE=0.1 ;;
    spam50)  SETTLE=0.05 ;;
    *) echo "unknown regime: $REGIME (paced|spam100|spam50)" >&2; exit 2 ;;
esac
# GNU sleep (coreutils) takes fractions; the guest is PopOs24.
# Samples land every 10 toggles, so the halfway anchor is the last
# multiple of 10 at or before TOGGLES/2.
HALF=$(( (TOGGLES / 20) * 10 ))

pid_of() { pidof "$1" 2>/dev/null | tr -s ' ' '\n' | sort -n | head -n1; }

# /proc/<pid>/smaps -> "heap anon dri memfd file" kB sums. Unreadable
# smaps (permissions changed mid-run, process gone) yields zeros; the
# total rss on the same line keeps that honest.
smaps_buckets() {
    awk '
        /^[0-9a-f]+-[0-9a-f]+ / {
            name = $6; cls = "anon"
            if (name == "[heap]") cls = "heap"
            else if (name ~ /^\/dev\/dri/) cls = "dri"
            else if (name ~ /^\/memfd:/) cls = "memfd"
            else if (name != "") cls = "file"
            next
        }
        /^Rss:/ { rss[cls] += $2 }
        END { printf "%d %d %d %d %d", rss["heap"], rss["anon"], rss["dri"], rss["memfd"], rss["file"] }
    ' /proc/$1/smaps 2>/dev/null || echo "0 0 0 0 0"
}

# The build identity block: the running exe, not a guessed path.
record_build() {
    exe="$(readlink /proc/$1/exe 2>/dev/null || command -v dropterm 2>/dev/null || echo '?')"
    sha="$(sha256sum "$exe" 2>/dev/null | awk '{print $1}')"
    sz="$(stat -c %s "$exe" 2>/dev/null || echo '?')"
    mt="$(stat -c %y "$exe" 2>/dev/null || echo '?')"
    echo "# build exe=$exe sha256=$sha size=$sz mtime=$mt"
}

LAST_PID=""
BUILT_FOR=""

# Anchor storage for the tail summary (values, kB/bytes/counts).
save_anchor() {
    # $1 = destination prefix
    eval "$1_rss=\$d_rss $1_heap=\$b_heap $1_anon=\$b_anon $1_dri=\$b_dri\
 $1_memfd=\$b_memfd $1_file=\$b_file $1_fds=\$d_fds $1_threads=\$d_thr\
 $1_dbg=\$d_dbg $1_comp=\$c_rss"
}
H_rss=0 H_heap=0 H_anon=0 H_dri=0 H_memfd=0 H_file=0 H_fds=0 H_threads=0 H_dbg=0 H_comp=0
S_rss=0 S_heap=0 S_anon=0 S_dri=0 S_memfd=0 S_file=0 S_fds=0 S_threads=0 S_dbg=0 S_comp=0

sample() {
    label="$1"
    dp="$(pid_of dropterm)"
    cp="$(pid_of cosmic-comp)"
    # Announce liveness changes once each: a vanishing pid and its
    # respawn are the crash shape the owner's signature pass pairs
    # with (journalctl -k oom/kill rows, coredumpctl list, stderr).
    if [ -n "$dp" ] && [ "$dp" != "$BUILT_FOR" ]; then
        record_build "$dp"
        if [ -n "$LAST_PID" ] && [ "$dp" != "$LAST_PID" ]; then
            echo "notice: dropterm pid changed $LAST_PID -> $dp at $label (respawn or restart)"
        fi
        LAST_PID="$dp"
        BUILT_FOR="$dp"
    elif [ -z "$dp" ] && [ -n "$LAST_PID" ]; then
        echo "notice: dropterm pid $LAST_PID gone at $label — capture journalctl -k oom/kill rows, coredumpctl list, stderr"
        LAST_PID=""
    fi
    d_rss=0; d_hwm=0; d_thr=0; c_rss=0; d_fds=0; d_dbg=0
    b_heap=0; b_anon=0; b_dri=0; b_memfd=0; b_file=0
    if [ -n "$dp" ]; then
        d_rss="$(awk '/^VmRSS/ {print $2}' /proc/$dp/status 2>/dev/null)"
        d_hwm="$(awk '/^VmHWM/ {print $2}' /proc/$dp/status 2>/dev/null)"
        d_thr="$(awk '/^Threads/ {print $2}' /proc/$dp/status 2>/dev/null)"
        d_fds="$(ls -1 /proc/$dp/fd 2>/dev/null | wc -l)"
        set -- $(smaps_buckets "$dp")
        b_heap=$1; b_anon=$2; b_dri=$3; b_memfd=$4; b_file=$5
    fi
    [ -n "$cp" ] && c_rss="$(awk '/^VmRSS/ {print $2}' /proc/$cp/status 2>/dev/null)"
    # Normalize empties (failed reads) so the tail summary's
    # arithmetic stays valid POSIX.
    d_rss=${d_rss:-0}; d_hwm=${d_hwm:-0}; d_thr=${d_thr:-0}
    c_rss=${c_rss:-0}; d_fds=${d_fds:-0}; c_rss=${c_rss:-0}
    b_heap=${b_heap:-0}; b_anon=${b_anon:-0}; b_dri=${b_dri:-0}
    b_memfd=${b_memfd:-0}; b_file=${b_file:-0}
    d_dbg="$(stat -c %s /tmp/dropterm-toggle.log 2>/dev/null || echo 0)"
    # pgrep -c prints the count (0 included) and exits 1 on no match —
    # only a missing pgrep leaves it empty.
    n_proc="$(pgrep -c -x dropterm 2>/dev/null)"
    n_proc="${n_proc:-?}"
    echo "$label rss=${d_rss:-0}kB hwm=${d_hwm:-0}kB heap=${b_heap}kB anon=${b_anon}kB dri=${b_dri}kB memfd=${b_memfd}kB file=${b_file}kB fds=${d_fds:-0} threads=${d_thr:-0} dbglog=${d_dbg}B comp_rss=${c_rss:-0}kB procs=$n_proc"
    save_anchor S
    [ "$HALF" -ge 10 ] && [ "$label" = "after-$HALF" ] && save_anchor H
}

echo "# vm-leak-probe regime=$REGIME settle=${SETTLE}s toggles=$TOGGLES"
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
if [ "$HALF" -ge 10 ]; then
    echo "# tail summary (settled minus after-$HALF — the sustained rate; positive heap/anon/dri/memfd names the growing mapping)"
    echo "# rss=+$((S_rss - H_rss))kB heap=+$((S_heap - H_heap))kB anon=+$((S_anon - H_anon))kB dri=+$((S_dri - H_dri))kB memfd=+$((S_memfd - H_memfd))kB file=+$((S_file - H_file))kB fds=+$((S_fds - H_fds)) threads=+$((S_threads - H_threads)) dbglog=+$((S_dbg - H_dbg))B comp_rss=+$((S_comp - H_comp))kB"
fi
