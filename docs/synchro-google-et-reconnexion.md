# Synchro : connexion avec Google et reconnexion d'un poste

Guide pour brancher une app (WorkTable, puis les suivantes) sur ces deux fonctions de
`nas-sync-client`. Tout ce qui est décrit ici est générique : rien n'est propre à WorkTable.

## 1. Côté serveur (une fois)

1. Dans Google Cloud, le projet de l'app a un **ID client OAuth** (type « Application de bureau »),
   de la forme `123-abc.apps.googleusercontent.com`.
2. Console NAS › Synchro › Vue d'ensemble › Réglages › **Connexion Google** : coller cet ID
   (un par ligne si plusieurs apps). Vide : la connexion Google est refusée.
3. Le Mac doit pouvoir joindre `https://www.googleapis.com` (l'agent y lit les clés publiques
   de Google avec `curl`, quelques fois par jour).

## 2. Créer un compte via Google

Console › Synchro › « + Nouveau compte » › Connexion **Via Google** › adresse Google de la personne.
Un code d'invitation est quand même donné : il sert de secours (connexion classique).

## 3. Dans l'app

### Identifiant du poste (reconnexion = même poste)

```rust
// Au premier lancement : tirer l'identifiant et le garder (fichier de l'app ou coffre du système).
// Il ne doit PAS être effacé à la déconnexion.
let install_id = load_or_create("install_id", nas_sync_client::new_install_id);
let mut client = Client::new(&server, APP_ID, &device_name).with_install_id(&install_id);
```

### Se connecter avec Google

1. L'app lance sa connexion Google habituelle (navigateur + redirection locale) en ajoutant les
   scopes `openid email`. La réponse du serveur de jetons Google contient alors un `id_token`.
2. `client.google_account(&id_token)` → identifiant NAS, adresse, `salt.pending`.
3. Selon `pending` :
   - `true` (premier accès) : demander un **mot de passe de chiffrement** (deux fois), puis
     `client.register_google(&id_token, &mdp)` → session + **code de récupération** à montrer une fois ;
   - `false` : demander le mot de passe, puis `client.login_google(&id_token, &mdp)`.

Le mot de passe reste nécessaire : Google prouve *qui* se connecte, le mot de passe ouvre la clé
qui chiffre les données. Le serveur ne peut jamais les lire. L'app peut garder la clé maître dans
le coffre du système pour ne pas redemander le mot de passe à chaque lancement.

### Restaurer les données après une (re)connexion

```rust
let key = session.master.app(APP_ID);
let cursor = client.restore(&key, |_meta, path, content| write_local(&path, &content))?;
// Garder `cursor` pour les `pull` suivants.
```

À appeler après toute connexion sur un poste sans données locales (ou après une déconnexion
qui les a effacées), **avant** de démarrer ce qui en dépend (services Google, cache…).

### Ce qu'il faut synchroniser pour tout retrouver

Seuls les fichiers envoyés avec `client.push` reviennent. Pour qu'un poste reconnecté retrouve
tout, l'app doit pousser aussi :

| Donnée | Chemin conseillé | Remarque |
|---|---|---|
| Jetons Google API (refresh token) | `secrets/google.json` | chiffrés de bout en bout comme le reste ; garder aussi la copie locale dans le coffre du système |
| Réglages | `settings.json` | |
| Cache utile (listes, index) | `cache/…` | inutile pour ce qui se retélécharge vite |
| Journaux | `logs/…` | attention au quota (8 Mo par fichier) |

L'ID client et le secret du projet Google Cloud sont propres à l'app : ils restent dans l'app,
pas dans la synchro.

## 4. Déconnexion et révocation

- **Déconnexion** depuis l'app (`client.logout`) : le jeton ne vaut plus rien mais le poste reste
  connu ; à la reconnexion (même `install_id`), il reprend sa ligne.
- **Révocation** (console ou autre poste) : le poste est effacé ; il en recréera un en se reconnectant.
