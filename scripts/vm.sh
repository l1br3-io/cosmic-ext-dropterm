#!/usr/bin/env bash
# Host-side plumbing for the COSMIC validation VM (docs/development.md
# has the walkthrough this serves; the just vm-* recipes delegate here).
# Ported from cosmic-ext-airlock's CP4 harness (scripts/vm.sh there,
# same structure and guards) — that repo's research record
# (docs/research/cosmic-vm-environment.md) is why the VM is what it is:
# virt-manager + virtio-gpu/virgl, because cosmic-comp renders GLES and
# needs no guest Vulkan.
#
# Every setting is an environment variable; guest-side commands run
# through the QEMU guest agent (so the guest needs `qemu-guest-agent`
# installed once — no SSH, no keys); file transfer rides the VM's NAT
# bridge over a short-lived HTTP server that stops when the transfer
# ends, sha256-verified on both sides; evidence is captured with
# `virsh screenshot`. Variables, commands, and the walk order:
# `scripts/vm.sh help`. `scripts/vm.sh status` checks every
# precondition and reports ok/missing instead of failing.
set -euo pipefail

VM_NAME="${VM_NAME:-PopOs24}"
VM_USER="${VM_USER:-jpt}"
BRIDGE_IP="${BRIDGE_IP:-192.168.122.1}"
PORT="${PORT:-8000}"
# Resolved per-run by resolve_guest() unless explicitly set above.
VM_HOME="${VM_HOME:-}"
SERVER_PID=""

die() { echo "vm: error: $*" >&2; exit 1; }
note() { echo "vm: $*"; }

# --- host-side guards -------------------------------------------------------

require() {  # require <command> <install hint>
    command -v "$1" >/dev/null 2>&1 || die "'$1' not found on this host — $2"
}

check_libvirt() {
    require virsh "sudo apt install libvirt-clients (or the full virt-manager stack)"
    virsh -c qemu:///system version >/dev/null 2>&1 ||
        die $'cannot reach qemu:///system\n  → permission denied usually means: sudo usermod -aG libvirt "$USER", then log out and back in'
}

check_host() {
    check_libvirt
    require python3 "sudo apt install python3"
    require curl "sudo apt install curl"
    require ip "sudo apt install iproute2"
    require ss "sudo apt install iproute2"
}

domain_state() { virsh -q domstate "$VM_NAME" 2>/dev/null; }

domain_must_exist() {
    [[ -n "$(domain_state)" ]] ||
        die "no libvirt domain '$VM_NAME' — check 'virsh list --all', or set the name: VM_NAME=<name> just vm-status"
}

domain_must_run() {
    domain_must_exist
    local state; state="$(domain_state || true)"
    [[ "$state" == "running" ]] ||
        die "VM '$VM_NAME' is '$state', not running — start it in virt-manager first (the agent and the transfer both need a live VM)"
}

check_bridge() {
    ip -4 addr show 2>/dev/null | grep -q "inet $BRIDGE_IP/" ||
        die "this host has no $BRIDGE_IP address — is the VM NAT network up? (sudo virsh net-start default; or override BRIDGE_IP=)"
}

# --- guest-agent plumbing ---------------------------------------------------

agent_call() { virsh -q qemu-agent-command "$VM_NAME" "$1"; }

# Runs a shell command in the guest as root, forwarding stdout, stderr,
# and the exit code.
guest_exec() {
    local cmd="$1" req reply pid exited
    req="$(CMD="$cmd" python3 -c '
import json, os
print(json.dumps({"execute": "guest-exec",
                  "arguments": {"path": "/bin/sh", "arg": ["-c", os.environ["CMD"]],
                                "capture-output": True}}))')"
    reply="$(agent_call "$req" 2>/dev/null)" ||
        die "guest agent unreachable in '$VM_NAME' — is the VM running, and is qemu-guest-agent installed in it? (sudo apt install qemu-guest-agent, inside the VM)"
    pid="$(printf '%s' "$reply" | python3 -c 'import json,sys; print(json.load(sys.stdin)["return"]["pid"])')"
    while :; do
        reply="$(agent_call "{\"execute\":\"guest-exec-status\",\"arguments\":{\"pid\":$pid}}")" \
            || die "lost the guest agent mid-command (is the VM still running?)"
        exited="$(printf '%s' "$reply" | python3 -c 'import json,sys; print(json.load(sys.stdin)["return"]["exited"])')"
        [[ "$exited" == "True" ]] && break
        sleep 0.2
    done
    printf '%s' "$reply" | python3 -c '
import base64, json, sys
r = json.load(sys.stdin)["return"]
sys.stdout.write(base64.b64decode(r.get("out-data") or "").decode("utf-8", "replace"))
sys.stderr.write(base64.b64decode(r.get("err-data") or "").decode("utf-8", "replace"))
sys.exit(r.get("exitcode") or 0)'
}

agent_alive() { agent_call '{"execute":"guest-ping"}' >/dev/null 2>&1; }

# Validates VM_USER against the guest's own account database and
# resolves VM_HOME from it (instead of assuming /home/<user>).
resolve_guest() {
    domain_must_run
    agent_alive ||
        die "guest agent not responding in '$VM_NAME' — install it inside the VM once: sudo apt install qemu-guest-agent"
    local passwd home
    passwd="$(guest_exec "getent passwd '$VM_USER'")" ||
        die "guest user '$VM_USER' does not exist in the VM — set the right one: VM_USER=<name> just vm-exec-user true"
    home="$(printf '%s' "$passwd" | cut -d: -f6)"
    [[ -n "$home" ]] || die "could not read the home directory of guest user '$VM_USER'"
    VM_HOME="${VM_HOME:-$home}"
}

# Runs a command in the guest as the desktop user, inside that user's
# session environment (runtime dir + session bus + Wayland socket) so
# session-aware CLIs — `dropterm toggle` — behave exactly
# as they would from a guest terminal. The Wayland
# socket name is resolved in the guest at run time (wayland-0 unless
# the session picked another).
guest_exec_as_user() {
    resolve_guest
    guest_exec "uid=\$(id -u '$VM_USER'); wd=\$(ls /run/user/\$uid 2>/dev/null | grep -m1 '^wayland-[0-9]*\$' || true); sudo -u '$VM_USER' env XDG_RUNTIME_DIR=/run/user/\$uid DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$uid/bus \${wd:+WAYLAND_DISPLAY=\$wd} sh -c $(printf '%q' "$1")"
}

# --- transfer ---------------------------------------------------------------

vm_ip() {
    virsh -q domifaddr "$VM_NAME" 2>/dev/null | awk '$3 == "ipv4" {print $4}' | cut -d/ -f1 | head -n1
}

serve_up() {  # serve_up <directory> <url-path>  — the URL to poll is $2
    local dir="$1" url="$2"
    curl -sf -o /dev/null --max-time 2 "$url" && return 0   # reuse existing
    ss -ltn 2>/dev/null | grep -q ":$PORT " &&
        die "port $PORT is busy with something that is not this transfer — set PORT=<other>"
    [[ -d "$dir" ]] || die "no directory '$dir' to serve — nothing to transfer"
    python3 -m http.server "$PORT" --bind "$BRIDGE_IP" --directory "$dir" \
        >/dev/null 2>&1 &
    SERVER_PID=$!
    local _
    for _ in $(seq 1 25); do
        curl -sf -o /dev/null --max-time 1 "$url" && return 0
        sleep 0.2
    done
    die "transfer server failed to come up on $BRIDGE_IP:$PORT"
}

serve_down() {
    [[ -n "$SERVER_PID" ]] && kill "$SERVER_PID" 2>/dev/null || true
    SERVER_PID=""
}

# --- commands ---------------------------------------------------------------

cmd_ip() {
    check_host
    domain_must_run
    local ip; ip="$(vm_ip || true)"
    [[ -n "$ip" ]] || die "no guest IP found on the NAT network — is the VM's NIC attached to the 'default' network?"
    echo "$ip"
}

# put <local-file> <guest-path> — bridge transfer with sha256 verified
# on both sides; files landing under the guest home arrive owned by
# VM_USER, everything else by root, mode preserved from the source.
cmd_put() {
    local file="${1:-}" dest="${2:-}"
    [[ -n "$file" && -n "$dest" ]] || die "usage: vm.sh put <local-file> <guest-path>"
    [[ -f "$file" ]] || die "no local file '$file'"
    file="$(readlink -f "$file")"
    local base mode host_sha guest_sha url downloader owner_user owner_group
    base="$(basename "$file")"
    mode="$(stat -c%a "$file")"
    host_sha="$(sha256sum "$file" | cut -d' ' -f1)"
    url="http://$BRIDGE_IP:$PORT/$base"
    check_host
    check_bridge
    resolve_guest
    domain_must_run
    serve_up "$(dirname "$file")" "$url"
    trap serve_down EXIT
    local gip; gip="$(vm_ip || true)"
    note "transferring $base to $VM_NAME at ${gip:-unknown IP}"
    downloader="$(guest_exec "command -v curl >/dev/null && echo curl || (command -v wget >/dev/null && echo wget)")" \
        || die "the guest has neither curl nor wget — install one inside the VM: sudo apt install curl"
    [[ -n "$downloader" ]] ||
        die "the guest has neither curl nor wget — install one inside the VM: sudo apt install curl"
    case "$downloader" in
        curl) guest_exec "curl -sf -o '/root/$base' '$url'" ;;
        wget) guest_exec "wget -q -O '/root/$base' '$url'" ;;
    esac || die "guest could not download $url"
    guest_sha="$(guest_exec "sha256sum '/root/$base'" | awk '{print $1}')" \
        || die "could not checksum the downloaded file inside the guest"
    [[ "$guest_sha" == "$host_sha" ]] ||
        die "checksum mismatch — host $host_sha, guest $guest_sha; the transfer was corrupted, retry"
    if [ "$dest" = "/root/$base" ]; then
        # The download already staged it there (root-owned); install(1)
        # refuses when src and dst are the same file, so just apply the
        # source mode.
        guest_exec "chmod $mode '/root/$base'"
    else
        case "$dest" in
            /home/*) owner_user="$VM_USER" owner_group="$VM_USER" ;;
            *)       owner_user=root owner_group=root ;;
        esac
        # install -D creates missing parent directories owned by root
        # (it runs as root here), which locks the desktop user out of
        # their own staging dir. Create the parents first, then hand any
        # root-owned ones over; -user root keeps existing user files
        # untouched.
        guest_exec "mkdir -p '$(dirname "$dest")'"
        [[ "$owner_user" == root ]] ||
            guest_exec "find '$(dirname "$dest")' -user root -exec chown '$owner_user:$owner_group' {} +"
        guest_exec "install -D -o '$owner_user' -g '$owner_group' -m $mode '/root/$base' '$dest' && rm '/root/$base'"
    fi
    guest_exec "test -f '$dest'" || die "staging verification failed — '$dest' is not present in the guest"
    serve_down
    trap - EXIT
    note "staged $dest (sha256 $host_sha, mode $mode)"
}

cmd_screenshot() {
    local file="${1:-}"
    [[ -n "$file" ]] || die "usage: vm.sh screenshot <file.png>"
    check_host
    domain_must_run
    virsh screenshot "$VM_NAME" "$file" >/dev/null ||
        die "screenshot failed (the VM's display must be SPICE/VNC and the VM running)"
    note "screenshot saved to $file"
}

cmd_snapshot() {
    check_host
    domain_must_run
    local name="${1:-checkpoint}"
    [[ -n "$name" ]] || die "snapshot name must not be empty"
    virsh snapshot-create-as "$VM_NAME" "$name" >/dev/null ||
        die "snapshot '$name' failed (live snapshots need a running VM and a qcow2 disk)"
    note "snapshot '$name' taken"
}

cmd_restore() {
    check_host
    domain_must_exist
    local name="${1:-}"
    [[ -n "$name" ]] || die "usage: vm.sh restore <name> — see 'vm.sh snapshots'"
    virsh snapshot-revert "$VM_NAME" "$name" --running ||
        die "revert to '$name' failed — does it exist? (scripts/vm.sh snapshots)"
    note "reverted to '$name' (VM running)"
}

cmd_snapshots() {
    check_host
    domain_must_exist
    virsh snapshot-list "$VM_NAME" --name 2>/dev/null | sed '/^$/d'
}

# Human accounts in a running guest, comma-joined — the candidates for
# VM_USER. UID 1000..65533 excludes root and nobody; a shut-off VM or
# a silent agent says so instead of inventing a list.
guest_users() {  # guest_users <domain>
    local name="$1" state users
    state="$(virsh -q domstate "$name" 2>/dev/null || true)"
    if [[ "$state" != "running" ]]; then
        echo "(not running)"
        return 0
    fi
    if ! ( VM_NAME="$name"; agent_alive ); then
        echo "(guest agent unavailable)"
        return 0
    fi
    users="$( ( VM_NAME="$name"; guest_exec "awk -F: '\$3 >= 1000 && \$3 < 65534 {print \$1}' /etc/passwd" ) 2>/dev/null || true)"
    users="$(printf '%s\n' "$users" | sed '/^$/d' | paste -sd',' - | sed 's/,/, /g')"
    echo "${users:-(none found)}"
}

# The domains this host offers as targets, on the same connection the
# other commands use, with their state and guest users: the answer to
# "what can I pass to vm-push / vm-exec", and what to use as VM_USER.
cmd_list() {
    check_libvirt
    require python3 "sudo apt install python3"
    local list
    list="$(virsh -q list --all)" ||
        die "could not list libvirt domains — run 'virsh list --all' to see why"
    if [[ -z "$list" ]]; then
        echo "vm: no libvirt domains defined — create one in virt-manager"
        return 0
    fi
    echo "vm: libvirt domains (targets for the vm-* recipes):"
    printf '  %-28s %-13s %s\n' name state users
    local id name rest users
    while read -r id name rest; do
        [[ -n "$name" ]] || continue
        users="$(guest_users "$name")"
        printf '  %-28s %-13s %s\n' "$name" "$rest" "$users"
    done <<<"$list"
    echo "vm: defaults: VM_NAME=$VM_NAME · VM_USER=$VM_USER"
}

# Doctor: runs every precondition and reports ok/missing instead of failing.
# Beyond airlock's host checks this also probes the walk's own guest
# preconditions (session, disk headroom).
cmd_status() {
    local state ip
    echo "config:"
    echo "  domain $VM_NAME · user $VM_USER · bridge $BRIDGE_IP:$PORT"
    echo "host:"
    if command -v virsh >/dev/null 2>&1; then
        echo "  ok        virsh on PATH"
        if virsh -c qemu:///system version >/dev/null 2>&1; then
            echo "  ok        qemu:///system reachable"
            state="$(domain_state || true)"
            if [[ -n "$state" ]]; then
                echo "  ok        domain '$VM_NAME' exists ($state)"
                if [[ "$state" == "running" ]]; then
                    ip="$(vm_ip || true)"
                    if [[ -n "$ip" ]]; then
                        echo "  ok        guest IP $ip"
                    else
                        echo "  MISSING   guest IP — no NIC on the 'default' network?"
                    fi
                    if agent_alive; then
                        echo "  ok        guest agent responding"
                        if guest_exec "getent passwd '$VM_USER'" >/dev/null 2>&1; then
                            echo "  ok        guest user '$VM_USER' exists"
                        else
                            echo "  MISSING   guest user '$VM_USER' — VM_USER=<name> just vm-status"
                        fi
                        if guest_exec "pgrep -x cosmic-comp >/dev/null 2>&1"; then
                            echo "  ok        COSMIC session running (cosmic-comp)"
                        else
                            echo "  MISSING   cosmic-comp not running — is the guest logged in to a COSMIC session?"
                        fi
                        local free; free="$(guest_exec "df -BG --output=avail / | tail -1 | tr -dc '0-9'" 2>/dev/null || true)"
                        if [[ -n "$free" && "$free" -ge 4 ]]; then
                            echo "  ok        guest disk headroom ${free}G"
                        else
                            echo "  MISSING   guest disk headroom ${free:-?}G (want ≥ 4G)"
                        fi
                        if guest_exec "test -x /usr/bin/dropterm"; then
                            echo "  ok        native dropterm installed in guest (/usr/bin)"
                        else
                            echo "  ok        native dropterm not installed (install it from the release tarball for the walk)"
                        fi
                    else
                        echo "  MISSING   guest agent — sudo apt install qemu-guest-agent (inside the VM)"
                    fi
                else
                    echo "  MISSING   VM not running — start it in virt-manager"
                fi
            else
                echo "  MISSING   domain '$VM_NAME' — VM_NAME=<name> just vm-status"
            fi
        else
            echo "  MISSING   qemu:///system — sudo usermod -aG libvirt \"\$USER\", then log out/in"
        fi
    else
        echo "  MISSING   virsh — sudo apt install libvirt-clients"
    fi
    if ip -4 addr show 2>/dev/null | grep -q "inet $BRIDGE_IP/"; then
        echo "  ok        bridge IP $BRIDGE_IP present"
    else
        echo "  MISSING   bridge IP $BRIDGE_IP — sudo virsh net-start default (or BRIDGE_IP=)"
    fi
    echo "snapshots:"
    if virsh -c qemu:///system snapshot-list "$VM_NAME" --name >/dev/null 2>&1; then
        local names; names="$(cmd_snapshots 2>/dev/null || true)"
        [[ -n "$names" ]] && sed 's/^/  /' <<<"$names" || echo "  (none)"
    else
        echo "  (unavailable — see host checks above)"
    fi
}

cmd_help() {
    cat <<'EOF'
vm — COSMIC validation-VM plumbing (scripts/vm.sh; guide: docs/development.md)

Ported from cosmic-ext-airlock's CP4 harness; drives the guest through
the QEMU guest agent (no SSH) — the guest needs qemu-guest-agent once.

Variables (override per run — forms below):
  VM_NAME    libvirt domain                  default PopOs24
  VM_USER    desktop user inside the guest   default jpt
  VM_HOME    guest home directory            default: looked up in the guest's /etc/passwd
  BRIDGE_IP  host IP on the VM NAT bridge    default 192.168.122.1
  PORT       transfer port                   default 8000

Override forms (VM_USER shown; same for every variable):
  VM_USER=alice scripts/vm.sh status         environment prefix
  just vm-user=alice vm-status               assignment, before the recipe
  just --set vm-user alice vm-status         same, long form

The just recipes also take the domain as an optional argument —
`just vm-push PopOs25.04`, `just vm-exec 'pgrep -a cosmic-comp'
PopOs25.04` — defaulting to VM_NAME, and the recipes that use the
guest user take it next (`just vm-status PopOs25.04 alice`),
defaulting to VM_USER; scripts/vm.sh itself reads only the
environment.

Commands:
  status                          preflight doctor — run this first
  list                            libvirt domains, state, and guest users
  ip                              the guest's current NAT IP
  exec '<cmd>'                    run a shell command in the guest (root)
  exec-user '<cmd>'               run it as VM_USER inside the session env
                                  (runtime dir + session bus + Wayland socket)
  put <local> <guest-path>        transfer with sha256 verified both sides
                                  (guest-home destinations arrive user-owned)
  screenshot <file.png>           capture the VM's display (evidence)
  snapshot [name]                 live libvirt snapshot
  restore <name>                  revert to a snapshot (VM left running)
  snapshots                       list them

The store walk, in order — docs/development.md has the full recipe:
  just vm-status                  preflight
  scripts/vm.sh restore pre-airlock            clean point (stock guest)
  ... native tarball install (vm exec), relogin/reboot ...
  ... panel walk + vm screenshot for evidence, vm snapshot at clean points ...
EOF
}

cmd="${1:-help}"
shift || true
case "$cmd" in
    ip)          cmd_ip ;;
    list)        cmd_list ;;
    exec)        [[ -n "${1:-}" ]] || die "usage: vm.sh exec '<cmd>'"; check_host; domain_must_run; guest_exec "$1" ;;
    exec-user)   [[ -n "${1:-}" ]] || die "usage: vm.sh exec-user '<cmd>'"; check_host; guest_exec_as_user "$1" ;;
    put)         cmd_put "$@" ;;
    screenshot)  cmd_screenshot "$@" ;;
    snapshot)    cmd_snapshot "${1:-checkpoint}" ;;
    restore)     cmd_restore "$@" ;;
    snapshots)   cmd_snapshots ;;
    status)      cmd_status ;;
    help|*)      cmd_help ;;
esac
