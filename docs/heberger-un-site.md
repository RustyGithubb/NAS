# Héberger un site sur le Mac mini

Ce qu'il faudra faire sur le serveur (Debian) pour que l'onglet **Sites** de la console pilote
de vrais sites. Tout est open source, installé avec `apt` (le gestionnaire de paquets de Debian),
sans Docker.

## 1. Les logiciels

```bash
sudo apt install caddy php-fpm php-mysql php-gd php-xml php-mbstring php-curl php-zip php-intl mariadb-server
```

- **Caddy** : serveur web ; obtient et renouvelle seul les certificats HTTPS (Let's Encrypt).
- **PHP-FPM** : fait tourner le code de WordPress.
- **MariaDB** : base de données de WordPress.

Dans `/etc/caddy/Caddyfile`, une seule ligne utile : l'agent écrit un fichier par site dans
`/etc/caddy/sites/`.

```
import /etc/caddy/sites/*.caddy
```

L'agent doit tourner en root (il écrit dans `/etc/caddy/sites/` et recharge Caddy).

## 2. Les dossiers

| Dossier | Contenu |
|---|---|
| `/srv/nas/sites/<nom>/` | le site (fichiers WordPress, ou pages HTML) |
| `/srv/nas/medias/<nom>/` | les médias du site (images, vidéos…), aussi dans l'onglet Fichiers |
| `/srv/nas/sauvegardes/sites/<nom>/` | sauvegardes faites depuis la console |

Les médias sont partagés entre WordPress (utilisateur `www-data`) et l'agent : un groupe commun
`nas-medias`, et le bit « setgid » (`2775`) pour que tout nouveau fichier hérite du groupe.

```bash
sudo groupadd nas-medias
sudo usermod -aG nas-medias www-data
sudo mkdir -p /srv/nas/medias/portfolio
sudo chown -R root:nas-medias /srv/nas/medias
sudo chmod -R 2775 /srv/nas/medias
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
4. Créer la base et son utilisateur dans MariaDB, importer le `.sql`, mettre les nouveaux
   identifiants dans `wp-config.php`.
5. Tester en « Maintenance » depuis la console, puis basculer le nom de domaine (étape 6).

## 5. Rendre le site joignable depuis Internet

- **Box** : rediriger les ports **80** et **443** (TCP) vers l'adresse locale du Mac mini, et lui
  réserver cette adresse (bail DHCP fixe). À tenter dans l'interface de la box.
- **Si la box ne le permet pas** : un petit serveur loué (VPS, ~4 €/mois) relié au Mac mini par
  WireGuard, qui fait suivre les visites. Rien d'autre ne change côté console.

## 6. Nom de domaine

Chez le registrar (OVH si le domaine y est) : faire pointer l'enregistrement **A** du domaine vers
l'adresse IP publique de la box. Si cette adresse change de temps en temps, il faudra un DNS
dynamique (DynHost chez OVH).

## 7. Sécurité minimale avant d'ouvrir

- Pare-feu : seuls 80/443 ouverts vers l'extérieur ; l'agent (7878) et SSH uniquement via WireGuard.
- `fail2ban` sur les tentatives de connexion à `wp-login.php`.
- Mises à jour automatiques de Debian (`unattended-upgrades`) et de WordPress.
