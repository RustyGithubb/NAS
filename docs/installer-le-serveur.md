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

L'agent n'est plus compilé sur le Mac mini : il est téléchargé déjà compilé depuis les versions
publiées sur GitHub (voir « Publier une version » dans le README). Il faut seulement les fichiers
de `deploy/` pour installer les services.

```sh
sudo apt install curl
curl -fsSL https://github.com/RustyGithubb/NAS/archive/refs/heads/main.tar.gz | tar -xz   # fichiers du dépôt, sans git
cd NAS-main
sudo sh deploy/install.sh                 # services, outils (curl, tar, xz-utils)
sudo systemctl start nas-agent-update     # télécharge et installe la dernière version de l'agent
cat /var/lib/nas-agent/update.json        # doit dire « Mis à jour vers la version … »
sudo nas-agent --show-key    # à coller dans la console : Compte › Clé d'accès
cd .. && rm -rf NAS-main                  # plus besoin de ces fichiers
```

`tar -xz` décompresse l'archive téléchargée ; `systemctl start` lance tout de suite un service
qui, sinon, attend son heure.

### Passage depuis l'ancienne mise à jour (une seule fois)

Avant la version 0.1.0, le Mac compilait l'agent lui-même depuis un dépôt cloné. Pour passer aux
versions publiées, une fois la version `v0.1.0` visible sur la page Releases de GitHub :

```sh
cd ~/NAS && git pull                      # récupère le nouveau script de mise à jour
sudo sh deploy/install.sh                 # l'installe (et curl, tar, xz-utils)
sudo systemctl start nas-agent-update     # remplace l'agent compilé ici par celui publié
cat /var/lib/nas-agent/update.json        # « Mis à jour vers la version 0.1.0 »
systemctl status nas-agent                # doit être « active (running) »
```

L'ancien agent est gardé en `/usr/local/bin/nas-agent.prev` et remis automatiquement si le
nouveau ne démarre pas. Quand tout fonctionne, le dépôt et Rust ne servent plus :

```sh
rm -rf ~/NAS                  # dépôt cloné
rustup self uninstall -y      # Rust et cargo (libère environ 1 Go)
```

`deploy/install.sh` installe :

| Service | Rôle |
|---|---|
| `nas-agent` | démarre l'agent avec le Mac, le relance s'il plante |
| `nas-agent-update` (+ `.timer`) | chaque nuit vers 4 h : nouvelle version publiée sur GitHub → téléchargement, vérification de l'empreinte, installation, relance ; ancienne version remise si la nouvelle ne démarre pas |
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
