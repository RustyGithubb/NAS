# Installer le serveur (Mac mini 2014 sous Debian)

Ce qui a été fait sur le Mac mini, dans l'ordre, pour pouvoir le refaire.

## 1. Matériel

- Disque dur remplacé par un SSD 2,5" de 2 To ([guide iFixit](https://www.ifixit.com/Guide/Mac+mini+Late+2014+Hard+Drive+Replacement/32815)), pâte thermique refaite ([guide](https://www.ifixit.com/Guide/Mac+Mini+Late+2014+Heat+Sink+Replacement/32830), étapes 26-27).
- **Pas de ventilateur** : son connecteur s'est arraché de la carte mère. Le processeur est bridé (étape 5). À revoir si le ventilateur est remonté un jour.

## 2. Clé d'installation (sur le PC Windows)

- Image Debian 13 « netinst » amd64 depuis [debian.org](https://www.debian.org/distrib/netinst).
- Écrite avec [Rufus](https://rufus.ie/fr/), en **mode Image DD**.

## 3. Installation de Debian

- Le Wi-Fi interne (Broadcom BCM4360) ne marche pas dans l'installateur : connexion internet par un **téléphone Android en partage USB** (un iPhone ne marche pas dans l'installateur).
- Démarrage : touche **Alt** maintenue, puis « EFI Boot ».
- Nom de machine `nas`, mot de passe root vide (on utilise `sudo`), disque entier en une partition.
- Logiciels : **serveur SSH** + utilitaires usuels, pas d'environnement de bureau.
- « Forcer l'installation vers le chemin de support amovible EFI » : **Oui** (Mac).

## 4. Wi-Fi interne

```sh
sudo sed -i 's/non-free-firmware$/non-free-firmware contrib non-free/' /etc/apt/sources.list /etc/apt/sources.list.d/*.sources
sudo apt update
sudo apt install linux-headers-amd64 broadcom-sta-dkms network-manager avahi-daemon
sudo reboot
nmtui    # « Activer une connexion », choisir la box
```

- `avahi-daemon` rend le Mac joignable en `nas.local`.
- Mise en veille du Wi-Fi coupée (coupures de quelques dizaines de secondes sinon) :

```sh
printf '[connection]\nwifi.powersave = 2\n' | sudo tee /etc/NetworkManager/conf.d/wifi-powersave-off.conf
sudo systemctl restart NetworkManager
```

## 5. Bridage thermique (sans ventilateur)

```sh
echo 'w /sys/devices/system/cpu/intel_pstate/no_turbo - - - - 1' | sudo tee /etc/tmpfiles.d/no-turbo.conf
echo 'w /sys/devices/system/cpu/intel_pstate/max_perf_pct - - - - 60' | sudo tee -a /etc/tmpfiles.d/no-turbo.conf
sudo systemd-tmpfiles --create /etc/tmpfiles.d/no-turbo.conf
```

Mesuré : 55 °C au repos, **75 °C** à pleine charge pendant 4 min (88 °C avant la limite à 60 %).
Contrôle : `sudo apt install lm-sensors stress-ng`, puis `stress-ng --cpu 2 --timeout 240s & sleep 230; sensors | grep Package`.

## 6. Disque

Réserve de l'administrateur ramenée de 5 % à 1 % (environ 75 Go rendus) :

```sh
sudo tune2fs -m 1 $(findmnt -n -o SOURCE /)
```

## 7. Agent NAS

```sh
sudo apt install git build-essential curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"
git clone https://github.com/RustyGithubb/NAS.git && cd NAS
cargo build --release -p nas-agent
sudo install -m 755 target/release/nas-agent /usr/local/bin/nas-agent
sudo sh deploy/install.sh
sudo systemctl start nas-agent
sudo nas-agent --show-key    # à coller dans la console : Réglages › Serveur
```

Si cargo n'arrive pas à télécharger : `CARGO_HTTP_MULTIPLEXING=false CARGO_NET_RETRY=10 CARGO_HTTP_TIMEOUT=120 cargo build …`.

`deploy/install.sh` installe :

| Service | Rôle |
|---|---|
| `nas-agent` | démarre l'agent avec le Mac, le relance s'il plante |
| `nas-agent-update` (+ `.timer`) | chaque nuit vers 4 h : nouvelle version sur GitHub, CI verte → compile, installe, relance ; ancienne version remise si la nouvelle ne démarre pas |
| `nas-autopoweron` | rallume le Mac quand le courant revient après une coupure |

Commandes utiles :

```sh
sudo systemctl start nas-agent-update      # chercher une mise à jour maintenant (ou bouton dans la console)
cat /var/lib/nas-agent/update.json         # résultat de la dernière recherche
journalctl -u nas-agent-update -n 30       # détail
systemctl list-timers nas-agent-update     # prochaine recherche
setpci -s 00:1f.0 0xa4.b                   # rallumage : le dernier chiffre doit être pair (bit 0 à 0)
```

Le rallumage automatique est à **tester** une fois : Mac allumé, débrancher le câble, attendre 10 s, rebrancher.
