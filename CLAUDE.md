# NAS (nom provisoire)

Pilotage d'un Mac mini 2014 (8 Go, Debian) reconverti en serveur. Workspace Cargo :
- `crates/console` : app de bureau Rust (egui) sous Windows. DA reprise de WorkTable (`../WorkTable`) : thèmes, pilule flottante, sélection liquide, barre de titre intégrée, textes `t!`/`tf!`.
- `crates/agent` : serveur sur le Mac mini, répond à la console. Sous Windows, ce qui est propre à Linux (systemd, journald, SMART) est simulé.
- `crates/proto` : types partagés (`proto::sync` : synchronisation des apps).
- `crates/sync-client` : client de synchronisation pour les apps (WorkTable…) : chiffrement de bout en bout + connexion à `/sync/…`. Bloquant (ureq), à appeler hors de l'interface.

## Règles
- Réponds en français, concis, sans récapitulatif.
- Je suis débutant en Linux/serveur : explique en une phrase le rôle de chaque commande système ou notion nouvelle.
- Fais UNIQUEMENT ce qui est demandé : aucune fonctionnalité, refacto ou amélioration non demandée.
- Fonctionnalité sur plus d'un fichier : plan court d'abord, j'attends ta validation.
- Que des briques open source, pas de service payant ni d'IA payante (API Claude).
- L'agent tourne sur une machine faible (2 cœurs, 8 Go, disque lent) : dépendances légères, pas de Docker.
- Git : ne jamais commit/branch/tag/push. Donne-moi les commandes à lancer moi-même (je travaille sur plusieurs PC).

## Mise en page des onglets (comme WorkTable)
- En-tête : `tabs::header(sur-titre, titre, info, accent)` (→ `widgets::page_header_with`).
- Contenu : panneaux pleine hauteur avec `widgets::pane_columns` ; colonne de gauche `widgets::SIDE_W` (260 px) avec `side_label` + `nav_row` (pastille liquide), puis liste (`pane_title`, `list_item`) et/ou détail.
- États vides : `widgets::empty_state` ; agent indisponible : `tabs::unavailable`.
- Réglages : pas un onglet, mais un panneau superposé façon Ctrl+Espace (`tabs::settings::SettingsPanel`, sur `overlay.rs`), ouvert/fermé par la pilule ⚙️ seulement ; sections via `group` (filet + titre en gras), aucune carte.

## Économie de tokens
- Ne lis pas Cargo.lock, target/, data/.
- Pour reprendre un élément de WorkTable : Grep dans `../WorkTable/src`, puis Read avec offset/limit.
- Vérifie avec `cargo check --workspace`, jamais `cargo build --release`.

## Agents (`.claude/agents/`, versionnés)
- `explorateur` : avant de lire du code, pour toute question « où est… / qui appelle… ».
- `planificateur` : avant toute fonctionnalité sur plus d'un fichier ; son plan est celui que je valide.
- `build-fixer` : dès que `cargo check` échoue.
- `relecteur` : toujours, avant de me donner les commandes de commit.
- `changelog` : quand je prépare une version (puis vérifie `cargo fmt --check`, exigé par la CI).
- `decoupeur` : uniquement si je le demande.

## Carte du code (crates/console/src)
- main.rs, app.rs : démarrage, boucle principale, enum `Tab`
- instance.rs : instance unique (Windows, version installée) ; `--after-update` : la nouvelle version attend que l'ancienne se ferme ; sans contrôle en debug
- build.rs + assets/logo.ico, logo.png : icône de l'exe (ressources Windows) et de la fenêtre
- theme.rs (thèmes + polices par groupe), liquid.rs, titlebar.rs, i18n.rs, widgets.rs (carte qui tremble, sélecteur segmenté) : repris de WorkTable
- sidebar.rs : pilule flottante, pastille Compte (ronde, en haut à gauche), logo vectoriel (roue au clic → tremblement des cartes), icônes
- account_panel.rs : panneau Compte (clic sur la pastille) : adresse de l'agent et clé d'accès en champs protégés, état de la liaison, mises à jour de l'agent ; initiales du nom déclaré par l'agent + pastille d'état
- overlay.rs : fond flouté `Backdrop` et cadre `overlay_frame`, repris de la recherche Ctrl+Espace de WorkTable
- prefs.rs : réglages (data/prefs.json en dev ; thème, langue, adresse, animation, polices), enregistrés 0,5 s après la dernière modification
- secret.rs : clé d'accès dans le coffre du système (crate keyring), jamais dans prefs.json
- drives.rs : disques de ce PC et leur nom (GetVolumeInformationW sous Windows)
- preview.rs : aperçus du volet Détails des Fichiers (images décodées hors de l'interface, début des textes) et vignettes de la vue en grandes icônes
- ole_drag.rs (Windows seulement) : glisser des fichiers de la console vers l'Explorateur / le Bureau (glisser-déposer OLE, objets COM écrits à la main avec windows-sys)
- tabs/sync.rs : onglet Synchro (vue d'ensemble, répertoire d'apps, comptes, invitations, postes) ; données via `server.poll_sync` / `sync_action` / `sync_invite`
- server.rs : liaison avec l'agent (état + services toutes les 2 s ; actions, journaux, fichiers et transferts dans des fils à part ; téléchargements dans Téléchargements\NAS)
- update.rs : mise à jour de la console depuis les releases GitHub (archive `nas-console-x86_64-pc-windows-msvc.zip` + SHA-256, self-replace ; désactivé en debug) ; carte dans Réglages › Général, pastille sur Réglages
- icons.rs : émojis Twemoji (assets/emoji, CC-BY 4.0) par type de fichier et par emplacement
- tabs/ : mod.rs (en-tête, agent indisponible, lignes de détail), dashboard.rs, services.rs, sites.rs, logs.rs, files.rs (explorateur façon Windows : onglets, barre d'adresse et de commandes, Accueil, Galerie, Détails, liste / grandes icônes, recherche instantanée, presse-papiers, épinglés, « Ce PC », glisser-déposer serveur ↔ PC, onglet Transferts), settings.rs (panneau Réglages, pilule ⚙️ ; sans le serveur, passé dans account_panel.rs)
- Racine (hors crates) : dist-workspace.toml + .github/workflows/release.yml (cargo-dist 0.32.0 ; tag vX.Y.Z → console Windows .zip et agent Linux .tar.xz), installer/ (NAS.iss, build.ps1 → installer\Output\NAS-<version>-setup.exe)

## Agent (crates/agent/src)
- main.rs : serveur HTTP (tiny_http), routes `/api/…` ; écoute sur `NAS_AGENT_ADDR` (défaut 127.0.0.1:7878)
- key.rs : clé d'accès (créée au premier lancement, fichier 600), vérifiée sur chaque demande ; `--show-key`
- backend/ : services et journaux — systemd.rs (`systemctl`, `journalctl`), mock.rs (simulation hors Linux ou `NAS_AGENT_MOCK=1`)
- files/ : emplacements (`NAS_AGENT_ROOTS="Nom=chemin;…"`), opérations (corbeille `.corbeille`), index.rs = index en mémoire façon Everything (refait toutes les 5 min ou après une modification)
- sites/ : sites web — caddy.rs (fichier par site dans /etc/caddy/sites, sauvegardes tar + mysqldump), mock.rs
- sampler.rs : mesures système chaque seconde (sysinfo 0.37 : ne pas monter de version sans adapter, l'API a changé)
- network.rs : réseau du serveur (route par défaut, compteurs /sys, nmcli, ip, ping vers la box et 1.1.1.1, coupures lues dans le journal de NetworkManager et wpa_supplicant, test de débit avec curl) ; simulé hors du Mac. Route `/api/network` ajoutée sans changer la version du protocole (404 = agent trop ancien)
- sync/ : synchronisation des apps — mod.rs (dossiers, purge, sauvegarde, limite de demandes), db.rs (SQLite : apps, comptes, accès, postes, documents), public.rs (`/sync/…`, sessions des comptes, traitées avant la clé dans `handle()`), admin.rs (`/api/sync/…`), google.rs (vérification des jetons d'identité Google : clés publiques téléchargées avec curl, crate rsa). Types : `nas_proto::sync`. Intégration dans une app : docs/synchro-google-et-reconnexion.md
- update.rs : recherche de mise à jour (lance le service `nas-agent-update`, lit /var/lib/nas-agent/update.json)
- Routes et types JSON : `crates/proto/src/lib.rs`

## Serveur (deploy/)
- `install.sh` (sudo) : services systemd de l'agent, mise à jour nocturne, rallumage après coupure ; installe curl, tar, xz-utils
- `nas-agent-update.sh` : télécharge la dernière release GitHub (sans git ni compilation), vérifie l'empreinte, installe, relance (ancienne version remise si échec)
- Mac mini sans ventilateur (connecteur arraché) : processeur bridé (turbo coupé, 60 %), voir docs/installer-le-serveur.md

## Docs
- `docs/installer-le-serveur.md` : Debian sur le Mac mini, Wi-Fi Broadcom, bridage thermique, agent, mises à jour
- `docs/heberger-un-site.md` : mise en place serveur (Caddy, PHP, MariaDB, médias partagés, migration OVH)

## Étapes du projet
1. Squelette (fait).
2. Console : thème, barre latérale, barre de titre, onglets vides (fait).
3. Agent (mesures réelles via sysinfo) + tableau de bord de la console (fait).
4. Contrôle des services, journaux en direct (fait, testé sur le Mac mini).
5. Clé d'accès entre console et agent (fait). Le chiffrement du trafic viendra de WireGuard.
6. Onglet Sites (fait, simulé) ; onglet Fichiers (fait) ; à faire : vignettes d'images en galerie, restauration de la corbeille, page de liens temporaire, migration du WordPress OVH.
7. Mac mini sous Debian 13 en Wi-Fi, agent installé (fait) ; mise à jour automatique depuis GitHub et rallumage après coupure (deploy/, à installer).
8. Première release 0.1.0 : console installable (Inno Setup) avec mise à jour automatique, agent publié et installé par le Mac depuis les releases (fait, à publier).
