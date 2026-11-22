# Le code du jeu : `sources`

> Version française de [sources.md](sources.md) (la liste complète des codes d'erreur et le moment où tourne chaque
> point d'accroche y sont détaillés).

**La règle : `project.gripforge` est l'entrée unique d'un jeu.** Les fichiers Rust du code propre au jeu sont déclarés
par le projet lui-même, dans `sources` ; le compilateur ne prend **aucun** fichier par convention (pas de `index.rs`, pas
de `src/` lu parce qu'il est là). Décision de gripforgeai/gripforge#420, annoncée sur gripforgeai/compiler#10 (le
`index.rs` qu'elle prenait implicitement a disparu).

## Exemple

```
mon-jeu/
  project.gripforge   les scènes, les kits, leurs réglages, et "sources"
  regles.rs           à toi, déclaré : crate::regles (un des fichiers déclare le module du jeu)
  code/armes.rs       à toi, déclaré : crate::armes
  index.rs            NON déclaré : jamais lu (avertissement source_undeclared)
  .build/             écrit par gripforge-build, jamais versionné
```

```json
{
  "id": "arene",
  "sources": ["regles.rs", "code/armes.rs"],
  "installed": [ … ],
  "data": { "scenes": [ … ] }
}
```

```rust
// regles.rs : le module du jeu
use gripforge_kit_guest::{KitModule, Kx};

pub struct MonJeu;

impl KitModule for MonJeu {
    fn id(&self) -> &'static str { "game.main" }
    fn setup(&self, kx: &mut Kx) { kx.set_num("mon_jeu.score", 0.0); }
}
```

- Chemins relatifs au dossier du fichier projet, écrits avec `/`, chacun une fois, chacun un `.rs` dans le projet (un
  lien qui en sort est refusé : `source_outside`).
- `gripforge-build` ajoute chaque fichier au **module du jeu `game.main`** (`.build/game`), par chemin (`#[path]`) :
  jamais copié ni écrit, mêmes octets avant et après le build. Le nom du fichier est son module : `regles.rs` est
  `crate::regles`, `code/armes.rs` est `crate::armes`.
- Un des fichiers déclare le module du jeu (`pub struct X;` avec `impl KitModule for X`) ; aucun →
  `sources_module_missing`, plusieurs → `sources_module_ambiguous`.
- La clé est vérifiée par le cœur commun du format (le même que `dev.builder`) et n'est pas emballée dans le jeu livré
  (`.build/game.json`) : le code est compilé, jamais lu à l'exécution.
- Un `.rs` posé à côté du projet sans être déclaré n'est **pas** compilé : avertissement `source_undeclared` (rien
  n'est deviné).

## Voir aussi

- [Chargement des kits](chargement-des-kits.md) : le build lit le projet comme `dev.builder` et refuse un kit requis
  absent (`kit_required`).
- [README](../README.md) : la commande et ses options.
