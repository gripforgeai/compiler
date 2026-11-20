# Fiche : le jeu du créateur dans le lecteur web (cible wasm)

Suite de compiler#10 (D01 de gripforgeai/gripforge#420). Demande : le jeu du créateur (`KitsPlugin` + le plugin de
`index.rs`) doit tourner dans le **lecteur web officiel**, en WebGPU puis en WebGL2, comme la recette du contributeur
(MCP → éditer → compiler → rejouer). **Fiche avant tout code : la demande bute sur un choix d'architecture qui
n'appartient pas au compilateur.**

## Ce que fait le lecteur web aujourd'hui (module-kits `feat_refonte-kits` c9f3be02e)

- Le lecteur web est un présentateur Bevy en wasm ; l'hôte des kits est `core/kithost-wasm` dans la page, et la logique
  d'un jeu n'y entre que par des **modules de kit wasm** chargés par `core/gateway-web/module-loader.mjs` depuis
  `kit-modules.json`, dans la mémoire commune (`gateways/gfkits-bevy-player-web/README.md`, « L'hôte de kits dans le
  document du lecteur »).
- La CLI 0.3.165 du contributeur (`packages/grip-forge/src/project-wasm.ts`, branche `feat/kit-project-release`) fait
  exactement cela pour `game.main` : `relocatableBuild` (plain et shared), `sealKit`, `checkRelocatable`, contrôle des
  structures ABI, puis `.build/web/kit-modules.json`. Ce qu'elle construit, c'est le crate **`.build/game`** (le
  module `game.main`), pas `.build/app`.
- `.build/app` (compiler#10) est un **jeu Bevy** qui lie `gfkits-bevy-native`. Sur wasm32, cette passerelle ne peut
  charger aucun kit : `core/kithost/src/modules/mod.rs:13` retire `native` (le `NativeSet` des bibliothèques) sur
  `wasm32`, et `core/gateway/src/modules.rs:46` (`kit_folder`, appelé par
  `gateways/gfkits-bevy-native/native/src/game.rs:69` et `:139`) ne sait ouvrir qu'un dossier de bibliothèques natives.
- Un plugin Bevy ne peut pas être chargé dans le lecteur web déjà compilé : il n'y a pas d'ABI Bevy entre deux modules
  wasm, et recompiler le lecteur par jeu est exclu (« le lecteur n'est presque jamais recompilé »).

**Conclusion : `index.rs` tel que D01 le définit (un `Plugin` Bevy) ne peut pas tourner dans le lecteur web officiel.**
Compiler `.build/app` en wasm donnerait au mieux une page Bevy à part, sans kits, pas le lecteur officiel.

## Options (décision à Damien : D01 et le cœur)

| Option | Ce que c'est | Lecteur officiel ? | Travail | Propriétaire |
|---|---|---|---|---|
| **A. `index.rs` = module du jeu** (recommandée) | les règles du créateur écrites sur l'API des modules (`gripforge_kit_guest`, `KitModule` : `setup`, `update(dt)` au tick, `on_event`, `Kx` pour appeler les kits et lire `game.engine`), compilées **dans `game.main`** ; le même code en natif et en wasm | oui, WebGPU et WebGL2, sans toucher au lecteur | compilateur : `index.rs` pris par `#[path]` dans `.build/game`, `module.rs` appelle ses fonctions ; template `fps-shooter` à réécrire sur `Kx` (règles inchangées, `reflect` remplacé par les vues des entités et `kithost.scene@1` `set_transform`) ; la CLI du contributeur marche telle quelle | D01 amendé (Damien) ; template : équipe des kits ; compilateur : nous |
| B. Passerelle Bevy sur le web | `gfkits-bevy-native` en wasm parlant à l'hôte de la page (`gateway-web`), `.build/app` compilé en page Bevy autonome | non (une page à part du lecteur) | gros : une façade Rust de `gateway-web`, chargement des kits wasm depuis un wasm | cœur (Damien) |
| C. Le lecteur charge le plugin du jeu | — | — | impossible sans ABI Bevy entre wasm, ou lecteur recompilé par jeu (exclu) | — |

Avec A, `.build/app` (natif) reste possible pour qui veut un jeu Bevy à soi, mais ce n'est plus le chemin officiel ;
on peut aussi le retirer pour n'avoir qu'un seul `index.rs`. À trancher avec A.

## Critères de vérification (si A est retenue)

1. `index.rs` (module) compilé dans `game.main` : sha256 identique avant/après build ; aucune écriture du build.
2. `.build/game` construit en wasm par les fonctions officielles du SDK (`relocatableBuild`, `sealKit`,
   `checkRelocatable`, structures ABI) et `.build/web/kit-modules.json` au format `gripforge.kit-modules/1` de la CLI
   0.3.165 : aucune recette de linker ni table ABI recopiée.
3. `fps-shooter` (version module, module-kits#1802 pour le format) joué dans le lecteur web officiel, WebGPU puis
   WebGL2 sur la RTX : tir, chute de la cible, score, rechargement ; captures ; mêmes valeurs en natif.
4. Tests rouges sans le code, verts avec ; `cargo test --workspace`, clippy, fmt.
