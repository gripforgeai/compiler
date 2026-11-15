# gripforge-abi

Le contrat seul entre l'hôte de kits, les modules de kits et les passerelles, sans aucune logique : tables C (`HostTable`, `KitTable`, contrat `kithost.native@2`), dispositions fixes, numéros de fonction, interfaces `.gfi`, appels entre modules, types des ports et format des trames. Lu par l'hôte, les modules et les passerelles ([architecture](../../docs/architecture.md)).

Il rassemble, sans les changer, quatre crates des sources ([sources](../../docs/sources.md)) :

| Module | Ancienne crate | Rôle |
|---|---|---|
| racine (`fast`, `fixed`, `layout`, `rows`, `zone`, `native`…) | `gripforge-kitcall` | les appels entre le cœur et les modules de kits, l'encodage `.gfi` |
| `calls` | `gripforge-modcall` | `gripforge:calls@1` : zone partagée, en-têtes d'appel, côté module (feature `guest`) |
| `ports` | `gripforge-ports` | types de la physique, de la navigation et des assets ; lien de physique entre fils |
| `wire` | `gripforge-wire` | valeurs et trames delta partagées avec le lecteur et le réseau |

## Ce qui a changé

- `kithost.native@2` (décision D3) : en natif, aucun allocateur commun ; un bloc donné porte la fonction qui le libère (`NativeBytes::release`), celui qui alloue libère.
- `ports` vient de MKP : sans l'attente pas à pas de la physique (la physique n'est jamais bloquée).

## Construire et tester

```sh
cargo test -p gripforge-abi
```

`test-modules/` contient deux modules de test de `gripforge:calls@1` (un workspace à part, construit pour wasm par `build.sh`).

## Voir aussi

- [Structures de la mémoire partagée](../../docs/structures-memoire.md)
- [Modules de kit](../../docs/modules-de-kit.md)
