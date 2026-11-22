# Les kits d'un jeu, fixés au build : `kit_required`

> English: [kit-loading.md](kit-loading.md). La règle commune à tous les lecteurs d'un jeu (passerelles web et native,
> `dev.builder`) est décrite dans module-kits `docs/chargement-des-kits.md` ; cette page dit ce qu'en fait le
> compilateur.

## La règle

- **Le build lit le `project.gripforge` comme `dev.builder`** : le même code (`gripforge-scene-format`, crate de ce
  dépôt, que module-kits importe épinglé), la même résolution (`includes`, variables, `$var`), le même verrou. Le verrou
  du jeu livré (`.build/game.json`, `lock.order`) est celui de `dev.builder` sans les kits de développement
  (`dev.builder`, `scene.builder`), avec le module du jeu `game.main` en dernier.
- **Les kits sont fixés au démarrage**, comme les bibliothèques d'un programme : `installed` liste **tous** les kits du
  jeu, y compris ceux dont ses kits ont besoin (leurs `requires` et leurs dépendances simples ou propres à la cible).
  Une recommandation (`recommended`) ou un kit facultatif (`optional`) n'est jamais exigé ni ajouté.
- **Un kit requis absent fait échouer le BUILD** (et `--check`) avec l'erreur `kit_required`, **le même message** que
  la passerelle et `dev.builder` au lancement : c'est la même fonction (`gripforge_scene_format::kits::check_required`,
  écrite une fois, jamais recopiée). Tous les manques sont listés d'un coup ; rien n'est écrit.
- **La cible** dit quelles dépendances comptent : `--target <passerelle>` (`bevy-web` par défaut, le lecteur web
  officiel ; `bevy`, `godot`…). Une dépendance déclarée `targets: ["bevy", "bevy-web"]` n'est pas exigée pour `godot`.
- Le jeu livré embarque ses kits ; `gf.kits` et `gf.kits-path` (les paramètres de la passerelle) servent au
  développement, aux tests et au banc, pas au build.

## Exemple

`game.engine` déclare dans son `kit.api.json` :

```json
"dependencies": [
  { "kit": "core.scene", "range": "*", "targets": ["bevy", "bevy-web"],
    "reason": "Sur Bevy, core.scene charge la partie native des scènes." }
]
```

Un projet qui installe `game.engine` sans `core.scene` :

```json
{
  "id": "arene",
  "installed": [ { "id": "game.engine" }, { "id": "core.light" } ],
  "data": { "scenes": [ … ] }
}
```

```console
$ gripforge-build arene
error kit_required — project.gripforge: kit_required: game.engine a besoin de core.scene (cible bevy-web) — ajoute-le dans "installed" du project.gripforge. Expected: ajoute core.scene dans "installed" du project.gripforge (docs/dependances.md)
project.gripforge: 1 error(s), 0 warning(s) — nothing generated; fix the project and build again
$ echo $?
1
$ gripforge-build arene --check --target godot     # Godot a ses propres scènes : core.scene n'est pas exigé
project.gripforge: checked (0 warning(s)) — …
```

La correction : ajouter le kit dans `installed`, jamais le laisser deviner.

```json
"installed": [ { "id": "game.engine" }, { "id": "core.scene" }, { "id": "core.light" } ]
```

`dev.builder` (`?gf.kits=dev.builder`) refuse le même projet au lancement avec la même ligne `kit_required`.

## Le code du jeu : déclaré par le projet (compiler#10)

Le `.gripforge` est l'**entrée unique** : il déclare lui-même ses fichiers Rust externes dans `sources`, et le build ne
prend aucun fichier par convention (un `index.rs` posé à côté n'est pas lu). Voir [sources.fr.md](sources.fr.md).

```json
"sources": ["regles.rs", "code/armes.rs"]
```

## La parité avec `dev.builder`

`tests/kits_required.rs` construit le même projet des deux façons : comme `dev.builder` (le projet emballé par
`project::pack_with`, la fonction que `devGame` appelle par le wasm du format, puis `kits::check_json`, celle que la
passerelle web appelle par `gf_kits_check`) et par `gripforge-build`. Même verrou (aux kits de développement et à
`game.main` près, comme dit plus haut), même refus, message pour message.

| Code | Quand |
|---|---|
| `kit_required` | un kit de `installed` a besoin (pour la cible) d'un kit ou d'une capacité que la liste n'a pas ; une ligne par manque |
