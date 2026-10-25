set -eu
pc_reboot_schedule='command -v reboot >/dev/null 2>&1 || exit 127
command -v nohup >/dev/null 2>&1 || exit 127
nohup sh -c "sleep 2; exec reboot" </dev/null >/dev/null 2>&1 &
printf "PORTCANVAS_REBOOT_SCHEDULED\n"'
if [ "$(id -u)" -eq 0 ]; then
    sh -c "$pc_reboot_schedule"
else
    sudo -n -- sh -c "$pc_reboot_schedule"
fi
