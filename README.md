# NAS (nom provisoire)

Pilotage d'un Mac mini 2014 reconverti en serveur Linux.

- `crates/console` : application de bureau (egui), même direction artistique que WorkTable.
- `crates/agent` : petit serveur installé sur le Mac mini, qui renvoie l'état de la machine.
- `crates/proto` : données échangées entre les deux.

```
cargo run -p nas-agent      # dans un terminal
cargo run -p nas-console    # dans un autre
```
