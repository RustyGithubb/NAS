#!/bin/sh
# Mise à jour de l'agent NAS depuis les versions publiées sur GitHub (releases), installée en
# /usr/local/sbin/nas-agent-update.
#
# 1. Dernière version publiée = version installée → rien à faire.
# 2. Sinon : téléchargement de l'agent déjà compilé par GitHub (nas-agent-…-linux-gnu.tar.xz),
#    vérification de son empreinte SHA-256, ancienne version gardée, installation, redémarrage.
# 3. Le nouvel agent ne démarre pas → l'ancien est remis.
# Rien n'est compilé sur le Mac mini (ni Rust ni dépôt nécessaires). Une version n'est publiée
# qu'une fois la CI de release passée.
# Le résultat est écrit dans /var/lib/nas-agent/update.json, lu par l'agent pour la console.
set -u

GITHUB="RustyGithubb/NAS"
ASSET="nas-agent-x86_64-unknown-linux-gnu.tar.xz"
STATE_DIR="/var/lib/nas-agent"
BIN="/usr/local/bin/nas-agent"
UA="nas-agent-update"

mkdir -p "$STATE_DIR"

# status <état> <message> [version] : écrit le résultat (états : voir UpdateState dans le code).
status() {
    printf '{"state":"%s","message":"%s","commit":"%s","checked_ms":%s}\n' "$1" "$2" "${3:-}" "$(date +%s%3N)" >"$STATE_DIR/update.json.tmp"
    mv "$STATE_DIR/update.json.tmp" "$STATE_DIR/update.json"
    echo "$2"
}

# Version installée (ex. 0.1.0). Un ancien agent compilé ici avait noté un commit : il sera
# remplacé par la version publiée.
DEPLOYED=$(cat "$STATE_DIR/deployed" 2>/dev/null || true)
case "$DEPLOYED" in
    [0-9]*.[0-9]*.[0-9]*) SHOWN="$DEPLOYED" ;;
    *) SHOWN="" ;;
esac

status running "Recherche de mise à jour…" "$SHOWN"
if ! RELEASE=$(curl -fsSL -A "$UA" -H "Accept: application/vnd.github+json" "https://api.github.com/repos/$GITHUB/releases/latest"); then
    status failed "GitHub injoignable, ou aucune version publiée" "$SHOWN"
    exit 1
fi
# Numéro de la dernière version (« v0.1.0 » → « 0.1.0 »), lu sans outil JSON.
TAG=$(printf '%s' "$RELEASE" | grep -o '"tag_name": *"[^"]*"' | head -n 1 | sed 's/.*"\([^"]*\)"$/\1/')
VERSION=${TAG#v}
if [ -z "$VERSION" ]; then
    status failed "Réponse de GitHub illisible" "$SHOWN"
    exit 1
fi
if [ "$VERSION" = "$DEPLOYED" ]; then
    status up_to_date "Déjà à jour" "$VERSION"
    exit 0
fi

URL="https://github.com/$GITHUB/releases/download/$TAG/$ASSET"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

status running "Installation de la version $VERSION…" "$SHOWN"
if ! curl -fsSL -A "$UA" -o "$TMP/$ASSET" "$URL" || ! curl -fsSL -A "$UA" -o "$TMP/$ASSET.sha256" "$URL.sha256"; then
    status failed "Téléchargement de la version $VERSION impossible : ancienne version gardée" "$SHOWN"
    exit 1
fi
# Empreinte obligatoire : l'archive doit être exactement celle publiée.
EXPECTED=$(cut -d ' ' -f 1 "$TMP/$ASSET.sha256")
ACTUAL=$(sha256sum "$TMP/$ASSET" | cut -d ' ' -f 1)
if [ -z "$EXPECTED" ] || [ "$EXPECTED" != "$ACTUAL" ]; then
    status failed "Archive de la version $VERSION corrompue (empreinte différente) : ancienne version gardée" "$SHOWN"
    exit 1
fi
# L'archive contient un dossier nas-agent-…/ avec le programme dedans.
if ! tar -xJf "$TMP/$ASSET" -C "$TMP"; then
    status failed "Archive de la version $VERSION illisible : ancienne version gardée" "$SHOWN"
    exit 1
fi
NEW=$(find "$TMP" -type f -name nas-agent | head -n 1)
if [ -z "$NEW" ]; then
    status failed "Programme absent de l'archive $VERSION : ancienne version gardée" "$SHOWN"
    exit 1
fi

# Ancienne version gardée ; sans agent installé, pas de sauvegarde périmée à remettre.
if [ -f "$BIN" ]; then cp "$BIN" "$BIN.prev"; else rm -f "$BIN.prev"; fi
install -m 755 "$NEW" "$BIN"
# Écrit avant le redémarrage : la console le lira dès que l'agent répondra à nouveau.
echo "$VERSION" >"$STATE_DIR/deployed"
status updated "Mis à jour vers la version $VERSION" "$VERSION"
systemctl restart nas-agent
sleep 5
if ! systemctl is-active --quiet nas-agent; then
    if [ -f "$BIN.prev" ]; then
        cp "$BIN.prev" "$BIN"
        echo "$DEPLOYED" >"$STATE_DIR/deployed"
        systemctl restart nas-agent
    fi
    status failed "La version $VERSION ne démarre pas : ancienne version remise" "$SHOWN"
    exit 1
fi
