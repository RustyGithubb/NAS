#!/bin/sh
# Mise à jour de l'agent NAS depuis GitHub, installée en /usr/local/sbin/nas-agent-update.
#
# 1. Rien de nouveau sur GitHub → rien à faire.
# 2. Nouveau code dont les vérifications GitHub (CI) ne sont pas au vert → on attend.
# 3. Sinon : récupération, compilation, ancienne version gardée, installation, redémarrage.
# 4. Le nouvel agent ne démarre pas → l'ancien est remis.
# Le résultat est écrit dans /var/lib/nas-agent/update.json, lu par l'agent pour la console.
set -u

GITHUB="RustyGithubb/NAS"
BRANCH="main"
STATE_DIR="/var/lib/nas-agent"
BIN="/usr/local/bin/nas-agent"
# Dossier du dépôt cloné : NAS_REPO, sinon le premier /home/*/NAS trouvé.
REPO="${NAS_REPO:-}"
if [ -z "$REPO" ]; then
    for d in /home/*/NAS; do
        if [ -d "$d/.git" ]; then
            REPO="$d"
            break
        fi
    done
fi

mkdir -p "$STATE_DIR"

# status <état> <message> [commit] : écrit le résultat (états : voir UpdateState dans le code).
status() {
    printf '{"state":"%s","message":"%s","commit":"%s","checked_ms":%s}\n' "$1" "$2" "${3:-}" "$(date +%s%3N)" >"$STATE_DIR/update.json.tmp"
    mv "$STATE_DIR/update.json.tmp" "$STATE_DIR/update.json"
    echo "$2"
}

if [ -z "$REPO" ] || [ ! -d "$REPO/.git" ]; then
    status failed "Dépôt NAS introuvable dans /home"
    exit 1
fi
OWNER=$(stat -c %U "$REPO")

# Commandes Git et Cargo lancées en tant que propriétaire du dépôt (pas en root).
as_owner() {
    runuser -u "$OWNER" -- sh -c "cd '$REPO' && . \"\$HOME/.cargo/env\" && $1"
}

DEPLOYED=$(cat "$STATE_DIR/deployed" 2>/dev/null || true)
SHORT_DEPLOYED=$(printf '%.7s' "$DEPLOYED")

status running "Recherche de mise à jour…" "$SHORT_DEPLOYED"
if ! as_owner "git fetch --quiet origin $BRANCH"; then
    status failed "GitHub injoignable" "$SHORT_DEPLOYED"
    exit 1
fi
REMOTE=$(as_owner "git rev-parse origin/$BRANCH")
SHORT=$(printf '%.7s' "$REMOTE")
if [ "$REMOTE" = "$DEPLOYED" ]; then
    status up_to_date "Déjà à jour" "$SHORT"
    exit 0
fi

# Vérifications GitHub (CI) de la nouvelle version : il faut au moins une réussite et aucun échec.
if ! CHECKS=$(curl -fsS -H "Accept: application/vnd.github+json" "https://api.github.com/repos/$GITHUB/commits/$REMOTE/check-runs"); then
    status failed "Vérifications GitHub illisibles" "$SHORT_DEPLOYED"
    exit 1
fi
if echo "$CHECKS" | grep -Eq '"conclusion": *"(failure|cancelled|timed_out|action_required)"'; then
    status failed "Vérifications GitHub en échec : version $SHORT ignorée" "$SHORT_DEPLOYED"
    exit 1
fi
if echo "$CHECKS" | grep -Eq '"status": *"(queued|in_progress|waiting|pending)"' || ! echo "$CHECKS" | grep -q '"conclusion": *"success"'; then
    status waiting "Version $SHORT en cours de vérification sur GitHub : nouvel essai plus tard" "$SHORT_DEPLOYED"
    exit 0
fi

status running "Installation de la version $SHORT…" "$SHORT_DEPLOYED"
if ! as_owner "git merge --ff-only --quiet origin/$BRANCH"; then
    status failed "Mise à jour du dépôt impossible (fichiers modifiés à la main ?)" "$SHORT_DEPLOYED"
    exit 1
fi
if ! as_owner "cargo build --release --quiet -p nas-agent"; then
    status failed "Compilation de $SHORT échouée : ancienne version gardée" "$SHORT_DEPLOYED"
    exit 1
fi

[ -f "$BIN" ] && cp "$BIN" "$BIN.prev"
install -m 755 "$REPO/target/release/nas-agent" "$BIN"
# Écrit avant le redémarrage : la console le lira dès que l'agent répondra à nouveau.
echo "$REMOTE" >"$STATE_DIR/deployed"
status updated "Mis à jour vers la version $SHORT" "$SHORT"
systemctl restart nas-agent
sleep 5
if ! systemctl is-active --quiet nas-agent; then
    if [ -f "$BIN.prev" ]; then
        cp "$BIN.prev" "$BIN"
        echo "$DEPLOYED" >"$STATE_DIR/deployed"
        systemctl restart nas-agent
    fi
    status failed "La version $SHORT ne démarre pas : ancienne version remise" "$SHORT_DEPLOYED"
    exit 1
fi
