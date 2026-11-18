# NAS (nom provisoire)

Pilotage d'un Mac mini 2014 reconverti en serveur Linux.

- `crates/console` : application de bureau (egui), même direction artistique que WorkTable.
- `crates/agent` : petit serveur installé sur le Mac mini, qui renvoie l'état de la machine.
- `crates/proto` : données échangées entre les deux.

```
cargo run -p nas-agent      # dans un terminal
cargo run -p nas-console    # dans un autre
```

## Installer la console (Windows)
Construire l'installeur avec `.\installer\build.ps1` (Rust et
[Inno Setup](https://jrsoftware.org/isdl.php) requis), puis lancer
`installer\Output\NAS-<version>-setup.exe`.
La console se met ensuite à jour d'elle-même : Réglages › Général › Mises à jour de la console.

L'agent du serveur s'installe et se met à jour depuis les mêmes versions publiées : voir
[docs/installer-le-serveur.md](docs/installer-le-serveur.md).

## Publier une version
Les versions sont construites par [`dist`](https://github.com/axodotdev/cargo-dist)
(`.github/workflows/release.yml`, configuration dans `dist-workspace.toml`). Un même tag publie :
- la console Windows : `nas-console-x86_64-pc-windows-msvc.zip` (+ `.sha256`) ;
- l'agent Linux : `nas-agent-x86_64-unknown-linux-gnu.tar.xz` (+ `.sha256`), installé chaque nuit
  par le Mac mini.

1. Noter les changements dans `CHANGELOG.md`.
2. Changer `version` dans `Cargo.toml` (section `[workspace.package]`, ex. `0.1.1`), puis
   `cargo check --workspace` (met à jour `Cargo.lock`).
3. Commiter, pousser, attendre la CI verte, puis poser le tag :
   ```
   git add -A
   git commit -m "Version 0.1.1"
   git push
   git tag v0.1.1
   git push origin v0.1.1
   ```
Le tag (`v` + exactement la version de `Cargo.toml`) déclenche la compilation et la publication.
Pas de suffixe (`-beta`…) : une préversion n'est proposée ni à la console ni au Mac mini.
Console et agent sortent toujours ensemble : leurs versions du protocole restent accordées.
