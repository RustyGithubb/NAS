#!/bin/sh
# Installe sur le Mac mini : le service de l'agent (démarrage automatique), la mise à jour
# nocturne depuis GitHub et le rallumage après une coupure de courant.
# Usage, depuis le dépôt cloné : sudo sh deploy/install.sh
set -eu
cd "$(dirname "$0")"

if [ "$(id -u)" -ne 0 ]; then
    echo "À lancer avec sudo : sudo sh deploy/install.sh" >&2
    exit 1
fi

# setpci (rallumage), curl (vérifications GitHub), git.
apt-get install -y pciutils curl git

install -m 755 nas-agent-update.sh /usr/local/sbin/nas-agent-update
install -m 644 nas-agent.service nas-agent-update.service nas-agent-update.timer nas-autopoweron.service /etc/systemd/system/
mkdir -p /srv/nas/medias /srv/nas/partage /srv/nas/sauvegardes /var/lib/nas-agent

systemctl daemon-reload
systemctl enable nas-agent.service
systemctl enable --now nas-agent-update.timer nas-autopoweron.service

echo
echo "Installé :"
echo "  - nas-agent           démarre avec le Mac mini"
echo "  - nas-agent-update    chaque nuit vers 4 h ; tout de suite : sudo systemctl start nas-agent-update"
echo "  - nas-autopoweron     rallumage après coupure (registre : $(setpci -s 00:1f.0 0xa4.b))"
systemctl list-timers nas-agent-update.timer --no-pager | head -n 2
