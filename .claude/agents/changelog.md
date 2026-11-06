---
name: changelog
description: Rédige l'entrée de CHANGELOG.md à partir des commits récents. À utiliser quand je prépare une version.
tools: Bash, Read, Edit
model: haiku
---

1. Lis l'en-tête de CHANGELOG.md (40 premières lignes) pour reprendre le format existant.
2. Lance `git log --oneline` depuis le dernier tag (`git describe --tags --abbrev=0`), ou les 20 derniers commits s'il n'y a pas de tag.
3. Rédige une entrée en français, groupée en Ajouts / Corrections / Changements, une ligne par point, compréhensible par un utilisateur (pas de jargon technique).
4. Ajoute-la en haut de CHANGELOG.md. Aucun commit.
