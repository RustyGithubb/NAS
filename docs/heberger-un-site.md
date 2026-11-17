# Héberger un site sur le Mac mini

Mise en place sur le serveur (Debian 13) pour que l'onglet **Sites** de la console pilote de
vrais sites et que la synchronisation des apps soit joignable depuis Internet. Tout est open
source, installé avec `apt` (le gestionnaire de paquets de Debian), sans Docker.

## 1. Les logiciels

```bash
sudo apt install caddy php-fpm php-mysql php-gd php-xml php-mbstring php-curl php-zip php-intl mariadb-server
```

- **Caddy** : serveur web ; obtient et renouvelle seul les certificats HTTPS (Let's Encrypt).
- **PHP-FPM** : fait tourner le code de WordPress (PHP 8.4 sur Debian 13).
- **MariaDB** : base de données de WordPress. N'écoute qu'en local (`127.0.0.1:3306`) par défaut.

### Caddy

Le Caddyfile de Debian contient un bloc `:80` qui affiche une page d'accueil : on le remplace.
L'agent écrit un fichier par site dans `/etc/caddy/sites/` ; le Caddyfile principal les importe.

```bash
sudo mkdir -p /etc/caddy/sites
sudo cp /etc/caddy/Caddyfile /etc/caddy/Caddyfile.origine
echo 'import /etc/caddy/sites/*.caddy' | sudo tee /etc/caddy/Caddyfile
sudo caddy validate --config /etc/caddy/Caddyfile   # avertissement « No files matching » normal sans site
sudo systemctl reload caddy
```

L'agent doit tourner en root (il écrit dans `/etc/caddy/sites/` et recharge Caddy). Ne rien
ajouter à la main dans `/etc/caddy/sites/` : seul le Caddyfile principal est modifié à la main
(bloc de la synchro, partie 7).

### PHP

Le « tuyau » de PHP (`/run/php/php-fpm.sock`, celui qu'utilise l'agent) n'est accessible qu'au
groupe `www-data` : Caddy doit en faire partie, sinon erreur 502.

```bash
sudo usermod -aG www-data caddy
sudo systemctl restart caddy
```

Limites d'envoi (2 Mo par défaut, trop peu pour un portfolio) :

```bash
printf 'upload_max_filesize = 64M\npost_max_size = 64M\nmemory_limit = 256M\n' | sudo tee /etc/php/8.4/fpm/conf.d/90-nas.ini
sudo systemctl restart php8.4-fpm
```

### Base de données

```bash
openssl rand -base64 24   # mot de passe de l'utilisateur wordpress, à garder
sudo mariadb
```

```sql
CREATE DATABASE wordpress CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
CREATE USER 'wordpress'@'localhost' IDENTIFIED BY 'MOT_DE_PASSE';
GRANT ALL PRIVILEGES ON wordpress.* TO 'wordpress'@'localhost';
FLUSH PRIVILEGES;
```

## 2. Les dossiers

| Dossier | Contenu |
|---|---|
| `/srv/nas/sites/<nom>/` | le site (fichiers WordPress, ou pages HTML) |
| `/srv/nas/medias/<nom>/` | les médias du site (images, vidéos…), aussi dans l'onglet Fichiers |
| `/srv/nas/sauvegardes/sites/<nom>/` | sauvegardes faites depuis la console |

Les médias sont partagés entre WordPress (utilisateur `www-data`) et l'agent : un groupe commun
`nas-medias`, et le bit « setgid » (`2775`) pour que tout nouveau fichier hérite du groupe.

```bash
sudo mkdir -p /srv/nas/sites/portfolio /srv/nas/medias/portfolio /srv/nas/sauvegardes/sites
sudo groupadd nas-medias
sudo usermod -aG nas-medias www-data
sudo chown -R root:nas-medias /srv/nas/medias
sudo chmod -R 2775 /srv/nas/medias
sudo systemctl restart php8.4-fpm
```

### Médias visibles depuis WordPress

Le dossier des envois de WordPress (`wp-content/uploads`) est **monté** sur le dossier des
médias (montage « bind » : le même dossier visible à deux endroits, pas une copie). Ligne à
ajouter dans `/etc/fstab` (la liste des montages faits au démarrage) :

```
/srv/nas/medias/portfolio  /srv/nas/sites/portfolio/wp-content/uploads  none  bind  0  0
```

- Un fichier envoyé depuis WordPress arrive dans `/srv/nas/medias/portfolio` → visible dans l'onglet Fichiers.
- Un fichier déposé depuis l'onglet Fichiers est sur le disque, mais WordPress ne le liste dans
  sa médiathèque qu'une fois **enregistré** : extension libre *Media Sync* (bouton « Scanner »),
  ou plus tard un bouton dans la console (commande `wp media import` de WP-CLI).

## 3. Déclarer un site

`/etc/nas-agent/sites.json` (l'agent le met ensuite à jour lui-même) :

```json
[
  { "name": "liens", "domain": "liens.exemple.fr", "kind": "Static",
    "root": "/srv/nas/sites/liens", "media_dir": "/srv/nas/medias/liens" },
  { "name": "portfolio", "domain": "exemple.fr", "kind": "WordPress",
    "root": "/srv/nas/sites/portfolio", "media_dir": "/srv/nas/medias/portfolio",
    "database": "wordpress" }
]
```

## 4. Migrer le WordPress depuis OVH

1. Sur l'hébergement OVH (FTP ou gestionnaire de fichiers) : récupérer tout le dossier `www/`.
2. Dans l'espace client OVH (phpMyAdmin ou « Bases de données » › Sauvegarde) : exporter la base (`.sql`).
3. Sur le Mac mini : copier les fichiers dans `/srv/nas/sites/portfolio/`, déplacer
   `wp-content/uploads/*` dans `/srv/nas/medias/portfolio/`, puis faire le montage de la partie 2.
   Donner le dossier du site à WordPress : `sudo chown -R www-data:www-data /srv/nas/sites/portfolio`.
4. Importer le `.sql` dans la base `wordpress` (partie 1), mettre les identifiants dans `wp-config.php`.
5. Tester en « Maintenance » depuis la console, puis basculer le nom de domaine (partie 6).

## 5. Rendre le serveur joignable depuis Internet

La Bbox a une **IPv4 partagée** : seuls les ports 57344 à 65535 peuvent être redirigés en IPv4,
donc pas 80 ni 443. Le serveur est joint en **IPv6**, où le Mac a sa propre adresse publique.

- Adresse IPv6 publique du Mac : `ip -6 addr show scope global` (celle qui n'est pas `temporary`).
  NetworkManager la garde stable (`stable-privacy`) tant que le préfixe donné par la box ne change pas.
- **Bbox** › Pare-feu IPv6 : deux règles « Accepter », TCP, source **tous**, destination
  **l'adresse IPv6 du Mac**, port **de destination** 80 et 443 (pas le port source).
- **Bail DHCP fixe** (Bbox › DHCP, en IPv4) : `192.168.1.169` réservée au Mac pour la console.
- Test : depuis un téléphone en 4G (Wi-Fi coupé), `http://[adresse IPv6]` avec un site de test.
  Depuis le réseau local, le test ne prouve rien (le pare-feu de la box n'est pas traversé).

Limite : les visiteurs sans IPv6 ne joignent pas le serveur. Pour l'éviter : demander à Bouygues
une IPv4 non partagée (redirection 80/443 vers `192.168.1.169`, enregistrement A en plus).

## 6. Nom de domaine

Chez le registrar (OVH, Zone DNS) : un enregistrement **AAAA** par nom vers l'adresse IPv6 du Mac,
**sans enregistrement A** (l'IPv4 de la box n'atteint pas le Mac). Ne pas toucher aux
enregistrements d'un domaine encore servi par OVH avant d'avoir migré le site. Vérifier depuis
le PC : `nslookup -type=AAAA sync.exemple.fr`.

Si le préfixe IPv6 change, mettre à jour les AAAA et les règles du pare-feu de la box.

## 7. Synchronisation des apps

Bloc à ajouter au Caddyfile principal **une fois le nom résolu** (Caddy demande le certificat
dès qu'il voit le bloc). Seul `/sync/…` est transmis, et **vers `127.0.0.1`** : l'agent ne lit
l'adresse du visiteur (`X-Forwarded-For`, pour la limite d'essais) que si la demande vient de la
machine elle-même, et refuse toute demande `/api/…` passée par Caddy.

```bash
printf '\nsync.exemple.fr {\n\treverse_proxy /sync/* 127.0.0.1:7878\n}\n' | sudo tee -a /etc/caddy/Caddyfile
sudo caddy validate --config /etc/caddy/Caddyfile
sudo systemctl reload caddy
sudo journalctl -u caddy --since "15 min ago" | grep -iE 'certificate obtained|error'
```

Test en 4G : `https://sync.exemple.fr/sync/auth/salt?login=test` répond en JSON,
`https://sync.exemple.fr/api/status` renvoie une page vide. Puis console › Synchro : adresse
publique `https://sync.exemple.fr`.

## 8. Sécurité

Pare-feu `ufw` (outil simple qui écrit les règles du pare-feu de Linux, IPv4 et IPv6) : seuls
80/443 ouverts à tous ; SSH et l'agent depuis le réseau local seulement (et en IPv4 : fermés en
IPv6, où le Mac est adressable depuis Internet).

```bash
sudo apt install ufw
sudo ufw default deny incoming
sudo ufw default allow outgoing
sudo ufw allow from 192.168.1.0/24 to any port 22 proto tcp
sudo ufw allow from 192.168.1.0/24 to any port 7878 proto tcp
sudo ufw allow 80,443/tcp
sudo ufw enable
```

À venir :
- SSH par clé seulement (`PasswordAuthentication no`) quand il sera joignable via WireGuard.
- `fail2ban` sur les tentatives de connexion à `wp-login.php`.
- Mises à jour automatiques de Debian (`unattended-upgrades`) et de WordPress.
