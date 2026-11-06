# NAS (nom provisoire)

Pilotage d'un Mac mini 2014 (8 Go, Debian) reconverti en serveur. Workspace Cargo :
- `crates/console` : app de bureau Rust (egui) sous Windows. DA reprise de WorkTable (`../WorkTable`) : thèmes, pilule flottante, sélection liquide, barre de titre intégrée, textes `t!`/`tf!`.
- `crates/agent` : serveur sur le Mac mini, répond à la console. Sous Windows, ce qui est propre à Linux (systemd, journald, SMART) est simulé.
- `crates/proto` : types partagés.

## Règles
- Réponds en français, concis, sans récapitulatif.
- Je suis débutant en Linux/serveur : explique en une phrase le rôle de chaque commande système ou notion nouvelle.
- Fais UNIQUEMENT ce qui est demandé : aucune fonctionnalité, refacto ou amélioration non demandée.
- Fonctionnalité sur plus d'un fichier : plan court d'abord, j'attends ta validation.
- Que des briques open source, pas de service payant ni d'IA payante (API Claude).
- L'agent tourne sur une machine faible (2 cœurs, 8 Go, disque lent) : dépendances légères, pas de Docker.
- Git : ne jamais commit/branch/tag/push. Donne-moi les commandes à lancer moi-même (je travaille sur plusieurs PC).

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
- theme.rs (thèmes + polices par groupe), liquid.rs, titlebar.rs, i18n.rs, widgets.rs (carte qui tremble, sélecteur segmenté) : repris de WorkTable
- sidebar.rs : pilule flottante, logo vectoriel (roue au clic → tremblement des cartes), icônes
- prefs.rs : réglages (data/prefs.json en dev ; thème, langue, adresse, animation, polices), enregistrés 0,5 s après la dernière modification
- secret.rs : clé d'accès dans le coffre du système (crate keyring), jamais dans prefs.json
- server.rs : liaison avec l'agent (état + services toutes les 2 s ; actions et journaux dans des fils à part)
- tabs/ : mod.rs (en-tête, carte, carte hors ligne, onglet Fichiers vide), dashboard.rs, services.rs, sites.rs, logs.rs, settings.rs

## Agent (crates/agent/src)
- main.rs : serveur HTTP (tiny_http), routes `/api/…` ; écoute sur `NAS_AGENT_ADDR` (défaut 127.0.0.1:7878)
- key.rs : clé d'accès (créée au premier lancement, fichier 600), vérifiée sur chaque demande ; `--show-key`
- backend/ : services et journaux — systemd.rs (`systemctl`, `journalctl`), mock.rs (simulation hors Linux ou `NAS_AGENT_MOCK=1`)
- sites/ : sites web — caddy.rs (fichier par site dans /etc/caddy/sites, sauvegardes tar + mysqldump), mock.rs
- sampler.rs : mesures système chaque seconde (sysinfo 0.37 : ne pas monter de version sans adapter, l'API a changé)
- Routes et types JSON : `crates/proto/src/lib.rs`

## Docs
- `docs/heberger-un-site.md` : mise en place serveur (Caddy, PHP, MariaDB, médias partagés, migration OVH)

## Étapes du projet
1. Squelette (fait).
2. Console : thème, barre latérale, barre de titre, onglets vides (fait).
3. Agent (mesures réelles via sysinfo) + tableau de bord de la console (fait).
4. Contrôle des services, journaux en direct (fait ; backend systemd pas encore testé sur une vraie machine).
5. Clé d'accès entre console et agent (fait). Le chiffrement du trafic viendra de WireGuard.
6. Onglet Sites (fait, simulé) ; à faire : onglet Fichiers (doit montrer /srv/nas/medias), page de liens temporaire, migration du WordPress OVH.
